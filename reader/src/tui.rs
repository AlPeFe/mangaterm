//! Interfaz de terminal: biblioteca / buscar → elegir fuente → capítulos → leer.
//!
//! Todo lo lento (host, búsquedas, descargas, decodificación, AniList) corre en tareas aparte y
//! vuelve como `Msg` al bucle principal, que es el único que toca el estado y dibuja.

use crate::anilist::{self, Credentials, Media, SyncResult};
use crate::host::{Host, LoadedSource};
use crate::library::{AniListLink, LastRead, Library, Status};
use crate::model::{Chapter, Manga, MangaDetails, Page};
use crate::paths;
use crate::render::{self, Encoded};
use crate::search::{self, SearchEvent};
use anyhow::Result;
use image::imageops::FilterType;
use image::DynamicImage;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{FontSize, StatefulImage};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Páginas que se descargan por delante de la actual.
const PREFETCH: usize = 4;
/// Páginas codificadas que se mantienen alrededor de la actual (el resto se libera).
const KEEP_AROUND: usize = 8;

enum Msg {
    Key(KeyEvent),
    Resize,
    HostReady(Result<(Arc<Host>, Vec<LoadedSource>, usize)>),
    Search(u64, SearchEvent),
    Details(u64, Result<MangaDetails>),
    Pages(u64, Result<Vec<Page>>),
    Image(u64, usize, Result<DynamicImage>),
    Encoded(u64, usize, (u16, u16), Encoded),
    AnilistMatches(Result<Vec<Media>>),
    /// (fuente, url del manga, resultado)
    AnilistSynced(String, String, Result<SyncResult>),
}

/// Una página descargada. En modo Sixel se codifica con nuestro render (`render.rs`) para el
/// tamaño de área actual; en los demás protocolos la dibuja ratatui-image.
struct PageImage {
    img: Arc<DynamicImage>,
    encoded: Option<((u16, u16), Encoded)>,
    encoding: Option<(u16, u16)>,
    proto: Option<StatefulProtocol>,
}

pub async fn run() -> Result<()> {
    let picker = make_picker();
    let mut terminal = ratatui::init();
    let result = App::new(picker).run(&mut terminal).await;
    ratatui::restore();
    result
}

/// Detecta el protocolo gráfico. Windows Terminal soporta Sixel pero no siempre contesta la
/// consulta, así que si estamos en WT y la detección falla, forzamos Sixel.
/// `MANGATERM_PROTOCOL` (sixel|kitty|iterm2|halfblocks) y `MANGATERM_FONT` (p. ej. 10x20) lo pisan.
fn make_picker() -> Picker {
    let font = std::env::var("MANGATERM_FONT")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))));
    let forced = std::env::var("MANGATERM_PROTOCOL").ok().and_then(|p| match p.as_str() {
        "sixel" => Some(ProtocolType::Sixel),
        "kitty" => Some(ProtocolType::Kitty),
        "iterm2" => Some(ProtocolType::Iterm2),
        "halfblocks" => Some(ProtocolType::Halfblocks),
        _ => None,
    });
    let in_wt = std::env::var_os("WT_SESSION").is_some();

    // from_fontsize está deprecado a favor de la consulta, pero es el único fallback con tamaño de fuente
    #[allow(deprecated)]
    let mut picker = match (font, Picker::from_query_stdio()) {
        (Some((w, h)), _) => Picker::from_fontsize(FontSize::new(w, h)),
        (None, Ok(p)) => p,
        (None, Err(_)) => Picker::from_fontsize(FontSize::new(10, 20)),
    };
    if let Some(p) = forced {
        picker.set_protocol_type(p);
    } else if in_wt && picker.protocol_type() == ProtocolType::Halfblocks {
        picker.set_protocol_type(ProtocolType::Sixel);
    }
    picker
}

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Library,
    Search,
    Sources,
    Manga,
    Reader,
    AnilistPick,
}

/// El mismo manga encontrado en varias fuentes.
struct Group {
    key: String,
    title: String,
    entries: Vec<(LoadedSource, Manga)>,
}

struct Reader {
    source: LoadedSource,
    manga: Manga,
    /// URL del manga tal como está en la biblioteca (clave del favorito).
    manga_url: String,
    chapters: Vec<Chapter>,
    chapter: usize,
    pages: Vec<Page>,
    page: usize,
    requested: HashSet<usize>,
    images: HashMap<usize, PageImage>,
    error: Option<String>,
}

impl Reader {
    /// Las extensiones suelen listar del capítulo más nuevo al más viejo.
    fn descending(&self) -> bool {
        let first = self.chapters.first().map(|c| c.chapter_number).unwrap_or(0.0);
        let last = self.chapters.last().map(|c| c.chapter_number).unwrap_or(0.0);
        first >= last
    }

    fn next_chapter(&self) -> Option<usize> {
        if self.descending() {
            self.chapter.checked_sub(1)
        } else {
            Some(self.chapter + 1).filter(|&c| c < self.chapters.len())
        }
    }

    fn prev_chapter(&self) -> Option<usize> {
        if self.descending() {
            Some(self.chapter + 1).filter(|&c| c < self.chapters.len())
        } else {
            self.chapter.checked_sub(1)
        }
    }
}

struct App {
    picker: Picker,
    sixel: bool,
    font: (u16, u16),
    /// Área (en celdas) donde va la imagen del lector, medida en el último dibujo.
    reader_area: Rect,
    /// Qué página y en qué posición está pintada ya en pantalla (Sixel se escribe aparte de ratatui).
    sixel_drawn: Option<(usize, Rect)>,
    tx: mpsc::UnboundedSender<Msg>,
    rx: mpsc::UnboundedReceiver<Msg>,
    host: Option<Arc<Host>>,
    sources: Vec<LoadedSource>,
    screen: Screen,
    status: String,
    quit: bool,
    clear: bool,
    /// Se incrementa al cambiar de contexto para descartar respuestas viejas.
    generation: u64,
    /// Igual, pero para búsquedas: así entrar a un manga no corta la búsqueda en curso.
    search_gen: u64,

