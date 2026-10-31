//! Servidor MCP (Model Context Protocol) por stdio: `hantho mcp`.
//!
//! Permite que un agente (Claude Code, Codex, Hermes, pi…) busque manga, consulte la biblioteca,
//! marque capítulos y abra el lector en un panel al lado del chat. JSON-RPC 2.0, un mensaje por
//! línea; stdout es solo protocolo (los mensajes de diagnóstico van a stderr).
//!
//! La biblioteca y el registro de leídos se releen del disco en cada llamada y se guardan al
//! momento, así que la TUI y el MCP pueden usarse a la vez.

use crate::anilist::{self, Credentials};
use crate::host::{Host, LoadedSource};
use crate::launch::{self, Placement};
use crate::library::Library;
use crate::model::Manga;
use crate::reads::{self, ReadLog};
use crate::search::{self, SearchEvent};
use crate::tui::ChapterTarget;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, OnceCell};

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Han Thỏ es un lector de manga para la terminal con extensiones de Mihon. \
Usa search_manga para encontrar un título en todas las fuentes instaladas, library para ver los favoritos del usuario \
con su progreso, chapters para listar capítulos, mark_read para marcar lo leído (actualiza AniList si el manga está \
vinculado) y open_reader para abrir el lector en un panel al lado del chat. Los identificadores de fuente (source_id) \
y las URLs (manga_url, chapter_url) salen de search_manga, library o chapters: no los inventes.";

struct Server {
    host: OnceCell<(Arc<Host>, Vec<LoadedSource>)>,
    out: Mutex<tokio::io::Stdout>,
}

pub async fn serve() -> Result<()> {
    let server = Arc::new(Server { host: OnceCell::new(), out: Mutex::new(tokio::io::stdout()) });
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let server = server.clone();
        // Cada pedido en su tarea: una búsqueda lenta no bloquea los demás
        tokio::spawn(async move {
            if let Some(response) = server.handle(&line).await {
                let mut out = server.out.lock().await;
                let _ = out.write_all(format!("{response}\n").as_bytes()).await;
                let _ = out.flush().await;
            }
        });
    }
    Ok(())
}

