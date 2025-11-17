#!/bin/sh

set -eu

REPO="instantOS/instantCLI"
API_URL="https://api.github.com/repos/$REPO/releases"
BIN_NAME="ins"

INSTALL_DIR=${INSTALL_DIR:-}

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
  INSTALL_DIR  Destination directory (default: first user bin in PATH, else /usr/local/bin)

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
		-h | --help)
			usage
			;;
		*)
			fatal "unknown argument: $1"
			;;
		esac
		shift
	done
}

choose_install_dir() {
	if [ -n "$INSTALL_DIR" ]; then
		return
	fi

	for candidate in "$HOME/.local/bin" "$HOME/bin"; do
		case ":$PATH:" in
		*:"$candidate":*)
			INSTALL_DIR="$candidate"
			return
			;;
		esac
	done

	INSTALL_DIR="/usr/local/bin"
}

require_commands() {
	for cmd in curl tar uname mktemp head find; do
		command -v "$cmd" >/dev/null 2>&1 || fatal "required command '$cmd' not found"
	done
}

detect_steam_deck() {
	if [ -f /etc/os-release ]; then
		if grep -q "steamdeck" /etc/os-release 2>/dev/null || grep -q "SteamOS" /etc/os-release 2>/dev/null; then
			return 0
		fi
	fi
	if [ -n "${STEAM_DECK:-}" ]; then
		return 0
	fi
	return 1
}

detect_target() {
	arch=$(uname -m)
	case "$arch" in
	x86_64 | amd64)
		TARGET="x86_64-unknown-linux-gnu"
		;;
	aarch64 | arm64)
		TARGET="aarch64-unknown-linux-gnu"
		;;
	*)
		fatal "unsupported architecture: $arch"
		;;
	esac

	if detect_steam_deck; then
		USE_APPIMAGE=1
		log "Steam Deck detected, using AppImage"
	else
		USE_APPIMAGE=0
	fi
}

fetch_releases_json() {
	releases_json=$(curl -fsSL \
		-H "Accept: application/vnd.github+json" \
		-H "User-Agent: instantcli-installer" \
		"$API_URL") || fatal "failed to fetch releases metadata"
}

# Parse releases and find one with assets for our target
find_working_release() {
	# Extract releases array and process each release
	printf '%s\n' "$releases_json" | awk -v target="$TARGET" -v use_appimage="$USE_APPIMAGE" '
	function find_asset(release_data, target_pattern) {
		rest = release_data
		while (match(rest, /"browser_download_url":"([^"]+)"/)) {
			url_start = index(rest, "\"browser_download_url\":\"") + 24
			url_end = index(substr(rest, url_start), "\"")
			url = substr(rest, url_start, url_end - 1)
			
			if (use_appimage == 1) {
				if (url ~ /\.AppImage$/ && url !~ /\.sha256$/) {
					return url
				}
			} else {
				if (index(url, target_pattern) && (url ~ /\.tar\.zst$/ || url ~ /\.tgz$/)) {
					return url
				}
			}
			rest = substr(rest, RSTART + RLENGTH)
		}
		return ""
	}
	
	function is_valid_release(release_data) {
		# Skip drafts and prereleases
		if (match(release_data, /"draft":true/) || match(release_data, /"prerelease":true/)) {
			return 0
		}
		return 1
	}
	
	function extract_tag(release_data) {
		if (match(release_data, /"tag_name":"([^"]+)"/)) {
			tag_start = index(release_data, "\"tag_name\":\"") + 12
			tag_end = index(substr(release_data, tag_start), "\"")
			tag = substr(release_data, tag_start, tag_end - 1)
			return tag
		}
		return ""
	}
	
	# Process each release in the array
	rest = $0
	in_release = 0
	release_data = ""
	
	while ((pos = index(rest, "{")) > 0) {
		if (!in_release) {
			# Start of a new release object
			in_release = 1
			bracket_count = 1
			release_data = "{"
			rest = substr(rest, pos + 1)
		} else {
			# We are inside a release object
			i = 1
			while (i <= length(rest)) {
				char = substr(rest, i, 1)
				if (char == "{") {
					bracket_count++
				} else if (char == "}") {
					bracket_count--
					if (bracket_count == 0) {
						# End of release object
						release_data = release_data substr(rest, 1, i)
						rest = substr(rest, i + 1)
						
						# Check if this is a valid release with assets for our target
						if (is_valid_release(release_data)) {
							asset_url = find_asset(release_data, target)
							if (asset_url != "") {
								# Found a working release, print it and exit
								print release_data
								exit 0
							}
						}
						
						in_release = 0
						release_data = ""
						break
					}
				}
				release_data = release_data char
				i++
			}
			if (in_release) {
				release_data = release_data rest
				rest = ""
			}
		}
	}
	
	# If we get here, no working release was found
	exit 1
	'
}

