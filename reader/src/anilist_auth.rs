//! Login con AniList por navegador, sin copiar ni pegar nada.
//!
//! OAuth implícito: AniList redirige a `http://localhost:47913/callback#access_token=…`.
//! El fragmento (`#…`) nunca llega al servidor, así que esa página trae un JS mínimo que lo
//! lee y lo manda por POST a `/token`. El servidor escucha solo en loopback y se cierra apenas
//! recibe el token (o a los 5 minutos).
//!
//! AniList exige que la app esté registrada: hace falta un client ID con esa redirect URL.
//! Lo registra el dueño del proyecto una vez y queda en `PROJECT_CLIENT_ID`; en el implicit
//! grant no hay secreto, así que el ID es público por diseño.

use crate::anilist::{self, Credentials};
use crate::paths;
use anyhow::{anyhow, bail, Context, Result};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const PORT: u16 = 47913;
pub const REDIRECT: &str = "http://localhost:47913/callback";
pub const DEVELOPER_URL: &str = "https://anilist.co/settings/developer";
const TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Client ID de Han Thỏ registrado en AniList (Redirect URL = `REDIRECT`).
pub const PROJECT_CLIENT_ID: Option<&str> = None;

/// El client ID a usar: `HANTHO_ANILIST_CLIENT_ID`, el configurado localmente, o el del proyecto.
pub fn client_id() -> Option<String> {
    std::env::var("HANTHO_ANILIST_CLIENT_ID")
        .ok()
        .or_else(|| std::fs::read_to_string(paths::anilist_client_file()).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| is_valid_client_id(s))
        .or(PROJECT_CLIENT_ID.map(String::from))
}

pub fn is_valid_client_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.chars().all(|c| c.is_ascii_digit())
}

pub fn save_client_id(id: &str) -> Result<()> {
    if !is_valid_client_id(id) {
        bail!("el client ID de AniList es un número");
    }
    std::fs::create_dir_all(paths::home())?;
    std::fs::write(paths::anilist_client_file(), id)?;
    Ok(())
}

/// Abre el navegador, espera a que el usuario apruebe y guarda la sesión.
pub async fn login(client_id: &str) -> Result<Credentials> {
    // "localhost" puede resolver a IPv4 o IPv6 según el sistema: escuchar en los dos
    let v4 = TcpListener::bind(("127.0.0.1", PORT)).await.with_context(|| {
        format!("el puerto {PORT} está ocupado (¿hay otro login abierto?); cerralo y probá de nuevo")
    })?;
    let v6 = TcpListener::bind(("::1", PORT)).await.ok();

    anilist::open_browser(&anilist::authorize_url(client_id));
    let token = tokio::time::timeout(TIMEOUT, serve(v4, v6))
        .await
        .map_err(|_| anyhow!("se venció el tiempo de espera (5 min) sin que se aprobara el acceso"))??;

    let (user_id, user_name) = anilist::viewer(&token).await.context("AniList no aceptó el token")?;
    let creds = Credentials { client_id: client_id.to_string(), token, user_id, user_name };
    creds.save()?;
    Ok(creds)
}

/// Atiende conexiones hasta recibir el token (o un error de autorización).
async fn serve(v4: TcpListener, v6: Option<TcpListener>) -> Result<String> {
    loop {
        let (stream, _) = match &v6 {
            Some(v6) => tokio::select! {
                r = v4.accept() => r?,
                r = v6.accept() => r?,
            },
            None => v4.accept().await?,
        };
        if let Some(result) = handle(stream).await? {
            return result;
        }
    }
}

