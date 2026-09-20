# -------------------------------------------------------------
# Environment & Command checks
# -------------------------------------------------------------
# Return success when runtime markers identify an ArchISO or instantOS live system.
is_live_disk() {
	[ -e /run/archiso/cowspace ] ||
		[ -e /etc/instantos/liveversion ] ||
		[ -e /usr/share/liveutils ] ||
		grep -q "archiso" /proc/cmdline 2>/dev/null
}

# Decide whether this invocation should continue from CLI setup into OS install.
# An explicit OS request wins; --cli-only suppresses automatic live-media launch.
should_launch_os_installer() {
	[ "$OS_INSTALL" -eq 1 ] || { [ "$CLI_ONLY" -eq 0 ] && is_live_disk; }
}

# Return success only for numbered Linux virtual-console device paths.
is_virtual_console_device() {
	case "$1" in
	/dev/tty[0-9]*)
		console_number=${1#/dev/tty}
		case "$console_number" in
		*[!0-9]*) return 1 ;;
		*) return 0 ;;
		esac
		;;
	esac
	return 1
}

# Print the Linux virtual-console device backing stdout, including through tmux.
linux_console_device() {
	[ -t 1 ] || return 1
	if [ -n "${TMUX:-}" ] && command -v tmux >/dev/null 2>&1; then
		outer_term=$(tmux display-message -p '#{client_termname}' 2>/dev/null || true)
		case "$outer_term" in
		linux*)
			client_tty=$(tmux display-message -p '#{client_tty}' 2>/dev/null || true)
			if is_virtual_console_device "$client_tty"; then
				printf '%s\n' "$client_tty"
				return 0
			fi
			;;
		esac
	fi
	if command -v tty >/dev/null 2>&1; then
		console_tty=$(tty 2>/dev/null || true)
		if is_virtual_console_device "$console_tty"; then
			printf '%s\n' "$console_tty"
			return 0
		fi
	fi
	return 1
}

# Detect if running on a Linux virtual console (TTY) or inside tmux attached to one.
is_linux_console() {
	linux_console_device >/dev/null
}

# Reprogram the Linux virtual console DAC palette to Catppuccin Mocha.
# The Linux kernel VT supports OSC escape sequences \e]P<index><rrggbb>.
set_catppuccin_tty() {
	console_tty=$(linux_console_device) || return 0

	export INS_COLOR_MODE=16

	esc=$(printf '\033')
	palette="${esc}]P01e1e2e${esc}]P1f38ba8${esc}]P2a6e3a1${esc}]P3f9e2af${esc}]P489b4fa${esc}]P5cba6f7${esc}]P694e2d5${esc}]P7bac2de${esc}]P8585b70${esc}]P9f38ba8${esc}]PAa6e3a1${esc}]PBf9e2af${esc}]PC89b4fa${esc}]PDf5c2e7${esc}]PE89dceb${esc}]PFcdd6f4"

	# Write to the originating VT. /dev/tty0 aliases whichever VT is active and
	# may therefore modify a different console when this one is in the background.
	printf '%s' "$palette" >"$console_tty" 2>/dev/null || true
	printf '%s[0m' "$esc"
}

# Initialize and refresh the Arch keyring, escalating only these commands when needed.
# Any failure is fatal because the following OS installation depends on pacman.
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

Environment:
  INSTALL_DIR                         Override the destination directory

Options:
  --install-dir <path>                Set installation directory
  --bin-name <name>                   Override installed binary name (default: ins)
  --cli-only, --no-launch             Install ins CLI only (do not launch OS installer on live disk)
  --os-install, --arch-install        Launch instantOS installer after installing ins
  --config, --unattended <path|url>   Run unattended OS installation with questions TOML file or URL
  --dry-run                           Run unattended OS installation in dry-run mode
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
		--config | --unattended | --questions-file)
			shift
			[ $# -gt 0 ] || fatal "--config requires a file path or URL"
			[ -n "$1" ] || fatal "--config requires a non-empty value"
			UNATTENDED_CONFIG=$1
			OS_INSTALL=1
			;;
		--dry-run)
			DRY_RUN=1
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
	if [ "$CLI_ONLY" -eq 1 ] && [ -n "$UNATTENDED_CONFIG" ]; then
		fatal "--cli-only and --config cannot be used together"
	fi
	if [ "$ONLY_ANIMATION" -eq 1 ] && [ "$NO_ANIMATION" -eq 1 ]; then
		fatal "--only-animation and --no-animation cannot be used together"
	fi
	if [ "$DRY_RUN" -eq 1 ] && [ -z "$UNATTENDED_CONFIG" ]; then
		fatal "--dry-run requires --config"
	fi
}

# Select a destination without mutating PATH. OS installs and root use the system
# directory; regular users prefer an existing user bin from PATH.
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

# Return success for SteamOS/Steam Deck, where the AppImage artifact is preferred.
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

# Map the runtime architecture to release-asset naming and choose archive/AppImage.
# Termux is handled separately to avoid installing glibc binaries into Android.
detect_target() {
	arch=$(uname -m)

	if [ -n "${TERMUX_VERSION:-}" ]; then
		case "$arch" in
		aarch64 | arm64)
			TARGET="aarch64-termux"
			USE_APPIMAGE=0
			return
			;;
		*)
			fatal "unsupported Termux architecture: $arch"
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
	armv7l | armv8l)
		TARGET="armv7-unknown-linux-gnueabihf"
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
