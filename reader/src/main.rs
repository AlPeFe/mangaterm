mod anilist;
mod anilist_auth;
mod cache;
mod catalog;
mod ext;
mod home;
mod host;
mod library;
mod model;
mod paths;
mod render;
mod search;
mod theme;
mod tui;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use host::Host;
use search::SearchEvent;

/// Han Thỏ: lector de manga para la terminal, con extensiones de Mihon/Keiyoushi y sync con AniList.
#[derive(Parser)]
#[command(name = "hantho", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Busca en todas las fuentes instaladas (sin TUI)
    Search {
        query: String,
        /// Solo fuentes de este idioma (p. ej. es)
        #[arg(long)]
        lang: Option<String>,
    },
    /// Gestiona extensiones
    #[command(subcommand)]
    Ext(ExtCmd),
    /// Cuenta de AniList
    #[command(subcommand)]
    Anilist(AnilistCmd),
}

#[derive(Subcommand)]
enum AnilistCmd {
    /// Inicia sesión: abre el navegador, aprobás y listo (también desde la TUI con `l`)
    Login {
        /// Client ID de AniList, si este binario no trae uno (se guarda para la próxima)
        #[arg(long)]
        client_id: Option<String>,
    },
    /// Muestra la sesión actual
    Status,
    /// Borra el token guardado
    Logout,
}

#[derive(Subcommand)]
enum ExtCmd {
    /// Descarga la última versión del catálogo de Keiyoushi
    Update,
    /// Busca extensiones en el catálogo
    Find {
        query: Option<String>,
        #[arg(long)]
        lang: Option<String>,
        /// Excluir extensiones NSFW
        #[arg(long)]
        sfw: bool,
    },
    /// Instala extensiones (por nombre o paquete); con --lang instala todas las de ese idioma
    Install {
        names: Vec<String>,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long)]
        sfw: bool,
    },
    /// Desinstala una extensión por paquete
    Remove { pkg: String },
    /// Lista las extensiones instaladas
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    paths::migrate_legacy();
    match cli.cmd {
        None => tui::run().await,
        Some(Cmd::Search { query, lang }) => cli_search(query, lang).await,
        Some(Cmd::Ext(cmd)) => ext_cmd(cmd).await,
        Some(Cmd::Anilist(cmd)) => anilist_cmd(cmd).await,
    }
}

async fn anilist_cmd(cmd: AnilistCmd) -> Result<()> {
    match cmd {
        AnilistCmd::Login { client_id } => {
            if let Some(id) = client_id {
                anilist_auth::save_client_id(&id)?;
            }
            let Some(id) = anilist_auth::client_id() else {
                bail!(
                    "falta registrar Han Thỏ en AniList (una sola vez):\n  \
                     1. {} → Create New Client\n  \
                     2. Name: Han Thỏ · Redirect URL: {}\n  \
                     3. hantho anilist login --client-id <ID>",
                    anilist_auth::DEVELOPER_URL,
                    anilist_auth::REDIRECT
                );
            };
            println!("✿ se abrió el navegador: aprobá el acceso en AniList y volvé acá…");
            let creds = anilist_auth::login(&id).await?;
            println!("✓ conectado como @{}", creds.user_name);
        }
        AnilistCmd::Status => match anilist::Credentials::load() {
            Some(c) => match anilist::viewer(&c.token).await {
                Ok((_, name)) => println!("✓ conectado como {name}"),
                Err(e) => println!("✗ {} (token guardado, pero {e})", c.user_name),
            },
            None => println!("sin sesión; usá `hantho anilist login`"),
        },
        AnilistCmd::Logout => {
            anilist::Credentials::delete();
            println!("sesión cerrada");
        }
    }
    Ok(())
}

