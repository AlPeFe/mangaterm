//! Interfaz de terminal: biblioteca / buscar → elegir fuente → capítulos → leer.
//!
//! Todo lo lento (host, búsquedas, descargas, decodificación, AniList) corre en tareas aparte y
//! vuelve como `Msg` al bucle principal, que es el único que toca el estado y dibuja.

use crate::anilist::{self, Credentials, Media, SyncResult};
use crate::anilist_auth;
use crate::backup::{self, Backup};
use crate::catalog;
use crate::reads::{self, ReadLog};
use crate::ext::Installed;
use crate::home::{Home, MenuItem};
use crate::host::{Host, LoadedSource};
use crate::library::{AniListLink, LastRead, Library, Status};
use crate::theme::{self, ERROR, FAINT, LAVENDER, MINT, MUTED, ON_PINK, PEACH, PINK, PINK_DIM, TEXT};
use crate::model::{Chapter, Manga, MangaDetails, Page};
use crate::paths;
use crate::render::{self, Encoded};
use crate::search::{self, SearchEvent};
use anyhow::Result;
use image::imageops::FilterType;
use image::DynamicImage;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};
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
    /// Paso de animación (~20 por segundo).
    Tick,
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
    AnilistLogin(Result<Credentials>),
    BackupRead(Result<(Backup, Vec<catalog::Extension>)>),
    /// Extensiones instaladas y cargadas tras importar un backup (y los errores, por nombre).
    BackupExtensions(Vec<Installed>, Vec<LoadedSource>, Vec<String>),
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
/// `HANTHO_PROTOCOL` (sixel|kitty|iterm2|halfblocks) y `HANTHO_FONT` (p. ej. 10x20) lo pisan.
fn make_picker() -> Picker {
    let font = std::env::var("HANTHO_FONT")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))));
    let forced = std::env::var("HANTHO_PROTOCOL").ok().and_then(|p| match p.as_str() {
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
enum ChapterInputKind {
    Filter,
    Goto,
}

struct ChapterInput {
    kind: ChapterInputKind,
    text: String,
}

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Home,
    Library,
    Extensions,
    AniList,
    Backup,
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
    anilist_user: Option<String>,
    /// Login por navegador en curso.
    anilist_login: bool,
    /// Client ID que se está escribiendo (configuración única, si el binario no trae uno).
    client_input: String,

    reads: ReadLog,
    /// Índices (en `details.chapters`) de los capítulos visibles: filtrados y en el orden elegido.
    chapter_view: Vec<usize>,
    chapter_filter: String,
    chapters_reversed: bool,
    /// Entrada de texto en la lista de capítulos: filtrar o ir a un número.
    chapter_input: Option<ChapterInput>,

    backup_files: Vec<std::path::PathBuf>,
    backup_state: ListState,
    backup_busy: bool,
    backup_report: Vec<String>,

    home: Home,
    /// Tamaño de pantalla del último dibujo (para animar los pétalos).
    screen_area: Rect,
    tick: u64,
    /// Hay que redibujar. Los ticks solo marcan esto en pantallas animadas.
    dirty: bool,
    installed: Vec<Installed>,
    extensions_state: ListState,
}

impl App {
    fn new(picker: Picker) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let font = picker.font_size();
        let library = Library::load();
        let has_favorites = !library.favorites.is_empty();
        let installed = crate::ext::list();
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
            screen: Screen::Home,
            status: "cargando extensiones…".into(),
            quit: false,
            clear: false,
            generation: 0,
            search_gen: 0,
            input: String::new(),
            editing: false,
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
            anilist_user: Credentials::load().map(|c| c.user_name),
            anilist_login: false,
            client_input: String::new(),
            reads: ReadLog::load(),
            chapter_view: Vec::new(),
            chapter_filter: String::new(),
            chapters_reversed: false,
            chapter_input: None,
            backup_files: Vec::new(),
            backup_state: ListState::default(),
            backup_busy: false,
            backup_report: Vec::new(),
            home: Home::new(),
            screen_area: Rect::default(),
            tick: 0,
            dirty: true,
            extensions_state: ListState::default().with_selected((!installed.is_empty()).then_some(0)),
            installed,
        }
    }

    async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        spawn_input(self.tx.clone());
        let tick_tx = self.tx.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_millis(50));
            while tick_tx.send(Msg::Tick).is_ok() {
                every.tick().await;
            }
        });
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
                self.dirty = true;
            }
            if std::mem::take(&mut self.dirty) {
                terminal.draw(|f| {
                    self.screen_area = f.area();
                    self.draw(f)
                })?;
                self.flush_sixel()?;
            }
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
        if !matches!(msg, Msg::Tick) {
            self.dirty = true;
        }
        match msg {
            Msg::Tick => {
                self.tick += 1;
                if self.screen == Screen::Home {
                    self.home.tick(self.screen_area);
                }
                // La sesión de AniList se inicia en otra terminal: detectarla sola
                if self.screen == Screen::AniList && self.tick % 40 == 0 {
                    self.anilist_user = Credentials::load().map(|c| c.user_name);
                }
                if self.animated() {
                    self.dirty = true;
                }
            }
            Msg::Key(key) => self.on_key(key),
            Msg::Resize => self.clear = true,
            Msg::HostReady(Ok((host, sources, errors))) => {
                self.status = format!("{} fuentes listas", sources.len())
                    + &if errors > 0 { format!(" ({errors} extensiones no han cargado)") } else { String::new() };
                if sources.is_empty() {
                    self.status = "no hay extensiones: instálalas con `hantho ext install --lang es`".into();
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
                    self.status = format!("{} capítulos", d.chapters.len());
                    self.details = Some(d);
                    self.chapter_filter.clear();
                    self.chapter_input = None;
                    self.rebuild_chapter_view();
                    match index {
                        Some(i) => self.select_chapter(i),
                        None => self.chapters_state.select((!self.chapter_view.is_empty()).then_some(0)),
                    }
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
                    self.status = "AniList no ha encontrado ese título".into();
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
            Msg::BackupRead(res) => self.on_backup_read(res),
            Msg::BackupExtensions(installed, sources, errors) => {
                self.backup_busy = false;
                let n = installed.len();
                self.sources.extend(sources);
                self.installed = crate::ext::list();
                self.backup_report.push(format!("✓ {n} extensiones instaladas y cargadas"));
                for e in errors {
                    self.backup_report.push(format!("✕ {e}"));
                }
                self.status = "importación terminada ✿".into();
            }
            Msg::AnilistLogin(res) => {
                self.anilist_login = false;
                match res {
                    Ok(creds) => {
                        self.status = format!("✓ conectado como @{}", creds.user_name);
                        self.anilist_user = Some(creds.user_name);
                    }
                    Err(e) => self.status = format!("✕ AniList: {e}"),
                }
            }
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
            Screen::Home => self.key_home(key),
            Screen::Extensions => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => self.screen = Screen::Home,
                KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.extensions_state),
                KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.extensions_state, self.installed.len()),
                _ => {}
            },
            Screen::AniList => self.key_anilist(key),
            Screen::Backup => self.key_backup(key),
            Screen::Library => self.key_library(key),
            Screen::Search => self.key_search(key),
            Screen::Sources => self.key_sources(key),
            Screen::Manga => self.key_manga(key),
            Screen::Reader => self.key_reader(key),
            Screen::AnilistPick => self.key_anilist_pick(key),
        }
    }

    /// Pantallas con algo en movimiento: solo ahí los ticks redibujan.
    /// (En el lector con la página ya en pantalla nunca: redibujar borraría el Sixel.)
    fn animated(&self) -> bool {
        match self.screen {
            Screen::Home => true,
            Screen::Search => self.editing || self.pending > 0,
            Screen::AnilistPick => self.anilist_matches.is_none(),
            Screen::AniList => self.anilist_login || anilist_auth::client_id().is_none(),
            Screen::Backup => self.backup_busy,
            Screen::Manga => self.details.is_none() || self.chapter_input.is_some(),
            Screen::Reader => self.reader.as_ref().is_some_and(|r| {
                r.error.is_none() && r.images.get(&r.page).is_none_or(|p| self.sixel && p.encoded.is_none())
            }),
            _ => false,
        }
    }

    fn key_anilist(&mut self, key: KeyEvent) {
        if self.anilist_login {
            if key.code == KeyCode::Esc {
                self.status = "login en curso: si has cerrado el navegador, se cancela solo a los 5 min".into();
            }
            return;
        }
        let configured = anilist_auth::client_id().is_some();
        match key.code {
            KeyCode::Esc => self.screen = Screen::Home,
            // Configuración única: escribir/pegar el client ID
            KeyCode::Char(c) if !configured && c.is_ascii_digit() => self.client_input.push(c),
            KeyCode::Backspace if !configured => {
                self.client_input.pop();
            }
            KeyCode::Char('d') if !configured => anilist::open_browser(anilist_auth::DEVELOPER_URL),
            KeyCode::Enter if !configured => match anilist_auth::save_client_id(&self.client_input) {
                Ok(()) => self.start_anilist_login(),
                Err(e) => self.status = format!("✕ {e}"),
            },
            KeyCode::Char('q') | KeyCode::Backspace => self.screen = Screen::Home,
            KeyCode::Char('l') | KeyCode::Enter => self.start_anilist_login(),
            KeyCode::Char('o') if self.anilist_user.is_some() => {
                Credentials::delete();
                self.anilist_user = None;
                self.status = "sesión de AniList cerrada".into();
            }
            _ => {}
        }
    }

    fn start_anilist_login(&mut self) {
        let Some(id) = anilist_auth::client_id() else { return };
        self.anilist_login = true;
        self.status = "se ha abierto el navegador: aprueba el acceso en AniList".into();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::AnilistLogin(anilist_auth::login(&id).await));
        });
    }

    fn key_home(&mut self, key: KeyEvent) {
        const ITEMS: usize = 6;
        let choice = match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.quit = true;
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.home.selected = (self.home.selected + ITEMS - 1) % ITEMS;
                return;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.home.selected = (self.home.selected + 1) % ITEMS;
                return;
            }
            KeyCode::Char(c @ '1'..='6') => c as usize - '1' as usize,
            KeyCode::Enter => self.home.selected,
            KeyCode::Char('/') => 1,
            _ => return,
        };
        self.home.selected = choice;
        match choice {
            0 => self.screen = Screen::Library,
            1 => {
                self.screen = Screen::Search;
                self.editing = true;
            }
            2 => {
                self.installed = crate::ext::list();
                self.screen = Screen::Extensions;
            }
            3 => {
                self.anilist_user = Credentials::load().map(|c| c.user_name);
                self.screen = Screen::AniList;
            }
            4 => {
                self.backup_files = backup::find_backups();
                self.backup_state.select(Some(0));
                self.screen = Screen::Backup;
            }
            _ => self.quit = true,
        }
    }

    fn key_library(&mut self, key: KeyEvent) {
        let len = self.library.favorites.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.screen = Screen::Home,
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
                KeyCode::Esc => self.screen = Screen::Home,
                KeyCode::Down | KeyCode::Tab if !self.groups.is_empty() => self.editing = false,
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.screen = Screen::Home,
            KeyCode::Char('/') | KeyCode::Char('s') => self.editing = true,
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
        if self.chapter_input.is_some() {
            self.key_chapter_input(key);
            return;
        }
        let len = self.chapter_view.len();
        match key.code {
            KeyCode::Char('/') if self.details.is_some() => {
                self.chapter_input = Some(ChapterInput { kind: ChapterInputKind::Filter, text: self.chapter_filter.clone() })
            }
            KeyCode::Char('#') | KeyCode::Char(':') if self.details.is_some() => {
                self.chapter_input = Some(ChapterInput { kind: ChapterInputKind::Goto, text: String::new() })
            }
            KeyCode::Char('o') => {
                let selected = self.selected_chapter();
                self.chapters_reversed = !self.chapters_reversed;
                self.rebuild_chapter_view();
                if let Some(i) = selected {
                    self.select_chapter(i);
                }
            }
            KeyCode::Char('m') => self.toggle_selected_read(),
            KeyCode::Char('M') => self.mark_read_up_to_selected(),
            KeyCode::Char('[') => page_move(&mut self.chapters_state, len, -100),
            KeyCode::Char(']') => page_move(&mut self.chapters_state, len, 100),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.generation += 1;
                self.screen = self.manga_back;
            }
            KeyCode::Char('f') => self.toggle_favorite(),
            KeyCode::Char('e') => {
                let Some(fav) = self.current_favorite_mut() else {
                    self.status = "primero añádelo a favoritos (f)".into();
                    return;
                };
                fav.status = fav.status.next();
                let label = fav.status.label();
                self.save_library();
                self.status = format!("estado: {label} (pulsa 'a' para enviarlo a AniList)");
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
                    _ => self.status = "no hay ninguna lectura que continuar".into(),
                }
            }
            KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.chapters_state),
            KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.chapters_state, len),
            KeyCode::PageUp => page_move(&mut self.chapters_state, len, -20),
            KeyCode::PageDown => page_move(&mut self.chapters_state, len, 20),
            KeyCode::Home | KeyCode::Char('g') => self.chapters_state.select((len > 0).then_some(0)),
            KeyCode::End | KeyCode::Char('G') => self.chapters_state.select(len.checked_sub(1)),
            KeyCode::Enter => {
                if let Some(i) = self.selected_chapter() {
                    self.open_chapter(i);
                }
            }
            _ => {}
        }
    }

    /// Escribiendo un filtro (`/`, en vivo) o un número de capítulo (`#`).
    fn key_chapter_input(&mut self, key: KeyEvent) {
        let Some(input) = self.chapter_input.as_mut() else { return };
        match key.code {
            KeyCode::Char(c) => input.text.push(c),
            KeyCode::Backspace => {
                input.text.pop();
            }
            KeyCode::Esc => {
                if input.kind == ChapterInputKind::Filter {
                    self.chapter_filter.clear();
                    self.rebuild_chapter_view();
                }
                self.chapter_input = None;
                return;
            }
            KeyCode::Enter => {
                let input = self.chapter_input.take().unwrap();
                if input.kind == ChapterInputKind::Goto {
                    self.goto_chapter_number(&input.text);
                }
                return;
            }
            _ => return,
        }
        if input.kind == ChapterInputKind::Filter {
            self.chapter_filter = input.text.clone();
            let selected = self.selected_chapter();
            self.rebuild_chapter_view();
            match selected.filter(|i| self.chapter_view.contains(i)) {
                Some(i) => self.select_chapter(i),
                None => self.chapters_state.select((!self.chapter_view.is_empty()).then_some(0)),
            }
        }
    }

    fn key_backup(&mut self, key: KeyEvent) {
        if self.backup_busy {
            return;
        }
        let len = self.backup_files.len() + 1;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => self.screen = Screen::Home,
            KeyCode::Up | KeyCode::Char('k') => select_prev(&mut self.backup_state),
            KeyCode::Down | KeyCode::Char('j') => select_next(&mut self.backup_state, len),
            KeyCode::Char('r') => self.backup_files = backup::find_backups(),
            KeyCode::Enter => match self.backup_state.selected() {
                Some(0) => self.export_backup(),
                Some(i) => {
                    if let Some(path) = self.backup_files.get(i - 1).cloned() {
                        self.backup_busy = true;
                        self.backup_report = vec![format!("leyendo {}…", path.display())];
                        let tx = self.tx.clone();
                        tokio::spawn(async move {
                            let res = async {
                                let backup = tokio::task::spawn_blocking(move || backup::read_file(&path)).await??;
                                Ok((backup, catalog::load().await?))
                            }
                            .await;
                            let _ = tx.send(Msg::BackupRead(res));
                        });
                    }
                }
                None => {}
            },
            _ => {}
        }
    }

    fn key_reader(&mut self, key: KeyEvent) {
        let Some(r) = self.reader.as_mut() else { return };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                // Al volver, dejar seleccionado el capítulo que se estaba leyendo
                let chapter = r.chapter;
                self.select_chapter(chapter);
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
            self.status = format!("no se ha podido guardar la biblioteca: {e}");
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
        self.status = if added { "★ añadido a favoritos".into() } else { "quitado de favoritos".into() };
        if added {
            self.library_state.select(Some(self.library.favorites.len() - 1));
        }
    }

    /// Sincroniza el favorito actual con AniList. La primera vez (o con `relink`) pide elegir la obra.
    fn anilist_sync(&mut self, relink: bool) {
        if Credentials::load().is_none() {
            self.status = "conecta tu cuenta de AniList (tecla l)".into();
            self.screen = Screen::AniList;
            return;
        }
        let Some(fav) = self.current_favorite().cloned() else {
            self.status = "primero añádelo a favoritos (f)".into();
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
                    if r.kept_remote_progress { " (AniList ya tenía más progreso; se ha conservado)" } else { "" }
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
        if last_page && self.reads.mark(&reads::key(&source_id, &url), &chapter) {
            self.save_reads();
        }
        let Some(fav) = self.library.get_mut(&source_id, &url) else { return };
        fav.last_read = Some(LastRead { chapter_url: chapter.url.clone(), chapter_name: chapter.name.clone(), page });
        if last_page {
            let before = fav.progress;
            fav.mark_chapter_read(&chapter);
            if fav.progress != before {
                self.save_library();
                self.auto_sync(&source_id, &url);
            }
        }
    }

    fn save_reads(&mut self) {
        if let Err(e) = self.reads.save() {
            self.status = format!("no se ha podido guardar el registro de leídos: {e}");
        }
    }

    /// Envía el progreso a AniList sin preguntar, si el favorito está vinculado y hay sesión.
    fn auto_sync(&mut self, source_id: &str, url: &str) {
        let (Some(creds), Some(fav)) = (Credentials::load(), self.library.favorites.iter().find(|f| f.is(source_id, url))) else {
            return;
        };
        let Some(link) = fav.anilist.clone().filter(|_| fav.needs_sync()) else { return };
        let (status, progress) = (fav.status.as_anilist(), fav.progress);
        let (tx, source_id, url) = (self.tx.clone(), source_id.to_string(), url.to_string());
        tokio::spawn(async move {
            let res = anilist::sync(&creds, link.media_id, status, progress).await;
            let _ = tx.send(Msg::AnilistSynced(source_id, url, res));
        });
    }

    // ───────────────────────────── lista de capítulos ─────────────────────────────

    fn current_key(&self) -> Option<String> {
        self.current.as_ref().map(|(src, m)| reads::key(&src.info.id, &m.url))
    }

    fn rebuild_chapter_view(&mut self) {
        let Some(d) = &self.details else {
            self.chapter_view.clear();
            return;
        };
        let filter = self.chapter_filter.trim().to_lowercase();
        let mut view: Vec<usize> = (0..d.chapters.len())
            .filter(|&i| {
                let c = &d.chapters[i];
                filter.is_empty() || c.name.to_lowercase().contains(&filter) || format_number(c.chapter_number) == filter
            })
            .collect();
        if self.chapters_reversed {
            view.reverse();
        }
        self.chapter_view = view;
    }

    fn selected_chapter(&self) -> Option<usize> {
        self.chapters_state.selected().and_then(|v| self.chapter_view.get(v).copied())
    }

    fn select_chapter(&mut self, index: usize) {
        if let Some(pos) = self.chapter_view.iter().position(|&i| i == index) {
            self.chapters_state.select(Some(pos));
        }
    }

    /// Salta al capítulo con ese número (o el más cercano), quitando el filtro si hace falta.
    fn goto_chapter_number(&mut self, text: &str) {
        let Ok(target) = text.trim().replace(',', ".").parse::<f32>() else {
            self.status = format!("«{text}» no es un número de capítulo");
            return;
        };
        let Some(d) = &self.details else { return };
        let best = d
            .chapters
            .iter()
            .enumerate()
            .filter(|(_, c)| c.chapter_number >= 0.0)
            .min_by(|(_, a), (_, b)| (a.chapter_number - target).abs().total_cmp(&(b.chapter_number - target).abs()))
            .map(|(i, c)| (i, c.name.clone()));
        match best {
            Some((i, name)) => {
                if !self.chapter_view.contains(&i) {
                    self.chapter_filter.clear();
                    self.rebuild_chapter_view();
                }
                self.select_chapter(i);
                self.status = format!("→ {name}");
            }
            None => self.status = "los capítulos de esta fuente no tienen número".into(),
        }
    }

    /// `m`: marca o desmarca el capítulo seleccionado.
    fn toggle_selected_read(&mut self) {
        let (Some(i), Some(key)) = (self.selected_chapter(), self.current_key()) else { return };
        let Some(chapter) = self.details.as_ref().map(|d| d.chapters[i].clone()) else { return };
        if self.reads.is_read(&key, &chapter.url) {
            self.reads.unmark(&key, &chapter.url);
            self.status = format!("○ {} sin leer", chapter.name);
        } else {
            self.reads.mark(&key, &chapter);
            self.status = format!("✓ {} leído", chapter.name);
        }
        self.save_reads();
        self.after_manual_marks(&key);
    }

    /// `M`: marca como leídos el seleccionado y todos los anteriores.
    fn mark_read_up_to_selected(&mut self) {
        let (Some(i), Some(key)) = (self.selected_chapter(), self.current_key()) else { return };
        let Some(d) = self.details.as_ref() else { return };
        let first = d.chapters.first().map_or(0.0, |c| c.chapter_number);
        let last = d.chapters.last().map_or(0.0, |c| c.chapter_number);
        // Las fuentes suelen listar del más nuevo al más viejo: los anteriores están "abajo"
        let older: Vec<usize> = if first >= last { (i..d.chapters.len()).collect() } else { (0..=i).collect() };
        let chapters: Vec<_> = older.iter().map(|&j| d.chapters[j].clone()).collect();
        let marked = chapters.iter().filter(|c| self.reads.mark(&key, c)).count();
        self.status = format!("✓ {marked} capítulos marcados como leídos");
        self.save_reads();
        self.after_manual_marks(&key);
    }

    /// Tras marcar a mano: actualizar el progreso del favorito y AniList.
    fn after_manual_marks(&mut self, key: &str) {
        let Some((src, manga)) = self.current.clone() else { return };
        let max = self.reads.max_number(key);
        let Some(fav) = self.library.get_mut(&src.info.id, &manga.url) else { return };
        if fav.progress != max {
            fav.progress = max;
            self.save_library();
            self.auto_sync(&src.info.id, &manga.url);
        }
    }

    // ───────────────────────────── backups ─────────────────────────────

    fn export_backup(&mut self) {
        let path = backup::default_export_path();
        let data = backup::export(&self.library, &self.reads);
        let chapters: usize = data.manga.iter().map(|m| m.chapters.len()).sum();
        self.backup_report = match backup::write_file(&data, &path) {
            Ok(()) => vec![
                format!("✓ exportados {} favoritos y {chapters} capítulos leídos", data.manga.len()),
                format!("  {}", path.display()),
                "  en Mihon: Más → Copia de seguridad → Restaurar".into(),
            ],
            Err(e) => vec![format!("✕ {e}")],
        };
    }

    fn on_backup_read(&mut self, res: Result<(Backup, Vec<catalog::Extension>)>) {
        let (data, catalog) = match res {
            Ok(v) => v,
            Err(e) => {
                self.backup_busy = false;
                self.backup_report = vec![format!("✕ {e}")];
                return;
            }
        };
        let installed: Vec<String> = self.installed.iter().map(|i| i.pkg.clone()).collect();
        let s = backup::import(&data, &catalog, &installed, &mut self.library, &mut self.reads);
        self.save_library();
        self.save_reads();
        self.backup_report = vec![
            format!("✓ {} favoritos nuevos · {} ya estaban", s.added, s.merged),
            format!("✓ {} capítulos marcados como leídos", s.chapters_read),
            format!("✓ {} vinculados con AniList", s.anilist_links),
        ];
        if !s.unknown_sources.is_empty() {
            self.backup_report.push(format!("! sin extensión en Keiyoushi: {}", s.unknown_sources.join(", ")));
        }
        let (Some(host), false) = (self.host.clone(), s.needed_extensions.is_empty()) else {
            self.backup_busy = false;
            self.status = "importación terminada ✿".into();
            return;
        };
        let names: Vec<_> = s.needed_extensions.iter().map(|e| e.name.clone()).collect();
        self.backup_report.push(format!("instalando {} extensiones: {}…", names.len(), names.join(", ")));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let (mut ok, mut errors) = (Vec::new(), Vec::new());
            for ext in &s.needed_extensions {
                match crate::ext::install(ext).await {
                    Ok(i) => ok.push(i),
                    Err(e) => errors.push(format!("{}: {e}", ext.name)),
                }
            }
            let (sources, load_errors) = host.load_all(&ok).await;
            errors.extend(load_errors.into_iter().map(|(n, e)| format!("{n}: {}", search::short_error(&e))));
            let _ = tx.send(Msg::BackupExtensions(ok, sources, errors));
        });
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
            Screen::Home => self.draw_home(f),
            Screen::Library => self.draw_library(f),
            Screen::Search => self.draw_search(f),
            Screen::Sources => self.draw_sources(f),
            Screen::Manga => self.draw_manga(f),
            Screen::Reader => self.draw_reader(f),
            Screen::AnilistPick => self.draw_anilist_pick(f),
            Screen::Extensions => self.draw_extensions(f),
            Screen::Backup => self.draw_backup(f),
            Screen::AniList => self.draw_anilist(f),
        }
    }

    fn menu_items(&self) -> Vec<MenuItem> {
        let favs = self.library.favorites.len();
        let pending = self.library.favorites.iter().filter(|f| f.needs_sync()).count();
        let sources = if self.host.is_some() { format!("{} fuentes", self.sources.len()) } else { "cargando…".into() };
        let anilist = match &self.anilist_user {
            Some(name) if pending > 0 => format!("@{name} · {pending} sin sincronizar"),
            Some(name) => format!("@{name}"),
            None => "sin sesión".into(),
        };
        vec![
            MenuItem { icon: "♡", label: "Biblioteca", detail: format!("{favs} favoritos") },
            MenuItem { icon: "⌕", label: "Buscar", detail: sources },
            MenuItem { icon: "✦", label: "Extensiones", detail: format!("{} instaladas", self.installed.len()) },
            MenuItem { icon: "☁", label: "AniList", detail: anilist },
            MenuItem { icon: "⇄", label: "Backup Mihon", detail: "importar / exportar".into() },
            MenuItem { icon: "✕", label: "Salir", detail: String::new() },
        ]
    }

    fn draw_home(&mut self, f: &mut Frame) {
        let items = self.menu_items();
        let footer = if self.host.is_none() {
            Line::from(vec![
                Span::raw(theme::spinner(self.tick)).fg(PINK),
                Span::raw(" cargando extensiones…").fg(MUTED),
            ])
        } else {
            Line::from(vec![
                Span::raw("↑↓").fg(PINK).bold(),
                Span::raw(" elegir  ").fg(MUTED),
                Span::raw("enter").fg(PINK).bold(),
                Span::raw(" abrir  ").fg(MUTED),
                Span::raw("1-5").fg(PINK).bold(),
                Span::raw(" atajos  ").fg(MUTED),
                Span::raw("q").fg(PINK).bold(),
                Span::raw(" salir").fg(MUTED),
            ])
        };
        self.home.draw(f, &items, footer);
    }

    fn draw_library(&mut self, f: &mut Frame) {
        let [main, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let title = format!("♡ biblioteca · {}", self.library.favorites.len());
        if self.library.favorites.is_empty() {
            let msg = vec![
                Line::raw(""),
                Line::from("todavía no tienes favoritos").fg(TEXT).centered(),
                Line::raw(""),
                Line::from(vec![
                    Span::raw("busca un manga con ").fg(MUTED),
                    Span::raw("/").fg(PINK).bold(),
                    Span::raw(", ábrelo y pulsa ").fg(MUTED),
                    Span::raw("f").fg(PINK).bold(),
                    Span::raw(" para guardarlo aquí ✿").fg(MUTED),
                ])
                .centered(),
            ];
            f.render_widget(Paragraph::new(msg).block(theme::block_focused(&title)), main);
        } else {
            let [list_area, card_area] =
                Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
            let items: Vec<ListItem> = self
                .library
                .favorites
                .iter()
                .map(|fav| {
                    let sync = match &fav.anilist {
                        None => Span::raw("  "),
                        Some(_) if fav.needs_sync() => Span::raw(" ↻").fg(PEACH),
                        Some(_) => Span::raw(" ✓").fg(MINT),
                    };
                    ListItem::new(Line::from(vec![
                        Span::raw(fav.manga.title.clone()).fg(TEXT),
                        Span::raw(format!("  cap. {}", fav.progress)).fg(MUTED),
                        sync,
                    ]))
                })
                .collect();
            f.render_stateful_widget(
                theme::highlight(List::new(items).block(theme::block_focused(&title))),
                list_area,
                &mut self.library_state,
            );
            if let Some(fav) = self.library_state.selected().and_then(|i| self.library.favorites.get(i)) {
                f.render_widget(favorite_card(fav), card_area);
            }
        }
        theme::status_bar(f, status_area, "BIBLIOTECA", &self.status, &[("enter", "abrir"), ("d", "quitar"), ("/", "buscar"), ("esc", "menú")]);
    }

    fn draw_extensions(&mut self, f: &mut Frame) {
        let [main, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let items: Vec<ListItem> = self
            .installed
            .iter()
            .map(|e| {
                let (label, color) = match e.content_warning {
                    3 => ("nsfw", ERROR),
                    2 => ("mixto", PEACH),
                    _ => ("apto", MINT),
                };
                let loaded = self.sources.iter().any(|s| s.info.name == e.name);
                ListItem::new(Line::from(vec![
                    Span::raw(if loaded { "● " } else { "○ " }).fg(if loaded { MINT } else { FAINT }),
                    Span::raw(format!("{:<34}", e.name)).fg(TEXT),
                    Span::raw(format!("{:<9}", e.version_name)).fg(MUTED),
                    Span::raw(format!("{:<8}", e.langs.join(","))).fg(LAVENDER),
                    Span::raw(label).fg(color),
                ]))
            })
            .collect();
        let title = format!("✦ extensiones · {} instaladas · {} fuentes cargadas", self.installed.len(), self.sources.len());
        f.render_stateful_widget(theme::highlight(List::new(items).block(theme::block_focused(&title))), main, &mut self.extensions_state);
        theme::status_bar(
            f,
            status_area,
            "EXTENSIONES",
            "instala más con `hantho ext install <nombre>` o `--lang es`",
            &[("↑↓", "mover"), ("esc", "menú")],
        );
    }

    fn draw_anilist(&mut self, f: &mut Frame) {
        let [main, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let [card] = Layout::horizontal([Constraint::Length(72.min(main.width))]).flex(Flex::Center).areas(main);
        let [card] = Layout::vertical([Constraint::Length(16.min(card.height))]).flex(Flex::Center).areas(card);
        let linked = self.library.favorites.iter().filter(|f| f.anilist.is_some()).count();
        let pending = self.library.favorites.iter().filter(|f| f.needs_sync()).count();
        let mut lines = vec![Line::raw("")];
        match &self.anilist_user {
            Some(name) => {
                lines.push(Line::from(vec![Span::raw("  conectado como ").fg(MUTED), Span::raw(format!("@{name}")).fg(PINK).bold()]));
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![Span::raw("  ✓ ").fg(MINT), Span::raw(format!("{linked} favoritos vinculados")).fg(TEXT)]));
                let pending_line = if pending > 0 {
                    vec![Span::raw("  ↻ ").fg(PEACH), Span::raw(format!("{pending} con cambios sin sincronizar")).fg(TEXT)]
                } else {
                    vec![Span::raw("  ✓ ").fg(MINT), Span::raw("todo sincronizado").fg(TEXT)]
                };
                lines.push(Line::from(pending_line));
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![
                    Span::raw("  para sincronizar, abre un favorito y pulsa ").fg(MUTED),
                    Span::raw("a").fg(PINK).bold(),
                ]));
            }
            None if self.anilist_login => {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {} ", theme::spinner(self.tick))).fg(PINK),
                    Span::raw("esperando que apruebes en el navegador…").fg(TEXT),
                ]));
                lines.push(Line::raw(""));
                lines.push(Line::from("  en la página de AniList pulsa «Approve»;").fg(MUTED));
                lines.push(Line::from("  Han Thỏ recibe la sesión solo, no hace falta copiar nada ✿").fg(MUTED));
            }
            None if anilist_auth::client_id().is_some() => {
                lines.push(Line::from("  sin sesión").fg(TEXT));
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![
                    Span::raw("  pulsa ").fg(MUTED),
                    Span::raw("l").fg(PINK).bold(),
                    Span::raw(" para conectar tu cuenta: se abre el navegador,").fg(MUTED),
                ]));
                lines.push(Line::from("  apruebas el acceso y listo.").fg(MUTED));
            }
            None => {
                let cursor = if self.tick % 16 < 9 { "▏" } else { " " };
                lines.push(Line::from("  configuración única: registrar Han Thỏ en AniList").fg(TEXT));
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![
                    Span::raw("  1. pulsa ").fg(MUTED),
                    Span::raw("d").fg(PINK).bold(),
                    Span::raw(" (abre anilist.co/settings/developer) → Create New Client").fg(MUTED),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("  2. Name: ").fg(MUTED),
                    Span::raw("Han Thỏ").fg(TEXT),
                    Span::raw("  ·  Redirect URL: ").fg(MUTED),
                    Span::raw(anilist_auth::REDIRECT).fg(LAVENDER),
                ]));
                lines.push(Line::from("  3. pega aquí el ID del client y pulsa enter:").fg(MUTED));
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![
                    Span::raw("     ID: ").fg(MUTED),
                    Span::raw(self.client_input.clone()).fg(PINK).bold(),
                    Span::raw(cursor).fg(PINK),
                ]));
            }
        }
        f.render_widget(Clear, card);
        f.render_widget(Paragraph::new(lines).block(theme::block_focused("☁ anilist")), card);
        let keys: &[(&str, &str)] = match (&self.anilist_user, anilist_auth::client_id().is_some()) {
            _ if self.anilist_login => &[("esc", "menú")],
            (Some(_), _) => &[("l", "reconectar"), ("o", "cerrar sesión"), ("esc", "menú")],
            (None, true) => &[("l", "conectar"), ("esc", "menú")],
            (None, false) => &[("d", "abrir AniList"), ("enter", "guardar y conectar"), ("esc", "menú")],
        };
        theme::status_bar(f, status_area, "ANILIST", &self.status, keys);
    }

    fn draw_backup(&mut self, f: &mut Frame) {
        let [main, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let [list_area, report_area] = Layout::vertical([Constraint::Min(6), Constraint::Length(8)]).areas(main);
        let mut items = vec![ListItem::new(Line::from(vec![
            Span::raw("⇪ ").fg(MINT),
            Span::raw("Exportar biblioteca").fg(TEXT).bold(),
            Span::raw(format!("  {} favoritos → Descargas (.tachibk)", self.library.favorites.len())).fg(MUTED),
        ]))];
        for path in &self.backup_files {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
            items.push(ListItem::new(Line::from(vec![
                Span::raw("⇩ ").fg(LAVENDER),
                Span::raw("Importar ").fg(MUTED),
                Span::raw(name).fg(TEXT).bold(),
                Span::raw(format!("  {dir}")).fg(FAINT),
            ])));
        }
        f.render_stateful_widget(
            theme::highlight(List::new(items).block(theme::block_focused("⇄ backup de Mihon"))),
            list_area,
            &mut self.backup_state,
        );

        let mut lines: Vec<Line> = if self.backup_report.is_empty() {
            let hint = if self.backup_files.is_empty() {
                "no se han encontrado backups: en Mihon, Más → Copia de seguridad → Crear, y deja el .tachibk en Descargas o Documentos (r para buscar de nuevo)"
            } else {
                "importar añade favoritos, capítulos leídos y vínculos con AniList; instala solas las extensiones que falten"
            };
            vec![Line::from(hint).fg(MUTED)]
        } else {
            self.backup_report
                .iter()
                .map(|l| {
                    let color = if l.starts_with('✕') { ERROR } else if l.starts_with('!') { PEACH } else if l.starts_with('✓') { MINT } else { MUTED };
                    Line::from(l.clone()).fg(color)
                })
                .collect()
        };
        if self.backup_busy {
            lines.push(Line::from(vec![Span::raw(theme::spinner(self.tick)).fg(PINK), Span::raw(" trabajando…").fg(MUTED)]));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(theme::block("resultado")), report_area);
        theme::status_bar(f, status_area, "BACKUP", &self.status, &[("enter", "elegir"), ("r", "buscar archivos"), ("esc", "menú")]);
    }

    fn draw_anilist_pick(&mut self, f: &mut Frame) {
        let [list_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let manga = self.current.as_ref().map(|(_, m)| m.title.clone()).unwrap_or_default();
        let title = format!("☁ ¿cuál es «{manga}» en AniList?");
        match &self.anilist_matches {
            None => f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw(theme::spinner(self.tick)).fg(PINK),
                    Span::raw(" buscando en AniList…").fg(MUTED),
                ]))
                .block(theme::block_focused(&title)),
                list_area,
            ),
            Some(list) => {
                let items: Vec<ListItem> = list
                    .iter()
                    .map(|m| {
                        let year = m.start_date.as_ref().and_then(|d| d.year).map(|y| y.to_string()).unwrap_or_default();
                        let chapters = m.chapters.map(|c| format!(" · {c} caps")).unwrap_or_default();
                        let english = m.title.english.clone().filter(|e| Some(e) != m.title.romaji.as_ref());
                        ListItem::new(Line::from(vec![
                            Span::raw(m.display_title()).fg(TEXT).bold(),
                            Span::raw(english.map(|e| format!("  ({e})")).unwrap_or_default()).fg(LAVENDER),
                            Span::raw(format!("  {} {year}{chapters}", m.format.clone().unwrap_or_default())).fg(MUTED),
                        ]))
                    })
                    .collect();
                f.render_stateful_widget(
                    theme::highlight(List::new(items).block(theme::block_focused(&title))),
                    list_area,
                    &mut self.anilist_state,
                );
            }
        }
        theme::status_bar(f, status_area, "ANILIST", "elige la obra que quieres vincular", &[("enter", "vincular y sincronizar"), ("esc", "cancelar")]);
    }

    fn draw_search(&mut self, f: &mut Frame) {
        let [input_area, list_area, status_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

        let input = if self.input.is_empty() && self.editing {
            Line::from(vec![Span::raw("▏").fg(PINK), Span::raw("¿qué te apetece leer hoy?").fg(FAINT).italic()])
        } else {
            let cursor = if self.editing && self.tick % 16 < 9 { "▏" } else { " " };
            Line::from(vec![Span::raw(self.input.clone()).fg(TEXT), Span::raw(cursor).fg(PINK)])
        };
        let input_block = if self.editing { theme::block_focused("⌕ buscar") } else { theme::block("⌕ buscar") };
        f.render_widget(Paragraph::new(input).block(input_block), input_area);

        let items: Vec<ListItem> = self
            .groups
            .iter()
            .map(|g| {
                let names: Vec<&str> = g.entries.iter().map(|(s, _)| s.info.name.as_str()).take(3).collect();
                let more = g.entries.len().saturating_sub(3);
                let mut spans = vec![
                    Span::raw(g.title.clone()).fg(TEXT).bold(),
                    Span::raw("  "),
                    Span::raw(names.join(" · ")).fg(MUTED),
                ];
                if more > 0 {
                    spans.push(Span::raw(format!(" +{more}")).fg(LAVENDER));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let title = if self.query.is_empty() {
            "resultados".to_string()
        } else {
            format!("«{}» · {} títulos", self.query, self.groups.len())
        };
        let list_block = if self.editing { theme::block(&title) } else { theme::block_focused(&title) };
        f.render_stateful_widget(theme::highlight(List::new(items).block(list_block)), list_area, &mut self.groups_state);

        let mut status = self.status.clone();
        if self.pending > 0 {
            status = format!("{} buscando… faltan {} fuentes", theme::spinner(self.tick), self.pending);
        } else if !self.query.is_empty() && !self.failures.is_empty() {
            status = format!("{status} · {} fuentes han fallado", self.failures.len());
        }
        let keys: &[(&str, &str)] = if self.editing {
            &[("enter", "buscar"), ("↓", "resultados"), ("esc", "menú")]
        } else {
            &[("enter", "abrir"), ("/", "editar"), ("b", "biblioteca"), ("esc", "menú")]
        };
        theme::status_bar(f, status_area, "BUSCAR", &status, keys);
    }

    fn draw_sources(&mut self, f: &mut Frame) {
        let Some(g) = self.group.and_then(|g| self.groups.get(g)) else { return };
        let [list_area, status_area] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let items: Vec<ListItem> = g
            .entries
            .iter()
            .map(|(s, m)| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{:<28}", s.info.name)).fg(TEXT).bold(),
                    Span::raw(format!(" {:<4} ", s.info.lang)).fg(LAVENDER),
                    Span::raw(m.title.clone()).fg(MUTED),
                ]))
            })
            .collect();
        let title = format!("{} · en {} fuentes", g.title, g.entries.len());
        f.render_stateful_widget(theme::highlight(List::new(items).block(theme::block_focused(&title))), list_area, &mut self.sources_state);
        theme::status_bar(f, status_area, "FUENTE", "elige de dónde leerlo", &[("enter", "abrir"), ("esc", "volver")]);
    }

    fn draw_manga(&mut self, f: &mut Frame) {
        let Some((source, manga)) = &self.current else { return };
        let [info_area, list_area, status_area] =
            Layout::vertical([Constraint::Length(8), Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

        let m = self.details.as_ref().map(|d| &d.manga).unwrap_or(manga);
        let title = if m.title.is_empty() { &manga.title } else { &m.title };
        let mut lines = vec![Line::from(vec![
            Span::raw(title.clone()).fg(PINK).bold(),
            Span::raw(format!("  {} · {}", source.info.name, source.info.lang)).fg(MUTED),
        ])];
        let mut meta = Vec::new();
        if let Some(author) = m.author.as_ref().filter(|a| !a.is_empty()) {
            meta.push(Span::raw(format!("✎ {author}")).fg(LAVENDER));
        }
        if let Some(genre) = m.genre.as_ref().filter(|g| !g.is_empty()) {
            meta.push(Span::raw(format!("   {genre}")).fg(MUTED));
        }
        if !meta.is_empty() {
            lines.push(Line::from(meta));
        }
        match self.current_favorite() {
            Some(fav) => {
                let anilist = match &fav.anilist {
                    None => Span::raw("  ☁ sin vincular").fg(FAINT),
                    Some(_) if fav.needs_sync() => Span::raw("  ☁ ↻ cambios sin sincronizar").fg(PEACH),
                    Some(link) => Span::raw(format!("  ☁ ✓ {}", link.title)).fg(MINT),
                };
                lines.push(Line::from(vec![
                    Span::raw(" ♥ favorito ").bg(PINK).fg(ON_PINK).bold(),
                    Span::raw(" "),
                    Span::raw(format!(" {} ", fav.status.label())).bg(theme::status_color(fav.status)).fg(ON_PINK),
                    Span::raw(format!("  leídos: {}", fav.progress)).fg(TEXT),
                    anilist,
                ]));
            }
            None => lines.push(Line::from(vec![
                Span::raw("♡ ").fg(FAINT),
                Span::raw("no está en favoritos · ").fg(MUTED),
                Span::raw("f").fg(PINK).bold(),
                Span::raw(" para añadirlo").fg(MUTED),
            ])),
        }
        if let Some(desc) = &m.description {
            lines.push(Line::from(desc.replace(['\n', '\r'], " ")).fg(MUTED));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(theme::block("manga")), info_area);

        let last_read = self.current_favorite().and_then(|f| f.last_read.as_ref()).map(|l| l.chapter_url.clone());
        let read_key = self.current_key().unwrap_or_default();
        match &self.details {
            None => f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw(theme::spinner(self.tick)).fg(PINK),
                    Span::raw(" cargando capítulos…").fg(MUTED),
                ]))
                .block(theme::block_focused("capítulos")),
                list_area,
            ),
            Some(d) => {
                let items: Vec<ListItem> = self
                    .chapter_view
                    .iter()
                    .map(|&i| {
                        let c = &d.chapters[i];
                        let read = self.reads.is_read(&read_key, &c.url);
                        let marker = if last_read.as_deref() == Some(c.url.as_str()) {
                            Span::raw("▸ ").fg(PINK)
                        } else if read {
                            Span::raw("✓ ").fg(PINK_DIM)
                        } else {
                            Span::raw("  ")
                        };
                        // Los leídos se apagan; los pendientes quedan en primer plano
                        let mut spans = vec![marker, Span::raw(c.name.clone()).fg(if read { FAINT } else { TEXT })];
                        if let Some(s) = c.scanlator.as_ref().filter(|s| !s.is_empty()) {
                            spans.push(Span::raw(format!("  {s}")).fg(FAINT));
                        }
                        ListItem::new(Line::from(spans))
                    })
                    .collect();
                let read_count = d.chapters.iter().filter(|c| self.reads.is_read(&read_key, &c.url)).count();
                let mut title = format!("capítulos · {} · {read_count} leídos", d.chapters.len());
                if self.chapters_reversed {
                    title += " · ↑ viejos primero";
                }
                match &self.chapter_input {
                    Some(input) => {
                        let label = if input.kind == ChapterInputKind::Filter { "filtrar" } else { "ir al capítulo" };
                        let cursor = if self.tick % 16 < 9 { "▏" } else { " " };
                        title = format!("{label}: {}{cursor}", input.text);
                    }
                    None if !self.chapter_filter.is_empty() => {
                        title += &format!(" · filtro «{}» ({})", self.chapter_filter, self.chapter_view.len());
                    }
                    None => {}
                }
                f.render_stateful_widget(
                    theme::highlight(List::new(items).block(theme::block_focused(&title))),
                    list_area,
                    &mut self.chapters_state,
                );
            }
        }
        theme::status_bar(
            f,
            status_area,
            "MANGA",
            &self.status,
            &if self.chapter_input.is_some() {
                vec![("enter", "aceptar"), ("esc", "cancelar")]
            } else {
                vec![("enter", "leer"), ("m/M", "leído"), ("/", "filtrar"), ("#", "ir a"), ("o", "orden"), ("c", "seguir"), ("f", "fav"), ("a", "anilist")]
            },
        );
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
        let (sixel, font, tick) = (self.sixel, self.font, self.tick);
        let Some(r) = self.reader.as_mut() else { return };

        let chapter = &r.chapters[r.chapter];
        let progress = if r.pages.is_empty() { "…".to_string() } else { format!("{}/{}", r.page + 1, r.pages.len()) };
        let mut info = format!("{} · {} · {}", r.manga.title, chapter.name, progress);

        let page = r.images.get_mut(&r.page);
        let ready = page.as_ref().is_some_and(|p| !sixel || p.encoded.as_ref().is_some_and(|(s, _)| *s == size));
        if let Some(err) = &r.error {
            f.render_widget(Paragraph::new(format!("✕ {err}")).fg(ERROR).wrap(Wrap { trim: true }).centered(), image_area);
        } else if let (true, Some(page)) = (ready, page) {
            if let Some((_, enc)) = page.encoded.as_ref().filter(|_| sixel) {
                // La imagen la escribe flush_sixel() después del frame; aquí solo la info
                let kind = if enc.gray { "gris" } else { "color" };
                info += &format!("  · sixel {kind} {}×{} · celda {}×{}", enc.out.0, enc.out.1, font.0, font.1);
            } else if let Some(proto) = page.proto.as_mut() {
                let resize = ratatui_image::Resize::Fit(Some(FilterType::Lanczos3));
                // Centrar horizontalmente: la imagen se ajusta al alto y suele sobrar ancho
                let fit = proto.size_for(resize.clone(), image_area.as_size());
                let x = image_area.x + image_area.width.saturating_sub(fit.width) / 2;
                let area = Rect::new(x, image_area.y, fit.width.min(image_area.width), image_area.height);
                f.render_stateful_widget(StatefulImage::<StatefulProtocol>::default().resize(resize), area, proto);
                info += &format!("  · {:?}", self.picker.protocol_type());
            }
        } else {
            let msg = if r.pages.is_empty() { " cargando páginas…" } else { " cargando imagen…" };
            let [center] = Layout::vertical([Constraint::Length(1)]).flex(Flex::Center).areas(image_area);
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::raw(theme::spinner(tick)).fg(PINK), Span::raw(msg).fg(MUTED)])).centered(),
                center,
            );
        }
        theme::status_bar(f, status_area, "LEYENDO", &info, &[("←→", "página"), ("n/p", "capítulo"), ("esc", "volver")]);
    }
}

