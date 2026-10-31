//! Cliente de AniList (GraphQL).
//!
//! El login (OAuth por navegador con servidor local) está en `anilist_auth`.
//!
//! Para no abusar del servicio (límite ~90/min, a veces 30/min): limitador local de 1 pedido
//! cada 2 s con ráfagas de 5, respeto de `Retry-After` en 429, freno preventivo cuando
//! `X-RateLimit-Remaining` se acerca a 0, y caché en disco de búsquedas.

use crate::cache::{self, RateLimiter};
use crate::paths;
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::LazyLock;
use std::time::Duration;

const API: &str = "https://graphql.anilist.co";
const SEARCH_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const MAX_RETRIES: u32 = 3;

static LIMITER: LazyLock<RateLimiter> = LazyLock::new(|| RateLimiter::new(5, Duration::from_secs(2)));
static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("hantho/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        // Sin pool de conexiones: el cliente es global y una conexión guardada queda ligada al
        // runtime de tokio que la abrió (en los tests cada uno tiene el suyo). AniList se
        // consulta poco, así que reabrir no cuesta nada.
        .pool_max_idle_per_host(0)
        .build()
        .expect("cliente HTTP")
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub client_id: String,
    pub token: String,
    pub user_id: i64,
    pub user_name: String,
}

impl Credentials {
    pub fn load() -> Option<Credentials> {
        serde_json::from_slice(&std::fs::read(paths::anilist_file()).ok()?).ok()
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(paths::home())?;
        let path = paths::anilist_file();
        std::fs::write(&path, serde_json::to_vec_pretty(self)?)?;
        // El token da acceso a la cuenta: en Unix, que solo lo lea el usuario
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn delete() {
        let _ = std::fs::remove_file(paths::anilist_file());
    }
}

pub fn authorize_url(client_id: &str) -> String {
    format!("https://anilist.co/api/v2/oauth/authorize?client_id={client_id}&response_type=token")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Title {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartDate {
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: i64,
    pub title: Title,
    pub format: Option<String>,
    pub chapters: Option<i32>,
    pub status: Option<String>,
    pub start_date: Option<StartDate>,
    pub site_url: Option<String>,
}

impl Media {
    pub fn display_title(&self) -> String {
        let t = &self.title;
        t.romaji.clone().or(t.english.clone()).or(t.native.clone()).unwrap_or_else(|| format!("#{}", self.id))
    }

    fn titles(&self) -> impl Iterator<Item = &String> {
        [&self.title.romaji, &self.title.english, &self.title.native].into_iter().flatten()
    }
}

/// La obra de AniList que corresponde sin duda a `title`, para vincular sin preguntar.
/// Solo vale una coincidencia exacta del título (romaji, inglés o nativo, ignorando mayúsculas
/// y signos); si hay varias, gana la que es manga normal frente a novela o one-shot. Ante
/// cualquier ambigüedad devuelve `None` y que elija el usuario: un vínculo equivocado
/// escribiría el progreso en otra obra.
pub fn best_match<'a>(title: &str, candidates: &'a [Media]) -> Option<&'a Media> {
    let key = crate::search::normalize(title);
    if key.is_empty() {
        return None;
    }
    let exact: Vec<&Media> = candidates.iter().filter(|m| m.titles().any(|t| crate::search::normalize(t) == key)).collect();
    match exact.as_slice() {
        [one] => Some(one),
        [] => None,
        many => {
            let manga: Vec<&&Media> = many.iter().filter(|m| m.format.as_deref() == Some("MANGA")).collect();
            (manga.len() == 1).then(|| *manga[0])
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListEntry {
    pub status: Option<String>,
    pub progress: Option<i32>,
}

/// Un pedido GraphQL con limitador y reintentos ante 429 / errores 5xx.
async fn graphql<T: DeserializeOwned>(query: &str, variables: Value, token: Option<&str>) -> Result<T> {
    let body = json!({ "query": query, "variables": variables });
    for attempt in 0..=MAX_RETRIES {
        LIMITER.acquire().await;
        let mut req = HTTP.post(API).header("Accept", "application/json").json(&body);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        // Fallos de red (conexión rechazada o cortada): reintentar con espera creciente
        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(_) if attempt < MAX_RETRIES => {
                LIMITER.back_off(Duration::from_secs(attempt as u64 + 1)).await;
                continue;
            }
            Err(e) => return Err(e).context("no se ha podido conectar con AniList"),
        };
        let status = resp.status();
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());

        if status.as_u16() == 429 || status.is_server_error() {
            let wait = header("Retry-After").unwrap_or(5 * (attempt as u64 + 1)).min(120);
            if attempt == MAX_RETRIES {
                bail!("AniList ha pedido esperar (HTTP {status}); prueba dentro de un rato");
            }
            LIMITER.back_off(Duration::from_secs(wait)).await;
            continue;
        }
        // Freno preventivo: si quedan muy pocos pedidos en la ventana, esperar a que se renueve
        if header("X-RateLimit-Remaining").is_some_and(|r| r <= 3) {
            LIMITER.back_off(Duration::from_secs(20)).await;
        }

        let value: Value = match resp.json().await {
            Ok(v) => v,
            Err(_) if attempt < MAX_RETRIES => {
                LIMITER.back_off(Duration::from_secs(attempt as u64 + 1)).await;
                continue;
            }
            Err(e) => return Err(e).context("respuesta inválida de AniList"),
        };
        if let Some(err) = value["errors"].as_array().and_then(|e| e.first()) {
            let msg = err["message"].as_str().unwrap_or("error desconocido");
            if status.as_u16() == 401 || msg.contains("Invalid token") {
                bail!("el token de AniList ha caducado o no es válido; vuelve a ejecutar `hantho anilist login`");
            }
            bail!("AniList: {msg}");
        }
        return serde_json::from_value(value["data"].clone()).context("formato inesperado de AniList");
    }
    unreachable!()
}

/// Valida un token y devuelve (id, nombre) del usuario.
pub async fn viewer(token: &str) -> Result<(i64, String)> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Viewer")]
        viewer: Viewer,
    }
    #[derive(Deserialize)]
    struct Viewer {
        id: i64,
        name: String,
    }
    let d: Data = graphql("query { Viewer { id name } }", json!({}), Some(token)).await?;
    Ok((d.viewer.id, d.viewer.name))
}