impl Server {
    async fn handle(&self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, -32700, &format!("JSON inválido: {e}"))),
        };
        // Notificaciones (sin id): no llevan respuesta
        let id = msg.get("id").cloned()?;
        let method = msg["method"].as_str().unwrap_or("");
        let params = &msg["params"];
        let result = match method {
            "initialize" => Ok(initialize(params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                Ok(match self.call(name, &args).await {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("Error: {e}") }], "isError": true }),
                })
            }
            _ => Err((-32601, format!("método desconocido: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, msg)) => error(id, code, &msg),
        })
    }

    /// El host de extensiones se arranca la primera vez que hace falta.
    async fn host(&self) -> Result<&(Arc<Host>, Vec<LoadedSource>)> {
        self.host
            .get_or_try_init(|| async {
                let host = Host::spawn().await?;
                let (sources, _) = host.load_all(&crate::ext::list()).await;
                if sources.is_empty() {
                    bail!("no hay extensiones instaladas: el usuario tiene que añadir un repositorio y elegirlas en «Añadir extensiones» (o `hantho ext repo add <url>`)");
                }
                Ok((host, sources))
            })
            .await
    }

    async fn call(&self, name: &str, args: &Value) -> Result<String> {
        match name {
            "search_manga" => self.search(args).await,
            "library" => library(),
            "chapters" => self.chapters(args).await,
            "mark_read" => self.mark_read(args).await,
            "set_favorite" => set_favorite(args).await,
            "link_anilist" => link_anilist(args).await,
            "open_reader" => open_reader(args),
            "sync_anilist" => sync_anilist(args).await,
            _ => bail!("herramienta desconocida: {name}"),
        }
    }

    async fn search(&self, args: &Value) -> Result<String> {
        let query = str_arg(args, "query")?;
        let lang = args["lang"].as_str();
        let limit = args["limit"].as_u64().unwrap_or(15) as usize;
        let (host, sources) = self.host().await?;
        let sources: Vec<_> = sources.iter().filter(|s| lang.is_none_or(|l| s.info.lang == l || s.info.lang == "all")).cloned().collect();

        let mut groups: Vec<(String, String, Vec<Value>)> = Vec::new(); // (clave, título, fuentes)
        let mut failed = 0;
        let mut rx = search::global(host.clone(), sources, query.to_string());
        while let Some(ev) = rx.recv().await {
            match ev {
                SearchEvent::Results { source, mangas } => {
                    for m in mangas {
                        let key = search::normalize(&m.title);
                        let entry = json!({ "source_id": source.info.id, "source": source.info.name, "lang": source.info.lang, "manga_url": m.url, "title": m.title });
                        match groups.iter_mut().find(|g| g.0 == key) {
                            Some(g) => g.2.push(entry),
                            None => groups.push((key, m.title.clone(), vec![entry])),
                        }
                    }
                }
                SearchEvent::Failed { .. } => failed += 1,
                SearchEvent::Done => break,
            }
        }
        // Igual que en la TUI: coincidencia exacta, luego por prefijo/contenido, luego por nº de fuentes
        let q = search::normalize(query);
        groups.sort_by_key(|(k, _, s)| (if *k == q { 0 } else if k.starts_with(&q) { 1 } else if k.contains(&q) { 2 } else { 3 }, std::cmp::Reverse(s.len())));
        let results: Vec<Value> = groups.into_iter().take(limit).map(|(_, title, sources)| json!({ "title": title, "sources": sources })).collect();
        Ok(pretty(&json!({ "query": query, "results": results, "failed_sources": failed })))
    }

    async fn chapters(&self, args: &Value) -> Result<String> {
        let (source_id, manga_url) = (str_arg(args, "source_id")?, str_arg(args, "manga_url")?);
        let (host, sources) = self.host().await?;
        if !sources.iter().any(|s| s.info.id == source_id) {
            bail!("la fuente {source_id} no está instalada");
        }
        let manga = Manga { url: manga_url.to_string(), ..Default::default() };
        let details = host.details(source_id, &manga, args["refresh"].as_bool().unwrap_or(false)).await?;
        let log = ReadLog::load();
        let key = reads::key(source_id, manga_url);
        let unread_only = args["unread_only"].as_bool().unwrap_or(false);
        let (from, to) = (args["from"].as_f64(), args["to"].as_f64());
        let limit = args["limit"].as_u64().unwrap_or(60) as usize;

        let mut chapters: Vec<_> = details.chapters.iter().collect();
        chapters.sort_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));
        let read_count = chapters.iter().filter(|c| log.is_read(&key, &c.url)).count();
        let list: Vec<Value> = chapters
            .iter()
            .filter(|c| !unread_only || !log.is_read(&key, &c.url))
            .filter(|c| from.is_none_or(|f| c.chapter_number as f64 >= f) && to.is_none_or(|t| c.chapter_number as f64 <= t))
            .take(limit)
            .map(|c| json!({ "number": c.chapter_number, "name": c.name, "chapter_url": c.url, "read": log.is_read(&key, &c.url) }))
            .collect();
        Ok(pretty(&json!({
            "title": details.manga.title,
            "total": details.chapters.len(),
            "read": read_count,
            "shown": list.len(),
            "chapters": list,
            "note": "ordenados del más viejo al más nuevo; usa from/to/unread_only/limit para ver otros"
        })))
    }

    async fn mark_read(&self, args: &Value) -> Result<String> {
        let (source_id, manga_url) = (str_arg(args, "source_id")?, str_arg(args, "manga_url")?);
        let read = args["read"].as_bool().unwrap_or(true);
        let key = reads::key(source_id, manga_url);
        let mut log = ReadLog::load();
        let mut changed = 0;

        if let Some(up_to) = args["up_to_number"].as_f64() {
            // Hace falta la lista de capítulos para saber cuáles son "hasta el N"
            let (host, _) = self.host().await?;
            let details = host.details(source_id, &Manga { url: manga_url.to_string(), ..Default::default() }, false).await?;
            for c in details.chapters.iter().filter(|c| c.chapter_number >= 0.0 && c.chapter_number as f64 <= up_to) {
                changed += usize::from(if read { log.mark(&key, c) } else { log.unmark(&key, &c.url) });
            }
        } else if let Some(url) = args["chapter_url"].as_str() {
            let chapter = crate::model::Chapter {
                url: url.to_string(),
                name: args["chapter_name"].as_str().unwrap_or(url).to_string(),
                chapter_number: args["chapter_number"].as_f64().unwrap_or(-1.0) as f32,
                ..Default::default()
            };
            changed += usize::from(if read { log.mark(&key, &chapter) } else { log.unmark(&key, url) });
        } else {
            bail!("indica chapter_url o up_to_number");
        }
        log.save()?;

        // Favorito: progreso = mayor capítulo leído, y AniList si está vinculado
        let mut lib = Library::load();
        let mut anilist = String::from("no vinculado a AniList");
        if let Some(fav) = lib.get_mut(source_id, manga_url) {
            fav.progress = log.max_number(&key);
            let (progress, status, link) = (fav.progress, fav.status, fav.anilist.clone());
            lib.save()?;
            if let (Some(link), Some(creds)) = (link, Credentials::load()) {
                anilist = match anilist::sync(&creds, link.media_id, status.as_anilist(), progress).await {
                    Ok(r) => {
                        let mut lib = Library::load();
                        if let Some(fav) = lib.get_mut(source_id, manga_url) {
                            fav.progress = fav.progress.max(r.entry.progress.unwrap_or(0));
                            if let Some(a) = fav.anilist.as_mut() {
                                a.synced_progress = Some(fav.progress);
                                a.synced_status = Some(status);
                                a.synced_at = Some(crate::library::now());
                            }
                            lib.save()?;
                        }
                        format!("AniList actualizado: capítulo {}", r.entry.progress.unwrap_or(progress))
                    }
                    Err(e) => format!("AniList no se ha podido actualizar: {e}"),
                };
            }
        }
        Ok(format!("{changed} capítulos {}. {anilist}.", if read { "marcados como leídos" } else { "desmarcados" }))
    }
}