/// Una conexión HTTP. Devuelve `Some` cuando llegó el resultado del login.
async fn handle(stream: TcpStream) -> Result<Option<Result<String>>> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0).min(16 * 1024);
            }
        }
    }
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));

    let (status, body, result) = match (method, path.split('?').next().unwrap_or("")) {
        ("GET", "/callback") => ("200 OK", page(CALLBACK_BODY), None),
        ("POST", "/token") => {
            let mut buf = vec![0; content_length];
            reader.read_exact(&mut buf).await?;
            let form = String::from_utf8_lossy(&buf).into_owned();
            let field = |key: &str| {
                form.split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| urldecode(v))
            };
            match (field("token"), field("error")) {
                (Some(token), _) if token.len() > 20 => ("200 OK", String::from("ok"), Some(Ok(token))),
                (_, Some(error)) => {
                    let msg = if error == "access_denied" { "cancelaste el acceso en AniList".into() } else { error };
                    ("200 OK", String::from("ok"), Some(Err(anyhow!(msg))))
                }
                _ => ("400 Bad Request", String::from("sin token"), None),
            }
        }
        _ => ("404 Not Found", page("<p>nada por acá ✿</p>"), None),
    };

    let content_type = if body.starts_with("<!doctype") { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = reader.into_inner();
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(result)
}

fn urldecode(s: &str) -> String {
    let bytes = s.replace('+', " ").into_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&String::from_utf8_lossy(&bytes[i + 1..i + 3]), 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn page(body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>Han Thỏ · AniList</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <style>body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#241a22;color:#f4e8f0;\
         font:16px/1.5 system-ui,sans-serif}}main{{text-align:center;padding:2rem;border:1px solid #96607c;\
         border-radius:16px;max-width:26rem}}h1{{color:#ffb6d5;font-size:1.6rem;margin:.2rem 0 .8rem}}\
         .flor{{font-size:2.5rem}}.ok{{color:#a0ecc8}}.err{{color:#ff828c}}small{{color:#a08c9c}}</style></head>\
         <body><main>{body}</main></body></html>"
    )
}

/// Lee el token del fragmento y se lo pasa al servidor local; después avisa al usuario.
const CALLBACK_BODY: &str = r#"<div class="flor">✿</div><h1>Han Thỏ</h1><p id="msg">conectando con AniList…</p>
<script>
const hash = new URLSearchParams(location.hash.slice(1));
const query = new URLSearchParams(location.search);
const token = hash.get('access_token');
const error = hash.get('error') || query.get('error') || (token ? null : 'AniList no devolvió un token');
const body = token ? 'token=' + encodeURIComponent(token) : 'error=' + encodeURIComponent(error);
history.replaceState(null, '', location.pathname); // que el token no quede en el historial
fetch('/token', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body })
  .then(() => {
    const msg = document.getElementById('msg');
    msg.className = token ? 'ok' : 'err';
    msg.innerHTML = token
      ? '✓ ¡listo! ya podés cerrar esta pestaña y volver a la terminal'
      : '✕ ' + error.replace(/[<>&]/g, '') + '<br><small>volvé a la terminal para reintentar</small>';
  })
  .catch(() => { document.getElementById('msg').textContent = '✕ no se pudo hablar con Han Thỏ (¿se cerró?)'; });
</script>"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Simula lo que hace el navegador después de "Approve": cargar /callback y mandar el token.
    async fn browser_flow(form: &'static str) -> Result<String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve(listener, None));

        let http = reqwest::Client::new();
        let page = http.get(format!("{base}/callback")).send().await.unwrap().text().await.unwrap();
        assert!(page.contains("access_token") && page.contains("fetch('/token'"), "la página lee el fragmento y lo envía");
        let resp = http
            .post(format!("{base}/token"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        server.await.unwrap()
    }

    #[tokio::test]
    async fn receives_token_from_browser() {
        let token = browser_flow("token=eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.abc%2Bdef").await.unwrap();
        assert_eq!(token, "eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.abc+def");
    }

    #[tokio::test]
    async fn reports_denied_access() {
        let err = browser_flow("error=access_denied").await.unwrap_err();
        assert!(err.to_string().contains("cancelaste"));
    }

    #[test]
    fn client_id_validation() {
        assert!(is_valid_client_id("12345"));
        assert!(!is_valid_client_id(""));
        assert!(!is_valid_client_id("abc"));
        assert!(!is_valid_client_id("https://anilist.co"));
    }
}
