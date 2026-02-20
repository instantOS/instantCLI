#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

VERSION="$(awk -F '"' '/^version =/ {print $2; exit}' Cargo.toml)"
if [[ -z "${VERSION}" ]]; then
	echo "failed to extract version from Cargo.toml" >&2
	exit 1
fi

if [[ -n "${1:-}" ]]; then
	INS_BIN="$1"
else
	INS_BIN="target/release/ins"
fi

if [[ ! -x "${INS_BIN}" ]]; then
	echo "binary not found or not executable: ${INS_BIN}" >&2
	echo "build with 'cargo build --release' or pass a binary path as first argument" >&2
	exit 1
fi

if command -v dpkg >/dev/null 2>&1; then
	ARCH="$(dpkg --print-architecture)"
else
	case "$(uname -m)" in
	x86_64) ARCH="amd64" ;;
	aarch64) ARCH="arm64" ;;
	armv7l) ARCH="armhf" ;;
	*) ARCH="$(uname -m)" ;;
	esac
fi

WORK_DIR="${ROOT_DIR}/target/deb"
PKG_DIR="${WORK_DIR}/ins_${VERSION}_${ARCH}"

rm -rf "${PKG_DIR}"
mkdir -p "${PKG_DIR}/DEBIAN" \
	"${PKG_DIR}/usr/bin" \
	"${PKG_DIR}/usr/share/doc/ins" \
	"${PKG_DIR}/usr/share/applications"

install -Dm755 "${INS_BIN}" "${PKG_DIR}/usr/bin/ins"
install -Dm644 "LICENSE" "${PKG_DIR}/usr/share/doc/ins/copyright"
install -Dm644 "README.md" "${PKG_DIR}/usr/share/doc/ins/README.md"

for desktop_file in desktop/*.desktop; do
	install -Dm644 "${desktop_file}" "${PKG_DIR}/usr/share/applications/$(basename "${desktop_file}")"
done

cat >"${PKG_DIR}/DEBIAN/control" <<EOF
Package: ins
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Depends: fzf, git, libsqlite3-0
Maintainer: paperbenni <paperbenni@gmail.com>
Homepage: https://instantos.io
Description: Instant CLI - command-line utilities
 A powerful command-line tool for managing dotfiles, system diagnostics,
 and instantOS configurations.
EOF

OUTPUT="${WORK_DIR}/ins_${VERSION}_${ARCH}.deb"
dpkg-deb --build --root-owner-group "${PKG_DIR}" "${OUTPUT}"

echo "deb package created at ${OUTPUT}"
