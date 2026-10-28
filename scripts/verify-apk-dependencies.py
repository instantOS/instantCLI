#!/usr/bin/env python3
"""Check explicit APK dependency names against Alpine's published indexes.

Read-only: downloads metadata without installing packages or changing repositories.
Uses only Python's standard library; run from any working directory.
"""

import argparse
import hashlib
import io
from pathlib import Path
import re
import sys
import tarfile
import urllib.error
import urllib.request


def dependency_names(root):
    names = set()
    for path in (root / "src").rglob("*.rs"):
        source = path.read_text()
        names.update(re.findall(
            r'PackageDefinition::new\(\s*"([^"]+)"\s*,\s*PackageManager::Apk',
            source,
        ))
        names.update(re.findall(r'\bapk:\s*"([^"]+)"', source))
    if not names:
        raise ValueError("No explicit APK dependency mappings found")
    return sorted(names)


def load_index(branch, repository, architecture):
    url = (
        "https://dl-cdn.alpinelinux.org/alpine/"
        f"{branch}/{repository}/{architecture}/APKINDEX.tar.gz"
    )
    with urllib.request.urlopen(url, timeout=30) as response:
        data = response.read()
    print(f"Index: {url} sha256={hashlib.sha256(data).hexdigest()}")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        index = archive.extractfile("APKINDEX")
        if index is None:
            raise ValueError(f"No APKINDEX in {url}")
        content = index.read().decode()
    packages = {}
    for block in content.split("\n\n"):
        fields = dict(line.split(":", 1) for line in block.splitlines() if ":" in line)
        if "P" in fields:
            packages[fields["P"]] = fields
    if not packages:
        raise ValueError(f"Empty package index: {url}")
    return packages


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--branch", default="v3.24", help="Alpine branch (default: v3.24)")
    parser.add_argument(
        "--arch", action="append", dest="architectures",
        help="Architecture to check; repeat for multiple (default: aarch64 and x86_64)",
    )
    args = parser.parse_args()
    for component in [args.branch, *(args.architectures or [])]:
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", component):
            parser.error(f"Invalid branch or architecture: {component!r}")
    names = dependency_names(Path(__file__).resolve().parent.parent)
    failed = False
    for architecture in args.architectures or ["aarch64", "x86_64"]:
        packages = {}
        for repository in ["main", "community"]:
            for name, fields in load_index(args.branch, repository, architecture).items():
                packages[name] = (repository, fields)
        for name in names:
            if name not in packages:
                print(f"MISSING: {args.branch}/{architecture}: {name}", file=sys.stderr)
                failed = True
            else:
                repository, fields = packages[name]
                print(f"OK: {args.branch}/{architecture}/{repository}: {name} {fields['V']}")
        print(f"Checked {len(names)} APK dependency names for {args.branch}/{architecture}")
    return 1 if failed else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, tarfile.TarError, urllib.error.URLError) as error:
        print(f"Verification failed: {error}", file=sys.stderr)
        sys.exit(1)
