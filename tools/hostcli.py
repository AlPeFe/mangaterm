"""Cliente de prueba para ext-host: habla el protocolo JSON por stdin/stdout.

Uso:
    python tools/hostcli.py <jar> search "<texto>"
    python tools/hostcli.py <jar> read "<texto>" [n_resultado] [n_capitulo]   # descarga la 1a página
"""
import itertools
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HOST = ROOT / "ext-host" / "build" / "install" / "ext-host" / "bin" / "ext-host.bat"


class Host:
    def __init__(self):
        self.p = subprocess.Popen(
            [str(HOST)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=sys.stderr,
            text=True, encoding="utf-8", bufsize=1,
        )
        self.ids = itertools.count(1)

    def call(self, op, **params):
        rid = next(self.ids)
        self.p.stdin.write(json.dumps({"id": rid, "op": op, **params}) + "\n")
        self.p.stdin.flush()
        resp = json.loads(self.p.stdout.readline())
        if not resp["ok"]:
            raise RuntimeError(resp["error"])
        return resp["result"]

    def close(self):
        self.p.stdin.close()
        self.p.wait(5)


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    jar, cmd, query = sys.argv[1], sys.argv[2], sys.argv[3]
    h = Host()
    try:
        ext = h.call("load", jar=jar)
        src = ext["sources"][0]
        print(f"cargada {ext['pkg']} -> {src['name']} ({src['lang']}) {src['baseUrl']}")
        res = h.call("search", source=src["id"], query=query)
        for i, m in enumerate(res["mangas"]):
            print(f"  [{i}] {m['title']}  {m['url']}")
        if cmd != "read" or not res["mangas"]:
            return
        pick = int(sys.argv[4]) if len(sys.argv) > 4 else 0
        info = h.call("manga", source=src["id"], manga=res["mangas"][pick])
        chapters = info["chapters"]
        print(f"\n{info['manga']['title']}: {len(chapters)} capítulos; autor: {info['manga']['author']}")
        for c in chapters[:5]:
            print(f"  - {c['name']}")
        chap = chapters[int(sys.argv[5])] if len(sys.argv) > 5 else chapters[-1]
        pages = h.call("pages", source=src["id"], chapter=chap)
        print(f"\n'{chap['name']}': {len(pages)} páginas")
        out = ROOT / "tools" / "out" / "page0"
        img = h.call("image", source=src["id"], page=pages[0], out=str(out))
        print(f"página 1 -> {img['path']} ({img['contentType']}, {img['bytes']} bytes)")
    finally:
        h.close()


if __name__ == "__main__":
    main()