const MEDIA_FIELDS: &str = "id title { romaji english native } format chapters status startDate { year } siteUrl";

/// Una obra por id (cacheada una semana).
pub async fn media(id: i64) -> Result<Media> {
    let key = id.to_string();
    if let Some(hit) = cache::get::<Media>("anilist-media", &key, SEARCH_TTL) {
        return Ok(hit);
    }
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Media")]
        media: Media,
    }
    let q = format!("query($id: Int) {{ Media(id: $id, type: MANGA) {{ {MEDIA_FIELDS} }} }}");
    let d: Data = graphql(&q, json!({ "id": id }), None).await?;
    cache::put("anilist-media", &key, &d.media);
    Ok(d.media)
}

/// Busca mangas por título. Cacheado una semana: los resultados casi no cambian.
pub async fn search(query: &str) -> Result<Vec<Media>> {
    let key = query.trim().to_lowercase();
    if let Some(hit) = cache::get::<Vec<Media>>("anilist-search", &key, SEARCH_TTL) {
        return Ok(hit);
    }
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "Page")]
        page: Page,
    }
    #[derive(Deserialize)]
    struct Page {
        media: Vec<Media>,
    }
    let q = format!("query($s: String) {{ Page(perPage: 10) {{ media(search: $s, type: MANGA) {{ {MEDIA_FIELDS} }} }} }}");
    let d: Data = graphql(&q, json!({ "s": query }), None).await?;
    cache::put("anilist-search", &key, &d.page.media);
    Ok(d.page.media)
}

