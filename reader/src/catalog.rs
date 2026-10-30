//! Catálogo de extensiones de Keiyoushi (`index.pb`: protobuf comprimido con gzip).
//! Schema oficial en `proto/index.proto`; acá se replica a mano con prost.

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use prost::Message;
use std::io::Read;

pub const INDEX_URL: &str = "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.pb";

#[derive(Clone, PartialEq, Message)]
pub struct Index {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub badge_label: String,
    #[prost(string, tag = "3")]
    pub signing_key: String,
    // En el .proto es un oneof (101 lista / 102 url); en la práctica siempre viene la lista
    #[prost(message, optional, tag = "101")]
    pub extension_list: Option<ExtensionList>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ExtensionList {
    #[prost(message, repeated, tag = "1")]
    pub extensions: Vec<Extension>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Extension {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub package_name: String,
    #[prost(message, optional, tag = "3")]
    pub resources: Option<Resources>,
    #[prost(string, tag = "4")]
    pub extension_lib: String,
    #[prost(int64, tag = "5")]
    pub version_code: i64,
    #[prost(string, tag = "6")]
    pub version_name: String,
    #[prost(int32, tag = "7")]
    pub content_warning: i32,
    #[prost(message, repeated, tag = "8")]
    pub sources: Vec<Source>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Resources {
    #[prost(string, tag = "1")]
    pub apk_url: String,
    #[prost(string, tag = "2")]
    pub icon_url: String,
    #[prost(string, tag = "501")]
    pub jar_url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Source {
    #[prost(int64, tag = "1")]
    pub id: i64,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub language: String,
    #[prost(string, tag = "4")]
    pub home_url: String,
}

impl Extension {
    pub fn jar_url(&self) -> Option<&str> {
        self.resources.as_ref().map(|r| r.jar_url.as_str()).filter(|u| !u.is_empty())
    }

    pub fn langs(&self) -> Vec<&str> {
        let mut langs: Vec<&str> = self.sources.iter().map(|s| s.language.as_str()).collect();
        langs.dedup();
        langs
    }

    pub fn warning_label(&self) -> &'static str {
        match self.content_warning {
            1 => "safe",
            2 => "mixed",
            3 => "nsfw",
            _ => "?",
        }
    }
}

pub async fn update() -> Result<Vec<Extension>> {
    let bytes = reqwest::get(INDEX_URL).await?.error_for_status()?.bytes().await?;
    let exts = decode(&bytes)?;
    std::fs::create_dir_all(crate::paths::home())?;
    std::fs::write(crate::paths::index_file(), &bytes)?;
    Ok(exts)
}

/// Lee el índice local; si no existe todavía, lo descarga.
pub async fn load() -> Result<Vec<Extension>> {
    match std::fs::read(crate::paths::index_file()) {
        Ok(bytes) => decode(&bytes),
        Err(_) => update().await,
    }
}

fn decode(gz: &[u8]) -> Result<Vec<Extension>> {
    let mut raw = Vec::new();
    GzDecoder::new(gz).read_to_end(&mut raw).context("index.pb no es gzip válido")?;
    let index = Index::decode(raw.as_slice()).context("index.pb no es un protobuf válido")?;
    Ok(index.extension_list.map(|l| l.extensions).unwrap_or_default())
}