fn library() -> Result<String> {
    let lib = Library::load();
    let log = ReadLog::load();
    let items: Vec<Value> = lib
        .favorites
        .iter()
        .map(|f| {
            json!({
                "title": f.manga.title,
                "source_id": f.source_id,
                "source": f.source_name,
                "manga_url": f.manga.url,
                "status": f.status.label(),
                "progress": f.progress,
                "chapters_read": log.chapters(&reads::key(&f.source_id, &f.manga.url)).map_or(0, |c| c.len()),
                "last_read": f.last_read.as_ref().map(|l| json!({ "chapter": l.chapter_name, "chapter_url": l.chapter_url, "page": l.page + 1 })),
                "anilist": f.anilist.as_ref().map(|a| json!({ "media_id": a.media_id, "title": a.title, "pending_sync": f.needs_sync() })),
            })
        })
        .collect();
    Ok(pretty(&json!({ "favorites": items, "anilist_user": Credentials::load().map(|c| c.user_name) })))
}

async fn set_favorite(args: &Value) -> Result<String> {
    let (source_id, manga_url) = (str_arg(args, "source_id")?, str_arg(args, "manga_url")?);
    let favorite = args["favorite"].as_bool().unwrap_or(true);
    let mut lib = Library::load();
    let exists = lib.find(source_id, manga_url).is_some();
    if exists == favorite {
        return Ok(if favorite { "ya estaba en favoritos".into() } else { "no estaba en favoritos".into() });
    }
    let manga = Manga { url: manga_url.to_string(), title: args["title"].as_str().unwrap_or(manga_url).to_string(), ..Default::default() };
    let source_name = args["source"].as_str().unwrap_or(source_id);
    let lang = args["lang"].as_str().unwrap_or("");
    lib.toggle(source_id, source_name, lang, &manga);
    lib.save()?;
    if !favorite {
        return Ok("quitado de favoritos".into());
    }
    // Vincular con AniList si hay una coincidencia exacta del título
    let linked = match anilist::search(&manga.title).await {
        Ok(candidates) => match anilist::best_match(&manga.title, &candidates) {
            Some(media) => link_and_sync(source_id, manga_url, media.id, &media.display_title()).await?,
            None => "no se ha vinculado con AniList automáticamente (sin coincidencia exacta; usa link_anilist)".into(),
        },
        Err(e) => format!("no se ha podido consultar AniList: {e}"),
    };
    Ok(format!("★ «{}» añadido a favoritos. {linked}", manga.title))
}