    input: String,
    editing: bool,
    query: String,
    groups: Vec<Group>,
    groups_state: ListState,
    pending: usize,
    failures: Vec<(String, String)>,

    group: Option<usize>,
    sources_state: ListState,

    current: Option<(LoadedSource, Manga)>,
    details: Option<MangaDetails>,
    chapters_state: ListState,
    /// A dónde vuelve "esc" desde la pantalla del manga.
    manga_back: Screen,

    reader: Option<Reader>,

    library: Library,
    library_state: ListState,
    anilist_matches: Option<Vec<Media>>,
    anilist_state: ListState,
}

impl App {
    fn new(picker: Picker) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let font = picker.font_size();
        let library = Library::load();
        let has_favorites = !library.favorites.is_empty();
        App {
            sixel: picker.protocol_type() == ProtocolType::Sixel,
            font: (font.width, font.height),
            reader_area: Rect::default(),
            sixel_drawn: None,
            picker,
            tx,
            rx,
            host: None,
            sources: Vec::new(),
            // Con favoritos se arranca en la biblioteca; si no, directo a buscar
            screen: if has_favorites { Screen::Library } else { Screen::Search },
            status: "cargando extensiones…".into(),
            quit: false,
            clear: false,
            generation: 0,
            search_gen: 0,
            input: String::new(),
            editing: !has_favorites,
            query: String::new(),
            groups: Vec::new(),
            groups_state: ListState::default(),
            pending: 0,
            failures: Vec::new(),
            group: None,
            sources_state: ListState::default(),
            current: None,
            details: None,
            chapters_state: ListState::default(),
            manga_back: Screen::Search,
            reader: None,
            library_state: ListState::default().with_selected(has_favorites.then_some(0)),
            library,
            anilist_matches: None,
            anilist_state: ListState::default(),
        }
    }

    async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        spawn_input(self.tx.clone());
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let res = async {
                let host = Host::spawn().await?;
                let (sources, errors) = host.load_all(&crate::ext::list()).await;
                Ok((host, sources, errors.len()))
            }
            .await;
            let _ = tx.send(Msg::HostReady(res));
        });

        while !self.quit {
            if std::mem::take(&mut self.clear) {
                terminal.clear()?;
                self.sixel_drawn = None;
            }
            terminal.draw(|f| self.draw(f))?;
            self.flush_sixel()?;
            let Some(msg) = self.rx.recv().await else { break };
            self.update(msg);
            // Procesar todo lo acumulado antes de redibujar (las búsquedas llegan en ráfagas)
            while let Ok(msg) = self.rx.try_recv() {
                self.update(msg);
            }
        }
        Ok(())
    }

    // ───────────────────────────── eventos ─────────────────────────────

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Key(key) => self.on_key(key),
            Msg::Resize => self.clear = true,
            Msg::HostReady(Ok((host, sources, errors))) => {
                self.status = format!("{} fuentes listas", sources.len())
                    + &if errors > 0 { format!(" ({errors} extensiones no cargaron)") } else { String::new() };
                if sources.is_empty() {
                    self.status = "no hay extensiones: instalá con `mangaterm ext install --lang es`".into();
                }
                self.host = Some(host);
                self.sources = sources;
                if !self.query.is_empty() {
                    self.start_search();
                }
            }
            Msg::HostReady(Err(e)) => self.status = format!("error iniciando ext-host: {e}"),
            Msg::Search(generation, ev) => {
                if generation == self.search_gen {
                    self.on_search_event(ev)
                }
            }
            Msg::Details(generation, res) if generation == self.generation => match res {
                Ok(d) => {
                    // Si es un favorito, posicionarse en el último capítulo leído
                    let last = self.current_favorite().and_then(|f| f.last_read.clone());
                    let index = last.and_then(|l| d.chapters.iter().position(|c| c.url == l.chapter_url));
                    self.chapters_state.select(index.or((!d.chapters.is_empty()).then_some(0)));
                    self.status = format!("{} capítulos", d.chapters.len());
                    self.details = Some(d);
                }
                Err(e) => self.status = format!("error: {}", search::short_error(&e.to_string())),
            },
            Msg::Pages(generation, res) if generation == self.generation => {
                let Some(r) = self.reader.as_mut() else { return };
                match res {
                    Ok(pages) if pages.is_empty() => r.error = Some("el capítulo no tiene páginas".into()),
                    Ok(pages) => {
                        // "Continuar" pudo fijar una página antes de saber cuántas hay
                        r.page = r.page.min(pages.len() - 1);
                        r.pages = pages;
                        self.track_progress();
                        self.prefetch();
                    }
                    Err(e) => r.error = Some(search::short_error(&e.to_string())),
                }
            }
            Msg::Image(generation, index, res) if generation == self.generation => {
                let Some(r) = self.reader.as_mut() else { return };
                match res {
                    Ok(img) => {
                        let img = Arc::new(img);
                        let proto = (!self.sixel).then(|| self.picker.new_resize_protocol((*img).clone()));
                        r.images.insert(index, PageImage { img, encoded: None, encoding: None, proto });
                        if index == r.page {
                            self.clear = true;
                        }
                        self.request_encode(index);
                    }
                    Err(e) if index == r.page => r.error = Some(search::short_error(&e.to_string())),
                    Err(_) => {
                        r.requested.remove(&index);
                    }
                }
            }
            Msg::Encoded(generation, index, size, enc) if generation == self.generation => {
                let Some(page) = self.reader.as_mut().and_then(|r| r.images.get_mut(&index)) else { return };
                if page.encoding == Some(size) {
                    page.encoding = None;
                }
                page.encoded = Some((size, enc));
            }
            Msg::Details(..) | Msg::Pages(..) | Msg::Image(..) | Msg::Encoded(..) => {}
            Msg::AnilistMatches(res) if self.screen == Screen::AnilistPick => match res {
                Ok(list) if list.is_empty() => {
                    self.status = "AniList no encontró ese título".into();
                    self.screen = Screen::Manga;
                }
                Ok(list) => {
                    self.anilist_state.select(Some(0));
                    self.anilist_matches = Some(list);
                }
                Err(e) => {
                    self.status = format!("AniList: {e}");
                    self.screen = Screen::Manga;
                }
            },
            Msg::AnilistMatches(_) => {}
            Msg::AnilistSynced(source_id, url, res) => self.on_synced(&source_id, &url, res),
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match self.screen {
            Screen::Library => self.key_library(key),
            Screen::Search => self.key_search(key),
            Screen::Sources => self.key_sources(key),
            Screen::Manga => self.key_manga(key),
            Screen::Reader => self.key_reader(key),
            Screen::AnilistPick => self.key_anilist_pick(key),
        }
    }

    fn key_library(&mut self, key: KeyEvent) {
        let len = self.library.favorites.len();
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('/') | KeyCode::Char('s') | KeyCode::Tab => {
                self.screen = Screen::Search;
                self.editing = true;
            }
            KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.library_state),
            KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.library_state, len),
            KeyCode::Char('d') | KeyCode::Delete => {
                let Some(i) = self.library_state.selected().filter(|&i| i < len) else { return };
                let fav = self.library.favorites.remove(i);
                self.save_library();
                self.status = format!("quitado de favoritos: {}", fav.manga.title);
                self.library_state.select(if self.library.favorites.is_empty() { None } else { Some(i.min(len - 2)) });
            }
            KeyCode::Enter => {
                let Some(fav) = self.library_state.selected().and_then(|i| self.library.favorites.get(i)).cloned() else {
                    return;
                };
                match self.sources.iter().find(|s| s.info.id == fav.source_id).cloned() {
                    Some(src) => {
                        self.manga_back = Screen::Library;
                        self.open_manga(src, fav.manga, false);
                    }
                    None if self.host.is_none() => self.status = "esperando a que carguen las extensiones…".into(),
                    None => self.status = format!("la fuente {} no está instalada", fav.source_name),
                }
            }
            _ => {}
        }
    }

    fn key_anilist_pick(&mut self, key: KeyEvent) {
        let len = self.anilist_matches.as_ref().map_or(0, Vec::len);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => self.screen = Screen::Manga,
            KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.anilist_state),
            KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.anilist_state, len),
            KeyCode::Enter => {
                let Some(media) = self.anilist_state.selected().and_then(|i| self.anilist_matches.as_ref()?.get(i)).cloned()
                else {
                    return;
                };
                let Some(fav) = self.current_favorite_mut() else { return };
                fav.anilist = Some(AniListLink {
                    media_id: media.id,
                    title: media.display_title(),
                    synced_progress: None,
                    synced_status: None,
                    synced_at: None,
                });
                self.save_library();
                self.screen = Screen::Manga;
                self.anilist_sync(false);
            }
            _ => {}
        }
    }

    fn key_search(&mut self, key: KeyEvent) {
        if self.editing {
            match key.code {
                KeyCode::Enter if !self.input.trim().is_empty() => {
                    self.query = self.input.trim().to_string();
                    self.editing = false;
                    self.start_search();
                }
                KeyCode::Char(c) => self.input.push(c),
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Esc if !self.groups.is_empty() => self.editing = false,
                KeyCode::Down | KeyCode::Tab if !self.groups.is_empty() => self.editing = false,
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('/') | KeyCode::Char('s') | KeyCode::Esc => self.editing = true,
            KeyCode::Char('b') | KeyCode::Tab => self.screen = Screen::Library,
            KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.groups_state),
            KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.groups_state, self.groups.len()),
            KeyCode::Enter => {
                let Some(i) = self.groups_state.selected() else { return };
                if self.groups[i].entries.len() == 1 {
                    let (src, manga) = self.groups[i].entries[0].clone();
                    self.group = None;
                    self.manga_back = Screen::Search;
                    self.open_manga(src, manga, false);
                } else {
                    self.group = Some(i);
                    self.sources_state.select(Some(0));
                    self.screen = Screen::Sources;
                }
            }
            _ => {}
        }
    }

    fn key_sources(&mut self, key: KeyEvent) {
        let Some(g) = self.group else { return };
        let len = self.groups[g].entries.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => self.screen = Screen::Search,
            KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.sources_state),
            KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.sources_state, len),
            KeyCode::Enter => {
                if let Some(i) = self.sources_state.selected() {
                    let (src, manga) = self.groups[g].entries[i].clone();
                    self.manga_back = Screen::Sources;
                    self.open_manga(src, manga, false);
                }
            }
            _ => {}
        }
    }

    fn key_manga(&mut self, key: KeyEvent) {
        let len = self.details.as_ref().map_or(0, |d| d.chapters.len());
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.generation += 1;
                self.screen = self.manga_back;
            }
            KeyCode::Char('f') => self.toggle_favorite(),
            KeyCode::Char('e') => {
                let Some(fav) = self.current_favorite_mut() else {
                    self.status = "primero agregalo a favoritos (f)".into();
                    return;
                };
                fav.status = fav.status.next();
                let label = fav.status.label();
                self.save_library();
                self.status = format!("estado: {label} (tocá 'a' para enviarlo a AniList)");
            }
            KeyCode::Char('a') => self.anilist_sync(false),
            KeyCode::Char('A') => self.anilist_sync(true),
            KeyCode::Char('r') => {
                if let Some((src, manga)) = self.current.clone() {
                    self.open_manga(src, manga, true);
                }
            }
            KeyCode::Char('c') => {
                // Continuar: abrir el último capítulo leído en la página donde quedó
                let last = self.current_favorite().and_then(|f| f.last_read.clone());
                let index = last.as_ref().and_then(|l| self.details.as_ref()?.chapters.iter().position(|c| c.url == l.chapter_url));
                match (index, last) {
                    (Some(i), Some(l)) => {
                        self.open_chapter(i);
                        if let Some(r) = self.reader.as_mut() {
                            r.page = l.page;
                        }
                    }
                    _ => self.status = "no hay lectura para continuar".into(),
                }
            }
            KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.chapters_state),
            KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.chapters_state, len),
            KeyCode::PageUp => page_move(&mut self.chapters_state, len, -20),
            KeyCode::PageDown => page_move(&mut self.chapters_state, len, 20),
            KeyCode::Home | KeyCode::Char('g') => self.chapters_state.select((len > 0).then_some(0)),
            KeyCode::End | KeyCode::Char('G') => self.chapters_state.select(len.checked_sub(1)),
            KeyCode::Enter => {
                if let Some(i) = self.chapters_state.selected() {
                    self.open_chapter(i);
                }
            }
            _ => {}
        }
    }

    fn key_reader(&mut self, key: KeyEvent) {
        let Some(r) = self.reader.as_mut() else { return };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                // Al volver, dejar seleccionado el capítulo que se estaba leyendo
                self.chapters_state.select(Some(r.chapter));
                self.reader = None;
                self.save_library();
                self.generation += 1;
                self.screen = Screen::Manga;
                self.clear = true;
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') | KeyCode::PageDown | KeyCode::Char('j') => {
                if r.page + 1 < r.pages.len() {
                    r.page += 1;
                    self.page_changed();
                } else if let Some(next) = r.next_chapter() {
                    self.open_chapter(next);
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::PageUp | KeyCode::Char('k') => {
                if r.page > 0 {
                    r.page -= 1;
                    self.page_changed();
                }
            }
            KeyCode::Char('n') => {
                if let Some(next) = r.next_chapter() {
                    self.open_chapter(next);
                }
            }
            KeyCode::Char('p') => {
                if let Some(prev) = r.prev_chapter() {
                    self.open_chapter(prev);
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                r.page = 0;
                self.page_changed();
            }
            KeyCode::End | KeyCode::Char('G') => {
                r.page = r.pages.len().saturating_sub(1);
                self.page_changed();
            }
            _ => {}
        }
    }

    // ───────────────────────────── acciones ─────────────────────────────

    fn start_search(&mut self) {
        self.groups.clear();
        self.groups_state.select(None);
        self.failures.clear();
        let Some(host) = self.host.clone() else {
            self.status = "esperando a que carguen las extensiones…".into();
            return;
        };
        self.search_gen += 1;
        self.pending = self.sources.len();
        let generation = self.search_gen;
        let mut rx = search::global(host, self.sources.clone(), self.query.clone());
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                if tx.send(Msg::Search(generation, ev)).is_err() {
                    break;
                }
            }
        });
    }

    fn on_search_event(&mut self, ev: SearchEvent) {
        match ev {
            SearchEvent::Results { source, mangas } => {
                self.pending = self.pending.saturating_sub(1);
                for manga in mangas {
                    let key = search::normalize(&manga.title);
                    match self.groups.iter_mut().find(|g| g.key == key) {
                        Some(g) => g.entries.push((source.clone(), manga)),
                        None => self.groups.push(Group { key, title: manga.title.clone(), entries: vec![(source.clone(), manga)] }),
                    }
                }
                self.sort_groups();
            }
            SearchEvent::Failed { source, error } => {
                self.pending = self.pending.saturating_sub(1);
                self.failures.push((source.info.name, error));
            }
            SearchEvent::Done => self.pending = 0,
        }
    }

    /// Coincidencia exacta primero, después las que empiezan/contienen la búsqueda,
    /// y a igualdad, las que están en más fuentes. Mantiene la selección en el mismo grupo.
    fn sort_groups(&mut self) {
        let selected = self.groups_state.selected().map(|i| self.groups[i].key.clone());
        let q = search::normalize(&self.query);
        let rank = |g: &Group| {
            let r = if g.key == q {
                0
            } else if g.key.starts_with(&q) {
                1
            } else if g.key.contains(&q) {
                2
            } else {
                3
            };
            (r, std::cmp::Reverse(g.entries.len()))
        };
        self.groups.sort_by_key(rank);
        let index = selected.and_then(|k| self.groups.iter().position(|g| g.key == k));
        self.groups_state.select(index.or((!self.groups.is_empty()).then_some(0)));
    }

    fn open_manga(&mut self, source: LoadedSource, manga: Manga, refresh: bool) {
        let Some(host) = self.host.clone() else { return };
        self.generation += 1;
        let generation = self.generation;
        self.details = None;
        self.chapters_state.select(None);
        self.status = format!("cargando {} desde {}…", manga.title, source.info.name);
        self.current = Some((source.clone(), manga.clone()));
        self.screen = Screen::Manga;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let res = host.details(&source.info.id, &manga, refresh).await;
            let _ = tx.send(Msg::Details(generation, res));
        });
    }

    // ───────────────────────────── favoritos y AniList ─────────────────────────────

    fn current_favorite(&self) -> Option<&crate::library::Favorite> {
        let (src, manga) = self.current.as_ref()?;
        self.library.favorites.iter().find(|f| f.is(&src.info.id, &manga.url))
    }

    fn current_favorite_mut(&mut self) -> Option<&mut crate::library::Favorite> {
        let (src, manga) = self.current.as_ref()?;
        self.library.get_mut(&src.info.id, &manga.url)
    }

    fn save_library(&mut self) {
        if let Err(e) = self.library.save() {
            self.status = format!("no se pudo guardar la biblioteca: {e}");
        }
    }

    fn toggle_favorite(&mut self) {
        let Some((src, manga)) = self.current.clone() else { return };
        // Guardar los datos completos si ya cargaron (portada, autor…), pero con la url original como clave
        let mut full = self.details.as_ref().map(|d| d.manga.clone()).unwrap_or_else(|| manga.clone());
        full.url = manga.url.clone();
        if full.title.is_empty() {
            full.title = manga.title.clone();
        }
        let added = self.library.toggle(&src.info.id, &src.info.name, &src.info.lang, &full);
        self.save_library();
        self.status = if added { "★ agregado a favoritos".into() } else { "quitado de favoritos".into() };
        if added {
            self.library_state.select(Some(self.library.favorites.len() - 1));
        }
    }

    /// Sincroniza el favorito actual con AniList. La primera vez (o con `relink`) pide elegir la obra.
    fn anilist_sync(&mut self, relink: bool) {
        if Credentials::load().is_none() {
            self.status = "no hay sesión de AniList: ejecutá `mangaterm anilist login` en otra terminal".into();
            return;
        }
        let Some(fav) = self.current_favorite().cloned() else {
            self.status = "primero agregalo a favoritos (f)".into();
            return;
        };
        match fav.anilist.filter(|_| !relink) {
            Some(link) => {
                let creds = Credentials::load().unwrap();
                self.status = format!("sincronizando con AniList: {}…", link.title);
                let tx = self.tx.clone();
                let (source_id, url) = (fav.source_id.clone(), fav.manga.url.clone());
                let (status, progress) = (fav.status.as_anilist(), fav.progress);
                tokio::spawn(async move {
                    let res = anilist::sync(&creds, link.media_id, status, progress).await;
                    let _ = tx.send(Msg::AnilistSynced(source_id, url, res));
                });
            }
            None => {
                self.anilist_matches = None;
                self.screen = Screen::AnilistPick;
                let tx = self.tx.clone();
                let title = fav.manga.title.clone();
                tokio::spawn(async move {
                    let _ = tx.send(Msg::AnilistMatches(anilist::search(&title).await));
                });
            }
        }
    }

    fn on_synced(&mut self, source_id: &str, url: &str, res: Result<SyncResult>) {
        let Some(fav) = self.library.get_mut(source_id, url) else { return };
        match res {
            Ok(r) => {
                let progress = r.entry.progress.unwrap_or(fav.progress);
                let status = r.entry.status.as_deref().and_then(Status::from_anilist).unwrap_or(fav.status);
                // Si AniList tenía más capítulos leídos, adoptarlos localmente
                fav.progress = fav.progress.max(progress);
                fav.status = status;
                if let Some(link) = fav.anilist.as_mut() {
                    link.synced_progress = Some(fav.progress);
                    link.synced_status = Some(status);
                    link.synced_at = Some(crate::library::now());
                }
                self.status = format!(
                    "✓ AniList: {} · capítulo {}{}",
                    status.label(),
                    progress,
                    if r.kept_remote_progress { " (AniList ya tenía más progreso; se conservó)" } else { "" }
                );
            }
            Err(e) => self.status = format!("✗ AniList: {e}"),
        }
        self.save_library();
    }

    /// Guarda dónde quedó la lectura y marca el capítulo como leído al llegar a su última página.
    fn track_progress(&mut self) {
        let Some(r) = self.reader.as_ref() else { return };
        let chapter = r.chapters[r.chapter].clone();
        let (page, last_page) = (r.page, !r.pages.is_empty() && r.page + 1 >= r.pages.len());
        let (source_id, url) = (r.source.info.id.clone(), r.manga_url.clone());
        let Some(fav) = self.library.get_mut(&source_id, &url) else { return };
        fav.last_read = Some(LastRead { chapter_url: chapter.url.clone(), chapter_name: chapter.name.clone(), page });
        if last_page {
            let before = fav.progress;
            fav.mark_chapter_read(&chapter);
            if fav.progress != before {
                self.save_library();
            }
        }
    }

    fn open_chapter(&mut self, index: usize) {
        let (Some(host), Some((source, manga)), Some(details)) = (self.host.clone(), self.current.clone(), self.details.as_ref())
        else {
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        let chapter = details.chapters[index].clone();
        self.reader = Some(Reader {
            source: source.clone(),
            manga_url: manga.url.clone(),
            manga: details.manga.title.is_empty().then_some(manga).unwrap_or_else(|| details.manga.clone()),
            chapters: details.chapters.clone(),
            chapter: index,
            pages: Vec::new(),
            page: 0,
            requested: HashSet::new(),
            images: HashMap::new(),
            error: None,
        });
        self.screen = Screen::Reader;
        self.clear = true;
        self.track_progress();
        self.save_library();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let res = host.pages(&source.info.id, &chapter).await;
            let _ = tx.send(Msg::Pages(generation, res));
        });
    }

    fn page_changed(&mut self) {
        self.clear = true;
        if let Some(r) = self.reader.as_mut() {
            r.error = None;
            let page = r.page;
            r.images.retain(|&i, _| i.abs_diff(page) <= KEEP_AROUND);
            r.requested.retain(|&i| i.abs_diff(page) <= KEEP_AROUND);
        }
        self.track_progress();
        self.prefetch();
    }

    /// Pide la página actual y las siguientes que falten.
    fn prefetch(&mut self) {
        let (Some(host), Some(r)) = (self.host.clone(), self.reader.as_mut()) else { return };
        let end = (r.page + 1 + PREFETCH).min(r.pages.len());
        for index in r.page..end {
            if !r.requested.insert(index) {
                continue;
            }
            let page = r.pages[index].clone();
            let source_id = r.source.info.id.clone();
            let path = page_cache_path(&r.source, &r.chapters[r.chapter], index);
            let (tx, host, generation) = (self.tx.clone(), host.clone(), self.generation);
            tokio::spawn(async move {
                let res = async {
                    if !path.exists() {
                        let tmp = path.with_extension("part");
                        host.image(&source_id, &page, &tmp).await?;
                        std::fs::rename(&tmp, &path)?;
                    }
                    let img = tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        Ok(image::ImageReader::open(&path)?.with_guessed_format()?.decode()?)
                    })
                    .await??;
                    Ok(img)
                }
                .await;
                let _ = tx.send(Msg::Image(generation, index, res));
            });
        }
    }

    /// Codifica (en segundo plano) la página para el tamaño actual del área del lector.
    fn request_encode(&mut self, index: usize) {
        let size = (self.reader_area.width, self.reader_area.height);
        let (true, Some(r)) = (self.sixel && size.0 > 0 && size.1 > 0, self.reader.as_mut()) else { return };
        let Some(page) = r.images.get_mut(&index) else { return };
        if page.encoding == Some(size) || page.encoded.as_ref().is_some_and(|(s, _)| *s == size) {
            return;
        }
        page.encoding = Some(size);
        let (img, font, tx, generation) = (page.img.clone(), self.font, self.tx.clone(), self.generation);
        tokio::task::spawn_blocking(move || {
            let enc = render::encode(&img, size.0, size.1, font);
            let _ = tx.send(Msg::Encoded(generation, index, size, enc));
        });
    }

    /// Escribe el Sixel de la página actual si todavía no está en pantalla. Va por fuera de
    /// ratatui: ratatui no dibuja nada en esa zona, así que la imagen queda intacta.
    fn flush_sixel(&mut self) -> Result<()> {
        use ratatui::crossterm::{cursor::MoveTo, queue};
        use std::io::Write;
        if !self.sixel || self.screen != Screen::Reader {
            return Ok(());
        }
        let area = self.reader_area;
        let size = (area.width, area.height);
        let Some(r) = self.reader.as_ref() else { return Ok(()) };
        let Some((_, enc)) = r.images.get(&r.page).and_then(|p| p.encoded.as_ref()).filter(|(s, _)| *s == size) else {
            return Ok(());
        };
        let x = area.x + area.width.saturating_sub(enc.cols) / 2;
        let target = Rect::new(x, area.y, enc.cols, enc.rows);
        if self.sixel_drawn == Some((r.page, target)) {
            return Ok(());
        }
        let mut out = std::io::stdout().lock();
        queue!(out, MoveTo(target.x, target.y))?;
        out.write_all(enc.sixel.as_bytes())?;
        out.flush()?;
        self.sixel_drawn = Some((r.page, target));
        Ok(())
    }

    // ───────────────────────────── dibujo ─────────────────────────────

    fn draw(&mut self, f: &mut Frame) {
        match self.screen {
            Screen::Library => self.draw_library(f),
            Screen::Search => self.draw_search(f),
            Screen::Sources => self.draw_sources(f),
            Screen::Manga => self.draw_manga(f),
            Screen::Reader => self.draw_reader(f),
            Screen::AnilistPick => self.draw_anilist_pick(f),
        }
    }

    fn draw_library(&mut self, f: &mut Frame) {
        let [list_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let items: Vec<ListItem> = self
            .library
            .favorites
            .iter()
            .map(|fav| {
                let anilist = match &fav.anilist {
                    None => Span::raw("  AniList: –").fg(Color::DarkGray),
                    Some(_) if fav.needs_sync() => Span::raw("  AniList: ↻ sin sincronizar").fg(Color::Yellow),
                    Some(_) => Span::raw("  AniList: ✓").fg(Color::Green),
                };
                ListItem::new(Line::from(vec![
                    Span::raw("★ ").fg(Color::Yellow),
                    Span::raw(fav.manga.title.clone()).bold(),
                    Span::raw(format!("  {} · {} · cap. {}", fav.source_name, fav.status.label(), fav.progress)).fg(Color::Gray),
                    anilist,
                ]))
            })
            .collect();
        let title = format!(" mangaterm · biblioteca ({}) ", self.library.favorites.len());
        if items.is_empty() {
            let msg = "Todavía no hay favoritos.\n\nBuscá un manga (tecla /), abrilo y tocá 'f' para guardarlo acá.";
            f.render_widget(Paragraph::new(msg).fg(Color::DarkGray).block(block(&title)), list_area);
        } else {
            f.render_stateful_widget(highlight(List::new(items).block(block(&title))), list_area, &mut self.library_state);
        }
        status_line(f, status_area, &self.status, "enter abrir · d quitar · / buscar · q salir");
    }

    fn draw_anilist_pick(&mut self, f: &mut Frame) {
        let [list_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let manga = self.current.as_ref().map(|(_, m)| m.title.clone()).unwrap_or_default();
        let title = format!(" ¿Cuál es «{manga}» en AniList? ");
        match &self.anilist_matches {
            None => f.render_widget(Paragraph::new("buscando en AniList…").fg(Color::DarkGray).block(block(&title)), list_area),
            Some(list) => {
                let items: Vec<ListItem> = list
                    .iter()
                    .map(|m| {
                        let year = m.start_date.as_ref().and_then(|d| d.year).map(|y| y.to_string()).unwrap_or_default();
                        let chapters = m.chapters.map(|c| format!(" · {c} caps")).unwrap_or_default();
                        let english = m.title.english.clone().filter(|e| Some(e) != m.title.romaji.as_ref());
                        ListItem::new(Line::from(vec![
                            Span::raw(m.display_title()).bold(),
                            Span::raw(english.map(|e| format!("  ({e})")).unwrap_or_default()).fg(Color::Gray),
                            Span::raw(format!(
                                "  {} {year}{chapters}",
                                m.format.clone().unwrap_or_default()
                            ))
                            .fg(Color::DarkGray),
                        ]))
                    })
                    .collect();
                f.render_stateful_widget(highlight(List::new(items).block(block(&title))), list_area, &mut self.anilist_state);
            }
        }
        status_line(f, status_area, "elegí la obra para vincular", "↑↓ mover · enter vincular y sincronizar · esc cancelar");
    }

    fn draw_search(&mut self, f: &mut Frame) {
        let [input_area, list_area, status_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

        let input_style = if self.editing { Style::new().fg(Color::Yellow) } else { Style::new() };
        let cursor = if self.editing { "▏" } else { "" };
        f.render_widget(
            Paragraph::new(format!("{}{cursor}", self.input))
                .block(block(" mangaterm · buscar ").border_style(input_style)),
            input_area,
        );

        let items: Vec<ListItem> = self
            .groups
            .iter()
            .map(|g| {
                let names: Vec<&str> = g.entries.iter().map(|(s, _)| s.info.name.as_str()).take(4).collect();
                let more = g.entries.len().saturating_sub(4);
                let mut spans = vec![
                    Span::raw(g.title.clone()).bold(),
                    Span::raw("  "),
                    Span::raw(names.join(" · ")).fg(Color::DarkGray),
                ];
                if more > 0 {
                    spans.push(Span::raw(format!(" +{more}")).fg(Color::DarkGray));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let title = if self.query.is_empty() {
            " resultados ".to_string()
        } else {
            format!(" «{}» · {} títulos ", self.query, self.groups.len())
        };
        f.render_stateful_widget(highlight(List::new(items).block(block(&title))), list_area, &mut self.groups_state);

        let mut status = self.status.clone();
        if self.pending > 0 {
            status = format!("buscando… faltan {} fuentes", self.pending);
        } else if !self.query.is_empty() && !self.failures.is_empty() {
            status = format!("{status} · {} fuentes fallaron", self.failures.len());
        }
        let help = if self.editing { "enter buscar · ↓ resultados · ctrl+c salir" } else { "↑↓ mover · enter abrir · / buscar · q salir" };
        status_line(f, status_area, &status, help);
    }

    fn draw_sources(&mut self, f: &mut Frame) {
        let Some(g) = self.group.and_then(|g| self.groups.get(g)) else { return };
        let [list_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let items: Vec<ListItem> = g
            .entries
            .iter()
            .map(|(s, m)| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{:<28}", s.info.name)).bold(),
                    Span::raw(format!(" {:<4} ", s.info.lang)).fg(Color::Cyan),
                    Span::raw(m.title.clone()).fg(Color::DarkGray),
                ]))
            })
            .collect();
        let title = format!(" {} · {} fuentes ", g.title, g.entries.len());
        f.render_stateful_widget(highlight(List::new(items).block(block(&title))), list_area, &mut self.sources_state);
        status_line(f, status_area, "elegí una fuente", "↑↓ mover · enter abrir · esc volver");
    }

    fn draw_manga(&mut self, f: &mut Frame) {
        let Some((source, manga)) = &self.current else { return };
        let [info_area, list_area, status_area] =
            Layout::vertical([Constraint::Length(7), Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

        let m = self.details.as_ref().map(|d| &d.manga).unwrap_or(manga);
        let title = if m.title.is_empty() { &manga.title } else { &m.title };
        let mut lines = vec![Line::from(vec![
            Span::raw(title.clone()).bold().fg(Color::Yellow),
            Span::raw(format!("  {} ({})", source.info.name, source.info.lang)).fg(Color::DarkGray),
        ])];
        if let Some(author) = m.author.as_ref().filter(|a| !a.is_empty()) {
            lines.push(Line::from(format!("autor: {author}")).fg(Color::Cyan));
        }
        if let Some(genre) = m.genre.as_ref().filter(|g| !g.is_empty()) {
            lines.push(Line::from(genre.clone()).fg(Color::DarkGray));
        }
        match self.current_favorite() {
            Some(fav) => {
                let anilist = match &fav.anilist {
                    None => Span::raw("AniList: sin vincular ('a' para vincular)").fg(Color::DarkGray),
                    Some(link) if fav.needs_sync() => {
                        Span::raw(format!("AniList: {} · cambios sin sincronizar ('a')", link.title)).fg(Color::Yellow)
                    }
                    Some(link) => Span::raw(format!("AniList: {} · sincronizado", link.title)).fg(Color::Green),
                };
                lines.push(Line::from(vec![
                    Span::raw("★ favorito").fg(Color::Yellow),
                    Span::raw(format!(" · {} · leídos: {} · ", fav.status.label(), fav.progress)),
                    anilist,
                ]));
            }
            None => lines.push(Line::from("☆ no está en favoritos ('f' para agregar)").fg(Color::DarkGray)),
        }
        if let Some(desc) = &m.description {
            lines.push(Line::from(desc.replace(['\n', '\r'], " ")));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(block(" manga ")), info_area);

        match &self.details {
            None => f.render_widget(Paragraph::new("cargando capítulos…").block(block(" capítulos ")), list_area),
            Some(d) => {
                let items: Vec<ListItem> = d
                    .chapters
                    .iter()
                    .map(|c| {
                        let mut spans = vec![Span::raw(c.name.clone())];
                        if let Some(s) = c.scanlator.as_ref().filter(|s| !s.is_empty()) {
                            spans.push(Span::raw(format!("  {s}")).fg(Color::DarkGray));
                        }
                        ListItem::new(Line::from(spans))
                    })
                    .collect();
                let title = format!(" {} capítulos ", d.chapters.len());
                f.render_stateful_widget(highlight(List::new(items).block(block(&title))), list_area, &mut self.chapters_state);
            }
        }
        status_line(f, status_area, &self.status, "enter leer · c continuar · f favorito · e estado · a AniList · r recargar · esc");
    }

    fn draw_reader(&mut self, f: &mut Frame) {
        let [image_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        if image_area != self.reader_area {
            // Cambió el tamaño: recodificar lo que ya está descargado
            self.reader_area = image_area;
            let loaded: Vec<usize> = self.reader.as_ref().map(|r| r.images.keys().copied().collect()).unwrap_or_default();
            for index in loaded {
                self.request_encode(index);
            }
        }
        let size = (image_area.width, image_area.height);
        let (sixel, font) = (self.sixel, self.font);
        let Some(r) = self.reader.as_mut() else { return };

        let chapter = &r.chapters[r.chapter];
        let progress = if r.pages.is_empty() { "…".to_string() } else { format!("{}/{}", r.page + 1, r.pages.len()) };
        let mut info = format!("{} · {} · {}", r.manga.title, chapter.name, progress);

        let page = r.images.get_mut(&r.page);
        let ready = page.as_ref().is_some_and(|p| !sixel || p.encoded.as_ref().is_some_and(|(s, _)| *s == size));
        if let Some(err) = &r.error {
            f.render_widget(Paragraph::new(format!("error: {err}")).fg(Color::Red).wrap(Wrap { trim: true }), image_area);
        } else if let (true, Some(page)) = (ready, page) {
            if let Some((_, enc)) = page.encoded.as_ref().filter(|_| sixel) {
                // La imagen la escribe flush_sixel() después del frame; acá solo la info
                let kind = if enc.gray { "gris" } else { "color" };
                info += &format!(
                    "  [sixel {kind} {}×{}→{}×{} · celda {}×{}]",
                    enc.src.0, enc.src.1, enc.out.0, enc.out.1, font.0, font.1
                );
            } else if let Some(proto) = page.proto.as_mut() {
                let resize = ratatui_image::Resize::Fit(Some(FilterType::Lanczos3));
                // Centrar horizontalmente: la imagen se ajusta al alto y suele sobrar ancho
                let fit = proto.size_for(resize.clone(), image_area.as_size());
                let x = image_area.x + image_area.width.saturating_sub(fit.width) / 2;
                let area = Rect::new(x, image_area.y, fit.width.min(image_area.width), image_area.height);
                f.render_stateful_widget(StatefulImage::<StatefulProtocol>::default().resize(resize), area, proto);
                info += &format!("  [{:?} {}×{}]", self.picker.protocol_type(), page.img.width(), page.img.height());
            }
        } else {
            let msg = if r.pages.is_empty() { "cargando páginas…" } else { "cargando imagen…" };
            f.render_widget(Paragraph::new(msg).fg(Color::DarkGray), image_area);
        }
        status_line(f, status_area, &info, "←→ página · n/p capítulo · esc volver");
    }
}

// ───────────────────────────── helpers ─────────────────────────────

fn spawn_input(tx: mpsc::UnboundedSender<Msg>) {
    std::thread::spawn(move || loop {
        if !event::poll(Duration::from_millis(250)).unwrap_or(false) {
            if tx.is_closed() {
                break;
            }
            continue;
        }
        let msg = match event::read() {
            Ok(Event::Key(k)) => Msg::Key(k),
            Ok(Event::Resize(..)) => Msg::Resize,
            _ => continue,
        };
        if tx.send(msg).is_err() {
            break;
        }
    });
}

fn page_cache_path(source: &LoadedSource, chapter: &Chapter, index: usize) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&source.info.id, &chapter.url).hash(&mut h);
    let dir = paths::pages_cache().join(format!("{:016x}", h.finish()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{index:04}"))
}

fn block(title: &str) -> Block<'_> {
    Block::bordered().border_type(BorderType::Rounded).title(title)
}

fn highlight(list: List<'_>) -> List<'_> {
    list.highlight_style(Style::new().bg(Color::Rgb(40, 60, 90)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ")
}

fn status_line(f: &mut Frame, area: Rect, left: &str, right: &str) {
    let [l, r] = Layout::horizontal([Constraint::Min(1), Constraint::Length(right.chars().count() as u16 + 1)]).areas(area);
    f.render_widget(Paragraph::new(left.to_string()).fg(Color::Gray), l);
    f.render_widget(Paragraph::new(right.to_string()).fg(Color::DarkGray), r);
}

fn select_next(state: &mut ListState, len: usize) {
    if len > 0 {
        state.select(Some(state.selected().map_or(0, |i| (i + 1).min(len - 1))));
    }
}

fn select_prev(state: &mut ListState) {
    state.select(Some(state.selected().map_or(0, |i| i.saturating_sub(1))));
}

fn page_move(state: &mut ListState, len: usize, delta: isize) {
    if len > 0 {
        let i = state.selected().unwrap_or(0) as isize + delta;
        state.select(Some(i.clamp(0, len as isize - 1) as usize));
    }
}
