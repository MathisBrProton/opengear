"""Lightweight standalone HTTP harness for OpenGear UI review without hardware.

Serves `crates/opengear-app/ui` and `crates/opengear-app/assets`, injects `/mock.js`
before `/js/main.js`, and supports `?scen=normal|empty|wizard|error`, `?lang=fr|en`,
and `?mode=simple|expert`.
"""

from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
UI_DIR = ROOT / "crates" / "opengear-app" / "ui"
ASSETS_DIR = ROOT / "crates" / "opengear-app" / "assets"
HARNESS_DIR = Path(__file__).resolve().parent


class HarnessHandler(SimpleHTTPRequestHandler):
    def translate_path(self, path: str) -> str:
        clean = path.split("?", 1)[0].split("#", 1)[0]
        if clean == "/" or clean == "/index.html":
            return str(UI_DIR / "index.html")
        if clean == "/mock.js":
            return str(HARNESS_DIR / "mock.js")
        if clean.startswith("/assets/"):
            return str(ASSETS_DIR / clean.removeprefix("/assets/"))
        return str(UI_DIR / clean.lstrip("/"))

    def do_GET(self) -> None:
        clean = self.path.split("?", 1)[0]
        if clean in ("/", "/index.html"):
            html = (UI_DIR / "index.html").read_text(encoding="utf-8")
            injected = html.replace(
                '<script type="module" src="/js/main.js"></script>',
                '<script src="/mock.js"></script>\n  <script type="module" src="/js/main.js"></script>',
            )
            body = injected.encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        super().do_GET()

    def log_message(self, format: str, *args) -> None:
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 4173
    print(f"OpenGear UI harness: http://127.0.0.1:{port}/  (?scen=normal|empty|wizard|error&lang=fr|en)")
    ThreadingHTTPServer(("127.0.0.1", port), HarnessHandler).serve_forever()
