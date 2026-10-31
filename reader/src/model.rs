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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MangaDetails {
    pub manga: Manga,
    pub chapters: Vec<Chapter>,
}

/// Muchas fuentes no informan el número de capítulo (-1) pero sí el nombre ("Capítulo 104.5",
/// "Ch. 12", "Episodio 3"). Como hace Mihon, se deduce del nombre: primero detrás de una
/// palabra clave y, si no, el primer número suelto.
pub fn chapter_number_from_name(name: &str) -> Option<f32> {
    use std::sync::LazyLock;
    static KEYWORD: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)(?:cap[ií]tulo|cap\.?|chapter|chap\.?|ch\.?|episodio|episode|ep\.?|#)\s*(\d+(?:[.,]\d+)?)").unwrap()
    });
    static NUMBER: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?:^|[^\w.])(\d+(?:[.,]\d+)?)\b").unwrap());
    let capture = KEYWORD.captures(name).or_else(|| NUMBER.captures(name))?;
    capture[1].replace(',', ".").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::chapter_number_from_name as n;

    #[test]
    fn chapter_numbers_from_names() {
        assert_eq!(n("Capítulo 104"), Some(104.0));
        assert_eq!(n("Capítulo 107.50 Hipnosis seria"), Some(107.5));
        assert_eq!(n("Cap. 12 - El mar"), Some(12.0));
        assert_eq!(n("Vol.3 Ch.25: Diving"), Some(25.0));
        assert_eq!(n("Episodio 3"), Some(3.0));
        assert_eq!(n("#45"), Some(45.0));
        assert_eq!(n("12,5"), Some(12.5));
        assert_eq!(n("Oneshot"), None);
    }
}
