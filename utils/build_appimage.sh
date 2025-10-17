#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

for cmd in cargo curl unzip bunzip2 tar jq python3 install; do
    if ! command -v "${cmd}" >/dev/null 2>&1; then
        echo "missing dependency: ${cmd}" >&2
        exit 1
    fi
done

RESTIC_VERSION="${RESTIC_VERSION:-0.18.1}"
RCLONE_VERSION="${RCLONE_VERSION:-1.71.1}"
FZF_VERSION="${FZF_VERSION:-0.66.0}"
GUM_VERSION="${GUM_VERSION:-0.17.0}"

INS_VERSION="$(cargo metadata --format-version 1 --no-deps | jq -r '.packages[] | select(.name=="ins") | .version' | head -n1)"

WORK_DIR="${ROOT_DIR}/target/appimage"
APPDIR="${WORK_DIR}/InstantCLI.AppDir"
DOWNLOAD_DIR="${WORK_DIR}/downloads"
BIN_DIR="${APPDIR}/usr/bin"

rm -rf "${APPDIR}"
mkdir -p "${BIN_DIR}" \
         "${APPDIR}/usr/share/applications" \
         "${APPDIR}/usr/share/doc/ins" \
         "${DOWNLOAD_DIR}"

cargo build --release --locked
install -Dm755 "target/release/ins" "${BIN_DIR}/ins"

download() {
    local url="$1"
    local dest="$2"
    if [[ ! -f "${dest}" ]]; then
        curl -L "${url}" -o "${dest}"
    fi
}

bundle_restic() {
    local archive="${DOWNLOAD_DIR}/restic_${RESTIC_VERSION}_linux_amd64.bz2"
    download "https://github.com/restic/restic/releases/download/v${RESTIC_VERSION}/restic_${RESTIC_VERSION}_linux_amd64.bz2" "${archive}"
    bunzip2 -c "${archive}" > "${BIN_DIR}/restic.tmp"
    mv "${BIN_DIR}/restic.tmp" "${BIN_DIR}/restic"
    chmod +x "${BIN_DIR}/restic"
}

bundle_rclone() {
    local archive="${DOWNLOAD_DIR}/rclone-v${RCLONE_VERSION}-linux-amd64.zip"
    download "https://github.com/rclone/rclone/releases/download/v${RCLONE_VERSION}/rclone-v${RCLONE_VERSION}-linux-amd64.zip" "${archive}"
    local tmp
    tmp="$(mktemp -d)"
    unzip -q "${archive}" -d "${tmp}"
    install -Dm755 "${tmp}/rclone-v${RCLONE_VERSION}-linux-amd64/rclone" "${BIN_DIR}/rclone"
    rm -rf "${tmp}"
}

bundle_gum() {
    local archive="${DOWNLOAD_DIR}/gum_${GUM_VERSION}_Linux_x86_64.tar.gz"
    download "https://github.com/charmbracelet/gum/releases/download/v${GUM_VERSION}/gum_${GUM_VERSION}_Linux_x86_64.tar.gz" "${archive}"
    local tmp
    tmp="$(mktemp -d)"
    tar -xzf "${archive}" -C "${tmp}"
    local binary
    binary="$(find "${tmp}" -type f -name gum -perm -u+x | head -n1)"
    if [[ -z "${binary}" ]]; then
        echo "unable to find gum binary in archive" >&2
        exit 1
    fi
    install -Dm755 "${binary}" "${BIN_DIR}/gum"
    rm -rf "${tmp}"
}

bundle_fzf() {
    local archive="${DOWNLOAD_DIR}/fzf-${FZF_VERSION}-linux_amd64.tar.gz"
    download "https://github.com/junegunn/fzf/releases/download/v${FZF_VERSION}/fzf-${FZF_VERSION}-linux_amd64.tar.gz" "${archive}"
    local tmp
    tmp="$(mktemp -d)"
    tar -xzf "${archive}" -C "${tmp}"
    install -Dm755 "${tmp}/fzf" "${BIN_DIR}/fzf"
    rm -rf "${tmp}"
}

bundle_restic
bundle_rclone
bundle_gum
bundle_fzf

cat > "${APPDIR}/AppRun" <<'EOF'
#!/bin/sh
set -e
APPDIR="$(dirname "$(readlink -f "$0")")"
export PATH="${APPDIR}/usr/bin:${PATH}"
exec "${APPDIR}/usr/bin/ins" "$@"
EOF
chmod +x "${APPDIR}/AppRun"

DESKTOP_TEMPLATE="${ROOT_DIR}/utils/appimage/ins.desktop"
if [[ ! -f "${DESKTOP_TEMPLATE}" ]]; then
    echo "desktop template not found: ${DESKTOP_TEMPLATE}" >&2
    exit 1
fi

python3 - "${DESKTOP_TEMPLATE}" "${APPDIR}/ins.desktop" "${INS_VERSION}" <<'PY'
import sys

template, output, version = sys.argv[1:4]
with open(template, 'r', encoding='utf-8') as src:
    content = src.read().replace('@VERSION@', version)
with open(output, 'w', encoding='utf-8') as dst:
    dst.write(content)
PY

install -Dm644 "${APPDIR}/ins.desktop" "${APPDIR}/usr/share/applications/ins.desktop"

install -Dm644 "LICENSE" "${APPDIR}/usr/share/doc/ins/LICENSE"

APPIMAGETOOL="${WORK_DIR}/appimagetool-x86_64.AppImage"
if [[ ! -x "${APPIMAGETOOL}" ]]; then
    download "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage" "${APPIMAGETOOL}"
    chmod +x "${APPIMAGETOOL}"
fi

OUTPUT="${WORK_DIR}/InstantCLI-${INS_VERSION}-x86_64.AppImage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "${APPIMAGETOOL}" "${APPDIR}" "${OUTPUT}"

echo "AppImage created at ${OUTPUT}"
