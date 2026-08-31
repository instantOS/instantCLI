is_arch_linux() {
	if [ -f /etc/os-release ]; then
		grep -qiE '^ID(_LIKE)?=.*(arch|instantos|manjaro|endeavouros)' /etc/os-release
	elif [ -f /etc/arch-release ]; then
		return 0
	else
		return 1
	fi
}

print_summary() {
	if [ -t 1 ] && [ "${TERM:-}" != "dumb" ]; then
		bold="$(printf '\033[1m')"
		dim="$(printf '\033[2m')"
		reset="$(printf '\033[0m')"
		cyan="$(printf '\033[38;5;45m')"
		orange="$(printf '\033[38;5;208m')"
	else
		bold=""
		dim=""
		reset=""
		cyan=""
		orange=""
	fi

	case ":$PATH:" in
	*:"$INSTALL_DIR":*) ;;
	*)
		warn "$INSTALL_DIR is not in PATH; add 'export PATH=\$PATH:$INSTALL_DIR' to your shell profile"
		;;
	esac

	printf '\n'
	if [ -n "$version" ]; then
		log "${bold}✓ ${BIN_NAME} v${version} installed successfully to ${INSTALL_DIR}/${BIN_NAME}${reset}"
	else
		log "${bold}✓ ${BIN_NAME} installed successfully to ${INSTALL_DIR}/${BIN_NAME}${reset}"
	fi

	printf '\n%sNext steps:%s\n' "$bold" "$reset"
	if is_arch_linux; then
		printf '  • Convert this Arch installation to %sinstantOS%s (adds [instant] repo, instantWM & tools):\n' "$orange" "$reset"
		printf '      %ssudo %s arch setup%s\n' "$cyan" "$BIN_NAME" "$reset"
	else
		printf '  • Build or install instantOS tools from source:\n'
		printf '      %s%s dev install%s\n' "$cyan" "$BIN_NAME" "$reset"
	fi

	printf '  • Manage dotfiles, system settings, and health checks:\n'
	printf '      %s%s --help%s\n' "$cyan" "$BIN_NAME" "$reset"

	printf '\n%sTip:%s To install a fresh %sinstantOS%s system on a computer, boot an Arch Linux live ISO and run:\n' "$dim" "$reset" "$orange" "$reset"
	printf '     %s%scurl -fsSL instantos.io/install | sh%s\n\n' "$dim" "$cyan" "$reset"
}
