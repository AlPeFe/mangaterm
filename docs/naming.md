# Propuesta de nombre

## Decisión

Se eligió **Han Thỏ** como nombre visible y **`hantho`** como comando, por encima de la
recomendación `sakuyomi` que se describe más abajo. Ya está aplicado en el código: crate y binario
`hantho`, datos en `~/.hantho` (se migran solos desde `~/.mangaterm`), variables `HANTHO_*` y
paquete `hantho.host` en el host. El repo de GitHub sigue siendo `AlPeFe/mangaterm` por ahora.

Verificación de choques de `hantho` (2026-10-30):

- **crates.io** (`cargo search hantho`): ningún resultado, el nombre está libre.
- **GitHub** (`gh search repos hantho`): nada con ese nombre exacto. Lo más parecido es
  `flizzywine/Hanthon` (Python en chino, 6★) y repos personales sin estrellas (`hanthogenny`,
  `Hanthoigian`…). Ninguno es un lector de manga ni una herramienta de terminal.
- **Distros** (Repology): ningún paquete `hantho`.

Conclusión: sin colisiones relevantes. Si se quiere `cargo install hantho`, conviene publicar o
reservar el crate pronto.

---

## Propuesta original

"mangaterm" describe bien lo que hace, pero es largo para tipear, genérico y no tiene nada de la
estética rosa pastel / sakura de la TUI. Esta es una lista de alternativas con lo que encontré
al verificar choques (consultado el 2026-10-30).

**Cómo se verificó**

- crates.io: `cargo search <nombre>` (¿existe un crate con ese nombre exacto?).
- GitHub: `gh search repos <nombre> --sort stars` (¿hay un proyecto conocido con ese nombre?).
- Paquetes de distros: API de [Repology](https://repology.org/) (`/api/v1/project/<nombre>`), que
  cubre Debian, Ubuntu, Arch, AUR, Fedora, Nix, Homebrew, Scoop, winget, etc.

(Lo que sigue es la propuesta tal como se escribió antes de la decisión.)

## Candidatos

| Nombre | Significado | Letras | crates.io | GitHub | Distros |
|---|---|---|---|---|---|
| **`sakuyomi`** | 咲 / 桜 *saku* (florecer, sakura) + 読み *yomi* (lectura) | 8 | libre | solo 2 repos vacíos (0★) | libre |
| `hanakoma` | 花 *hana* (flor) + コマ *koma* (viñeta de manga) | 8 | libre | libre | libre |
| `koma` | コマ: viñeta / cuadro de manga | 4 | sin crate exacto (hay `komac`, `komando`) | muchos `koma*`, ninguno igual y conocido | libre |
| `tsuzuki` | 続き: "continuación" (el botón *continuar* leyendo) | 7 | libre | `tobfd/tsuzuki`: cliente Android de **AniList** (2★) | libre |
| `komachi` | 小町: belleza clásica; suena tierno | 7 | libre | proyectos chicos sin relación (≤56★) | libre |
| `sakukoma` | sakura + koma | 8 | libre | libre | libre |

### Descartados por choque

| Nombre | Por qué no |
|---|---|
| `yomu` (読む, leer) | Ya existe el crate `yomu`, **un lector de Markdown para terminal**, y `sendyhalim/Yomu` (lector de manga para macOS, 197★). Confusión garantizada. |
| `shiori` (栞, señalador) | `go-shiori/shiori` (11,7k★, gestor de marcadores) está empaquetado en Nix y otras distros; crate `shiori` ocupado. |
| `yomi` | Crate ocupado, nombre muy usado. |
| `hanabira` (花びら, pétalo) | `hanabira.org` tiene su propio *manga reader* (60★). |
| `komorebi` | Gestor de ventanas tiling muy conocido en Windows (crate y comando). |
| `petal`, `sakura` | Genéricos, decenas de proyectos. |

## Pros y contras

**`sakuyomi`**
- ✅ Dice exactamente qué es: *leer* (yomi) con el tema *sakura* de la TUI. Se pronuncia como se escribe.
- ✅ Libre en crates.io, GitHub (nada con estrellas) y distros. El dominio de búsqueda está limpio.
- ✅ Sigue la convención de lectores de manga de nombre japonés (Tachiyomi, Mihon) sin copiarlos; "-yomi" remite a Tachiyomi, de donde vienen las extensiones.
- ⚠️ 8 letras: más largo que lo ideal para un comando, pero menos que `mangaterm` (9) y se puede alias `sky`/`saku`.
- ⚠️ El "-yomi" puede hacer pensar que es un fork de Tachiyomi. Se aclara en el README.

**`hanakoma`**
- ✅ Totalmente libre. "Flor + viñeta" encaja con la estética y con el manga.
- ⚠️ Menos obvio que es un *lector*; 8 letras.

**`koma`**
- ✅ El más corto (4 letras), ideal como comando. Muy "manga".
- ⚠️ Es un prefijo saturado en GitHub (komari, komanda, Komac…): difícil de encontrar buscando, y es probable que alguien más lo use pronto en un paquete de distro.

**`tsuzuki`**
- ✅ Lindo concepto ("seguir leyendo").
- ❌ Ya hay un cliente de AniList llamado Tsuzuki; como este proyecto también sincroniza con AniList, se pisan en el mismo nicho.

**`komachi`**
- ✅ Suave, combina con la estética.
- ⚠️ No dice nada de manga ni de lectura; suena a personaje.

**`sakukoma`**
- ✅ Libre, mismo concepto que `sakuyomi`.
- ⚠️ Más difícil de pronunciar y recordar.

## Recomendación

**`sakuyomi`**: es el único que a la vez comunica "lector", combina con el rosa/sakura y está
libre en los tres lugares. Segunda opción: `hanakoma`. Si se prioriza un comando cortísimo, `koma`,
aceptando que va a costar más encontrarlo.

## Si se decide renombrar

Lista de lo que habría que tocar (para hacerlo en un solo commit):

- `reader/Cargo.toml` (`name`) y el nombre del binario.
- Carpeta de datos `~/.mangaterm` → `~/.sakuyomi` (con migración: si existe la vieja y no la nueva, moverla).
- Variables `MANGATERM_HOME`, `MANGATERM_PROTOCOL`, `MANGATERM_FONT`, `MANGATERM_HOST_LIB` (aceptar las viejas un tiempo).
- Paquete Kotlin `mangaterm.host` en `ext-host/`.
- CI (`.github/workflows/ci.yml`), README, `NOTES.md`, logo (`assets/logo.svg` dice "mangaterm").
- Nombre del repo en GitHub (GitHub redirige el nombre viejo).
- Publicar un crate vacío/placeholder en crates.io para reservar el nombre, si se quiere `cargo install`.