/// Vincula un favorito con una obra de AniList y, si hay sesión, envía el progreso.
async fn link_and_sync(source_id: &str, manga_url: &str, media_id: i64, title: &str) -> Result<String> {
    let mut lib = Library::load();
    let fav = lib.get_mut(source_id, manga_url).ok_or_else(|| anyhow!("ese manga no está en favoritos"))?;
    fav.anilist = Some(crate::library::AniListLink { media_id, title: title.to_string(), synced_progress: None, synced_status: None, synced_at: None });
    let (status, progress) = (fav.status, fav.progress);
    lib.save()?;
    let Some(creds) = Credentials::load() else {
        return Ok(format!("vinculado con «{title}» en AniList (sin sesión: se sincronizará al conectar la cuenta)"));
    };
    match anilist::sync(&creds, media_id, status.as_anilist(), progress).await {
        Ok(r) => {
            let remote = r.entry.progress.unwrap_or(progress);
            let mut lib = Library::load();
            if let Some(fav) = lib.get_mut(source_id, manga_url) {
                fav.progress = fav.progress.max(remote);
                if let Some(a) = fav.anilist.as_mut() {
                    a.synced_progress = Some(fav.progress);
                    a.synced_status = Some(status);
                    a.synced_at = Some(crate::library::now());
                }
                lib.save()?;
            }
            Ok(format!("vinculado con «{title}» en AniList y sincronizado (capítulo {remote})"))
        }
        Err(e) => Ok(format!("vinculado con «{title}» en AniList, pero no se ha podido sincronizar: {e}")),
    }
}

async fn link_anilist(args: &Value) -> Result<String> {
    let (source_id, manga_url) = (str_arg(args, "source_id")?, str_arg(args, "manga_url")?);
    let lib = Library::load();
    let fav = lib
        .favorites
        .iter()
        .find(|f| f.is(source_id, manga_url))
        .ok_or_else(|| anyhow!("ese manga no está en favoritos: añádelo primero con set_favorite"))?;
    let query = args["query"].as_str().unwrap_or(&fav.manga.title).to_string();
    let candidates = anilist::search(&query).await?;

    if let Some(id) = args["media_id"].as_i64() {
        let title = match candidates.iter().find(|m| m.id == id) {
            Some(m) => m.display_title(),
            None => anilist::media(id).await.map_err(|e| anyhow!("AniList no tiene ningún manga con id {id}: {e}"))?.display_title(),
        };
        return link_and_sync(source_id, manga_url, id, &title).await;
    }
    if let Some(media) = anilist::best_match(&fav.manga.title, &candidates) {
        return link_and_sync(source_id, manga_url, media.id, &media.display_title()).await;
    }
    // Sin coincidencia segura: devolver candidatas para que el usuario elija
    let list: Vec<Value> = candidates
        .iter()
        .take(8)
        .map(|m| {
            json!({
                "media_id": m.id,
                "title": m.display_title(),
                "english": m.title.english,
                "format": m.format,
                "year": m.start_date.as_ref().and_then(|d| d.year),
                "chapters": m.chapters,
            })
        })
        .collect();
    if list.is_empty() {
        bail!("AniList no ha encontrado «{query}»; prueba con otro título en query");
    }
    Ok(format!(
        "No hay una coincidencia exacta para «{}». Pregunta al usuario cuál es y vuelve a llamar a link_anilist con media_id:\n{}",
        fav.manga.title,
        pretty(&json!(list))
    ))
}

fn open_reader(args: &Value) -> Result<String> {
    let chapter = if let Some(url) = args["chapter_url"].as_str() {
        ChapterTarget::Url(url.to_string())
    } else if let Some(n) = args["chapter_number"].as_f64() {
        ChapterTarget::Number(n as f32)
    } else if args["continue"].as_bool().unwrap_or(true) {
        ChapterTarget::Continue
    } else {
        ChapterTarget::None
    };
    let lib = Library::load();
    let target = launch::resolve(&lib, args["title"].as_str(), args["source_id"].as_str(), args["manga_url"].as_str(), args["title"].as_str(), chapter)?;
    let placement = if args["placement"].as_str() == Some("window") { Placement::Window } else { Placement::Side };
    let where_ = launch::launch(&target, placement)?;
    Ok(format!("Abierto «{}» en {where_}.", target.title))
}

