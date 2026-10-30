# mangaterm

Lector de manga para la terminal, con las extensiones de [Keiyoushi](https://github.com/keiyoushi/extensions)
(las mismas de Mihon) y sincronización con AniList. Funciona en **Linux** y **Windows**.

- `reader/`: el programa (Rust). TUI, búsqueda global, lector Sixel, biblioteca, AniList.
- `ext-host/`: corre las extensiones de Mihon en una JVM normal (Kotlin). El reader lo lanza solo.

## Requisitos

- Java 21 (JDK, para compilar ext-host)
- Rust estable
- Una terminal con imágenes: **Sixel** (Windows Terminal, foot, WezTerm, xterm `-ti vt340`, Konsole…)
  o **Kitty** (kitty, Ghostty, WezTerm). Sin soporte gráfico se usa un modo de bloques de color.

Linux (Debian/Ubuntu):

```bash
sudo apt install openjdk-21-jdk
curl https://sh.rustup.rs -sSf | sh
```

## Compilar

```bash
cd ext-host && ./gradlew installDist && cd ..
cd reader && cargo build --release && cd ..
```

En Windows, `gradlew.bat installDist`.

## Uso

```bash
reader/target/release/mangaterm ext install --lang es   # instala todas las extensiones en español
reader/target/release/mangaterm                          # abre la TUI
```

Otros comandos: `mangaterm search "título"`, `mangaterm ext find|list|remove|update`.

### Teclas

| Pantalla | Teclas |
|---|---|
| Biblioteca | `enter` abrir · `d` quitar · `/` buscar · `q` salir |
| Búsqueda | `enter` buscar/abrir · `b` biblioteca · `esc` editar |
| Manga | `enter` leer · `c` continuar · `f` favorito · `e` estado · `a` sincronizar AniList · `A` re-vincular · `r` recargar |
| Lector | `→`/`espacio` siguiente · `←` anterior · `n`/`p` capítulo · `g`/`G` inicio/fin · `esc` volver |

## AniList

1. Creá un API client en <https://anilist.co/settings/developer> con
   Redirect URL `https://anilist.co/api/v2/oauth/pin`.
2. `mangaterm anilist login` → pegá el Client ID y después el token que muestra AniList.
3. En la TUI, en un manga de tu biblioteca, tocá `a`: la primera vez elegís la obra en AniList;
   después envía estado y capítulos leídos. El progreso nunca retrocede: si AniList tiene más, se conserva.

El token queda en `~/.mangaterm/anilist.json` (permisos 600 en Linux).

Para no abusar del servicio: máximo 1 pedido cada 2 s a AniList (ráfagas de 5), se respeta
`Retry-After` y `X-RateLimit-Remaining`, y las búsquedas se cachean una semana. Las búsquedas en
fuentes se cachean 10 min y los capítulos 30 min (`r` fuerza recarga).

## Variables de entorno

| Variable | Para qué |
|---|---|
| `MANGATERM_HOME` | Carpeta de datos (por defecto `~/.mangaterm`) |
| `MANGATERM_PROTOCOL` | Forzar `sixel`, `kitty`, `iterm2` o `halfblocks` |
| `MANGATERM_FONT` | Tamaño de celda en píxeles, p. ej. `10x20`, si la detección falla |
| `MANGATERM_HOST_LIB` | Ruta a `ext-host/lib` si el binario no está dentro del repo |
| `JAVA_HOME` | JVM a usar (si no, `java` del PATH) |
