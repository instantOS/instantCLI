#!/usr/bin/env python3
"""Exercise archive validation, npm packing/installing, and launcher behavior offline."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import sys
sys.dont_write_bytecode = True
import tomllib

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("packaging", ROOT / "npm/build-packages.py")
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)
version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def run(*args, cwd=None):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True, check=True)


with tempfile.TemporaryDirectory() as temporary:
    work = Path(temporary)
    artifacts = work / "artifacts"
    artifacts.mkdir()
    for target in packaging.TARGETS.values():
        archive = artifacts / f"ins-{target}-v{version}.tgz"
        # A shell fixture makes argument and exit-code assertions architecture independent.
        binary = b'#!/bin/sh\nprintf "%s\\n" "$@"\nexit 42\n'
        with tarfile.open(archive, "w:gz") as tar:
            member = tarfile.TarInfo(f"ins-{target}-v{version}/ins")
            member.size = len(binary)
            member.mode = 0o755
            tar.addfile(member, io.BytesIO(binary))
        Path(f"{archive}.sha256").write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + "  " + archive.name)
    try:
        packaging.stage(artifacts, work / "bad-tag", "v0.0.0")
        raise AssertionError("accepted mismatched tag")
    except ValueError:
        pass
    output = work / "packages"
    packaging.stage(artifacts, output, f"v{version}")
    main = json.loads((output / "cli/package.json").read_text())
    assert main["optionalDependencies"] == {f"@instantos/cli-linux-{arch}": version for arch in packaging.TARGETS}
    tarballs = {}
    for directory in output.iterdir():
        packed_result = json.loads(run("npm", "pack", "--json", "--pack-destination", str(work), cwd=directory).stdout)
        packed = packed_result[0] if isinstance(packed_result, list) else next(iter(packed_result.values()))
        assert "LICENSE" in {entry["path"] for entry in packed["files"]}
        tarballs[directory.name] = work / packed["filename"]
    install = work / "install"
    install.mkdir()
    (install / "package.json").write_text('{"private":true}')
    arch = run("node", "-p", "process.arch").stdout.strip()
    assert arch in packaging.TARGETS, "smoke test needs a supported Linux host"
    run("npm", "install", "--offline", "--ignore-scripts", "--no-audit", "--no-fund",
        str(tarballs[f"cli-linux-{arch}"]), str(tarballs["cli"]), cwd=install)
    launcher = install / "node_modules/.bin/ins"
    result = subprocess.run([str(launcher), "--example", "argument with spaces"], capture_output=True, text=True)
    assert result.returncode == 42, result
    assert result.stdout == "--example\nargument with spaces\n", result
    (install / f"node_modules/@instantos/cli-linux-{arch}/bin/ins").unlink()
    result = subprocess.run([str(launcher)], capture_output=True, text=True)
    assert result.returncode == 1 and "optional dependencies" in result.stderr, result
    archive.write_bytes(b"corrupted")
    try:
        packaging.stage(artifacts, work / "bad-checksum", f"v{version}")
        raise AssertionError("accepted corrupted archive")
    except ValueError as error:
        assert "Checksum mismatch" in str(error)
print("npm packaging, local install, argument/exit forwarding, and validation passed")
