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
                    Err(_) => SearchEvent::Failed { source, error: "tiempo de espera agotado".into() },
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
    let msg = e.rsplit_once("Exception: ").map(|(_, m)| m).unwrap_or(e);
    msg.chars().take(80).collect()
}

/// Normaliza un título para agrupar el mismo manga visto en varias fuentes.
pub fn normalize(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
