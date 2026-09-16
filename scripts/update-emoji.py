#!/usr/bin/env python3
"""Regenerate the compact, pinned Unicode emoji catalog (not run at build time)."""

from pathlib import Path
import urllib.request

VERSION = "17.0.0"
URL = f"https://www.unicode.org/Public/{VERSION}/emoji/emoji-test.txt"
DESTINATION = Path(__file__).resolve().parents[1] / "src/assist/actions/emoji/catalog.tsv"


def catalog(source: str) -> str:
    rows = []
    seen = set()
    for line in source.splitlines():
        if not line or line.startswith("#"):
            continue
        codepoints, rest = line.split(";", 1)
        status, comment = rest.split("#", 1)
        # Exclude unqualified duplicates and standalone components.
        if status.strip() != "fully-qualified":
            continue
        emoji = "".join(chr(int(codepoint, 16)) for codepoint in codepoints.split())
        displayed, _version, name = comment.strip().split(" ", 2)
        assert emoji == displayed and emoji not in seen
        assert name and not any(c in name for c in "\t\r\n")
        seen.add(emoji)
        rows.append(f"{emoji}\t{name}\n")
    assert len(rows) == 3944, "Unexpected catalog size; review the Unicode update"
    return "".join(rows)


if __name__ == "__main__":
    with urllib.request.urlopen(URL, timeout=30) as response:
        source = response.read().decode("utf-8")
    notice = DESTINATION.with_name("LICENSE").read_text(encoding="utf-8")
    output = f"# Source: {URL}\n" + "".join(f"# {line}\n" for line in notice.splitlines())
    output += catalog(source)
    DESTINATION.parent.mkdir(parents=True, exist_ok=True)
    DESTINATION.write_text(output, encoding="utf-8")
    print(f"Wrote {len(output.encode('utf-8'))} bytes to {DESTINATION}")