async fn sync_anilist(args: &Value) -> Result<String> {
    let (source_id, manga_url) = (str_arg(args, "source_id")?, str_arg(args, "manga_url")?);
    let creds = Credentials::load().ok_or_else(|| anyhow!("no hay sesión de AniList: conéctala desde la TUI (menú AniList, tecla l)"))?;
    let lib = Library::load();
    let fav = lib.favorites.iter().find(|f| f.is(source_id, manga_url)).ok_or_else(|| anyhow!("ese manga no está en favoritos"))?;
    let link = fav.anilist.clone().ok_or_else(|| anyhow!("ese favorito no está vinculado a AniList: vincúlalo con link_anilist"))?;
    let r = anilist::sync(&creds, link.media_id, fav.status.as_anilist(), fav.progress).await?;
    Ok(format!("AniList: {} · capítulo {}", r.entry.status.unwrap_or_default(), r.entry.progress.unwrap_or(0)))
}

fn initialize(params: &Value) -> Value {
    let requested = params["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSIONS[0]);
    let version = PROTOCOL_VERSIONS.iter().find(|v| **v == requested).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "hantho", "title": "Han Thỏ", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tools() -> Value {
    let manga_ref = json!({
        "source_id": { "type": "string", "description": "Id de la fuente (de search_manga, library o chapters)" },
        "manga_url": { "type": "string", "description": "URL del manga en la fuente (de search_manga o library)" },
    });
    let with = |extra: Value| {
        let mut props = manga_ref.as_object().unwrap().clone();
        props.extend(extra.as_object().unwrap().clone());
        Value::Object(props)
    };
    json!([
        {
            "name": "search_manga",
            "title": "Buscar manga",
            "description": "Busca un manga por título en todas las fuentes instaladas a la vez (tarda hasta ~25 s). Devuelve los títulos agrupados con las fuentes donde aparecen.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "Título a buscar" },
                "lang": { "type": "string", "description": "Solo fuentes de este idioma (p. ej. es, en)" },
                "limit": { "type": "integer", "description": "Máximo de títulos (por defecto 15)" }
            }, "required": ["query"] },
            "annotations": { "readOnlyHint": true, "openWorldHint": true }
        },
        {
            "name": "library",
            "title": "Biblioteca",
            "description": "Lista los favoritos del usuario con estado, progreso, último capítulo leído y vínculo con AniList.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "chapters",
            "title": "Capítulos",
            "description": "Lista los capítulos de un manga (del más viejo al más nuevo) indicando cuáles están leídos. Para mangas largos usa from/to o unread_only.",
            "inputSchema": { "type": "object", "properties": with(json!({
                "from": { "type": "number", "description": "Desde el capítulo número" },
                "to": { "type": "number", "description": "Hasta el capítulo número" },
                "unread_only": { "type": "boolean", "description": "Solo los no leídos" },
                "limit": { "type": "integer", "description": "Máximo de capítulos (por defecto 60)" },
                "refresh": { "type": "boolean", "description": "Ignorar la caché y pedir la lista a la fuente" }
            })), "required": ["source_id", "manga_url"] },
            "annotations": { "readOnlyHint": true, "openWorldHint": true }
        },
        {
            "name": "mark_read",
            "title": "Marcar como leído",
            "description": "Marca (o desmarca) un capítulo, o todos hasta un número, como leídos. Si el manga es favorito y está vinculado a AniList, actualiza AniList.",
            "inputSchema": { "type": "object", "properties": with(json!({
                "chapter_url": { "type": "string", "description": "Un capítulo concreto (de chapters)" },
                "chapter_number": { "type": "number", "description": "Número de ese capítulo (para el progreso)" },
                "chapter_name": { "type": "string", "description": "Nombre de ese capítulo" },
                "up_to_number": { "type": "number", "description": "Marcar todos los capítulos hasta este número incluido" },
                "read": { "type": "boolean", "description": "false para desmarcar (por defecto true)" }
            })), "required": ["source_id", "manga_url"] },
            "annotations": { "destructiveHint": false, "idempotentHint": true }
        },
        {
            "name": "set_favorite",
            "title": "Favorito",
            "description": "Añade o quita un manga de los favoritos del usuario. Al añadirlo intenta vincularlo con AniList si el título coincide exactamente.",
            "inputSchema": { "type": "object", "properties": with(json!({
                "title": { "type": "string", "description": "Título del manga" },
                "source": { "type": "string", "description": "Nombre de la fuente" },
                "lang": { "type": "string", "description": "Idioma de la fuente" },
                "favorite": { "type": "boolean", "description": "false para quitarlo (por defecto true)" }
            })), "required": ["source_id", "manga_url"] },
            "annotations": { "idempotentHint": true }
        },
        {
            "name": "open_reader",
            "title": "Abrir lector",
            "description": "Abre el lector de Han Thỏ en un panel al lado del chat (Windows Terminal, tmux, WezTerm, zellij o kitty) o en una ventana nueva. Por defecto continúa donde se quedó el usuario.",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string", "description": "Título de un favorito (alternativa a source_id + manga_url)" },
                "source_id": { "type": "string", "description": "Id de la fuente" },
                "manga_url": { "type": "string", "description": "URL del manga" },
                "chapter_number": { "type": "number", "description": "Abrir este capítulo" },
                "chapter_url": { "type": "string", "description": "Abrir este capítulo (de chapters)" },
                "continue": { "type": "boolean", "description": "Seguir donde se quedó (por defecto true)" },
                "placement": { "type": "string", "enum": ["side", "window"], "description": "side = panel al lado (por defecto), window = ventana nueva" }
            } },
            "annotations": { "openWorldHint": false }
        },
        {
            "name": "link_anilist",
            "title": "Vincular con AniList",
            "description": "Vincula un favorito con su obra en AniList (necesario para que el progreso se guarde en la cuenta del usuario) y sincroniza. Sin media_id, vincula solo si el título coincide exactamente; si hay dudas devuelve candidatas: pregunta al usuario y vuelve a llamar con media_id.",
            "inputSchema": { "type": "object", "properties": with(json!({
                "media_id": { "type": "integer", "description": "Id de la obra en AniList (de las candidatas)" },
                "query": { "type": "string", "description": "Buscar en AniList con otro título (por defecto, el del favorito)" }
            })), "required": ["source_id", "manga_url"] },
            "annotations": { "openWorldHint": true, "idempotentHint": true }
        },
        {
            "name": "sync_anilist",
            "title": "Sincronizar con AniList",
            "description": "Envía a AniList el estado y el progreso de un favorito ya vinculado.",
            "inputSchema": { "type": "object", "properties": manga_ref, "required": ["source_id", "manga_url"] },
            "annotations": { "openWorldHint": true }
        }
    ])
}

