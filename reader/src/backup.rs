//! Importar y exportar backups de Mihon (`.tachibk`): protobuf comprimido con gzip.
//!
//! Los números de campo replican `eu.kanade.tachiyomi.data.backup.models` de Mihon. Dos detalles:
//! - Mihon serializa con kotlinx (sin escribir valores por defecto) y `BackupManga.favorite`
//!   vale `true` por defecto: si el campo no está, el manga ES favorito.
//! - Las fuentes se identifican con el mismo id que usa Keiyoushi, así que se pueden instalar
//!   solas las extensiones que falten.

use crate::catalog::Extension;
use crate::library::{AniListLink, Favorite, LastRead, Library, Status};
use crate::model::Manga;
use crate::reads::{self, ReadChapter, ReadLog};
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use prost::Message;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// `TrackerManager`: AniList es el tracker 2.
const ANILIST_SYNC_ID: i32 = 2;

#[derive(Clone, PartialEq, Message)]
pub struct Backup {
    #[prost(message, repeated, tag = "1")]
    pub manga: Vec<BackupManga>,
    #[prost(message, repeated, tag = "101")]
    pub sources: Vec<BackupSource>,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupManga {
    #[prost(int64, tag = "1")]
    pub source: i64,
    #[prost(string, tag = "2")]
    pub url: String,
    #[prost(string, tag = "3")]
    pub title: String,
    #[prost(string, optional, tag = "4")]
    pub artist: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub author: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub description: Option<String>,
    #[prost(string, repeated, tag = "7")]
    pub genre: Vec<String>,
    #[prost(int32, tag = "8")]
    pub status: i32,
    #[prost(string, optional, tag = "9")]
    pub thumbnail_url: Option<String>,
    #[prost(int64, tag = "13")]
    pub date_added: i64,
    #[prost(message, repeated, tag = "16")]
    pub chapters: Vec<BackupChapter>,
    #[prost(message, repeated, tag = "18")]
    pub tracking: Vec<BackupTracking>,
    /// Por defecto `true` en Mihon: ausente = favorito.
    #[prost(bool, optional, tag = "100")]
    pub favorite: Option<bool>,
    #[prost(message, repeated, tag = "104")]
    pub history: Vec<BackupHistory>,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupChapter {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, optional, tag = "3")]
    pub scanlator: Option<String>,
    #[prost(bool, tag = "4")]
    pub read: bool,
    #[prost(bool, tag = "5")]
    pub bookmark: bool,
    #[prost(int64, tag = "6")]
    pub last_page_read: i64,
    #[prost(int64, tag = "8")]
    pub date_upload: i64,
    #[prost(float, tag = "9")]
    pub chapter_number: f32,
    #[prost(int64, tag = "10")]
    pub source_order: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupTracking {
    #[prost(int32, tag = "1")]
    pub sync_id: i32,
    #[prost(int64, tag = "2")]
    pub library_id: i64,
    #[prost(int32, tag = "3")]
    pub media_id_int: i32,
    #[prost(string, tag = "4")]
    pub tracking_url: String,
    #[prost(string, tag = "5")]
    pub title: String,
    #[prost(float, tag = "6")]
    pub last_chapter_read: f32,
    #[prost(int32, tag = "7")]
    pub total_chapters: i32,
    #[prost(int32, tag = "9")]
    pub status: i32,
    #[prost(int64, tag = "100")]
    pub media_id: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupHistory {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(int64, tag = "2")]
    pub last_read: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupSource {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int64, tag = "2")]
    pub source_id: i64,
}

/// Estados del tracker de AniList en Mihon (`Anilist.READING` …).
fn status_from_mihon(s: i32) -> Option<Status> {
    Some(match s {
        1 => Status::Current,
        2 => Status::Completed,
        3 => Status::Paused,
        4 => Status::Dropped,
        5 => Status::Planning,
        6 => Status::Repeating,
        _ => return None,
    })
}

fn status_to_mihon(s: Status) -> i32 {
    match s {
        Status::Current => 1,
        Status::Completed => 2,
        Status::Paused => 3,
        Status::Dropped => 4,
        Status::Planning => 5,
        Status::Repeating => 6,
    }
}

