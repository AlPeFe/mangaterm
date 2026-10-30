"""Prueba rápida: muestra una imagen en la terminal usando Sixel.

Uso (en Windows Terminal):
    python tools/sixel_test.py <ruta-o-url-de-imagen> [ancho_px]
    python tools/sixel_test.py <ruta-o-url-de-imagen> [columnas] --blocks
"""
import io
import sys
import urllib.request

from PIL import Image


def load(src: str) -> Image.Image:
    if src.startswith("http"):
        req = urllib.request.Request(src, headers={"User-Agent": "hantho-sixel-test"})
        return Image.open(io.BytesIO(urllib.request.urlopen(req).read()))
    return Image.open(src)


def to_sixel(img: Image.Image, width: int) -> str:
    img = img.convert("RGB")
    height = round(img.height * width / img.width)
    img = img.resize((width, height), Image.LANCZOS)
    # Manga suele ser blanco y negro: 64 colores sobran y aceleran el encode
    pal = img.quantize(colors=64, method=Image.Quantize.MEDIANCUT)
    palette = pal.getpalette()[: 64 * 3]
    px = pal.load()

    out = ["\x1bPq", f'"1;1;{width};{height}']
    for i in range(0, len(palette), 3):
        r, g, b = (round(c * 100 / 255) for c in palette[i : i + 3])
        out.append(f"#{i // 3};2;{r};{g};{b}")

    for band in range(0, height, 6):
        rows = range(band, min(band + 6, height))
        # bits[color][x] = máscara de 6 bits de ese color en la columna x
        bits: dict[int, list[int]] = {}
        for bit, y in enumerate(rows):
            for x in range(width):
                c = px[x, y]
                col = bits.get(c)
                if col is None:
                    col = bits[c] = [0] * width
                col[x] |= 1 << bit
        first = True
        for color, col in bits.items():
            if not first:
                out.append("$")  # volver al inicio de la banda
            first = False
            out.append(f"#{color}")
            x = 0
            while x < width:
                v = col[x]
                run = 1
                while x + run < width and col[x + run] == v:
                    run += 1
                ch = chr(63 + v)
                out.append(f"!{run}{ch}" if run > 3 else ch * run)
                x += run
        out.append("-")  # siguiente banda
    out.append("\x1b\\")
    return "".join(out)


def to_blocks(img: Image.Image, cols: int) -> str:
    """Fallback para terminales sin Sixel: cada celda = 2 píxeles verticales con '▀'."""
    img = img.convert("RGB")
    # Las celdas son ~2x más altas que anchas; con '▀' cada celda cubre 1x2 píxeles
    rows = round(img.height * cols / img.width)
    rows += rows % 2
    img = img.resize((cols, rows), Image.LANCZOS)
    px = img.load()
    lines = []
    for y in range(0, rows, 2):
        line = []
        for x in range(cols):
            (r1, g1, b1), (r2, g2, b2) = px[x, y], px[x, y + 1]
            line.append(f"\x1b[38;2;{r1};{g1};{b1}m\x1b[48;2;{r2};{g2};{b2}m▀")
        lines.append("".join(line) + "\x1b[0m")
    return "\n".join(lines)


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if a != "--blocks"]
    if not args:
        sys.exit(__doc__)
    if "--blocks" in sys.argv:
        sys.stdout.reconfigure(encoding="utf-8")
        cols = int(args[1]) if len(args) > 1 else 80
        sys.stdout.write(to_blocks(load(args[0]), cols) + "\n")
    else:
        w = int(args[1]) if len(args) > 1 else 700
        sys.stdout.write(to_sixel(load(args[0]), w) + "\n")
    sys.stdout.flush()
