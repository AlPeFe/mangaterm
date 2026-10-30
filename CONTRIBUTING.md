# Cómo contribuir

¡Gracias por querer ayudar! Algunas pautas cortas:

## Antes de abrir un issue

- **¿Una fuente no anda o falta una?** Si también falla en Mihon, el problema es de la extensión:
  reportalo en [keiyoushi/extensions](https://github.com/keiyoushi/extensions/issues). Si anda en
  Mihon pero no en mangaterm, es nuestro (probablemente falta un *shim* de Android): abrí un issue
  con el nombre de la extensión y el error que muestra `mangaterm search`.
- Las fuentes con Cloudflare no se pueden usar por ahora (ver *Limitaciones* en el README).
- No compartas tu token de AniList ni el contenido de `~/.mangaterm/anilist.json` en un issue.

## Desarrollo

```bash
(cd ext-host && ./gradlew installDist)        # gradlew.bat en Windows
(cd reader && cargo build && cargo test)
```

- `reader/` (Rust): TUI, búsqueda, render, biblioteca, AniList.
- `ext-host/` (Kotlin): carga las extensiones y responde pedidos JSON por stdin/stdout.
  Probarlo a mano: `echo '{"id":1,"op":"ping"}' | ext-host/build/install/ext-host/bin/ext-host`.
- `tools/`: scripts de prueba en Python (cliente del host, pruebas de Sixel).
- `NOTES.md` tiene la investigación técnica (formato del índice, API de extensions-lib, render).

Usá `MANGATERM_HOME=/tmp/mangaterm-dev` para no tocar tus datos reales mientras probás.

## Pull requests

- Uno por tema, chico y enfocado. Si es un cambio grande, abrí un issue antes para charlarlo.
- Tiene que pasar el CI (Linux y Windows): `cargo build`, `cargo test` y el build de `ext-host`.
- Corré `cargo fmt` y `cargo clippy` antes de mandar.
- Para cambios en la TUI, sumá una captura al PR.
- Comentarios y mensajes de commit en español, como el resto del código.

Al contribuir aceptás que tu aporte se publique bajo la [licencia Apache-2.0](LICENSE) del proyecto.
