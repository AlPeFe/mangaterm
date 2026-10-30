<div align="center">

<img src="assets/logo.svg" alt="Logo de Han Thỏ: una flor de sakura en una ventana de terminal" width="128" height="128">

# Han Thỏ

**Manga en tu terminal, con las extensiones de Mihon y un toque de sakura.**

[![CI](https://github.com/AlPeFe/mangaterm/actions/workflows/ci.yml/badge.svg)](https://github.com/AlPeFe/mangaterm/actions/workflows/ci.yml)
[![Licencia: Apache-2.0](https://img.shields.io/badge/licencia-Apache--2.0-FFB6D5?labelColor=241A22)](LICENSE)
![Linux](https://img.shields.io/badge/Linux-✓-A0ECC8?labelColor=241A22&logo=linux&logoColor=white)
![Windows](https://img.shields.io/badge/Windows-✓-C8B2FF?labelColor=241A22&logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-2024-FFCCAA?labelColor=241A22&logo=rust&logoColor=white)

</div>

---

**Han Thỏ** (en la terminal, `hantho`) es un lector de manga para la terminal. Busca en cientos de fuentes a la vez usando las
mismas extensiones de [Keiyoushi](https://github.com/keiyoushi/extensions) que usa
[Mihon](https://github.com/mihonapp/mihon) en Android, muestra las páginas como imágenes de verdad
(Sixel o Kitty) y lleva tu progreso a [AniList](https://anilist.co).

### ¿Por qué?

- **Sin salir de la terminal.** Si vivís en una consola (o por SSH), no hace falta abrir el navegador
  ni el celular para leer un capítulo.
- **El ecosistema de Mihon, en el escritorio.** En lugar de reescribir scrapers, Han Thỏ ejecuta las
  extensiones existentes (más de 1400, en 150 idiomas) en una JVM normal. Cuando una fuente cambia,
  se actualiza la extensión y listo.
- **Pensado para manga.** Un render Sixel propio, ajustado para páginas en blanco y negro: sin moiré en
  las tramas ni ruido de dithering.

## Funciones

- 🌸 **TUI** con tema rosa pastel, pantalla de inicio animada, biblioteca, extensiones y AniList.
- 🔎 **Búsqueda global** en todas las fuentes instaladas en paralelo (≈165 fuentes en español en ~25 s),
  con resultados agrupados por obra.
- 📖 **Lector** con imágenes Sixel o Kitty; fallback a bloques de color si la terminal no tiene gráficos.
  Precarga de páginas siguientes.
- 💗 **Biblioteca**: favoritos, estado de lectura (leyendo, en pausa, completado…), "continuar" donde dejaste.
- 🧩 **Extensiones** de Keiyoushi: instalar por nombre o por idioma, buscar, actualizar, filtrar NSFW.
- ☁️ **AniList**: sincronización manual de estado y capítulos leídos, que nunca hace retroceder tu progreso.
- 🐢 **Respetuoso con los servicios**: rate limiting y caché tanto para AniList como para las fuentes.
- 🐧🪟 **Linux y Windows**, probado en CI en ambos.

## Capturas

<!-- Reemplazar estas imágenes por capturas reales (960×540 o similar). -->

| Inicio | Búsqueda global |
|---|---|
| ![Pantalla de inicio](assets/screenshots/inicio.png) | ![Búsqueda global](assets/screenshots/busqueda.png) |
| **Lector** | **AniList** |
| ![Lector de páginas](assets/screenshots/lector.png) | ![Vinculación con AniList](assets/screenshots/anilist.png) |

## Requisitos

- **Java 21** (JDK) para compilar y correr `ext-host`.
- **Rust** estable (edición 2024).
- Una terminal con imágenes:

  | Protocolo | Terminales |
  |---|---|
  | **Sixel** (recomendado) | Windows Terminal ≥ 1.22, foot, WezTerm, Konsole, xterm (`-ti vt340`), mlterm |
  | **Kitty** | kitty, Ghostty, WezTerm |
  | iTerm2 | iTerm2, WezTerm |
  | Bloques de color | cualquier terminal con color (fallback, se ve pixelado) |

## Instalación

Por ahora se compila desde el código. Son dos piezas: `ext-host` (Kotlin) y `reader` (Rust).
El reader encuentra y lanza el host solo.

### Linux

```bash
# Debian / Ubuntu
sudo apt install openjdk-21-jdk git curl
# Arch
sudo pacman -S jdk21-openjdk git
# Fedora
sudo dnf install java-21-openjdk-devel git

curl https://sh.rustup.rs -sSf | sh
```

```bash
git clone https://github.com/AlPeFe/mangaterm.git
cd mangaterm
(cd ext-host && ./gradlew installDist)
(cd reader && cargo build --release)
```

### Windows

En PowerShell:

```powershell
winget install EclipseAdoptium.Temurin.21.JDK Rustlang.Rustup Git.Git Microsoft.WindowsTerminal
```

```powershell
git clone https://github.com/AlPeFe/mangaterm.git
cd mangaterm
cd ext-host; .\gradlew.bat installDist; cd ..
cd reader; cargo build --release; cd ..
```

Usá **Windows Terminal** (la consola clásica `conhost` no muestra imágenes).

El binario queda en `reader/target/release/hantho` (`hantho.exe` en Windows). Si lo movés
fuera del repo, definí `HANTHO_HOST_LIB` apuntando a `ext-host/build/install/ext-host/lib`.

## Uso

```bash
hantho ext install --lang es   # instala todas las extensiones en español
hantho                         # abre la TUI
```

Desde la línea de comandos, sin TUI:

| Comando | Qué hace |
|---|---|
| `hantho search "grand blue" [--lang es]` | Busca en todas las fuentes instaladas |
| `hantho ext update` | Descarga el catálogo más reciente de Keiyoushi |
| `hantho ext find [texto] [--lang es] [--sfw]` | Busca extensiones en el catálogo |
| `hantho ext install <nombre…> [--lang es] [--sfw]` | Instala por nombre/paquete o todas las de un idioma |
| `hantho ext list` / `ext remove <paquete>` | Lista o desinstala extensiones |
| `hantho anilist login\|status\|logout` | Maneja la cuenta de AniList |

### Teclas

En todas las listas funcionan `↑`/`↓` y `k`/`j`; `esc` o `q` vuelve atrás; `ctrl+c` sale.

| Pantalla | Teclas |
|---|---|
| Inicio | `1`–`5` o `enter` elegir · `/` buscar · `q` salir |
| Biblioteca | `enter` abrir · `d` quitar · `/` buscar |
| Búsqueda | escribí y `enter` · `tab`/`↓` ir a resultados · `/` editar · `b` biblioteca |
| Manga | `enter` leer · `c` continuar · `f` favorito · `e` estado · `a` sincronizar AniList · `A` re-vincular · `r` recargar · `g`/`G` primero/último |
| Lector | `→` `espacio` `l` siguiente · `←` `h` anterior · `n`/`p` capítulo siguiente/anterior · `g`/`G` primera/última página |

### Variables de entorno

| Variable | Para qué |
|---|---|
| `HANTHO_HOME` | Carpeta de datos (por defecto `~/.hantho`) |
| `HANTHO_PROTOCOL` | Forzar `sixel`, `kitty`, `iterm2` o `halfblocks` |
| `HANTHO_FONT` | Tamaño de celda en píxeles, p. ej. `10x20`, si la detección falla |
| `HANTHO_HOST_LIB` | Ruta a `ext-host/lib` si el binario no está dentro del repo |
| `JAVA_HOME` | JVM a usar (si no, `java` del `PATH`) |

> El proyecto antes se llamaba *mangaterm*. Si tenés datos en `~/.mangaterm`, se mueven solos a
> `~/.hantho` la primera vez que abrís `hantho`.

## AniList

La sincronización es **manual**: vos decidís cuándo mandar el progreso.

1. Creá un *API client* en <https://anilist.co/settings/developer> con Redirect URL
   `https://anilist.co/api/v2/oauth/pin`.
2. Corré `hantho anilist login`, pegá el Client ID; se abre el navegador, autorizás y AniList te
   muestra un token: pegalo en la terminal.
3. En la TUI, abrí un manga de tu biblioteca y tocá `a`. La primera vez elegís la obra correcta en
   AniList; después se envían el estado y los capítulos leídos. `A` vuelve a elegir la obra.

El progreso **nunca retrocede**: si AniList tiene más capítulos marcados que Han Thỏ, se conserva
el de AniList. El token queda en `~/.hantho/anilist.json` (permisos `600` en Linux) y no sale de
tu máquina más que hacia AniList.

**Rate limiting y caché.** Han Thỏ hace como mucho 1 pedido cada 2 s a AniList (con ráfagas de 5),
respeta `Retry-After` y `X-RateLimit-Remaining`, y reintenta con espera ante un 429. Las búsquedas en
AniList se cachean una semana. Para las fuentes: las búsquedas se cachean 10 minutos y los detalles
y capítulos 30 minutos (`r` fuerza la recarga).

## Cómo funciona

```mermaid
flowchart LR
    subgraph reader["reader (Rust)"]
        TUI["TUI ratatui"] --> Core["búsqueda · biblioteca · caché"]
        Core --> Render["render Sixel propio"]
        Core --> AL["cliente AniList"]
    end
    subgraph host["ext-host (Kotlin, JVM)"]
        Loader["cargador de JARs"] --> API["extensions-lib 1.6<br/>OkHttp · Jsoup · Injekt"]
        API --> Shims["shims de Android"]
    end
    Core <-- "JSON por stdin/stdout" --> Loader
    Loader --> Ext["extensiones de Keiyoushi<br/>(.jar)"]
    Ext -- HTTP --> Sitios[("sitios de manga")]
    AL -- GraphQL --> AniList[("AniList")]
    Render --> Term["terminal<br/>Sixel / Kitty"]
```

- **Catálogo.** El índice de Keiyoushi (`index.pb`, protobuf con gzip) lista ~1440 extensiones y
  ~2350 fuentes. Han Thỏ lo descarga e instala los `.jar` de cada extensión en `~/.hantho`.
- **ext-host.** Las extensiones son clases JVM normales (ya convertidas desde dex), pero esperan
  vivir en Android. `ext-host` provee la implementación real de la API
  [`extensions-lib`](https://github.com/keiyoushi/extensions-lib) 1.6 (la publicada son solo stubs),
  más OkHttp, Jsoup, kotlinx-serialization, Injekt y *shims* mínimos de las clases de Android que usan
  (`Context`, `SharedPreferences`, `Uri`, `Base64`, `androidx.preference`…).
- **Protocolo.** El reader lanza el host como subproceso y le habla con una línea JSON por pedido
  (`{"id":1,"op":"ping"}` → `{"id":1,"result":"pong"}`). Las búsquedas a muchas fuentes corren en
  paralelo y van llegando a medida que responden.
- **Render.** `ratatui-image` detecta el protocolo y el tamaño de celda, pero escala con vecino más
  cercano y usa un dithering fuerte que en manga produce moiré. Por eso Han Thỏ codifica su propio
  Sixel: escala con Lanczos3 al tamaño exacto en píxeles y usa 64 grises sin dithering para páginas en
  blanco y negro (o 256 colores con dithering suave para color). Una página tarda ~25 ms.

Más detalles técnicos en [`NOTES.md`](NOTES.md).

## Limitaciones conocidas

- **Cloudflare.** Las fuentes protegidas por el desafío de Cloudflare no funcionan: en Android se
  resuelven con un WebView, y acá no hay. En las pruebas, ~12 de 165 fuentes en español fallaron por esto.
- **Shims de Android incompletos.** Algunas extensiones usan clases que todavía no están
  (`android.webkit.CookieManager`, `android.util.LruCache`, `android.graphics.ColorMatrix`,
  `Uri.Builder`, `Dispatchers.Main`…) y fallan al cargar o al buscar.
- **Sin pantalla de ajustes de extensiones.** Las preferencias de cada fuente quedan en sus valores
  por defecto.
- **Sin descargas offline** ni seguimiento automático de capítulos nuevos (por ahora).
- **macOS** no está probado; debería andar, pero no hay CI.

## Aviso legal

Han Thỏ es un **lector**: no aloja, distribuye ni indexa contenido. Todo el material lo sirven
sitios de terceros a través de extensiones mantenidas por la comunidad de
[Keiyoushi](https://github.com/keiyoushi/extensions), que Han Thỏ descarga desde su repositorio
público y no modifica. Los autores de Han Thỏ no tienen relación con esos sitios ni con las
extensiones, y no se hacen responsables por su contenido ni por su legalidad en tu país.

Respetá los derechos de autor y, cuando puedas, apoyá a los autores leyendo y comprando por canales
oficiales. Han Thỏ no está afiliado con Mihon, Tachiyomi, Keiyoushi ni AniList.

## Contribuir

¡Bienvenido! Mirá [`CONTRIBUTING.md`](CONTRIBUTING.md). No se aceptan pedidos de fuentes nuevas acá:
eso va al [repositorio de Keiyoushi](https://github.com/keiyoushi/extensions).

## Créditos

- [**Keiyoushi**](https://github.com/keiyoushi/extensions) — las extensiones y la API
  `extensions-lib` que `ext-host` implementa.
- [**Mihon**](https://github.com/mihonapp/mihon) y el difunto **Tachiyomi** — el ecosistema de
  extensiones sobre el que se apoya todo esto.
- [**ratatui**](https://ratatui.rs) y [**ratatui-image**](https://github.com/benjajaja/ratatui-image) — la TUI
  y la detección de protocolos gráficos.
- [**icy_sixel**](https://github.com/mkrueger/icy_sixel), [**image**](https://github.com/image-rs/image),
  [**tokio**](https://tokio.rs), [**reqwest**](https://github.com/seanmonstar/reqwest), [**clap**](https://github.com/clap-rs/clap),
  [**prost**](https://github.com/tokio-rs/prost).
- [**OkHttp**](https://square.github.io/okhttp/), [**Jsoup**](https://jsoup.org),
  [**kotlinx.serialization**](https://github.com/Kotlin/kotlinx.serialization) e
  [**Injekt**](https://github.com/null2264/injekt) (fork de null2264) en el host.
- [**AniList**](https://anilist.co) por su API pública.

## Licencia

[Apache License 2.0](LICENSE). Las extensiones de Keiyoushi y `extensions-lib` también son Apache-2.0
y pertenecen a sus autores.
