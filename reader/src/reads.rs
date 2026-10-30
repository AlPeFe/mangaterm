//! Capítulos leídos, por manga (favorito o no). Se guarda en `~/.hantho/read.json`.
//! Guarda nombre y número de cada capítulo leído para poder exportarlos a Mihon sin volver a la fuente.

use crate::model::Chapter;
use crate::paths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadChapter {
    pub name: String,
    pub number: f32,
}

#[derive(Default)]
pub struct ReadLog {
    /// "fuente|url del manga" → (url del capítulo → datos)
    mangas: HashMap<String, BTreeMap<String, ReadChapter>>,
}

pub fn key(source_id: &str, manga_url: &str) -> String {
    format!("{source_id}|{manga_url}")
}

impl ReadLog {
    pub fn load() -> ReadLog {
        let mangas = std::fs::read(paths::read_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        ReadLog { mangas }
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(paths::home())?;
        let tmp = paths::read_file().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&self.mangas)?)?;
        std::fs::rename(tmp, paths::read_file())?;
        Ok(())
    }

    pub fn is_read(&self, key: &str, chapter_url: &str) -> bool {
        self.mangas.get(key).is_some_and(|m| m.contains_key(chapter_url))
    }

    pub fn chapters(&self, key: &str) -> Option<&BTreeMap<String, ReadChapter>> {
        self.mangas.get(key)
    }

    /// Marca como leído. Devuelve si cambió algo.
    pub fn mark(&mut self, key: &str, chapter: &Chapter) -> bool {
        let entry = ReadChapter { name: chapter.name.clone(), number: chapter.chapter_number };
        self.mangas.entry(key.to_string()).or_default().insert(chapter.url.clone(), entry).is_none()
    }

    pub fn mark_raw(&mut self, key: &str, url: String, read: ReadChapter) {
        self.mangas.entry(key.to_string()).or_default().insert(url, read);
    }

    pub fn unmark(&mut self, key: &str, chapter_url: &str) -> bool {
        self.mangas.get_mut(key).is_some_and(|m| m.remove(chapter_url).is_some())
    }

    /// Mayor número de capítulo leído (lo que AniList llama progress).
    pub fn max_number(&self, key: &str) -> i32 {
        self.mangas
            .get(key)
            .map(|m| m.values().filter(|c| c.number >= 1.0).map(|c| c.number.floor() as i32).max().unwrap_or(0))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(url: &str, n: f32) -> Chapter {
        Chapter { url: url.into(), name: format!("Cap {n}"), chapter_number: n, ..Default::default() }
    }

    #[test]
    fn mark_unmark_and_progress() {
        let mut log = ReadLog::default();
        let k = key("1", "/m");
        assert!(log.mark(&k, &ch("/c1", 1.0)));
        assert!(!log.mark(&k, &ch("/c1", 1.0)), "marcar dos veces no cambia nada");
        log.mark(&k, &ch("/c12", 12.5));
        log.mark(&k, &ch("/extra", -1.0));
        assert!(log.is_read(&k, "/c12"));
        assert_eq!(log.chapters(&k).unwrap().len(), 3);
        assert_eq!(log.max_number(&k), 12);
        assert!(log.unmark(&k, "/c12"));
        assert_eq!(log.max_number(&k), 1);
        assert!(!log.is_read(&key("2", "/m"), "/c1"), "cada manga tiene su registro");
    }
}