pub fn read_file(path: &Path) -> Result<Backup> {
    let raw = std::fs::read(path).with_context(|| format!("no se ha podido leer {}", path.display()))?;
    // Mihon siempre comprime, pero los backups viejos o editados pueden venir sin gzip
    let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        GzDecoder::new(raw.as_slice()).read_to_end(&mut out).context("el archivo está dañado (gzip)")?;
        out
    } else {
        raw
    };
    Backup::decode(bytes.as_slice()).context("no es un backup de Mihon (.tachibk) válido")
}

#[derive(Debug, Default)]
pub struct ImportSummary {
    pub added: usize,
    pub merged: usize,
    pub chapters_read: usize,
    pub anilist_links: usize,
    /// Extensiones que hay que instalar para abrir lo importado.
    pub needed_extensions: Vec<Extension>,
    /// Fuentes que ninguna extensión de Keiyoushi provee.
    pub unknown_sources: Vec<String>,
}

/// Incorpora el backup a la biblioteca y al registro de leídos (no guarda en disco).
pub fn import(backup: &Backup, catalog: &[Extension], installed: &[String], library: &mut Library, log: &mut ReadLog) -> ImportSummary {
    let mut summary = ImportSummary::default();
    let source_names: BTreeMap<i64, &str> = backup.sources.iter().map(|s| (s.source_id, s.name.as_str())).collect();

    for m in &backup.manga {
        let source_id = m.source.to_string();
        let key = reads::key(&source_id, &m.url);

        // Capítulos leídos (también de los que no son favoritos: es historial útil)
        for c in m.chapters.iter().filter(|c| c.read) {
            if !log.is_read(&key, &c.url) {
                summary.chapters_read += 1;
            }
            log.mark_raw(&key, c.url.clone(), ReadChapter { name: c.name.clone(), number: c.chapter_number });
        }
        if m.favorite == Some(false) {
            continue;
        }

        // ¿Qué extensión provee esta fuente?
        let found = catalog.iter().find_map(|e| e.sources.iter().find(|s| s.id == m.source).map(|s| (e, s)));
        let (source_name, lang) = match found {
            Some((ext, src)) => {
                if !installed.contains(&ext.package_name) && !summary.needed_extensions.iter().any(|e| e.package_name == ext.package_name) {
                    summary.needed_extensions.push(ext.clone());
                }
                (src.name.clone(), src.language.clone())
            }
            None => {
                let name = source_names.get(&m.source).map_or_else(|| format!("fuente {}", m.source), |n| n.to_string());
                if !summary.unknown_sources.contains(&name) {
                    summary.unknown_sources.push(name.clone());
                }
                (name, String::new())
            }
        };

        let anilist = m.tracking.iter().find(|t| t.sync_id == ANILIST_SYNC_ID);
        let tracked_progress = anilist.map_or(0, |t| t.last_chapter_read.floor() as i32);
        let progress = tracked_progress.max(log.max_number(&key));
        let status = anilist.and_then(|t| status_from_mihon(t.status)).unwrap_or(Status::Current);
        let link = anilist.map(|t| {
            let media_id = if t.media_id != 0 { t.media_id } else { t.media_id_int as i64 };
            AniListLink {
                media_id,
                title: if t.title.is_empty() { m.title.clone() } else { t.title.clone() },
                synced_progress: Some(tracked_progress),
                synced_status: status_from_mihon(t.status),
                synced_at: None,
            }
        });
        let last_read = m.history.iter().max_by_key(|h| h.last_read).and_then(|h| {
            let chapter = m.chapters.iter().find(|c| c.url == h.url)?;
            Some(LastRead { chapter_url: h.url.clone(), chapter_name: chapter.name.clone(), page: chapter.last_page_read.max(0) as usize })
        });

        match library.get_mut(&source_id, &m.url) {
            Some(fav) => {
                fav.progress = fav.progress.max(progress);
                if fav.anilist.is_none() && link.is_some() {
                    fav.anilist = link;
                    summary.anilist_links += 1;
                }
                if fav.last_read.is_none() {
                    fav.last_read = last_read;
                }
                summary.merged += 1;
            }
            None => {
                if link.is_some() {
                    summary.anilist_links += 1;
                }
                library.favorites.push(Favorite {
                    source_id,
                    source_name,
                    lang,
                    manga: Manga {
                        url: m.url.clone(),
                        title: m.title.clone(),
                        thumbnail_url: m.thumbnail_url.clone(),
                        author: m.author.clone(),
                        artist: m.artist.clone(),
                        status: m.status,
                        description: m.description.clone(),
                        genre: (!m.genre.is_empty()).then(|| m.genre.join(", ")),
                        memo: serde_json::Value::Null,
                    },
                    added_at: (m.date_added / 1000).max(0) as u64,
                    status,
                    progress,
                    last_read,
                    anilist: link,
                });
                summary.added += 1;
            }
        }
    }
    summary
}

