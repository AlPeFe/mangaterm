"""Regenera las capturas del README (assets/screenshots/*.png) a partir de la TUI real.

Ejecuta el test `screens_preview`, que dibuja cada pantalla con datos de ejemplo en un buffer de
ratatui, convierte los volcados en PNG con tools/preview.py y les pone marco de ventana. La página
del lector es un dibujo original generado aquí (la terminal la pintaría con Sixel, fuera del buffer).

Genera dos juegos: en español (assets/screenshots/) y en inglés (assets/screenshots/en/).

Uso (desde la raíz del repo): python tools/screenshots.py
"""
import json
import math
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, str(Path(__file__).parent))
import preview  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "screenshots"
PINK = (255, 182, 213)
LAVENDER = (200, 178, 255)
MINT = (160, 236, 200)
TEXT = (244, 232, 240)
MUTED = (160, 140, 156)
FAINT = (100, 86, 98)
FRAME = (36, 28, 38)

# Textos de las capturas (lo que no sale de la TUI) en cada idioma
TEXTS = {
    "es": {
        "bubble": ("¡A leer!", "capítulo 13"),
        "titles": {
            "inicio": "hantho",
            "biblioteca": "hantho · biblioteca",
            "busqueda": "hantho · buscar",
            "capitulos": "hantho · Grand Blue",
            "anilist": "hantho · vincular con AniList",
            "backup": "hantho · backup de Mihon",
            "anadir_extensiones": "hantho · añadir extensiones",
            "repositorio": "hantho · añadir repositorio",
            "lector": "hantho · lector",
            "mcp": "terminal · agente + hantho",
        },
        "agent": [
            "> ábreme Grand Blue por donde iba",
            "● hantho · library",
            "Grand Blue: leyendo, capítulo 12 de 20",
            "● hantho · open_reader",
            "manga: Grand Blue · continuar",
            "abierto en un panel a la derecha",
            "Listo: Grand Blue está abierto a la",
            "derecha, capítulo 13, página 7.",
            "Cuando termines el capítulo se marca",
            "como leído y se envía a AniList.",
            "> marca como leídos hasta el 12",
            "● hantho · mark_read",
            "capítulos 1–12 leídos · AniList ✓",
        ],
    },
    "en": {
        "bubble": ("Let's read!", "chapter 13"),
        "titles": {
            "inicio": "hantho",
            "biblioteca": "hantho · library",
            "busqueda": "hantho · search",
            "capitulos": "hantho · Grand Blue",
            "anilist": "hantho · link to AniList",
            "backup": "hantho · Mihon backup",
            "anadir_extensiones": "hantho · add extensions",
            "repositorio": "hantho · add repository",
            "lector": "hantho · reader",
            "mcp": "terminal · agent + hantho",
        },
        "agent": [
            "> open Grand Blue where I left off",
            "● hantho · library",
            "Grand Blue: reading, chapter 12 of 20",
            "● hantho · open_reader",
            "manga: Grand Blue · continue",
            "opened in a pane on the right",
            "Done: Grand Blue is open on the right,",
            "chapter 13, page 7.",
            "When you finish the chapter it gets",
            "marked as read and sent to AniList.",
            "> mark everything up to 12 as read",
            "● hantho · mark_read",
            "chapters 1–12 read · AniList ✓",
        ],
    },
}


def font(names, size):
    return preview.load_font(names, size)


