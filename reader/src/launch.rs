//! Abrir el lector "al lado": divide la ventana de la terminal actual y arranca
//! `hantho open …` en el panel nuevo. Lo usa el servidor MCP para que un agente
//! (Claude Code, Codex, …) abra el manga junto al chat.

use crate::library::Library;
use crate::tui::{ChapterTarget, OpenTarget};
use anyhow::{bail, Result};
use std::process::Command;

/// Busca qué abrir: por fuente+URL, o por título entre los favoritos.
pub fn resolve(library: &Library, query: Option<&str>, source: Option<&str>, url: Option<&str>, title: Option<&str>, chapter: ChapterTarget) -> Result<OpenTarget> {
    if let (Some(source), Some(url)) = (source, url) {
        let fav = library.favorites.iter().find(|f| f.is(source, url));
        return Ok(OpenTarget {
            source_id: source.to_string(),
            source_name: fav.map_or_else(|| source.to_string(), |f| f.source_name.clone()),
            lang: fav.map(|f| f.lang.clone()).unwrap_or_default(),
            manga_url: url.to_string(),
            title: title.map(String::from).or(fav.map(|f| f.manga.title.clone())).unwrap_or_else(|| url.to_string()),
            chapter,
        });
    }
    let Some(query) = query.map(str::trim).filter(|q| !q.is_empty()) else {
        bail!("indica un título de tu biblioteca, o --source y --url");
    };
    let q = crate::search::normalize(query);
    let mut matches: Vec<_> = library.favorites.iter().filter(|f| crate::search::normalize(&f.manga.title).contains(&q)).collect();
    // Coincidencia exacta primero
    matches.sort_by_key(|f| crate::search::normalize(&f.manga.title) != q);
    match matches.first() {
        Some(fav) => Ok(OpenTarget {
            source_id: fav.source_id.clone(),
            source_name: fav.source_name.clone(),
            lang: fav.lang.clone(),
            manga_url: fav.manga.url.clone(),
            title: fav.manga.title.clone(),
            chapter,
        }),
        None => bail!("«{query}» no está en tu biblioteca; búscalo y añádelo a favoritos, o pasa --source y --url"),
    }
}

/// Argumentos de `hantho open` que reproducen el destino.
pub fn open_args(target: &OpenTarget) -> Vec<String> {
    let mut args = vec![
        "open".to_string(),
        "--source".into(),
        target.source_id.clone(),
        "--url".into(),
        target.manga_url.clone(),
        "--title".into(),
        target.title.clone(),
    ];
    match &target.chapter {
        ChapterTarget::None => {}
        ChapterTarget::Continue => args.push("--continue".into()),
        ChapterTarget::Number(n) => args.extend(["--chapter".into(), n.to_string()]),
        ChapterTarget::Url(u) => args.extend(["--chapter-url".into(), u.clone()]),
    }
    args
}

#[derive(Clone, Copy, PartialEq)]
pub enum Placement {
    /// Panel al lado, en la misma ventana (si la terminal lo permite).
    Side,
    /// Ventana nueva.
    Window,
}