/// Arma un backup de Mihon con la biblioteca: favoritos, capítulos leídos y vínculos con AniList.
pub fn export(library: &Library, log: &ReadLog) -> Backup {
    let mut sources = BTreeMap::new();
    let manga = library
        .favorites
        .iter()
        .filter_map(|fav| {
            let source: i64 = fav.source_id.parse().ok()?;
            sources.insert(source, fav.source_name.clone());
            let key = reads::key(&fav.source_id, &fav.manga.url);
            let chapters = log
                .chapters(&key)
                .map(|m| {
                    m.iter()
                        .map(|(url, c)| BackupChapter {
                            url: url.clone(),
                            name: c.name.clone(),
                            read: true,
                            last_page_read: fav.last_read.as_ref().filter(|l| &l.chapter_url == url).map_or(0, |l| l.page as i64),
                            chapter_number: c.number,
                            ..Default::default()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let tracking = fav
                .anilist
                .as_ref()
                .map(|a| BackupTracking {
                    sync_id: ANILIST_SYNC_ID,
                    media_id_int: a.media_id as i32,
                    media_id: a.media_id,
                    tracking_url: format!("https://anilist.co/manga/{}", a.media_id),
                    title: a.title.clone(),
                    last_chapter_read: fav.progress as f32,
                    status: status_to_mihon(fav.status),
                    ..Default::default()
                })
                .into_iter()
                .collect();
            let history = fav
                .last_read
                .as_ref()
                .map(|l| BackupHistory { url: l.chapter_url.clone(), last_read: (fav.added_at as i64).max(1) * 1000 })
                .into_iter()
                .collect();
            Some(BackupManga {
                source,
                url: fav.manga.url.clone(),
                title: fav.manga.title.clone(),
                artist: fav.manga.artist.clone(),
                author: fav.manga.author.clone(),
                description: fav.manga.description.clone(),
                genre: fav.manga.genre.as_deref().map(|g| g.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
                status: fav.manga.status,
                thumbnail_url: fav.manga.thumbnail_url.clone(),
                date_added: fav.added_at as i64 * 1000,
                chapters,
                tracking,
                favorite: Some(true),
                history,
            })
        })
        .collect();
    Backup { manga, sources: sources.into_iter().map(|(source_id, name)| BackupSource { name, source_id }).collect() }
}

pub fn write_file(backup: &Backup, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut gz = GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&backup.encode_to_vec())?;
    std::fs::write(path, gz.finish()?).with_context(|| format!("no se ha podido escribir {}", path.display()))?;
    Ok(())
}

/// Nombre por defecto, en la carpeta de descargas: `hantho_AAAA-MM-DD_HH-MM.tachibk`.
pub fn default_export_path() -> PathBuf {
    let dir = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
    let secs = crate::library::now();
    let (y, mo, d) = civil_from_days((secs / 86_400) as i64);
    let (h, mi) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    dir.join(format!("hantho_{y:04}-{mo:02}-{d:02}_{h:02}-{mi:02}.tachibk"))
}

/// Backups de Mihon en las carpetas habituales (descargas, documentos, home), los más nuevos primero.
pub fn find_backups() -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = [dirs::download_dir(), dirs::document_dir(), dirs::home_dir()]
        .into_iter()
        .flatten()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "tachibk"))
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path()))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.dedup_by(|a, b| a.1 == b.1);
    found.into_iter().map(|(_, p)| p).collect()
}