find_asset_urls() {
	if [ "$USE_APPIMAGE" -eq 1 ]; then
		asset_url=$(printf '%s\n' "$release_json" | awk '
            {
                rest = $0
                while (match(rest, /"browser_download_url":"([^"]+)"/)) {
                    url_start = index(rest, "\"browser_download_url\":\"") + 24
                    url_end = index(substr(rest, url_start), "\"")
                    url = substr(rest, url_start, url_end - 1)
                    if (url ~ /\.AppImage$/ && url !~ /\.sha256$/) {
                        print url
                        exit
                    }
                    rest = substr(rest, RSTART + RLENGTH)
                }
            }
        ')
		[ -n "$asset_url" ] || fatal "no AppImage found in release"
	else
		asset_url=$(printf '%s\n' "$release_json" | awk -v target="$TARGET" '
            {
                rest = $0
                while (match(rest, /"browser_download_url":"([^"]+)"/)) {
                    url_start = index(rest, "\"browser_download_url\":\"") + 24
                    url_end = index(substr(rest, url_start), "\"")
                    url = substr(rest, url_start, url_end - 1)
                    if (index(url, target) && (url ~ /\.tar\.zst$/ || url ~ /\.tgz$/)) {
                        print url
                        exit
                    }
                    rest = substr(rest, RSTART + RLENGTH)
                }
            }
        ')
		[ -n "$asset_url" ] || fatal "no prebuilt archive found for $TARGET"
	fi

	sha_url=$(printf '%s\n' "$release_json" | awk -v archive="$asset_url" '
        {
            rest = $0
            target_sha = archive ".sha256"
            while (match(rest, /"browser_download_url":"([^"]+)"/)) {
                url_start = index(rest, "\"browser_download_url\":\"") + 24
                url_end = index(substr(rest, url_start), "\"")
                url = substr(rest, url_start, url_end - 1)
                if (url == target_sha) {
                    print url
                    exit
                }
                rest = substr(rest, RSTART + RLENGTH)
            }
        }
    ')

	version=$(printf '%s\n' "$release_json" | awk '
        match($0, /"tag_name":"v?([^"]+)"/) {
            tag_start = index($0, "\"tag_name\":\"") + 12
            tag_end = index(substr($0, tag_start), "\"")
            tag = substr($0, tag_start, tag_end - 1)
            sub(/^v/, "", tag)
            print tag
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

	checksum_basename=$(basename "$archive_path")
	if ! grep -q "  $checksum_basename$" "$checksum_file" 2>/dev/null; then
		tmp_checksum_file="$checksum_file.tmp"
		if awk -v name="$checksum_basename" '{print $1 "  " name}' "$checksum_file" >"$tmp_checksum_file" 2>/dev/null; then
			mv "$tmp_checksum_file" "$checksum_file"
		else
			warn "failed to normalize checksum file; skipping verification"
			return 0
		fi
	fi

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
	*.tgz | *.tar.gz)
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
	needs_sudo=0

	if [ ! -d "$INSTALL_DIR" ]; then
		if ! mkdir -p "$INSTALL_DIR" 2>/dev/null; then
			needs_sudo=1
		fi
	fi

	if [ ! -w "$INSTALL_DIR" ]; then
		needs_sudo=1
	fi

	if [ "$needs_sudo" -eq 1 ]; then
		if ! command -v sudo >/dev/null 2>&1; then
			fatal "cannot write to $INSTALL_DIR and sudo not available; set INSTALL_DIR to a writable directory"
		fi

		log "Requesting elevated permissions to install to $INSTALL_DIR..."

		if [ ! -d "$INSTALL_DIR" ]; then
			sudo mkdir -p "$INSTALL_DIR" || fatal "failed to create $INSTALL_DIR with sudo"
		fi

		if command -v install >/dev/null 2>&1; then
			sudo install -m 755 "$binary_path" "$INSTALL_DIR/$BIN_NAME"
		else
			warn "install(1) not found; falling back to cp"
			sudo cp "$binary_path" "$INSTALL_DIR/$BIN_NAME"
			sudo chmod 755 "$INSTALL_DIR/$BIN_NAME"
		fi
	else
		if command -v install >/dev/null 2>&1; then
			install -m 755 "$binary_path" "$INSTALL_DIR/$BIN_NAME"
		else
			warn "install(1) not found; falling back to cp"
			cp "$binary_path" "$INSTALL_DIR/$BIN_NAME"
			chmod 755 "$INSTALL_DIR/$BIN_NAME"
		fi
	fi
}

print_summary() {
	if [ -n "$version" ]; then
		log "Installed $BIN_NAME v$version to $INSTALL_DIR"
	else
		log "Installed $BIN_NAME to $INSTALL_DIR"
	fi

	case ":$PATH:" in
	*:"$INSTALL_DIR":*) ;;
	*)
		warn "$INSTALL_DIR is not in PATH; add 'export PATH=\$PATH:$INSTALL_DIR' to your shell profile"
		;;
	esac
}

main() {
	parse_args "$@"
	choose_install_dir
	require_commands
	detect_target
	fetch_releases_json

	# Try to find a working release (fallback to previous releases if latest is missing assets)
	release_json=$(find_working_release) || fatal "no working release found with assets for $TARGET"

	find_asset_urls

	TMPDIR=$(mktemp -d)
	trap 'rm -rf "$TMPDIR"' EXIT INT TERM HUP

	archive="$TMPDIR/$(basename "$asset_url")"
	curl -fsSL -H "User-Agent: instantcli-installer" "$asset_url" -o "$archive" || fatal "failed to download release archive"

	verify_checksum "$archive"

	if [ "$USE_APPIMAGE" -eq 1 ]; then
		chmod +x "$archive"
		binary_path="$archive"
	else
		extract_dir="$TMPDIR/extracted"
		mkdir "$extract_dir"
		extract_archive "$archive" "$extract_dir"
		binary_path=$(find_binary_path "$extract_dir")
	fi
	install_binary "$binary_path"
	print_summary
}

main "$@"
