# -------------------------------------------------------------
# Main workflow
# -------------------------------------------------------------
# Download the release archive with curl's progress meter only for an interactive
# terminal, keeping redirected output and CI logs quiet.
download_release_asset() {
	archive_path=$1

	if [ -n "$version" ]; then
		log "Downloading ${BIN_NAME} v${version}..."
	else
		log "Downloading ${BIN_NAME}..."
	fi

	if [ -t 2 ]; then
		curl --fail --location --progress-bar \
			-H "User-Agent: instantcli-installer" \
			"$asset_url" -o "$archive_path" || fatal "failed to download release archive"
	else
		curl -fsSL -H "User-Agent: instantcli-installer" \
			"$asset_url" -o "$archive_path" || fatal "failed to download release archive"
	fi
}

# Hand off to the freshly installed CLI once the artifacts are in place:
# unattended with a questions config when one was provided, otherwise the
# interactive instantOS installer. A downloaded questions file needs the shell
# to stay alive until the CLI exits so it can remove the temporary copy.
launch_os_installer() {
	if ! should_launch_os_installer; then
		return 0
	fi

	if [ -n "$UNATTENDED_CONFIG" ]; then
		case "$UNATTENDED_CONFIG" in
		http://* | https://*)
			downloaded_config=$(mktemp /tmp/instant_questions.XXXXXX.toml)
			trap 'rm -f "$downloaded_config"' EXIT
			trap 'exit 130' INT
			trap 'exit 143' TERM
			trap 'exit 129' HUP
			log "Downloading installation configuration from $UNATTENDED_CONFIG..."
			if ! curl -fsSL "$UNATTENDED_CONFIG" -o "$downloaded_config"; then
				fatal "failed to download configuration from $UNATTENDED_CONFIG"
			fi
			log "Starting instantOS unattended installer..."
			if [ "$DRY_RUN" -eq 1 ]; then
				"$INSTALL_DIR/$BIN_NAME" arch exec --dry-run -f "$downloaded_config"
			else
				"$INSTALL_DIR/$BIN_NAME" arch exec -f "$downloaded_config"
			fi
			exit $?
			;;
		*)
			[ -f "$UNATTENDED_CONFIG" ] || fatal "configuration file not found: $UNATTENDED_CONFIG"
			UNATTENDED_CONFIG=$(cd "$(dirname "$UNATTENDED_CONFIG")" && pwd)/$(basename "$UNATTENDED_CONFIG")
			;;
		esac

		log "Starting instantOS unattended installer..."
		if [ "$DRY_RUN" -eq 1 ]; then
			exec "$INSTALL_DIR/$BIN_NAME" arch exec --dry-run -f "$UNATTENDED_CONFIG"
		else
			exec "$INSTALL_DIR/$BIN_NAME" arch exec -f "$UNATTENDED_CONFIG"
		fi
	else
		log "Starting instantOS installer..."
		# The CLI configures networking as the desktop user, then escalates itself.
		exec "$INSTALL_DIR/$BIN_NAME" arch install
	fi
}

# Coordinate validation, artifact installation, cleanup, and optional OS handoff.
main() {
	parse_args "$@"

	set_catppuccin_tty

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
	if should_launch_os_installer && [ "$DRY_RUN" -eq 0 ]; then
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
	download_release_asset "$archive"

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

	launch_os_installer

	print_summary
}