/// Días desde 1970-01-01 → (año, mes, día), en UTC (algoritmo de Howard Hinnant).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Extension as CatExt, Source as CatSource};

    fn catalog() -> Vec<CatExt> {
        vec![CatExt {
            name: "MangaTV".into(),
            package_name: "eu.kanade.tachiyomi.extension.es.mangatv".into(),
            sources: vec![CatSource { id: 42, name: "Manga TV".into(), language: "es".into(), home_url: String::new() }],
            ..Default::default()
        }]
    }

    fn mihon_backup() -> Backup {
        Backup {
            manga: vec![
                BackupManga {
                    source: 42,
                    url: "/manga/grand-blue".into(),
                    title: "Grand Blue".into(),
                    genre: vec!["Comedia".into(), "Buceo".into()],
                    favorite: None, // así lo escribe Mihon: true por defecto
                    chapters: vec![
                        BackupChapter { url: "/c1".into(), name: "Cap 1".into(), read: true, chapter_number: 1.0, ..Default::default() },
                        BackupChapter { url: "/c2".into(), name: "Cap 2".into(), read: true, last_page_read: 7, chapter_number: 2.0, ..Default::default() },
                        BackupChapter { url: "/c3".into(), name: "Cap 3".into(), read: false, chapter_number: 3.0, ..Default::default() },
                    ],
                    tracking: vec![BackupTracking { sync_id: 2, media_id: 87395, title: "Grand Blue".into(), last_chapter_read: 2.0, status: 1, ..Default::default() }],
                    history: vec![BackupHistory { url: "/c2".into(), last_read: 1_700_000_000_000 }],
                    ..Default::default()
                },
                BackupManga { source: 99, url: "/otro".into(), title: "Fuente rara".into(), ..Default::default() },
                BackupManga { source: 42, url: "/visto".into(), title: "No favorito".into(), favorite: Some(false), ..Default::default() },
            ],
            sources: vec![BackupSource { name: "Fuente Rara".into(), source_id: 99 }],
        }
    }

    #[test]
    fn imports_mihon_backup() {
        // Ida y vuelta por gzip + protobuf, como un archivo real
        let dir = std::env::temp_dir().join(format!("hantho-test-{}", std::process::id()));
        let path = dir.join("mihon.tachibk");
        write_file(&mihon_backup(), &path).unwrap();
        let backup = read_file(&path).unwrap();

        let (mut lib, mut log) = (Library::default(), ReadLog::default());
        let s = import(&backup, &catalog(), &[], &mut lib, &mut log);
        assert_eq!((s.added, s.chapters_read, s.anilist_links), (2, 2, 1));
        assert_eq!(s.needed_extensions.len(), 1, "hay que instalar MangaTV");
        assert_eq!(s.unknown_sources, vec!["Fuente Rara".to_string()]);

        let fav = &lib.favorites[0];
        assert_eq!((fav.source_name.as_str(), fav.progress, fav.status), ("Manga TV", 2, Status::Current));
        assert_eq!(fav.anilist.as_ref().unwrap().media_id, 87395);
        assert_eq!(fav.last_read.as_ref().unwrap().page, 7);
        assert!(log.is_read(&reads::key("42", "/manga/grand-blue"), "/c2"));
        assert!(!lib.favorites.iter().any(|f| f.manga.title == "No favorito"));

        // Importar dos veces no duplica
        let again = import(&backup, &catalog(), &[], &mut lib, &mut log);
        assert_eq!((again.added, again.merged, again.chapters_read), (0, 2, 0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn export_roundtrip() {
        let (mut lib, mut log) = (Library::default(), ReadLog::default());
        import(&mihon_backup(), &catalog(), &[], &mut lib, &mut log);
        let exported = export(&lib, &log);
        let gb = exported.manga.iter().find(|m| m.title == "Grand Blue").unwrap();
        assert_eq!(gb.favorite, Some(true));
        assert_eq!(gb.chapters.iter().filter(|c| c.read).count(), 2);
        assert_eq!((gb.tracking[0].sync_id, gb.tracking[0].media_id, gb.tracking[0].status), (2, 87395, 1));

        // Lo exportado se vuelve a importar igual
        let (mut lib2, mut log2) = (Library::default(), ReadLog::default());
        let s = import(&Backup::decode(exported.encode_to_vec().as_slice()).unwrap(), &catalog(), &[], &mut lib2, &mut log2);
        assert_eq!((s.added, s.chapters_read, s.anilist_links), (2, 2, 1));
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_757), (2026, 10, 31));
    }
}