/// Ficha del favorito seleccionado en la biblioteca.
fn favorite_card(fav: &crate::library::Favorite) -> Paragraph<'_> {
    let mut lines = vec![
        Line::raw(""),
        Line::from(fav.manga.title.clone()).fg(PINK).bold(),
        Line::from(format!("{} · {}", fav.source_name, fav.lang)).fg(MUTED),
        Line::raw(""),
        Line::from(vec![
            Span::raw(format!(" {} ", fav.status.label())).bg(theme::status_color(fav.status)).fg(ON_PINK).bold(),
            Span::raw(format!("  {} capítulos leídos", fav.progress)).fg(TEXT),
        ]),
    ];
    if let Some(last) = &fav.last_read {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("▸ seguir en ").fg(MUTED),
            Span::raw(last.chapter_name.clone()).fg(TEXT),
            Span::raw(format!(" · pág. {}", last.page + 1)).fg(MUTED),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(match &fav.anilist {
        None => Line::from("☁ sin vincular a AniList").fg(FAINT),
        Some(link) if fav.needs_sync() => Line::from(format!("☁ {} · ↻ cambios sin sincronizar", link.title)).fg(PEACH),
        Some(link) => Line::from(format!("☁ {} · ✓ sincronizado", link.title)).fg(MINT),
    });
    if let Some(desc) = fav.manga.description.as_ref().filter(|d| !d.is_empty()) {
        lines.push(Line::raw(""));
        lines.push(Line::from(desc.replace(['\n', '\r'], " ")).fg(MUTED));
    }
    Paragraph::new(lines).wrap(Wrap { trim: true }).block(theme::block("ficha").padding(ratatui::widgets::Padding::horizontal(1)))
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

/// "12" para 12.0, "12.5" para 12.5 (para comparar con lo que escribe el usuario).
fn format_number(n: f32) -> String {
    if n.fract() == 0.0 { format!("{n:.0}") } else { n.to_string() }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Favorite;
    use crate::model::SourceInfo;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn fake_favorite(title: &str, status: Status, progress: i32, linked: bool, synced: bool) -> Favorite {
        Favorite {
            source_id: "1".into(),
            source_name: "Manga TV".into(),
            lang: "es".into(),
            manga: Manga {
                url: format!("/manga/{title}"),
                title: title.into(),
                description: Some("Iori Kitahara se muda a una ciudad costera para empezar la universidad y termina en un club de buceo muy particular.".into()),
                ..Default::default()
            },
            added_at: 0,
            status,
            progress,
            last_read: Some(LastRead { chapter_url: "/c/12".into(), chapter_name: "Capítulo 12".into(), page: 7 }),
            anilist: linked.then(|| AniListLink {
                media_id: 87395,
                title: "Grand Blue".into(),
                synced_progress: Some(if synced { progress } else { progress - 2 }),
                synced_status: Some(status),
                synced_at: None,
            }),
        }
    }

    /// Previsualiza biblioteca y manga como JSON (ver tools/preview.py). Uso:
    /// HANTHO_PREVIEW_DIR=<dir> cargo test --release screens_preview
    #[test]
    fn screens_preview() {
        let Ok(dir) = std::env::var("HANTHO_PREVIEW_DIR") else { return };
        let mut app = App::new(Picker::halfblocks());
        app.library.favorites = vec![
            fake_favorite("Grand Blue", Status::Current, 12, true, false),
            fake_favorite("Chainsaw Man", Status::Completed, 97, true, true),
            fake_favorite("Frieren", Status::Planning, 0, false, false),
            fake_favorite("Dandadan", Status::Paused, 54, true, true),
        ];
        app.library_state.select(Some(0));
        app.anilist_user = Some("alexp".into());

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        app.screen = Screen::Library;
        app.status = "★ añadido a favoritos".into();
        terminal.draw(|f| app.draw(f)).unwrap();
        theme::dump_buffer(terminal.backend().buffer(), &format!("{dir}/library.json"));

        let fav = app.library.favorites[0].clone();
        app.current = Some((LoadedSource { info: SourceInfo { id: "1".into(), name: "Manga TV".into(), lang: "es".into(), base_url: None, supports_latest: true } }, fav.manga.clone()));
        let chapters: Vec<Chapter> = (1..=20)
            .rev()
            .map(|n| Chapter { url: format!("/c/{n}"), name: format!("Capítulo {n}"), chapter_number: n as f32, scanlator: Some("Plot Twist No Fansub".into()), ..Default::default() })
            .collect();
        let mut manga = fav.manga.clone();
        manga.author = Some("Kenji Inoue, Kimitake Yoshioka".into());
        manga.genre = Some("Comedia, Recuentos de la vida, Seinen".into());
        let key = reads::key("1", &fav.manga.url);
        for c in chapters.iter().filter(|c| c.chapter_number <= 12.0) {
            app.reads.mark(&key, c);
        }
        app.details = Some(MangaDetails { manga, chapters });
        app.rebuild_chapter_view();
        app.chapters_state.select(Some(7));
        app.screen = Screen::Manga;
        app.status = "✓ AniList: Leyendo · capítulo 12".into();
        terminal.draw(|f| app.draw(f)).unwrap();
        theme::dump_buffer(terminal.backend().buffer(), &format!("{dir}/manga.json"));

        app.backup_files = vec![
            std::path::PathBuf::from(r"C:\Users\alexp\Downloads\app.mihon_2026-10-29_21-04.tachibk"),
            std::path::PathBuf::from(r"C:\Users\alexp\Documents\tachiyomi_2025-03-02_10-11.tachibk"),
        ];
        app.backup_state.select(Some(1));
        app.backup_report = vec![
            "✓ 48 favoritos nuevos · 2 ya estaban".into(),
            "✓ 3120 capítulos marcados como leídos".into(),
            "✓ 31 vinculados con AniList".into(),
            "! sin extensión en Keiyoushi: Fuente Rara".into(),
            "✓ 6 extensiones instaladas y cargadas".into(),
        ];
        app.screen = Screen::Backup;
        app.status = "importación terminada ✿".into();
        terminal.draw(|f| app.draw(f)).unwrap();
        theme::dump_buffer(terminal.backend().buffer(), &format!("{dir}/backup.json"));
    }
}
