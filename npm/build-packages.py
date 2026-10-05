#!/usr/bin/env python3
"""Stage npm packages from the existing release archives; no registry access."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x64": "x86_64-unknown-linux-musl",
    "arm64": "aarch64-unknown-linux-musl",
}


def stage(artifacts: Path, output: Path, tag: str):
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
    version = cargo["version"]
    if tag != f"v{version}":
        raise ValueError(f"Release tag {tag!r} does not match Cargo.toml v{version}")
    output.mkdir(parents=True, exist_ok=False)
    common = {
        "version": version,
        "license": cargo["license"],
        "homepage": cargo["homepage"],
        "repository": {"type": "git", "url": f"git+{cargo['repository']}.git"},
        "publishConfig": {"access": "public"},
    }
    dependencies = {}
    for arch, target in TARGETS.items():
        name = f"@instantos/cli-linux-{arch}"
        dependencies[name] = version
        archive = artifacts / f"ins-{target}-v{version}.tgz"
        expected = Path(f"{archive}.sha256").read_text().split()[0]
        if hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
            raise ValueError(f"Checksum mismatch: {archive}")
        package = output / f"cli-linux-{arch}"
        (package / "bin").mkdir(parents=True)
        with tarfile.open(archive) as tar:
            member = tar.getmember(f"ins-{target}-v{version}/ins")
            if not member.isfile():
                raise ValueError(f"Not a regular binary: {archive}")
            with tar.extractfile(member) as source, (package / "bin/ins").open("wb") as dest:
                shutil.copyfileobj(source, dest)
        (package / "bin/ins").chmod(0o755)
        metadata = dict(common, name=name, os=["linux"], cpu=[arch], files=["bin/ins"])
        (package / "package.json").write_text(json.dumps(metadata, indent=2) + "\n")
        shutil.copy2(ROOT / "LICENSE", package)
        shutil.copy2(ROOT / "npm/README.md", package)
    package = output / "cli"
    (package / "bin").mkdir(parents=True)
    shutil.copy2(ROOT / "npm/bin/ins.cjs", package / "bin/ins.cjs")
    (package / "bin/ins.cjs").chmod(0o755)
    metadata = dict(common, name="@instantos/cli", description=cargo["description"],
                    bin={"ins": "bin/ins.cjs"}, engines={"node": ">=18"},
                    os=["linux"], cpu=list(TARGETS), files=["bin/ins.cjs"],
                    optionalDependencies=dependencies)
    (package / "package.json").write_text(json.dumps(metadata, indent=2) + "\n")
    shutil.copy2(ROOT / "LICENSE", package)
    shutil.copy2(ROOT / "npm/README.md", package)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    stage(args.artifacts, args.output, args.tag)