/// Abre el lector y devuelve una descripción de dónde lo abrió.
pub fn launch(target: &OpenTarget, placement: Placement) -> Result<String> {
    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let args = open_args(target);
    let env = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    let mut tried = Vec::new();

    if placement == Placement::Side {
        // Windows Terminal: la sesión actual tiene WT_SESSION; `-w 0` = la ventana más reciente
        if env("WT_SESSION") {
            let mut cmd = Command::new("wt.exe");
            cmd.args(["-w", "0", "split-pane", "--vertical", "--size", "0.5", "--title", "Han Thỏ", "--", &exe]);
            cmd.args(args.iter().map(|a| wt_escape(a)));
            if spawn(cmd, &mut tried, "wt") {
                return Ok("panel lateral de Windows Terminal".into());
            }
        }
        if env("TMUX") {
            let line = std::iter::once(exe.as_str()).chain(args.iter().map(String::as_str)).map(shell_quote).collect::<Vec<_>>().join(" ");
            let mut cmd = Command::new("tmux");
            cmd.args(["split-window", "-h", "-l", "50%", &line]);
            if spawn(cmd, &mut tried, "tmux") {
                return Ok("panel lateral de tmux".into());
            }
        }
        if env("WEZTERM_PANE") {
            let mut cmd = Command::new("wezterm");
            cmd.args(["cli", "split-pane", "--right", "--percent", "50", "--", &exe]).args(&args);
            if spawn(cmd, &mut tried, "wezterm") {
                return Ok("panel lateral de WezTerm".into());
            }
        }
        if env("ZELLIJ") {
            let mut cmd = Command::new("zellij");
            cmd.args(["run", "--direction", "right", "--name", "Han Thỏ", "--", &exe]).args(&args);
            if spawn(cmd, &mut tried, "zellij") {
                return Ok("panel lateral de zellij".into());
            }
        }
        if env("KITTY_WINDOW_ID") {
            // Requiere `allow_remote_control yes` en kitty.conf
            let mut cmd = Command::new("kitty");
            cmd.args(["@", "launch", "--location=vsplit", "--cwd=current", "--title", "Han Thỏ", &exe]).args(&args);
            if spawn(cmd, &mut tried, "kitty") {
                return Ok("panel lateral de kitty".into());
            }
        }
    }

    // Ventana nueva
    #[cfg(windows)]
    {
        let mut cmd = Command::new("wt.exe");
        cmd.args(["-w", "new", "--title", "Han Thỏ", "--", &exe]).args(args.iter().map(|a| wt_escape(a)));
        if spawn(cmd, &mut tried, "wt") {
            return Ok("ventana nueva de Windows Terminal".into());
        }
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "Han Thỏ", &exe]).args(&args);
        if spawn(cmd, &mut tried, "cmd") {
            return Ok("ventana nueva de consola (sin Windows Terminal no hay imágenes Sixel)".into());
        }
    }
    #[cfg(not(windows))]
    {
        for (term, flag) in [("x-terminal-emulator", "-e"), ("foot", ""), ("wezterm", "start --"), ("kitty", ""), ("konsole", "-e"), ("gnome-terminal", "--"), ("xterm", "-e")] {
            let mut cmd = Command::new(term);
            cmd.args(flag.split_whitespace()).arg(&exe).args(&args);
            if spawn(cmd, &mut tried, term) {
                return Ok(format!("ventana nueva de {term}"));
            }
        }
    }
    bail!(
        "no se ha podido abrir una terminal (probado: {}). Ábrelo a mano con: {} {}",
        if tried.is_empty() { "nada".to_string() } else { tried.join(", ") },
        exe,
        args.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ")
    )
}

fn spawn(mut cmd: Command, tried: &mut Vec<String>, name: &str) -> bool {
    tried.push(name.to_string());
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    // El lector no tiene que heredar el stdio del servidor MCP
    match cmd.spawn() {
        Ok(mut child) => {
            // Los lanzadores (wt, tmux, wezterm cli…) terminan enseguida; si fallan, probar el siguiente
            match child.wait_timeout_ok() {
                Some(status) => status,
                None => true,
            }
        }
        Err(_) => false,
    }
}

trait WaitBriefly {
    /// `Some(éxito)` si terminó en menos de 3 s; `None` si sigue corriendo (cuenta como éxito).
    fn wait_timeout_ok(&mut self) -> Option<bool>;
}

impl WaitBriefly for std::process::Child {
    fn wait_timeout_ok(&mut self) -> Option<bool> {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(3) {
            match self.try_wait() {
                Ok(Some(status)) => return Some(status.success()),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(_) => return Some(false),
            }
        }
        None
    }
}

/// `wt.exe` usa `;` para separar subcomandos: hay que escaparlo dentro de los argumentos.
fn wt_escape(arg: &str) -> String {
    arg.replace(';', "\\;")
}

/// Comillas simples de shell POSIX (para tmux, que recibe una línea de comando).
fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c)) {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Manga;

    #[test]
    fn resolves_by_title_and_builds_args() {
        let mut lib = Library::default();
        lib.toggle("42", "Manga TV", "es", &Manga { url: "/manga/grand-blue".into(), title: "Grand Blue".into(), ..Default::default() });
        lib.toggle("42", "Manga TV", "es", &Manga { url: "/manga/grand-blue-extra".into(), title: "Grand Blue Extra".into(), ..Default::default() });

        let t = resolve(&lib, Some("grand blue"), None, None, None, ChapterTarget::Continue).unwrap();
        assert_eq!(t.manga_url, "/manga/grand-blue", "la coincidencia exacta gana");
        assert_eq!(
            open_args(&t),
            ["open", "--source", "42", "--url", "/manga/grand-blue", "--title", "Grand Blue", "--continue"]
        );
        assert!(resolve(&lib, Some("one piece"), None, None, None, ChapterTarget::None).is_err());

        let direct = resolve(&lib, None, Some("7"), Some("/x"), Some("X"), ChapterTarget::Number(12.0)).unwrap();
        assert_eq!(open_args(&direct).last().unwrap(), "12");
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/manga/abc"), "/manga/abc");
        assert_eq!(shell_quote("Grand Blue"), "'Grand Blue'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(wt_escape("a;b"), "a\\;b");
    }
}