async fn cli_search(query: String, lang: Option<String>) -> Result<()> {
    let installed = ext::list();
    if installed.is_empty() {
        bail!("no hay extensiones instaladas; probá `hantho ext install --lang es`");
    }
    let started = std::time::Instant::now();
    let host = Host::spawn().await?;
    let (mut sources, errors) = host.load_all(&installed).await;
    for (name, e) in errors {
        eprintln!("! {name}: {}", search::short_error(&e));
    }
    if let Some(lang) = &lang {
        sources.retain(|s| &s.info.lang == lang || s.info.lang == "all");
    }
    eprintln!("{} fuentes listas en {:.1}s; buscando…", sources.len(), started.elapsed().as_secs_f32());

    let mut rx = search::global(host, sources, query);
    while let Some(ev) = rx.recv().await {
        match ev {
            SearchEvent::Results { source, mangas } if !mangas.is_empty() => {
                println!("── {} ({}) · {} resultados", source.info.name, source.info.lang, mangas.len());
                for m in mangas.iter().take(5) {
                    println!("   {}", m.title);
                }
            }
            SearchEvent::Results { .. } => {}
            SearchEvent::Failed { source, error } => eprintln!("✗ {}: {error}", source.info.name),
            SearchEvent::Done => break,
        }
    }
    eprintln!("listo en {:.1}s", started.elapsed().as_secs_f32());
    Ok(())
}

async fn ext_cmd(cmd: ExtCmd) -> Result<()> {
    let matches = |e: &catalog::Extension, lang: &Option<String>, sfw: bool| {
        lang.as_ref().is_none_or(|l| e.langs().contains(&l.as_str())) && !(sfw && e.content_warning == 3)
    };
    match cmd {
        ExtCmd::Update => {
            let exts = catalog::update().await?;
            println!("catálogo actualizado: {} extensiones", exts.len());
        }
        ExtCmd::Find { query, lang, sfw } => {
            let catalog = catalog::load().await?;
            let found: Vec<_> = match &query {
                Some(q) => ext::find(&catalog, q),
                None => catalog.iter().collect(),
            };
            let installed = ext::list();
            for e in found.into_iter().filter(|e| matches(e, &lang, sfw)) {
                let mark = if installed.iter().any(|i| i.pkg == e.package_name) { "✓" } else { " " };
                println!("{mark} {:<32} {:<6} {:<6} {}", e.name, e.langs().join(","), e.warning_label(), e.package_name);
            }
        }
        ExtCmd::Install { names, lang, sfw } => {
            let catalog = catalog::load().await?;
            let mut targets: Vec<&catalog::Extension> = Vec::new();
            for name in &names {
                match ext::find(&catalog, name).as_slice() {
                    [] => eprintln!("✗ '{name}' no está en el catálogo"),
                    [one] => targets.push(one),
                    many => {
                        eprintln!("'{name}' es ambiguo:");
                        for e in many.iter().take(10) {
                            eprintln!("   {} ({})", e.name, e.package_name);
                        }
                    }
                }
            }
            if names.is_empty() && lang.is_some() {
                targets.extend(catalog.iter().filter(|e| matches(e, &lang, sfw)));
            }
            // Descargas en paralelo, de a 8
            use futures::StreamExt;
            let results: Vec<_> = futures::stream::iter(targets)
                .map(|e| async move { (e, ext::install(e).await) })
                .buffer_unordered(8)
                .collect()
                .await;
            for (e, res) in results {
                match res {
                    Ok(i) => println!("✓ {} {}", i.name, i.version_name),
                    Err(err) => eprintln!("✗ {}: {err}", e.name),
                }
            }
        }
        ExtCmd::Remove { pkg } => {
            if ext::uninstall(&pkg)? {
                println!("desinstalada {pkg}");
            } else {
                bail!("{pkg} no está instalada");
            }
        }
        ExtCmd::List => {
            for i in ext::list() {
                println!("{:<32} {:<8} {:<6} {}", i.name, i.version_name, i.langs.join(","), i.pkg);
            }
        }
    }
    Ok(())
}
