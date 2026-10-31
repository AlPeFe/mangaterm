//! Búsqueda global: lanza la misma consulta en todas las fuentes a la vez y
//! entrega los resultados por un canal a medida que cada fuente responde.

use crate::cache;
use crate::host::{Host, LoadedSource};
use crate::model::Manga;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const TIMEOUT: Duration = Duration::from_secs(25);
const SEARCH_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug)]
pub enum SearchEvent {
    Results { source: LoadedSource, mangas: Vec<Manga> },
    Failed { source: LoadedSource, error: String },
    Done,
}

pub fn global(host: Arc<Host>, sources: Vec<LoadedSource>, query: String) -> mpsc::UnboundedReceiver<SearchEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let tasks = sources.into_iter().map(|source| {
            let (host, tx, query) = (host.clone(), tx.clone(), query.clone());
            tokio::spawn(async move {
                // Repetir la misma búsqueda en poco tiempo no vuelve a pegarle a los sitios
                let key = format!("{}|{}", source.info.id, query.trim().to_lowercase());
                if let Some(mangas) = cache::get::<Vec<Manga>>("search", &key, SEARCH_TTL) {
                    let _ = tx.send(SearchEvent::Results { source, mangas });
                    return;
                }
                let event = match tokio::time::timeout(TIMEOUT, host.search(&source.info.id, &query, 1)).await {
                    Ok(Ok(page)) => {
                        cache::put("search", &key, &page.mangas);
                        SearchEvent::Results { source, mangas: page.mangas }
                    }
                    Ok(Err(e)) => SearchEvent::Failed { source, error: short_error(&e.to_string()) },
                    Err(_) => SearchEvent::Failed { source, error: tr!("tiempo de espera agotado", "timed out").into() },
                };
                let _ = tx.send(event);
            })
        });
        futures::future::join_all(tasks).await;
        let _ = tx.send(SearchEvent::Done);
    });
    rx
}

/// Los errores vienen con el nombre completo de la excepción Java; nos quedamos con el mensaje.
pub fn short_error(e: &str) -> String {
    // Fallos de red: decir qué web no responde (y si es de Cloudflare, que puede estar bloqueada)
    if let Some(rest) = e.split("Failed to connect to ").nth(1) {
        let target = rest.split_whitespace().next().unwrap_or(rest);
        let (host, ip) = target.split_once('/').unwrap_or((target, ""));
        let cloudflare = ["188.114.96.", "188.114.97.", "104.16.", "104.17.", "104.18.", "104.19.", "104.20.", "104.21.", "172.64.", "172.65.", "172.66.", "172.67."]
            .iter()
            .any(|p| ip.starts_with(p));
        return if cloudflare {
            trf!("{host} no responde (está en Cloudflare: puede que tu red la esté bloqueando)", "{host} isn't responding (it's behind Cloudflare: your network may be blocking it)")
        } else {
            trf!("{host} no responde (caída o bloqueada)", "{host} isn't responding (down or blocked)")
        };
    }
    if e.contains("SocketTimeoutException") || e.contains("timed out") || e.contains("timeout") {
        return tr!("la web tarda demasiado en responder", "the site took too long to respond").into();
    }
    if e.contains("UnknownHostException") {
        return tr!("no se encuentra la web (¿dominio cambiado o sin conexión?)", "site not found (domain changed, or no connection?)").into();
    }
    if let Some(code) = e.split("HTTP error ").nth(1).and_then(|c| c.get(..3)) {
        return trf!("la web ha respondido con un error {code}", "the site answered with error {code}");
    }
    if e.contains("NoClassDefFoundError") || e.contains("ClassNotFoundException") || e.contains("NoSuchMethodError") || e.contains("NoSuchFieldError") {
        return tr!("la extensión usa algo de Android que Han Thỏ aún no tiene (ver host.log)", "the extension needs an Android API Han Thỏ doesn't have yet (see host.log)").into();
    }
    let msg = e.rsplit_once("Exception: ").map(|(_, m)| m).unwrap_or(e);
    msg.chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::short_error;

    #[test]
    fn friendly_errors() {
        let cf = short_error("java.net.ConnectException: Failed to connect to mangafire.to/188.114.96.5:443");
        assert!(cf.starts_with("mangafire.to") && cf.contains("Cloudflare"), "{cf}");
        let other = short_error("java.net.ConnectException: Failed to connect to es.mi2manga.com/80.97.124.233:443");
        assert!(other.starts_with("es.mi2manga.com") && !other.contains("Cloudflare"), "{other}");
        assert!(short_error("eu.kanade.tachiyomi.network.HttpException: HTTP error 404").contains("404"));
        assert!(short_error("java.lang.NoClassDefFoundError: android/webkit/CookieManager").contains("Android"));
        assert_eq!(short_error("java.io.IOException: Cloudflare pide verificación"), "Cloudflare pide verificación");
    }
}

/// Normaliza un título para agrupar el mismo manga visto en varias fuentes.
pub fn normalize(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
