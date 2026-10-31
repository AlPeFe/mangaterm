//! Idioma de la interfaz: español si el sistema está en español, inglés en cualquier otro caso.
//! `HANTHO_LANG=es|en` lo fuerza.
//!
//! Los textos van en el código, en pareja: `tr!("español", "english")` para literales y
//! `trf!("{n} capítulos", "{n} chapters")` para los que llevan formato.

use std::sync::OnceLock;

static SPANISH: OnceLock<bool> = OnceLock::new();

pub fn es() -> bool {
    *SPANISH.get_or_init(|| {
        let tag = std::env::var("HANTHO_LANG")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(sys_locale::get_locale)
            .unwrap_or_default();
        is_spanish(&tag)
    })
}

/// `es`, `es-ES`, `es_ES.UTF-8`, `es-419`… (sin confundir con `est`, etc.)
fn is_spanish(tag: &str) -> bool {
    let tag = tag.to_ascii_lowercase();
    tag == "es" || tag.starts_with("es-") || tag.starts_with("es_") || tag.starts_with("es.") || tag == "spanish"
}

#[macro_export]
macro_rules! tr {
    ($es:expr, $en:expr $(,)?) => {
        if $crate::i18n::es() { $es } else { $en }
    };
}

#[macro_export]
macro_rules! trf {
    ($es:literal, $en:literal $(,)?) => {
        if $crate::i18n::es() { format!($es) } else { format!($en) }
    };
    ($es:literal, $en:literal, $($arg:tt)*) => {
        if $crate::i18n::es() { format!($es, $($arg)*) } else { format!($en, $($arg)*) }
    };
}

#[cfg(test)]
mod tests {
    use super::is_spanish;

    #[test]
    fn locale_tags() {
        for tag in ["es", "es-ES", "es_ES.UTF-8", "es-419", "ES-mx"] {
            assert!(is_spanish(tag), "{tag}");
        }
        for tag in ["en-US", "et-EE", "pt-BR", "", "C", "POSIX"] {
            assert!(!is_spanish(tag), "{tag}");
        }
    }
}
