#!/usr/bin/env python3
"""Catalogue livré : attribution, PR approuvée et code identique à la révision relue."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

REPO = "samirmedani/rustudio"
OWNER = "samirmedani"
LOGIN = re.compile(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?")
SOURCE = re.compile(r"crates/[a-z0-9][a-z0-9-]{0,63}")
NOREPLY = re.compile(r"(?:[0-9]+\+)?[A-Za-z0-9-]+@users\.noreply\.github\.com|noreply@github\.com")


class Invalid(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise Invalid(message)


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def validate(index, root):
    require(index.get("schema") == 2, "Schéma de catalogue attendu : 2")
    modules = index.get("modules")
    require(isinstance(modules, list) and len(modules) <= 500, "Liste de modules invalide")
    ids = set()
    for m in modules:
        mid = m.get("id", "")
        require(isinstance(mid, str) and re.fullmatch(r"[a-z0-9][a-z0-9-]{0,63}", mid), "Identifiant invalide")
        require(mid not in ids, "Identifiant dupliqué : " + mid)
        ids.add(mid)
        status = m.get("status")
        require(status in ("official", "community", "wanted"), mid + " : origine invalide")
        for key, limit in (("name", 80), ("vendor", 60), ("summary", 400)):
            value = m.get(key)
            require(isinstance(value, str) and 0 < len(value) <= limit and all(ord(c) >= 32 for c in value), mid + " : " + key + " invalide")
        if status == "wanted":
            require(not m.get("builtin") and not m.get("source") and not m.get("author") and not m.get("review_pr"), mid + " : une demande ne peut pas se déclarer livrée")
            continue
        author, source, pr = m.get("author"), m.get("source"), m.get("review_pr")
        require(isinstance(author, str) and LOGIN.fullmatch(author), mid + " : pseudo GitHub public requis")
        require(isinstance(source, str) and SOURCE.fullmatch(source), mid + " : chemin de module invalide")
        require((root / source / "Cargo.toml").is_file(), mid + " : crate absente")
        require(m.get("builtin") is True, mid + " : module non livré interdit au catalogue")
        if status == "official":
            require(author == OWNER and pr is None, mid + " : attribution officielle invalide")
        else:
            require(type(pr) is int and 0 < pr <= 2**32 - 1, mid + " : numéro de PR obligatoire")
    return modules


class GitHub:
    def __init__(self):
        self.cache = {}

    def __call__(self, endpoint):
        if endpoint not in self.cache:
            # gh utilise l'authentification existante ; aucun jeton dans les arguments ou journaux.
            self.cache[endpoint] = json.loads(subprocess.check_output(["gh", "api", endpoint]))
        return self.cache[endpoint]

    def pages(self, endpoint):
        result = []
        for page in range(1, 21):
            batch = self(endpoint + "?per_page=100&page=" + str(page))
            require(isinstance(batch, list), "Réponse GitHub invalide")
            result.extend(batch)
            if len(batch) < 100:
                return result
        raise Invalid("Trop de réponses GitHub : vérification interrompue")


def local_tree(root, source):
    require((root / source).resolve().is_relative_to(root.resolve()), "Source hors du dépôt")
    require(not git(root, "status", "--porcelain", "--untracked-files=all", "--", source).strip(), "Sources modifiées ou non suivies")
    files = git(root, "ls-tree", "-r", "-z", "HEAD", "--", source).decode().split("\0")
    tree = {}
    for entry in filter(None, files):
        meta, path = entry.split("\t", 1)
        mode, kind, oid = meta.split()
        file = root / path
        require(kind == "blob" and mode in ("100644", "100755") and not file.is_symlink(), "Source non régulière")
        require(file.resolve().is_relative_to((root / source).resolve()), "Source hors de sa crate")
        tree[path] = (mode, oid)
    require(tree, "Crate vide")
    # Une source non suivie ne doit pas contourner la comparaison avec la PR.
    require(not git(root, "ls-files", "--others", "--exclude-standard", "--", source).strip(), "Sources non suivies")
    return tree


def verify_module(module, root, api, pending_pr=None):
    number = module["review_pr"]
    base = "repos/" + REPO
    pr = api(base + "/pulls/" + str(number))
    require(pr.get("base", {}).get("repo", {}).get("full_name") == REPO and pr["base"]["ref"] == "main", "PR hors du dépôt ou de main")
    head = pr["head"]["sha"]
    require(re.fullmatch(r"[0-9a-f]{40}", head), "Révision GitHub invalide")
    pending = number == pending_pr and not pr.get("merged")
    if not pending:
        require(pr.get("merged") is True, "PR communautaire non fusionnée : #" + str(number))
        # GitHub interdit l'auto-approbation. Le mainteneur valide sa propre PR par sa fusion.
        own_merge = pr["user"]["login"] == OWNER and (pr.get("merged_by") or {}).get("login") == OWNER
        reviews = [r for r in api.pages(base + "/pulls/" + str(number) + "/reviews")
                   if r.get("user", {}).get("login") == OWNER and r.get("state") in ("APPROVED", "CHANGES_REQUESTED", "DISMISSED")]
        last = max(reviews, key=lambda r: (r.get("submitted_at") or "", r.get("id", 0)), default={})
        approved = last.get("state") == "APPROVED" and last.get("commit_id") == head
        require(own_merge or approved, "Validation du mainteneur absente ou périmée : #" + str(number))
    response = api(base + "/git/trees/" + head + "?recursive=1")
    require(not response.get("truncated"), "Arbre GitHub tronqué")
    prefix = module["source"] + "/"
    reviewed = {f["path"]: (f["mode"], f["sha"]) for f in response["tree"] if f["type"] != "tree" and f["path"].startswith(prefix)}
    require(local_tree(root, module["source"]) == reviewed, "Code différent de la PR validée : " + module["id"])


def check_submission(modules, before, pr):
    """Une nouvelle contribution tierce ne peut pas s'attribuer l'origine officielle."""
    if pr["user"]["login"] == OWNER:
        return
    old = {m["id"]: m for m in before["modules"]}
    for m in modules:
        previous = old.get(m["id"], {})
        if m["status"] != "wanted" and previous.get("status") not in ("official", "community"):
            require(m["status"] == "community" and m["author"] == pr["user"]["login"] and m["review_pr"] == pr["number"], "Nouvelle contribution : origine communautaire, créateur et PR courante requis")
        elif previous.get("author"):
            require(m.get("author") == previous["author"], "Le créateur d'origine doit être conservé")


def public_files(root):
    paths = git(root, "ls-files", "-z").decode().split("\0")
    require(not any(p.startswith("private/") for p in paths), "Le dossier privé est interdit dans le dépôt public")
    emails = git(root, "log", "--all", "--format=%ae%n%ce").decode().splitlines()
    require(all(NOREPLY.fullmatch(e) for e in emails if e), "Utilise une adresse GitHub noreply dans les commits publics")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-github", action="store_true")
    parser.add_argument("--public", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    modules = validate(json.loads((root / "library/index.json").read_text(encoding="utf-8")), root)
    pending = None
    if args.public:
        public_files(root)
    if os.environ.get("GITHUB_EVENT_NAME") == "pull_request":
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        pr = event["pull_request"]
        before = json.loads(git(root, "show", pr["base"]["sha"] + ":library/index.json"))
        check_submission(modules, before, pr)
        pending = pr["number"]
    if args.verify_github:
        api = GitHub()
        for module in modules:
            if module["status"] == "community":
                verify_module(module, root, api, pending)
    print("Catalogue conforme : " + str(len(modules)) + " fiches" + (" ; preuves GitHub vérifiées" if args.verify_github else " (contrôle local)"))


if __name__ == "__main__":
    try:
        main()
    except (Invalid, OSError, KeyError, TypeError, json.JSONDecodeError, subprocess.CalledProcessError) as error:
        print("Catalogue refusé : " + str(error), file=sys.stderr)
        sys.exit(1)
