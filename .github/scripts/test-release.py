#!/usr/bin/env python3
"""Exercise release preparation and tag creation in disposable repositories."""

import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True

SOURCE = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "prepare_release", SOURCE / ".github/scripts/prepare-release.py"
)
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Release test")
        self.git("config", "user.email", "test@example.com")
        subprocess.run(
            ["git", "init", "--bare", str(self.root.parent / "remote")],
            check=True,
            capture_output=True,
        )
        self.git("remote", "add", "origin", str(self.root.parent / "remote"))
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            "CHANGELOG.md",
            "pkgbuild/ins/PKGBUILD",
            "pkgbuild/ins/.SRCINFO",
        ]:
            dest = self.root / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SOURCE / name, dest)
        for directory in [".github/scripts", "scripts/ci"]:
            shutil.copytree(
                SOURCE / directory,
                self.root / directory,
                ignore=shutil.ignore_patterns("__pycache__"),
            )
        # Normalize the initial fixture, independently of the real package version.
        with patch.object(prepare, "ROOT", self.root):
            prepare.update_cargo_toml("0.14.24")
            prepare.update_cargo_lock("0.14.24")
            prepare.update_pkgbuild("0.14.24")
        self.commit("Initial release")
        self.git("tag", "v0.14.24")

    def git(self, *args):
        return subprocess.check_output(
            ["git", *args], cwd=self.root, text=True, stderr=subprocess.DEVNULL
        ).strip()

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "--allow-empty", "-m", message)

    def prepare(self, *args):
        return subprocess.check_output(
            ["python3", ".github/scripts/prepare-release.py", *args],
            cwd=self.root,
            text=True,
        )

    def metadata(self, event="push", ref_type="branch", ref="main", success=True):
        output = self.root.parent / "output"
        output.write_text("")
        env = dict(
            os.environ,
            GITHUB_OUTPUT=str(output),
            GITHUB_REF_TYPE=ref_type,
            GITHUB_REF_NAME=ref,
            GITHUB_EVENT_NAME=event,
        )
        result = subprocess.run(
            ["bash", "scripts/ci/determine-release.sh"],
            check=False,
            cwd=self.root,
            env=env,
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return dict(line.split("=", 1) for line in output.read_text().splitlines())

    def test_feature_preparation_updates_all_metadata_and_is_repeatable(self):
        self.commit(
            "feat(menu): Add picker\n\nBREAKING CHANGE: Use --shell-command for scripts."
        )
        self.prepare()
        snapshot = self.git("diff")
        self.assertIn('version = "0.15.0"', (self.root / "Cargo.toml").read_text())
        self.assertIn(
            'name = "ins"\nversion = "0.15.0"', (self.root / "Cargo.lock").read_text()
        )
        self.assertIn(
            "pkgver=0.15.0", (self.root / "pkgbuild/ins/PKGBUILD").read_text()
        )
        srcinfo = (self.root / "pkgbuild/ins/.SRCINFO").read_text()
        self.assertIn(
            "ins-0.15.0.tar.gz::https://github.com/instantOS/instantCLI/archive/refs/tags/v0.15.0.tar.gz",
            srcinfo,
        )
        changelog = (self.root / "CHANGELOG.md").read_text()
        self.assertEqual(changelog.count("## [0.15.0]"), 1)
        self.assertNotIn("## [0.14.25]", changelog)
        self.assertIn("## [0.14.24]", changelog)
        self.assertIn("- Add picker", changelog)
        self.assertIn("### Breaking changes", changelog)
        self.assertIn("Use --shell-command for scripts.", changelog)
        self.prepare()
        self.assertEqual(self.git("diff"), snapshot)
        # Preparing a version must not change shared Git dependency revisions.
        with patch.object(prepare, "ROOT", self.root):
            original = subprocess.check_output(
                ["git", "show", "HEAD:Cargo.lock"], cwd=self.root, text=True
            )
            self.assertEqual(
                prepare.read("Cargo.lock").replace(
                    'name = "ins"\nversion = "0.15.0"',
                    'name = "ins"\nversion = "0.14.24"',
                ),
                original,
            )

    def test_bump_policy_and_manual_override(self):
        with patch.object(prepare, "ROOT", self.root):

            def level(subject, body="", requested="auto"):
                return prepare.bump_level([("sha", subject, body)], requested)

            self.assertEqual(level("fix: bug"), "patch")
            self.assertEqual(level("Improve picker"), "patch")
            self.assertEqual(level("feat(menu): picker"), "minor")
            self.assertEqual(level("feat!: new arguments"), "minor")
            self.assertEqual(level("refactor: args", "BREAKING CHANGE: args"), "minor")
            self.assertIsNone(level("docs: usage"))
            self.assertEqual(level("docs: usage", requested="major"), "major")
            prepare.update_cargo_toml("1.2.3")
            self.assertEqual(level("fix!: changed interface"), "major")

    def test_docs_only_and_dry_run_do_not_modify_files(self):
        self.commit("docs: Explain installation")
        self.prepare()
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.commit("fix: bug")
        self.prepare("--dry-run")
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.prepare("--bump", "major")
        self.assertIn('version = "1.0.0"', (self.root / "Cargo.toml").read_text())

    def test_branch_release_retry_and_tag_validation(self):
        self.commit("fix: bug")
        self.assertEqual(self.metadata()["should_release"], "false")
        self.prepare()
        self.commit("chore: release v0.14.25 (#123)")
        self.assertEqual(self.metadata()["should_release"], "true")
        self.assertEqual(
            self.git("rev-parse", "v0.14.25^{commit}"), self.git("rev-parse", "HEAD")
        )
        self.assertEqual(self.metadata()["should_release"], "true")
        self.assertEqual(
            self.metadata(ref_type="tag", ref="v0.14.25")["tag"], "v0.14.25"
        )
        self.metadata(ref_type="tag", ref="v9.9.9", success=False)
        self.commit("chore: release v0.14.25")
        self.metadata(success=False)

    def test_ordinary_merge_release_and_manual_dispatch(self):
        self.git("checkout", "-b", "release/version-bump")
        self.commit("feat: picker")
        self.prepare()
        self.commit("chore: release v0.15.0")
        self.git("checkout", "main")
        self.git(
            "merge", "--no-ff", "release/version-bump", "-m", "Merge pull request #123"
        )
        self.assertEqual(self.metadata()["should_release"], "true")
        # A manual run on a different version can release without a bot subject.
        with patch.object(prepare, "ROOT", self.root):
            prepare.update_cargo_toml("0.15.1")
        self.commit("Manual version")
        self.assertEqual(self.metadata(event="workflow_dispatch")["tag"], "v0.15.1")


if __name__ == "__main__":
    unittest.main()