/// La entrada actual en la lista del usuario (None si no la tiene). Sin caché: se usa para sincronizar.
pub async fn list_entry(creds: &Credentials, media_id: i64) -> Result<Option<ListEntry>> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "MediaList")]
        entry: Option<ListEntry>,
    }
    let q = "query($u: Int, $m: Int) { MediaList(userId: $u, mediaId: $m) { status progress } }";
    match graphql::<Data>(q, json!({ "u": creds.user_id, "m": media_id }), Some(&creds.token)).await {
        Ok(d) => Ok(d.entry),
        // AniList responde "Not Found" cuando el manga no está en la lista
        Err(e) if e.to_string().contains("Not Found") => Ok(None),
        Err(e) => Err(e),
    }
}

/// Crea o actualiza la entrada del manga en la lista del usuario.
pub async fn save_entry(creds: &Credentials, media_id: i64, status: &str, progress: i32) -> Result<ListEntry> {
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "SaveMediaListEntry")]
        entry: ListEntry,
    }
    let q = "mutation($m: Int, $s: MediaListStatus, $p: Int) { SaveMediaListEntry(mediaId: $m, status: $s, progress: $p) { status progress } }";
    let d: Data = graphql(q, json!({ "m": media_id, "s": status, "p": progress }), Some(&creds.token)).await?;
    Ok(d.entry)
}

/// Resultado de sincronizar un favorito.
pub struct SyncResult {
    pub entry: ListEntry,
    /// El progreso que ya tenía AniList era mayor que el local: no se pisó.
    pub kept_remote_progress: bool,
}

/// Sincroniza estado y progreso. El progreso nunca retrocede: si AniList tiene más capítulos
/// (p. ej. leíste en otra app), se conserva el de AniList.
pub async fn sync(creds: &Credentials, media_id: i64, status: &str, progress: i32) -> Result<SyncResult> {
    let remote = list_entry(creds, media_id).await?.and_then(|e| e.progress).unwrap_or(0);
    let kept_remote_progress = remote > progress;
    let entry = save_entry(creds, media_id, status, progress.max(remote)).await?;
    Ok(SyncResult { entry, kept_remote_progress })
}

pub fn open_browser(url: &str) {
    // No `cmd /C start`: cmd interpreta el `&` de la URL como separador de comandos
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pega a la API real una vez; la segunda búsqueda tiene que salir de la caché.
    #[tokio::test]
    async fn search_hits_api_then_cache() {
        let t = std::time::Instant::now();
        let first = search("Grand Blue").await.unwrap();
        let api_time = t.elapsed();
        assert!(first.iter().any(|m| m.id == 87395), "Grand Blue tiene id 87395");
        let t = std::time::Instant::now();
        let second = search("grand blue ").await.unwrap();
        println!("API: {api_time:?} · caché: {:?}", t.elapsed());
        assert_eq!(first.len(), second.len());
        assert!(t.elapsed() < std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod match_tests {
    use super::*;

    fn media(id: i64, romaji: &str, english: Option<&str>, format: &str) -> Media {
        Media {
            id,
            title: Title { romaji: Some(romaji.into()), english: english.map(String::from), native: None },
            format: Some(format.into()),
            chapters: None,
            status: None,
            start_date: None,
            site_url: None,
        }
    }

    #[test]
    fn best_match_is_conservative() {
        let gb = [media(87395, "Grand Blue", Some("Grand Blue Dreaming"), "MANGA"), media(103069, "Guraburu!", None, "MANGA")];
        assert_eq!(best_match("Grand Blue", &gb).map(|m| m.id), Some(87395));
        assert_eq!(best_match("grand blue dreaming", &gb).map(|m| m.id), Some(87395), "también por el título en inglés");
        assert_eq!(best_match("Grand Blue Extra", &gb).map(|m| m.id), None, "sin coincidencia exacta no se vincula");

        // Mismo título en manga y novela: gana el manga
        let both = [media(1, "Overlord", None, "NOVEL"), media(2, "Overlord", None, "MANGA")];
        assert_eq!(best_match("Overlord", &both).map(|m| m.id), Some(2));
        // Dos mangas con el mismo título: ambiguo
        let twins = [media(1, "Monster", None, "MANGA"), media(2, "Monster", None, "MANGA")];
        assert_eq!(best_match("Monster", &twins).map(|m| m.id), None);
    }
}