fn str_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args[name].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("falta el parámetro «{name}»"))
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn protocol_basics() {
        let server = Server { host: OnceCell::new(), out: Mutex::new(tokio::io::stdout()) };
        let init = server.handle(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#).await.unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "hantho");

        assert!(server.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await.is_none(), "las notificaciones no se responden");

        let list = server.handle(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).await.unwrap();
        let names: Vec<_> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["search_manga", "library", "chapters", "mark_read", "set_favorite", "open_reader", "link_anilist", "sync_anilist"]);
        for t in list["result"]["tools"].as_array().unwrap() {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        }

        let bad = server.handle(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"chapters","arguments":{}}}"#).await.unwrap();
        assert_eq!(bad["result"]["isError"], true);
        assert!(bad["result"]["content"][0]["text"].as_str().unwrap().contains("source_id"));

        let unknown = server.handle(r#"{"jsonrpc":"2.0","id":4,"method":"nope"}"#).await.unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        assert_eq!(server.handle("{no es json").await.unwrap()["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn favorites_and_library_roundtrip() {
        // Escribe favoritos: solo con una carpeta de datos de prueba, nunca en la biblioteca real
        if std::env::var_os("HANTHO_HOME").is_none() {
            return;
        }
        let server = Server { host: OnceCell::new(), out: Mutex::new(tokio::io::stdout()) };
        let add = server.handle(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"set_favorite","arguments":{"source_id":"42","manga_url":"/gb","title":"Grand Blue","source":"Manga TV","lang":"es"}}}"#).await.unwrap();
        assert!(add["result"]["content"][0]["text"].as_str().unwrap().contains("añadido"));
        let mark = server.handle(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"mark_read","arguments":{"source_id":"42","manga_url":"/gb","chapter_url":"/gb/c7","chapter_number":7}}}"#).await.unwrap();
        assert_eq!(mark["result"]["isError"], false, "{mark}");
        let lib = server.handle(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"library","arguments":{}}}"#).await.unwrap();
        let text = lib["result"]["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        let gb = v["favorites"].as_array().unwrap().iter().find(|f| f["title"] == "Grand Blue").unwrap();
        assert_eq!(gb["progress"], 7);
        assert_eq!(gb["chapters_read"], 1);
    }
}
