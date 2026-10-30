//! Rutas de datos. Todo vive en `~/.mangaterm` (o `$MANGATERM_HOME`), compartido con ext-host.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("MANGATERM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().expect("sin directorio home").join(".mangaterm"))
}

pub fn index_file() -> PathBuf {
    home().join("index.pb")
}

pub fn extensions_dir() -> PathBuf {
    home().join("extensions")
}

pub fn installed_file() -> PathBuf {
    extensions_dir().join("installed.json")
}

pub fn pages_cache() -> PathBuf {
    home().join("cache").join("pages")
}

pub fn host_log() -> PathBuf {
    home().join("host.log")
}
