//! Biblioteca local: favoritos, progreso de lectura y vínculo con AniList.
//! Se guarda en `~/.mangaterm/library.json`.

use crate::model::{Chapter, Manga};
use crate::paths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Mismos estados que `MediaListStatus` de AniList.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    #[default]
    Current,
    Planning,
    Completed,
    Paused,
    Dropped,
    Repeating,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Current => "Leyendo",
            Status::Planning => "Planeado",
            Status::Completed => "Completado",
            Status::Paused => "Pausado",
            Status::Dropped => "Abandonado",
            Status::Repeating => "Releyendo",
        }
    }

    pub fn as_anilist(self) -> &'static str {
        match self {
            Status::Current => "CURRENT",
            Status::Planning => "PLANNING",
            Status::Completed => "COMPLETED",
            Status::Paused => "PAUSED",
            Status::Dropped => "DROPPED",
            Status::Repeating => "REPEATING",
        }
    }

    pub fn from_anilist(s: &str) -> Option<Status> {
        Some(match s {
            "CURRENT" => Status::Current,
            "PLANNING" => Status::Planning,
            "COMPLETED" => Status::Completed,
            "PAUSED" => Status::Paused,
            "DROPPED" => Status::Dropped,
            "REPEATING" => Status::Repeating,
            _ => return None,
        })
    }

    pub fn next(self) -> Status {
        match self {
            Status::Current => Status::Planning,
            Status::Planning => Status::Completed,
            Status::Completed => Status::Paused,
            Status::Paused => Status::Dropped,
            Status::Dropped => Status::Repeating,
            Status::Repeating => Status::Current,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastRead {
    pub chapter_url: String,
    pub chapter_name: String,
    pub page: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AniListLink {
    pub media_id: i64,
    pub title: String,
    /// Lo último que se envió a AniList (para mostrar si hay cambios sin sincronizar).
    pub synced_progress: Option<i32>,
    pub synced_status: Option<Status>,
    pub synced_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    pub source_id: String,
    pub source_name: String,
    pub lang: String,
    pub manga: Manga,
    pub added_at: u64,
    #[serde(default)]
    pub status: Status,
    /// Capítulos leídos: el mayor número de capítulo terminado (lo que AniList llama progress).
    #[serde(default)]
    pub progress: i32,
    #[serde(default)]
    pub last_read: Option<LastRead>,
    #[serde(default)]
    pub anilist: Option<AniListLink>,
}

impl Favorite {
    pub fn is(&self, source_id: &str, url: &str) -> bool {
        self.source_id == source_id && self.manga.url == url
    }

    /// Hay cambios locales que AniList todavía no tiene.
    pub fn needs_sync(&self) -> bool {
        self.anilist
            .as_ref()
            .is_some_and(|a| a.synced_progress != Some(self.progress) || a.synced_status != Some(self.status))
    }

    /// Registra que se terminó un capítulo: el progreso solo avanza.
    pub fn mark_chapter_read(&mut self, chapter: &Chapter) {
        if chapter.chapter_number >= 1.0 {
            self.progress = self.progress.max(chapter.chapter_number.floor() as i32);
        }
    }
}

#[derive(Default)]
pub struct Library {
    pub favorites: Vec<Favorite>,
}

impl Library {
    pub fn load() -> Library {
        let favorites = std::fs::read(paths::library_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Library { favorites }
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(paths::home())?;
        // Escribir a un temporal y renombrar: un corte a mitad no corrompe la biblioteca
        let tmp = paths::library_file().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.favorites)?)?;
        std::fs::rename(tmp, paths::library_file())?;
        Ok(())
    }

    pub fn find(&self, source_id: &str, url: &str) -> Option<usize> {
        self.favorites.iter().position(|f| f.is(source_id, url))
    }

    pub fn get_mut(&mut self, source_id: &str, url: &str) -> Option<&mut Favorite> {
        self.favorites.iter_mut().find(|f| f.is(source_id, url))
    }

    /// Agrega o quita. Devuelve si quedó como favorito.
    pub fn toggle(&mut self, source_id: &str, source_name: &str, lang: &str, manga: &Manga) -> bool {
        if let Some(i) = self.find(source_id, &manga.url) {
            self.favorites.remove(i);
            return false;
        }
        self.favorites.push(Favorite {
            source_id: source_id.into(),
            source_name: source_name.into(),
            lang: lang.into(),
            manga: manga.clone(),
            added_at: now(),
            status: Status::Current,
            progress: 0,
            last_read: None,
            anilist: None,
        });
        true
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(n: f32) -> Chapter {
        Chapter { chapter_number: n, ..Default::default() }
    }

    #[test]
    fn progress_only_moves_forward() {
        let mut lib = Library::default();
        lib.toggle("1", "Fuente", "es", &Manga { url: "/m".into(), title: "X".into(), ..Default::default() });
        let fav = lib.get_mut("1", "/m").unwrap();
        fav.mark_chapter_read(&chapter(12.0));
        fav.mark_chapter_read(&chapter(5.0));
        fav.mark_chapter_read(&chapter(13.5));
        fav.mark_chapter_read(&chapter(-1.0)); // número desconocido: se ignora
        assert_eq!(fav.progress, 13);
    }

    #[test]
    fn needs_sync_tracks_local_changes() {
        let mut lib = Library::default();
        lib.toggle("1", "Fuente", "es", &Manga { url: "/m".into(), ..Default::default() });
        let fav = lib.get_mut("1", "/m").unwrap();
        assert!(!fav.needs_sync(), "sin vincular no hay nada que sincronizar");
        fav.anilist = Some(AniListLink { media_id: 1, title: "X".into(), synced_progress: Some(0), synced_status: Some(Status::Current), synced_at: None });
        assert!(!fav.needs_sync());
        fav.progress = 3;
        assert!(fav.needs_sync());
        assert!(!lib.toggle("1", "Fuente", "es", &Manga { url: "/m".into(), ..Default::default() }), "segundo toggle quita");
        assert!(lib.favorites.is_empty());
    }

    #[test]
    fn status_roundtrip() {
        let mut s = Status::Current;
        for _ in 0..6 {
            assert_eq!(Status::from_anilist(s.as_anilist()), Some(s));
            s = s.next();
        }
        assert_eq!(s, Status::Current);
    }
}