def sample_page(path, bubble):
    """Página de manga de ejemplo: viñetas, tramas y bocadillos, en escala de grises."""
    w, h = 760, 1100
    img = Image.new("L", (w, h), 255)
    d = ImageDraw.Draw(img)
    m, g = 36, 18
    panels = [
        (m, m, w - m, 400),
        (m, 400 + g, w // 2 - g // 2, 760),
        (w // 2 + g // 2, 400 + g, w - m, 760),
        (m, 760 + g, w - m, h - m),
    ]
    # 1: mar con trama de puntos y olas
    x0, y0, x1, y1 = panels[0]
    for yy in range(y0, y1, 9):
        for xx in range(x0, x1, 9):
            r = 1 + 2.6 * (yy - y0) / (y1 - y0)
            d.ellipse((xx - r, yy - r, xx + r, yy + r), fill=150)
    d.rectangle((x0, y0, x1, y0 + 150), fill=255)
    for k in range(7):
        yy = y0 + 150 + k * 34
        pts = [(xx, yy + 10 * math.sin(xx / 38 + k)) for xx in range(x0, x1 + 1, 6)]
        d.line(pts, fill=40, width=3)
    d.ellipse((w - 230, y0 + 30, w - 120, y0 + 140), outline=0, width=4)  # sol
    for a in range(0, 360, 30):
        cx, cy = w - 175, y0 + 85
        d.line((cx + 70 * math.cos(math.radians(a)), cy + 70 * math.sin(math.radians(a)),
                cx + 95 * math.cos(math.radians(a)), cy + 95 * math.sin(math.radians(a))), fill=0, width=4)
    # 2: conejo
    x0, y0, x1, y1 = panels[1]
    cx = (x0 + x1) // 2
    lines = Image.new("L", (x1 - x0, y1 - y0), 225)
    ld = ImageDraw.Draw(lines)
    for k in range(0, 360, 12):  # líneas de acción
        a = math.radians(k)
        c = (cx - x0, 200)
        ld.line((c[0] + 400 * math.cos(a), c[1] + 400 * math.sin(a), c[0] + 120 * math.cos(a), c[1] + 120 * math.sin(a)), fill=170, width=2)
    img.paste(lines, (x0, y0))
    d.ellipse((cx - 32, y0 + 60, cx - 8, y0 + 190), fill=255, outline=0, width=4)
    d.ellipse((cx + 8, y0 + 60, cx + 32, y0 + 190), fill=255, outline=0, width=4)
    d.ellipse((cx - 80, y0 + 150, cx + 80, y0 + 290), fill=255, outline=0, width=4)
    d.ellipse((cx - 40, y0 + 200, cx - 24, y0 + 222), fill=0)
    d.ellipse((cx + 24, y0 + 200, cx + 40, y0 + 222), fill=0)
    d.arc((cx - 14, y0 + 225, cx + 14, y0 + 245), 0, 180, fill=0, width=3)
    d.ellipse((cx - 62, y0 + 228, cx - 42, y0 + 240), fill=200)
    d.ellipse((cx + 42, y0 + 228, cx + 62, y0 + 240), fill=200)
    # 3: bocadillo
    x0, y0, x1, y1 = panels[2]
    title = font(["comicbd.ttf", "DejaVuSans-Bold.ttf"], 40)
    body = font(["comic.ttf", "DejaVuSans.ttf"], 30)
    d.ellipse((x0 + 20, y0 + 30, x1 - 20, y0 + 230), fill=255, outline=0, width=4)
    d.polygon([(x0 + 90, y0 + 210), (x0 + 60, y0 + 300), (x0 + 140, y0 + 222)], fill=255, outline=0)
    d.line((x0 + 92, y0 + 214, x0 + 138, y0 + 222), fill=255, width=6)
    d.text(((x0 + x1) // 2, y0 + 105), bubble[0], font=title, fill=0, anchor="mm")
    d.text(((x0 + x1) // 2, y0 + 155), bubble[1], font=body, fill=0, anchor="mm")
    for i, (px, py) in enumerate([(x0 + 200, y0 + 280), (x0 + 120, y0 + 310), (x0 + 250, y0 + 320)]):
        draw_flower(d, px, py, 16 + 5 * i)
    # 4: paisaje con pétalos
    x0, y0, x1, y1 = panels[3]
    for yy in range(y0, y1):
        d.line((x0, yy, x1, yy), fill=int(250 - 70 * (yy - y0) / (y1 - y0)))
    d.polygon([(x0, y1), (x0 + 180, y0 + 120), (x0 + 330, y1)], fill=90)
    d.polygon([(x0 + 220, y1), (x0 + 450, y0 + 60), (x1, y1)], fill=60)
    d.polygon([(x0 + 400, y0 + 110), (x0 + 450, y0 + 60), (x0 + 500, y0 + 110)], fill=245)
    for i in range(14):
        draw_flower(d, x0 + 40 + (i * 97) % (x1 - x0 - 80), y0 + 30 + (i * 53) % 150, 9)
    for box in panels:
        d.rectangle(box, outline=0, width=5)
    img.save(path)


def draw_flower(d, x, y, r):
    for k in range(5):
        a = math.radians(k * 72 - 90)
        px, py = x + r * math.cos(a), y + r * math.sin(a)
        d.ellipse((px - r * 0.6, py - r * 0.6, px + r * 0.6, py + r * 0.6), fill=255, outline=0, width=2)
    d.ellipse((x - r * 0.3, y - r * 0.3, x + r * 0.3, y + r * 0.3), fill=0)


def paste_page(img, page, cols, rows=29):
    """Coloca la página como lo haría el Sixel: ajustada al área de imagen y centrada."""
    max_w, max_h = cols * preview.CW, rows * preview.CH
    s = min(max_w / page.width, max_h / page.height)
    p = page.convert("RGB").resize((int(page.width * s), int(page.height * s)), Image.LANCZOS)
    img.paste(p, ((max_w - p.width) // 2, 0))
    return img


def window(content, title):
    """Marco de ventana de terminal alrededor de una captura."""
    pad, bar = 14, 34
    w, h = content.width + 2 * pad, content.height + bar + pad
    img = Image.new("RGB", (w, h), FRAME)
    d = ImageDraw.Draw(img)
    for i, c in enumerate([PINK, LAVENDER, MINT]):
        d.ellipse((pad + i * 22, 11, pad + i * 22 + 12, 23), fill=c)
    d.text((w // 2, bar // 2), title, font=font(["CascadiaMono.ttf", "consola.ttf", "DejaVuSansMono.ttf"], 14), fill=MUTED, anchor="mm")
    img.paste(content, (pad, bar))
    out = Image.new("RGB", (w + 2, h + 2), (0, 0, 0))
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w - 1, h - 1), 12, fill=255)
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    out.paste(img, (0, 0), mask)
    ImageDraw.Draw(out).rounded_rectangle((0, 0, w - 1, h - 1), 12, outline=(70, 56, 72), width=1)
    return out


def agent_pane(cols, rows, text):
    """Panel izquierdo genérico de un agente de terminal que usa el MCP de Han Thỏ."""
    cells = [[[" ", None, None] for _ in range(cols)] for _ in range(rows)]

    def put(y, x, text, color):
        for i, ch in enumerate(text):
            if x + i < cols:
                cells[y][x + i] = [ch, "#%02x%02x%02x" % color, None]

    layout = [
        (1, 1, TEXT), (3, 1, LAVENDER), (4, 3, MUTED), (6, 1, LAVENDER), (7, 3, MUTED), (8, 3, MUTED),
        (10, 1, TEXT), (11, 1, TEXT), (12, 1, TEXT), (13, 1, TEXT), (16, 1, TEXT), (18, 1, LAVENDER), (19, 3, MUTED),
    ]
    for (y, x, color), text in zip(layout, text):
        put(y, x, text, color)
    put(rows - 2, 0, "─" * cols, FAINT)
    put(rows - 1, 1, "> ", PINK)
    return cells


def generate(lang, out_dir):
    texts = TEXTS[lang]
    out_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        page_path = tmp / "page.png"
        sample_page(page_path, texts["bubble"])
        env = dict(os.environ, HANTHO_LANG=lang, HANTHO_PREVIEW_DIR=str(tmp), HANTHO_PREVIEW_PAGE=str(page_path), HANTHO_HOME=str(tmp / "home"))
        subprocess.run(
            ["cargo", "test", "--release", "--target-dir", "target/check", "screens_preview", "--", "--exact", "tui::tests::screens_preview"],
            cwd=ROOT / "reader", env=env, check=True,
        )
        load = lambda name: json.loads((tmp / f"{name}.json").read_text(encoding="utf-8"))
        page = Image.open(page_path)
        titles = texts["titles"]

        shots = {
            "inicio": "home",
            "biblioteca": "library",
            "busqueda": "search",
            "capitulos": "manga",
            "anilist": "anilist_pick",
            "backup": "backup",
            "anadir_extensiones": "add_extensions",
            "repositorio": "add_repo",
        }
        for out, name in shots.items():
            window(preview.render(load(name)), titles[out]).save(out_dir / f"{out}.png")
            print(out_dir / f"{out}.png")

        reader = paste_page(preview.render(load("reader")), page, 120)
        window(reader, titles["lector"]).save(out_dir / "lector.png")
        print(out_dir / "lector.png")

        # Panel lateral del MCP: agente a la izquierda, lector a la derecha
        side = load("reader_side")
        left_cols = 55
        agent = agent_pane(left_cols, 30, texts["agent"])
        rows = [agent[y] + [["│", "#%02x%02x%02x" % FAINT, None]] + side[y] for y in range(30)]
        split = preview.render(rows)
        right = paste_page(preview.render(side), page, 64)
        split.paste(right, ((left_cols + 1) * preview.CW, 0))
        window(split, titles["mcp"]).save(out_dir / "mcp.png")
        print(out_dir / "mcp.png")


def main():
    generate("es", OUT)
    generate("en", OUT / "en")


if __name__ == "__main__":
    main()
