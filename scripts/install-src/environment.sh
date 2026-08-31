# -------------------------------------------------------------
# Environment & Command checks
# -------------------------------------------------------------
is_live_disk() {
	[ -e /run/archiso/cowspace ] ||
		[ -d /run/archiso ] ||
		[ -e /etc/instantos/liveversion ] ||
		[ -e /usr/share/liveutils ] ||
		grep -q "archiso" /proc/cmdline 2>/dev/null
}

should_launch_os_installer() {
	[ "$OS_INSTALL" -eq 1 ] || { [ "$CLI_ONLY" -eq 0 ] && is_live_disk; }
}

prepare_live_keyring() {
	command -v pacman-key >/dev/null 2>&1 || fatal "required command 'pacman-key' not found"
	command -v pacman >/dev/null 2>&1 || fatal "required command 'pacman' not found"

	log "Preparing the Arch Linux package keyring..."
	if [ "$(id -u)" -eq 0 ]; then
		pacman-key --init || fatal "failed to initialize the package keyring"
		pacman-key --populate archlinux || fatal "failed to populate the package keyring"
		pacman -Sy --needed archlinux-keyring --noconfirm || fatal "failed to update the Arch Linux keyring"
	elif command -v sudo >/dev/null 2>&1; then
		sudo pacman-key --init || fatal "failed to initialize the package keyring"
		sudo pacman-key --populate archlinux || fatal "failed to populate the package keyring"
		sudo pacman -Sy --needed archlinux-keyring --noconfirm || fatal "failed to update the Arch Linux keyring"
	else
		fatal "preparing the package keyring requires root permissions"
	fi
}

usage() {
	cat <<EOF
Usage: install.sh [OPTIONS]

Options:
  --install-dir <path>                Set installation directory
  --bin-name <name>                   Override installed binary name (default: ins)
  --cli-only, --no-launch             Install ins CLI only (do not launch OS installer on live disk)
  --os-install, --arch-install        Launch instantOS installer after installing ins
  --only-animation, --animation-only  Play the logo animation and exit
  --no-animation                      Skip the logo animation
  -h, --help                          Show this help message
EOF
	exit 0
}

parse_args() {
	while [ $# -gt 0 ]; do
		case "$1" in
		--install-dir)
			shift
			[ $# -gt 0 ] || fatal "--install-dir requires a value"
			[ -n "$1" ] || fatal "--install-dir requires a non-empty value"
			INSTALL_DIR=$1
			;;
		--bin-name)
			shift
			[ $# -gt 0 ] || fatal "--bin-name requires a value"
			case "$1" in
			"" | . | .. | */*) fatal "--bin-name must be a file name, not a path" ;;
			esac
			BIN_NAME=$1
			;;
		--cli-only | --no-launch)
			CLI_ONLY=1
			;;
		--os-install | --arch-install)
			OS_INSTALL=1
			;;
		--only-animation | --animation-only)
			ONLY_ANIMATION=1
			;;
		--no-animation)
			NO_ANIMATION=1
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

	if [ "$CLI_ONLY" -eq 1 ] && [ "$OS_INSTALL" -eq 1 ]; then
		fatal "--cli-only and --os-install cannot be used together"
	fi
	if [ "$ONLY_ANIMATION" -eq 1 ] && [ "$NO_ANIMATION" -eq 1 ]; then
		fatal "--only-animation and --no-animation cannot be used together"
	fi
}

choose_install_dir() {
	if [ -n "$INSTALL_DIR" ]; then
		return
	fi

	# On a live ISO or when explicitly running OS install, install system-wide to /usr/local/bin
	if should_launch_os_installer; then
		INSTALL_DIR="/usr/local/bin"
		return
	fi

	# If already root, install to /usr/local/bin
	if [ "$(id -u)" -eq 0 ]; then
		INSTALL_DIR="/usr/local/bin"
		return
	fi

	# Standard user installation
	for candidate in "$HOME/.local/bin" "$HOME/bin"; do
		case ":$PATH:" in
		*:"$candidate":*)
			INSTALL_DIR="$candidate"
			return
			;;
		esac
	done

	INSTALL_DIR="$HOME/.local/bin"
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

	if [ -n "${TERMUX_VERSION:-}" ]; then
		case "$arch" in
		aarch64 | arm64)
			TARGET="aarch64-termux"
			USE_APPIMAGE=0
			return
			;;
		esac
	fi

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
