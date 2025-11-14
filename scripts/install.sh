#!/bin/sh

set -eu

REPO="instantOS/instantCLI"
API_URL="https://api.github.com/repos/$REPO/releases/latest"
BIN_NAME="ins"

INSTALL_DIR=${INSTALL_DIR:-"$HOME/.local/bin"}

log() {
    printf '%s\n' "$1"
}

warn() {
    printf 'warning: %s\n' "$1" >&2
}

fatal() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

usage() {
    cat <<EOF
Usage: install.sh [--install-dir <path>] [--bin-name <name>]

Environment variables:
  INSTALL_DIR  Destination directory (default: \$HOME/.local/bin)

Options:
  --install-dir <path>  Set installation directory
  --bin-name <name>     Override installed binary name (default: ins)
  -h, --help            Show this help message
EOF
    exit 0
}

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --install-dir)
                shift
                [ $# -gt 0 ] || fatal "--install-dir requires a value"
                INSTALL_DIR=$1
                ;;
            --bin-name)
                shift
                [ $# -gt 0 ] || fatal "--bin-name requires a value"
                BIN_NAME=$1
                ;;
            -h|--help)
                usage
                ;;
            *)
                fatal "unknown argument: $1"
                ;;
        esac
        shift
    done
}

require_commands() {
    for cmd in curl tar uname mktemp head; do
        command -v "$cmd" >/dev/null 2>&1 || fatal "required command '$cmd' not found"
    done
}

detect_target() {
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64)
            TARGET="x86_64-unknown-linux-gnu"
            ;;
        aarch64|arm64)
            TARGET="aarch64-unknown-linux-gnu"
            ;;
        *)
            fatal "unsupported architecture: $arch"
            ;;
    esac
}

fetch_release_json() {
    release_json=$(curl -fsSL \
        -H "Accept: application/vnd.github+json" \
        -H "User-Agent: instantcli-installer" \
        "$API_URL") || fatal "failed to fetch release metadata"
}

find_asset_urls() {
    asset_url=$(printf '%s\n' "$release_json" | awk -v target="$TARGET" -F'"' '
        /browser_download_url/ {
            url=$4
            if (index(url, target) && (url ~ /\.tar\.zst$/ || url ~ /\.tgz$/)) {
                print url
                exit
            }
        }
    ')

    [ -n "$asset_url" ] || fatal "no prebuilt archive found for $TARGET"

    sha_url=$(printf '%s\n' "$release_json" | awk -v archive="$asset_url" -F'"' '
        /browser_download_url/ {
            url=$4
            if (url == archive ".sha256") {
                print url
                exit
            }
        }
    ')

    version=$(printf '%s\n' "$release_json" | awk -F'"' '
        /"tag_name"/ {
            v=$4
            sub(/^v/, "", v)
            print v
            exit
        }
    ')
}

verify_checksum() {
    archive_path=$1

    if [ -z "$sha_url" ]; then
        warn "no checksum published for this asset; skipping verification"
        return 0
    fi

    if ! command -v sha256sum >/dev/null 2>&1; then
        warn "sha256sum not available; skipping checksum verification"
        return 0
    fi

    checksum_file="$TMPDIR/$(basename "$archive_path").sha256"
    curl -fsSL -H "User-Agent: instantcli-installer" "$sha_url" -o "$checksum_file" || {
        warn "failed to download checksum file; skipping verification"
        return 0
    }

    (cd "$TMPDIR" && sha256sum -c "$(basename "$checksum_file")") || fatal "checksum verification failed"
}

extract_archive() {
    archive_path=$1
    dest_dir=$2

    case "$archive_path" in
        *.tar.zst)
            if tar --help 2>/dev/null | grep -q "--zstd"; then
                tar --zstd -xf "$archive_path" -C "$dest_dir"
            elif command -v unzstd >/dev/null 2>&1; then
                unzstd -c "$archive_path" | tar -xf - -C "$dest_dir"
            elif command -v zstd >/dev/null 2>&1; then
                zstd -d --stdout "$archive_path" | tar -xf - -C "$dest_dir"
            else
                fatal "extracting .tar.zst requires tar with zstd support or the zstd utility"
            fi
            ;;
        *.tgz|*.tar.gz)
            tar -xzf "$archive_path" -C "$dest_dir"
            ;;
        *)
            fatal "unsupported archive format: $archive_path"
            ;;
    esac
}

find_binary_path() {
    search_root=$1

    binary_path=$(find "$search_root" -type f -name "$BIN_NAME" 2>/dev/null | head -n 1)

    [ -n "$binary_path" ] || fatal "failed to locate $BIN_NAME in extracted archive"

    printf '%s\n' "$binary_path"
}

install_binary() {
    binary_path=$1

    mkdir -p "$INSTALL_DIR" || fatal "unable to create $INSTALL_DIR"

    if command -v install >/dev/null 2>&1; then
        install -m 755 "$binary_path" "$INSTALL_DIR/$BIN_NAME"
    else
        warn "install(1) not found; falling back to cp"
        cp "$binary_path" "$INSTALL_DIR/$BIN_NAME"
        chmod 755 "$INSTALL_DIR/$BIN_NAME"
    fi
}

print_summary() {
    if [ -n "$version" ]; then
        log "Installed $BIN_NAME v$version to $INSTALL_DIR"
    else
        log "Installed $BIN_NAME to $INSTALL_DIR"
    fi

    case ":$PATH:" in
        *:"$INSTALL_DIR":*)
            ;;
        *)
            warn "$INSTALL_DIR is not in PATH; add 'export PATH=\\$PATH:$INSTALL_DIR' to your shell profile"
            ;;
    esac
}

main() {
    parse_args "$@"
    require_commands
    detect_target
    fetch_release_json
    find_asset_urls

    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT INT TERM HUP

    archive="$TMPDIR/$(basename "$asset_url")"
    curl -fsSL -H "User-Agent: instantcli-installer" "$asset_url" -o "$archive" || fatal "failed to download release archive"

    verify_checksum "$archive"

    extract_dir="$TMPDIR/extracted"
    mkdir "$extract_dir"
    extract_archive "$archive" "$extract_dir"

    binary_path=$(find_binary_path "$extract_dir")
    install_binary "$binary_path"
    print_summary
}

main "$@"
