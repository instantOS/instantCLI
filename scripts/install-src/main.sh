# -------------------------------------------------------------
# Main workflow
# -------------------------------------------------------------
# Coordinate validation, artifact installation, cleanup, and optional OS handoff.
main() {
	parse_args "$@"

	if [ "$ONLY_ANIMATION" -eq 1 ]; then
		instantos_logo_animation
		exit 0
	fi

	choose_install_dir
	require_commands
	detect_target

	# A fresh instantOS installation currently supports x86_64 only. Other
	# architectures can still use the CLI-only installation path.
	if should_launch_os_installer && [ "$TARGET" != "x86_64-unknown-linux-gnu" ]; then
		fatal "the instantOS system installer currently supports x86_64 only; use --cli-only to install the CLI"
	fi

	instantos_logo_animation

	# If live disk or forced OS install, prepare the keyring before downloading.
	if should_launch_os_installer; then
		prepare_live_keyring
	fi

	fetch_release_json

	find_asset_urls

	INSTALL_WORK_DIR=$(mktemp -d)
	cleanup_tmpdir() {
		rm -rf "$INSTALL_WORK_DIR"
	}
	trap cleanup_tmpdir EXIT
	trap 'exit 130' INT
	trap 'exit 143' TERM
	trap 'exit 129' HUP

	archive="$INSTALL_WORK_DIR/$(basename "$asset_url")"
	curl -fsSL -H "User-Agent: instantcli-installer" "$asset_url" -o "$archive" || fatal "failed to download release archive"

	verify_checksum "$archive"

	if [ "$USE_APPIMAGE" -eq 1 ]; then
		chmod +x "$archive"
		binary_path="$archive"
	else
		# Check if it's an archive or a bare binary
		case "$archive" in
		*.tar.zst | *.tgz | *.tar.gz)
			extract_dir="$INSTALL_WORK_DIR/extracted"
			mkdir "$extract_dir"
			extract_archive "$archive" "$extract_dir"
			binary_path=$(find_binary_path "$extract_dir")
			;;
		*)
			# Bare binary file
			chmod +x "$archive"
			binary_path="$archive"
			;;
		esac
	fi

	install_binary "$binary_path"
	cleanup_tmpdir
	trap - EXIT INT TERM HUP

	# Launch instantOS installer if on live disk or requested
	if should_launch_os_installer; then
		log "Starting instantOS installer..."
		# The CLI configures networking as the desktop user, then escalates itself.
		exec "$INSTALL_DIR/$BIN_NAME" arch install
	fi

	print_summary
}
