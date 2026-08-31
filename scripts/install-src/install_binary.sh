# Extract supported release archives, selecting an available zstd implementation.
extract_archive() {
	archive_path=$1
	dest_dir=$2

	case "$archive_path" in
	*.tar.zst)
		if tar --help 2>/dev/null | grep -q -- "--zstd"; then
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

# Print the packaged source binary path; BIN_NAME may differ only at installation.
find_binary_path() {
	search_root=$1

	binary_path=$(find "$search_root" -type f -name "$SOURCE_BIN_NAME" 2>/dev/null | head -n 1)

	[ -n "$binary_path" ] || fatal "failed to locate $SOURCE_BIN_NAME in extracted archive"

	printf '%s\n' "$binary_path"
}

# Copy binary_path with executable permissions, requesting sudo only when the
# chosen destination cannot be written by the current user.
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
