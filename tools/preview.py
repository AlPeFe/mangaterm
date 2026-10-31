"""Convierte el volcado JSON de un buffer de ratatui (ver theme::dump_buffer) en una imagen PNG.

Uso: python tools/preview.py <volcado.json> <salida.png>
"""
import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

BG = (28, 22, 30)  # fondo de terminal oscuro
FG = (230, 220, 230)
CW, CH = 10, 20


def load_font(names, size):
    dirs = [Path("C:/Windows/Fonts"), Path("/usr/share/fonts/truetype/dejavu"), Path("/usr/share/fonts")]
    for name in names:
        for d in dirs:
            for p in [d / name, *d.rglob(name)] if d.exists() else []:
                if p.exists():
                    return ImageFont.truetype(str(p), size)
    return ImageFont.load_default()


def render(rows):
    """Dibuja un volcado (lista de filas de celdas [carácter, fg, bg]) y devuelve la imagen."""
    mono = load_font(["CascadiaMono.ttf", "consola.ttf", "DejaVuSansMono.ttf"], 16)
    symbols = load_font(["seguisym.ttf", "DejaVuSans.ttf"], 15)
    img = Image.new("RGB", (len(rows[0]) * CW, len(rows) * CH), BG)
    draw = ImageDraw.Draw(img)
    for y, row in enumerate(rows):
        for x, (ch, fg, bg) in enumerate(row):
            box = (x * CW, y * CH, (x + 1) * CW, (y + 1) * CH)
            if bg:
                draw.rectangle(box, fill=bg)
            if ch.strip():
                color = fg or FG
                if ch == "█":  # bloque lleno: pintarlo exacto, sin depender de la fuente
                    draw.rectangle(box, fill=color)
                    continue
                # Cascadia/Consolas no traen estos símbolos (la terminal los toma de otra fuente)
                font = symbols if ord(ch) > 0x2000 and not 0x2500 <= ord(ch) <= 0x259F else mono
                draw.text((x * CW, y * CH + 1), ch, font=font, fill=color)
    return img


def main():
    img = render(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8")))
    img.save(sys.argv[2])
    print(sys.argv[2], img.size)


if __name__ == "__main__":
    main()
