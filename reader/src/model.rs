//! Tipos que viajan por el protocolo de ext-host. `memo` se conserva tal cual: algunas
//! extensiones guardan ahí datos que necesitan en pedidos posteriores.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub base_url: Option<String>,
    pub supports_latest: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manga {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub memo: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Chapter {
    pub url: String,
    pub name: String,
    #[serde(default)]
    pub chapter_number: f32,
    #[serde(default)]
    pub scanlator: Option<String>,
    #[serde(default)]
    pub date_upload: i64,
    #[serde(default)]
    pub memo: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub index: i32,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MangasPage {
    pub mangas: Vec<Manga>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MangaDetails {
    pub manga: Manga,
    pub chapters: Vec<Chapter>,
}
