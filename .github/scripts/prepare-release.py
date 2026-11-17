#!/usr/bin/env python3
import argparse
import datetime as dt
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def run(args: list[str]) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def read(path: str) -> str:
    return (ROOT / path).read_text()


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text)


def current_version() -> str:
    match = re.search(r'^version = "([^"]+)"', read("Cargo.toml"), re.MULTILINE)
    if not match:
        raise SystemExit("failed to find package version in Cargo.toml")
    return match.group(1)


def latest_version_tag() -> str | None:
    tags = run(
        ["git", "tag", "--merged", "HEAD", "--sort=-v:refname", "--list", "v[0-9]*"]
    )
    return next(
        (tag for tag in tags.splitlines() if re.fullmatch(r"v\d+\.\d+\.\d+", tag)), None
    )


def commits_since(tag: str | None) -> list[tuple[str, str, str]]:
    git_range = f"{tag}..HEAD" if tag else "HEAD"
    raw = run(["git", "log", "--no-merges", "--format=%H%x00%s%x00%b%x1e", git_range])
    commits = []
    for entry in raw.strip("\x1e").split("\x1e"):
        if not entry.strip():
            continue
        sha, subject, body = entry.lstrip("\n").split("\x00", 2)
        if subject.startswith(("chore: release v", "chore(release):")):
            continue
        commits.append((sha, subject.strip(), body.strip()))
    return commits


def bump_level(commits: list[tuple[str, str, str]], requested: str) -> str | None:
    if requested != "auto":
        return requested
    level: str | None = None
    for _, subject, body in commits:
        if re.search(r"BREAKING[ -]CHANGE:", body) or re.match(
            r"^[a-zA-Z]+(?:\([^)]+\))?!:", subject
        ):
            return "minor" if current_version().startswith("0.") else "major"
        commit_type = subject.split(":", 1)[0].split("(", 1)[0].rstrip("!")
        if commit_type == "feat":
            level = "minor"
        elif (
            commit_type not in {"chore", "ci", "docs", "style", "test"}
            and level != "minor"
        ):
            level = "patch"
    return level


def bump_version(version: str, level: str) -> str:
    major, minor, patch = [int(part) for part in version.split(".")]
    if level == "major":
        return f"{major + 1}.0.0"
    if level == "minor":
        return f"{major}.{minor + 1}.0"
    if level == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise SystemExit(f"unknown bump level: {level}")


def update_cargo_toml(version: str) -> None:
    text = read("Cargo.toml")
    text = re.sub(
        r'^(version = )"[^"]+"', rf'\1"{version}"', text, count=1, flags=re.MULTILINE
    )
    write("Cargo.toml", text)


def update_cargo_lock(version: str) -> None:
    text = read("Cargo.lock")
    # This repository has one package today; fail loudly if that changes.
    pattern = re.compile(r'(\[\[package\]\]\nname = "ins"\nversion = )"[^"]+"')
    text, count = pattern.subn(rf'\1"{version}"', text, count=1)
    if count == 0:
        raise SystemExit("failed to find ins package in Cargo.lock")
    write("Cargo.lock", text)


def update_pkgbuild(version: str) -> None:
    text = read("pkgbuild/ins/PKGBUILD")
    text = re.sub(r"^pkgver=.*", f"pkgver={version}", text, flags=re.MULTILINE)
    text = re.sub(r"^pkgrel=.*", "pkgrel=1", text, flags=re.MULTILINE)
    write("pkgbuild/ins/PKGBUILD", text)
    # .SRCINFO contains the expanded source URL as well as the package version.
    text = read("pkgbuild/ins/.SRCINFO")
    match = re.search(r"^\s*pkgver = (.+)$", text, re.MULTILINE)
    if not match:
        raise SystemExit("failed to find pkgver in .SRCINFO")
    old_version = match.group(1)
    text = re.sub(r"^(\s*pkgver = ).+$", rf"\g<1>{version}", text, flags=re.MULTILINE)
    text = re.sub(r"^(\s*pkgrel = ).+$", r"\g<1>1", text, flags=re.MULTILINE)
    text = text.replace(f"ins-{old_version}.tar.gz", f"ins-{version}.tar.gz")
    text = text.replace(f"/tags/v{old_version}.tar.gz", f"/tags/v{version}.tar.gz")
    write("pkgbuild/ins/.SRCINFO", text)


