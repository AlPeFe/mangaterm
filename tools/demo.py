"""Genera el vídeo de demo del README (assets/demo.gif en inglés, assets/demo.es.gif en español).

Ejecuta el test `demo_frames`, que recorre una sesión de la TUI real (inicio → búsqueda → manga →
capítulo 13 → lectura → AniList) y vuelca cada fotograma; aquí se pintan, se les pone encima la
página del lector (en la terminal la dibujaría el Sixel) y se juntan en un GIF.

Uso (desde la raíz del repo): python tools/demo.py
"""
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import preview  # noqa: E402
import screenshots  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
BUBBLES = {
    "es": [("¡A leer!", "capítulo 13"), ("¡Al agua!", "¡sin miedo!"), ("¡Fin!", "capítulo 13")],
    "en": [("Let's read!", "chapter 13"), ("Dive in!", "no fear!"), ("The end!", "chapter 13")],
}
TITLE = {"es": "hantho · demo", "en": "hantho · demo"}


def generate(lang, out):
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        for i, bubble in enumerate(BUBBLES[lang]):
            screenshots.sample_page(tmp / f"page_{i}.png", bubble)
        env = dict(os.environ, HANTHO_LANG=lang, HANTHO_DEMO_DIR=str(tmp), HANTHO_DEMO_PAGES=str(tmp), HANTHO_HOME=str(tmp / "home"))
        subprocess.run(
            ["cargo", "test", "--release", "--target-dir", "target/check", "demo_frames", "--", "--exact", "tui::tests::demo_frames"],
            cwd=ROOT / "reader", env=env, check=True,
        )
        pages = [Image.open(tmp / f"page_{i}.png") for i in range(len(BUBBLES[lang]))]
        frames, durations = [], []
        for f in json.loads((tmp / "frames.json").read_text(encoding="utf-8")):
            img = preview.render(json.loads((tmp / f["file"]).read_text(encoding="utf-8")))
            if f["page"] is not None:
                img = screenshots.paste_page(img, pages[f["page"] % len(pages)], 120)
            win = screenshots.window(img, TITLE[lang])
            flat = Image.new("RGB", win.size, screenshots.FRAME)
            flat.paste(win, (0, 0), win)
            frames.append(flat)
            durations.append(f["ms"])

    # Una sola paleta para todos los fotogramas (incluidos los grises de las páginas): sin parpadeos
    sample = Image.new("RGB", (frames[0].width, frames[0].height * 3))
    reader = next(i for i, f in enumerate(frames) if f.getpixel((frames[0].width // 2, 200))[0] > 200)
    for k, i in enumerate([len(frames) // 2, reader, len(frames) - 1]):
        sample.paste(frames[i], (0, frames[0].height * k))
    palette = sample.quantize(colors=255, method=Image.Quantize.MEDIANCUT)
    quantized = [f.quantize(palette=palette, dither=Image.Dither.NONE) for f in frames]
    quantized[0].save(out, save_all=True, append_images=quantized[1:], duration=durations, loop=0, optimize=True, disposal=1)
    print(out, f"{len(frames)} fotogramas, {sum(durations) / 1000:.1f} s, {out.stat().st_size / 1e6:.1f} MB")


def main():
    generate("en", ROOT / "assets" / "demo.gif")
    generate("es", ROOT / "assets" / "demo.es.gif")


if __name__ == "__main__":
    main()
