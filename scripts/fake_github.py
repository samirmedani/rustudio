#!/usr/bin/env python3
"""Faux GitHub local pour les tests de mise a jour sur Windows (scripts/migration-test.ps1, scripts/update-test.ps1,
scripts/release-update-test.ps1).

    python scripts/fake_github.py <dossier> <version> <port> [fichier...]

Sert la release vX.Y.Z (GET /repos/<depot>/releases/latest) avec les fichiers indiques (par defaut GoXLR-Mini.zip
et GoXLR-Mini.sha256), et ces fichiers (GET /dl/vX.Y.Z/<nom>), comme GitHub le fait pour les updaters.
"""
import hashlib
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

dist, version, port = sys.argv[1], sys.argv[2], int(sys.argv[3])
tag = "v" + version
base = f"http://127.0.0.1:{port}"
NAMES = tuple(sys.argv[4:]) or ("GoXLR-Mini.zip", "GoXLR-Mini.sha256")


def asset(name):
    with open(os.path.join(dist, name), "rb") as f:
        data = f.read()
    return {"name": name, "size": len(data), "browser_download_url": f"{base}/dl/{tag}/{name}",
            "digest": "sha256:" + hashlib.sha256(data).hexdigest()}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        if os.environ.get("RUSTUDIO_FAKE_GITHUB_LOG") == "1":
            print(args[0] % args[1:], flush=True)

    def reply(self, code, body, ctype):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path.endswith("/releases/latest"):
            rel = {"tag_name": tag, "name": "Rustudio " + tag, "draft": False, "prerelease": False,
                   "body": "Rustudio " + version, "html_url": f"{base}/release/{tag}",
                   "published_at": "2026-01-01T00:00:00Z", "assets": [asset(n) for n in NAMES]}
            return self.reply(200, json.dumps(rel).encode(), "application/json")
        name = path.rsplit("/", 1)[-1]
        if path.startswith(f"/dl/{tag}/") and name in NAMES:
            with open(os.path.join(dist, name), "rb") as f:
                return self.reply(200, f.read(), "application/octet-stream")
        self.reply(404, b"{}", "application/json")


ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
