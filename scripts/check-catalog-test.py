#!/usr/bin/env python3
"""Tests des preuves de provenance : aucune API réelle ni approbation automatique."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("catalog", Path(__file__).with_name("check-catalog.py"))
catalog = importlib.util.module_from_spec(spec)
spec.loader.exec_module(catalog)


class Api:
    def __init__(self, root):
        self.pr = {"base": {"repo": {"full_name": catalog.REPO}, "ref": "main"}, "head": {"sha": "a" * 40},
                   "merged": True, "user": {"login": "contributeur"}, "merged_by": {"login": catalog.OWNER}}
        self.reviews = [{"user": {"login": catalog.OWNER}, "state": "APPROVED", "commit_id": "a" * 40, "submitted_at": "2026-01-02T00:00:00Z"}]
        self.tree = {"truncated": False, "tree": [{"path": p, "type": "blob", "mode": m, "sha": s} for p, (m, s) in catalog.local_tree(root, "crates/demo").items()]}

    def __call__(self, endpoint):
        return self.tree if "/git/trees/" in endpoint else self.pr

    def pages(self, endpoint):
        return self.reviews


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="rustudio-catalog-test-")
        self.root = Path(self.tmp.name)
        source = self.root / "crates/demo"
        source.mkdir(parents=True)
        (source / "Cargo.toml").write_text('[package]\nname="demo"\nversion="1.0.0"\n', encoding="utf-8")
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=test", "-c", "user.email=test@users.noreply.github.com", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"], check=True)
        self.module = {"id":"demo", "name":"Démo", "vendor":"Test", "summary":"Module de test.", "status":"community", "author":"contributeur", "review_pr":17, "builtin":True, "source":"crates/demo"}
        self.api = Api(self.root)

    def tearDown(self):
        self.tmp.cleanup()

    def check(self):
        return catalog.verify_module(self.module, self.root, self.api)

    def test_validated_exact_source(self):
        catalog.validate({"schema":2,"modules":[self.module]}, self.root)
        self.check()

    def test_unmerged_is_never_published(self):
        self.api.pr["merged"] = False
        with self.assertRaises(catalog.Invalid): self.check()
        catalog.verify_module(self.module, self.root, self.api, pending_pr=17)

    def test_review_at_previous_commit(self):
        self.api.reviews[0]["commit_id"] = "b" * 40
        with self.assertRaises(catalog.Invalid): self.check()

    def test_contributor_cannot_approve(self):
        self.api.reviews[0]["user"]["login"] = "contributeur"
        with self.assertRaises(catalog.Invalid): self.check()

    def test_dismissal_or_change_request_revokes_approval(self):
        for state in ("DISMISSED", "CHANGES_REQUESTED"):
            self.api.reviews.append({**self.api.reviews[0], "state":state, "submitted_at":"2026-01-03T00:00:00Z"})
            with self.assertRaises(catalog.Invalid): self.check()

    def test_maintainer_own_pr_requires_his_merge(self):
        self.api.pr["user"]["login"] = catalog.OWNER
        self.api.reviews = []
        self.check()
        self.api.pr["merged_by"]["login"] = "autre"
        with self.assertRaises(catalog.Invalid): self.check()

    def test_different_source_and_truncated_tree(self):
        self.api.tree["tree"][0]["sha"] = "b" * 40
        with self.assertRaises(catalog.Invalid): self.check()
        self.api.tree["truncated"] = True
        with self.assertRaises(catalog.Invalid): self.check()

    def test_worktree_change_is_not_reviewed_code(self):
        (self.root / "crates/demo/Cargo.toml").write_text("autre code", encoding="utf-8")
        with self.assertRaises(catalog.Invalid): self.check()

    def test_unsafe_metadata(self):
        for changes in ({"source":"crates/../private"}, {"review_pr":True}, {"review_pr":0}, {"author":"adresse@example.com"}, {"status":"official"}, {"builtin":False}):
            with self.subTest(changes=changes), self.assertRaises(catalog.Invalid):
                catalog.validate({"schema":2,"modules":[{**self.module, **changes}]}, self.root)

    def test_external_submission_cannot_claim_official_or_other_author(self):
        before = {"modules":[]}
        pr = {"number":17,"user":{"login":"contributeur"}}
        catalog.check_submission([self.module], before, pr)
        for changes in ({"status":"official","author":catalog.OWNER}, {"author":"autre"}, {"review_pr":18}):
            with self.assertRaises(catalog.Invalid): catalog.check_submission([{**self.module, **changes}], before, pr)

    def test_public_commits_accept_masked_github_bots_but_not_private_mail(self):
        def commit(email, text):
            (self.root / "proof.txt").write_text(text, encoding="utf-8")
            subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
            subprocess.run(["git", "-C", str(self.root), "-c", "user.name=test", "-c", "user.email=" + email, "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"], check=True)
        commit("49699333+dependabot[bot]@users.noreply.github.com", "masqué")
        catalog.public_files(self.root)
        commit("personne@example.com", "autre")
        with self.assertRaises(catalog.Invalid): catalog.public_files(self.root)

    def test_wrong_repository_or_branch(self):
        self.api.pr["base"]["ref"] = "autre"
        with self.assertRaises(catalog.Invalid): self.check()


if __name__ == "__main__":
    unittest.main()