def clean_subject(subject: str) -> tuple[str, str]:
    match = re.match(r"^(?P<type>[a-zA-Z]+)(?:\([^)]+\))?!?:\s*(?P<text>.+)$", subject)
    if not match:
        return "Other", subject
    commit_type = match.group("type")
    text = match.group("text")
    if commit_type == "feat":
        return "Added", text
    if commit_type == "fix":
        return "Fixed", text
    if commit_type in {"perf", "refactor"}:
        return "Changed", text
    return "Other", text


def update_changelog(
    version: str, previous_tag: str | None, commits: list[tuple[str, str, str]]
) -> None:
    text = read("CHANGELOG.md")
    # Rebuild pending release notes from Git. This also replaces duplicate
    # untagged sections left by release-plz during the migration.
    released = (
        tuple(map(int, previous_tag.removeprefix("v").split(".")))
        if previous_tag
        else None
    )
    sections = re.split(r"(?=^## \[)", text, flags=re.MULTILINE)
    kept = []
    for section in sections:
        match = re.match(r"## \[(\d+\.\d+\.\d+)\]", section)
        if match:
            section_version = match.group(1)
            if section_version == version or (
                released and tuple(map(int, section_version.split("."))) > released
            ):
                continue
        kept.append(section)
    text = "".join(kept)

    today = dt.date.today().isoformat()
    compare_base = previous_tag or "HEAD"
    header = f"## [{version}](https://github.com/instantOS/instantCLI/compare/{compare_base}...v{version}) - {today}"
    groups: dict[str, list[str]] = {
        "Breaking changes": [],
        "Added": [],
        "Changed": [],
        "Fixed": [],
        "Other": [],
    }
    seen: set[str] = set()
    for _, subject, body in reversed(commits):
        group, line = clean_subject(subject)
        breaking = re.search(
            r"BREAKING[ -]CHANGE:\s*(.+?)(?:\n\s*\n|\Z)", body, re.DOTALL
        )
        if breaking:
            detail = " ".join(breaking.group(1).split())
            if f"- {detail}" not in groups["Breaking changes"]:
                groups["Breaking changes"].append(f"- {detail}")
        elif re.match(r"^[a-zA-Z]+(?:\([^)]+\))?!:", subject):
            group = "Breaking changes"
        if line in seen:
            continue
        seen.add(line)
        groups[group].append(f"- {line}")

    sections = [header]
    for group, lines in groups.items():
        if lines:
            sections.append(f"### {group}\n\n" + "\n".join(lines))
    release_notes = "\n\n".join(sections)

    marker = "## [Unreleased]"
    if marker not in text:
        raise SystemExit("failed to find Unreleased section in CHANGELOG.md")
    prefix, _, history = text.partition(marker)
    text = f"{prefix}{marker}\n\n{release_notes}\n\n{history.lstrip()}"
    write("CHANGELOG.md", text)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--bump", choices=["auto", "patch", "minor", "major"], default="auto"
    )
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    old_version = current_version()
    previous_tag = latest_version_tag()
    commits = commits_since(previous_tag)
    level = bump_level(commits, args.bump)
    if not level:
        print("No release-worthy commits found; nothing to do.")
        return 0

    # Base the bump on the last release so repeated preparation before tagging
    # produces the same version, including when migrating an untagged release PR.
    base_version = previous_tag.removeprefix("v") if previous_tag else old_version
    new_version = bump_version(base_version, level)
    print(f"Bumping {old_version} -> {new_version} ({level})")
    if args.dry_run:
        return 0

    update_cargo_toml(new_version)
    update_cargo_lock(new_version)
    update_pkgbuild(new_version)
    update_changelog(new_version, previous_tag, commits)
    return 0


if __name__ == "__main__":
    sys.exit(main())
