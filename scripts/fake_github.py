#!/usr/bin/env python3
"""Faux GitHub local pour les tests de mise a jour sur Windows (scripts/migration-test.ps1, scripts/update-test.ps1,
scripts/release-update-test.ps1).

    python scripts/fake_github.py <dossier> <version> <port> [fichier...]

Sert la release vX.Y.Z (GET /repos/<depot>/releases/latest) avec les fichiers indiques (par defaut GoXLR-Mini.zip
et GoXLR-Mini.sha256), et ces fichiers (GET /dl/vX.Y.Z/<nom>), comme GitHub le fait pour les updaters.

RUSTUDIO_FAKE_OLD=<dossier>|<version> : une version plus ancienne, lisible par son tag
(GET /repos/<depot>/releases/tags/vA.B.C) et listee avec la derniere (GET /repos/<depot>/releases), pour le retour a
une version precedente.
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
old_dist, old_version = (os.environ.get("RUSTUDIO_FAKE_OLD", "").split("|") + ["", ""])[:2]
old_tag = "v" + old_version if old_version else None


def asset(name, folder=None, release_tag=None):
    with open(os.path.join(folder or dist, name), "rb") as f:
        data = f.read()
    return {"name": name, "size": len(data), "browser_download_url": f"{base}/dl/{release_tag or tag}/{name}",
            "digest": "sha256:" + hashlib.sha256(data).hexdigest()}


def release(release_tag, folder):
    return {"tag_name": release_tag, "name": "Rustudio " + release_tag, "draft": False, "prerelease": False,
            "body": "Rustudio " + release_tag[1:], "html_url": f"{base}/release/{release_tag}",
            "published_at": "2026-01-01T00:00:00Z", "assets": [asset(n, folder, release_tag) for n in NAMES]}


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
            return self.reply(200, json.dumps(release(tag, dist)).encode(), "application/json")
        if path.endswith("/releases"):
            rels = [release(tag, dist)] + ([release(old_tag, old_dist)] if old_tag else [])
            return self.reply(200, json.dumps(rels).encode(), "application/json")
        if "/releases/tags/" in path:
            wanted = path.rsplit("/", 1)[-1]
            if wanted == tag:
                return self.reply(200, json.dumps(release(tag, dist)).encode(), "application/json")
            if old_tag and wanted == old_tag:
                return self.reply(200, json.dumps(release(old_tag, old_dist)).encode(), "application/json")
            return self.reply(404, b"{}", "application/json")
        name = path.rsplit("/", 1)[-1]
        for release_tag, folder in ((tag, dist), (old_tag, old_dist)):
            if release_tag and path.startswith(f"/dl/{release_tag}/") and name in NAMES:
                with open(os.path.join(folder, name), "rb") as f:
                    return self.reply(200, f.read(), "application/octet-stream")
        self.reply(404, b"{}", "application/json")


ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
