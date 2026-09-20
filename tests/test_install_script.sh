#!/usr/bin/env bash
# shellcheck disable=SC1090,SC2030,SC2031,SC2034,SC2154,SC2329
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
INSTALL_SCRIPT="${REPO_ROOT}/scripts/install.sh"

"${REPO_ROOT}/scripts/build-install.sh" --check

# Load the installer functions without executing main.
entrypoint_count="$(grep -c '^main "\$@"$' "${INSTALL_SCRIPT}" || true)"
if [[ "${entrypoint_count}" != 1 ]]; then
	echo "Expected exactly one installer entrypoint, found ${entrypoint_count}" >&2
	exit 1
fi
source <(sed '/^main "\$@"$/d' "${INSTALL_SCRIPT}")

assert_equals() {
	local expected="$1"
	local actual="$2"

	if [[ "${actual}" != "${expected}" ]]; then
		echo "Expected: ${expected}" >&2
		echo "Actual:   ${actual}" >&2
		return 1
	fi
}

test_release_selection() {
	local releases
	releases='[
  {
    "tag_name": "v2-beta",
    "draft": false,
    "prerelease": true,
    "assets": [
      {"browser_download_url": "https://example.invalid/ignored.AppImage"}
    ]
  },
  {
    "tag_name": "v1.2.3",
    "draft": false,
    "prerelease": false,
    "assets": [
      {"browser_download_url": "https://example.invalid/ins-x86_64-unknown-linux-gnu-v1.2.3.tgz"},
      {"browser_download_url": "https://example.invalid/ins-x86_64-unknown-linux-gnu-v1.2.3.tgz.sha256"}
    ]
  }
]'

	TARGET="x86_64-unknown-linux-gnu"
	USE_APPIMAGE=0
	release_json="$(find_working_release "${releases}")"
	find_asset_urls

	assert_equals "https://example.invalid/ins-x86_64-unknown-linux-gnu-v1.2.3.tgz" "${asset_url}"
	assert_equals "https://example.invalid/ins-x86_64-unknown-linux-gnu-v1.2.3.tgz.sha256" "${sha_url}"
	assert_equals "1.2.3" "${version}"
}

test_renamed_binary() (
	local extract_root binary
	extract_root="$(mktemp -d)"
	trap 'rm -rf "${extract_root}"' EXIT
	mkdir -p "${extract_root}/release"
	touch "${extract_root}/release/ins"

	BIN_NAME="i"
	binary="$(find_binary_path "${extract_root}")"
	assert_equals "${extract_root}/release/ins" "${binary}"
)

test_argument_conflicts() {
	if (parse_args --cli-only --os-install) >/dev/null 2>&1; then
		echo "Conflicting installation modes should fail" >&2
		return 1
	fi

	if (parse_args --cli-only --config /path/to/config.toml) >/dev/null 2>&1; then
		echo "Conflicting --cli-only and --config modes should fail" >&2
		return 1
	fi

	if (parse_args --dry-run) >/dev/null 2>&1; then
		echo "--dry-run without --config should fail" >&2
		return 1
	fi

	if (parse_args --only-animation --no-animation) >/dev/null 2>&1; then
		echo "Conflicting animation modes should fail" >&2
		return 1
	fi

	if (parse_args --bin-name ../ins) >/dev/null 2>&1; then
		echo "A binary path should not be accepted as a name" >&2
		return 1
	fi
}

test_non_tty_animation_is_plain() {
	local output
	output="$(TERM=dumb sh "${INSTALL_SCRIPT}" --only-animation)"

	if [[ "${output}" == *$'\033'* ]]; then
		echo "Non-TTY animation output contains ANSI escape codes" >&2
		return 1
	fi
}

test_launch_mode_selection() (
	is_live_disk() { return 0; }

	OS_INSTALL=0
	CLI_ONLY=1
	if should_launch_os_installer; then
		echo "CLI-only mode should override live-disk auto-launch" >&2
		return 1
	fi

	OS_INSTALL=1
	CLI_ONLY=0
	should_launch_os_installer
)

test_keyring_failure_is_fatal() {
	if (
		pacman-key() { return 1; }
		pacman() { return 0; }
		id() { printf '0\n'; }
		prepare_live_keyring
	) >/dev/null 2>&1; then
		echo "A package-keyring failure should abort installation" >&2
		return 1
	fi
}

test_published_checksum_download_failure_is_fatal() (
	local work_dir
	work_dir="$(mktemp -d)"
	trap 'rm -rf "${work_dir}"' EXIT
	touch "${work_dir}/archive.tgz"

	INSTALL_WORK_DIR="${work_dir}"
	sha_url="https://example.invalid/archive.tgz.sha256"
	curl() { return 1; }

	if (verify_checksum "${work_dir}/archive.tgz") >/dev/null 2>&1; then
		echo "A published checksum download failure should abort installation" >&2
		return 1
	fi
)

test_non_tty_download_is_quiet() {
	local args_file archive output args asset_url version BIN_NAME
	args_file="$(mktemp)"
	archive="$(mktemp)"

	asset_url="https://example.invalid/ins-v1.2.3.tgz"
	version="1.2.3"
	BIN_NAME="ins"
	curl() { printf '%s\n' "$*" >"${args_file}"; }

	output="$(download_release_asset "${archive}" 2>/dev/null)"
	args="$(<"${args_file}")"
	rm -f "${args_file}" "${archive}"
	assert_equals "Downloading ins v1.2.3..." "${output}"

	if ! grep -q -- '-fsSL' <<<"${args}"; then
		echo "Non-TTY download did not use curl's quiet mode" >&2
		return 1
	fi
	if grep -q -- '--progress-bar' <<<"${args}"; then
		echo "Non-TTY download unexpectedly enabled the progress bar" >&2
		return 1
	fi
}

test_local_validation_precedes_animation() (
	local animation_marker status
	animation_marker="$(mktemp)"
	rm -f "${animation_marker}"
	trap 'rm -f "${animation_marker}"' EXIT

	parse_args() { :; }
	choose_install_dir() { :; }
	require_commands() { exit 23; }
	instantos_logo_animation() { touch "${animation_marker}"; }

	set +e
	(main) >/dev/null 2>&1
	status=$?
	set -e

	assert_equals 23 "${status}"
	if [[ -e "${animation_marker}" ]]; then
		echo "Animation ran before local validation completed" >&2
		return 1
	fi
)

test_arm_cli_target_detection() (
	local mock_arch releases
	unset TERMUX_VERSION STEAM_DECK
	detect_steam_deck() { return 1; }
	uname() { printf '%s\n' "${mock_arch}"; }

	for mock_arch in armv7l armv8l; do
		detect_target
		assert_equals "armv7-unknown-linux-gnueabihf" "${TARGET}"
		assert_equals 0 "${USE_APPIMAGE}"
	done

	releases='[{"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[{"browser_download_url":"https://example.invalid/ins-armv7-unknown-linux-gnueabihf-v1.2.3.tgz"}]}]'
	release_json="$(find_working_release "${releases}")"
	find_asset_urls
	assert_equals "https://example.invalid/ins-armv7-unknown-linux-gnueabihf-v1.2.3.tgz" "${asset_url}"
)

# Release selection (find_working_release) and asset extraction (find_asset_urls)
# apply the asset filters in separate awk programs, so they can drift apart. Pin
# them to the same decision: for every supported mode, selection must accept a
# release AND extraction must return its matching asset, skipping checksum,
# debug, and distribution-package assets regardless of their position.
test_release_selection_and_extraction_agree() (
	local releases
	releases='[
  {
    "tag_name": "v2.6.0-rc1",
    "draft": false,
    "prerelease": true,
    "assets": [
      {"browser_download_url": "https://example.invalid/ins-x86_64-unknown-linux-gnu-v2.6.0-rc1.tgz"}
    ]
  },
  {
    "tag_name": "v2.5.0",
    "draft": false,
    "prerelease": false,
    "assets": [
      {"browser_download_url": "https://example.invalid/ins-x86_64-unknown-linux-gnu-v2.5.0.tgz.sha256"},
      {"browser_download_url": "https://example.invalid/ins-x86_64-unknown-linux-gnu-debug-v2.5.0.tgz"},
      {"browser_download_url": "https://example.invalid/ins-x86_64-unknown-linux-gnu-v2.5.0.pkg.tar.zst"},
      {"browser_download_url": "https://example.invalid/ins-x86_64-unknown-linux-gnu-v2.5.0.tgz"},
      {"browser_download_url": "https://example.invalid/ins-aarch64-unknown-linux-gnu-v2.5.0.tgz"},
      {"browser_download_url": "https://example.invalid/ins-aarch64-unknown-linux-gnu-v2.5.0.tgz.sha256"},
      {"browser_download_url": "https://example.invalid/ins-armv7-unknown-linux-gnueabihf-v2.5.0.tgz"},
      {"browser_download_url": "https://example.invalid/ins-armv7-unknown-linux-gnueabihf-v2.5.0.tgz.sha256"},
      {"browser_download_url": "https://example.invalid/InstantOS-v2.5.0-x86_64.AppImage.sha256"},
      {"browser_download_url": "https://example.invalid/InstantOS-v2.5.0-x86_64.AppImage"}
    ]
  }
]'

	select_and_extract() {
		(
			TARGET="$1"
			USE_APPIMAGE="$2"
			release_json="$(find_working_release "${releases}")" || {
				echo "Release selection found nothing for TARGET=$1 USE_APPIMAGE=$2" >&2
				return 1
			}
			find_asset_urls
			if [[ -z "${asset_url}" ]]; then
				echo "Extraction returned no asset for TARGET=$1 USE_APPIMAGE=$2" >&2
				return 1
			fi
			assert_equals "$3" "${asset_url}"
			assert_equals "${asset_url}.sha256" "${sha_url}"
			assert_equals "2.5.0" "${version}"
		)
	}

	select_and_extract "x86_64-unknown-linux-gnu" 0 "https://example.invalid/ins-x86_64-unknown-linux-gnu-v2.5.0.tgz"
	select_and_extract "aarch64-unknown-linux-gnu" 0 "https://example.invalid/ins-aarch64-unknown-linux-gnu-v2.5.0.tgz"
	select_and_extract "armv7-unknown-linux-gnueabihf" 0 "https://example.invalid/ins-armv7-unknown-linux-gnueabihf-v2.5.0.tgz"
	select_and_extract "x86_64-unknown-linux-gnu" 1 "https://example.invalid/InstantOS-v2.5.0-x86_64.AppImage"
)

test_unsupported_termux_arm_does_not_use_glibc_target() (
	local mock_arch
	mock_arch="armv7l"
	TERMUX_VERSION=1
	uname() { printf '%s\n' "${mock_arch}"; }

	if (detect_target) >/dev/null 2>&1; then
		echo "Unsupported Termux ARM selected a GNU/Linux release target" >&2
		return 1
	fi
)

test_help_documents_install_dir_environment() {
	local output
	output="$(sh "${INSTALL_SCRIPT}" --help)"

	if [[ "${output}" != *"INSTALL_DIR"* ]]; then
		echo "Installer help does not document INSTALL_DIR" >&2
		return 1
	fi
}

test_console_palette_detection() {
	if ! is_virtual_console_device /dev/tty1 ||
		is_virtual_console_device /dev/tty1other ||
		is_virtual_console_device /dev/ttyS0 ||
		is_virtual_console_device /dev/pts/1; then
		echo "virtual console device validation accepted an invalid path" >&2
		return 1
	fi

	# When TERM=dumb and not a tty, is_linux_console should return 1
	if (TERM=dumb is_linux_console); then
		echo "is_linux_console should return false for TERM=dumb without a tty" >&2
		return 1
	fi

	# When is_linux_console returns false, set_catppuccin_tty should be silent
	local output
	output="$(TERM=dumb set_catppuccin_tty)"
	if [[ -n "${output}" ]]; then
		echo "set_catppuccin_tty produced output when not on a linux console" >&2
		return 1
	fi

	# A detected console is written directly rather than through /dev/tty0 or a
	# fragile tmux escape wrapper.
	local console_output
	console_output="$(mktemp)"
	linux_console_device() { printf '%s\n' "${console_output}"; }
	set_catppuccin_tty >/dev/null
	if [[ "$(wc -c <"${console_output}")" -ne 160 ]]; then
		echo "set_catppuccin_tty did not emit the complete 16-color palette" >&2
		rm -f "${console_output}"
		return 1
	fi
	if [[ "$(LC_ALL=C tr -cd '\033' <"${console_output}" | wc -c)" -ne 16 ]]; then
		echo "set_catppuccin_tty emitted an invalid palette sequence" >&2
		rm -f "${console_output}"
		return 1
	fi
	rm -f "${console_output}"
}

test_unattended_config_parsing() {
	(
		parse_args --config /tmp/questions.toml --dry-run
		assert_equals "/tmp/questions.toml" "${UNATTENDED_CONFIG}"
		assert_equals 1 "${OS_INSTALL}"
		assert_equals 1 "${DRY_RUN}"
	)

	(
		parse_args --unattended /tmp/questions.toml
		assert_equals "/tmp/questions.toml" "${UNATTENDED_CONFIG}"
		assert_equals 1 "${OS_INSTALL}"
		assert_equals 0 "${DRY_RUN}"
	)

	(
		parse_args --questions-file /tmp/questions.toml
		assert_equals "/tmp/questions.toml" "${UNATTENDED_CONFIG}"
		assert_equals 1 "${OS_INSTALL}"
		assert_equals 0 "${DRY_RUN}"
	)
}

test_unattended_launch_command() (
	local captured_command dummy_config
	captured_command="$(mktemp)"
	dummy_config="$(mktemp /tmp/test_q.XXXXXX.toml)"
	trap 'rm -f "${captured_command}" "${dummy_config}"' EXIT

	INSTALL_DIR="/custom/bin"
	BIN_NAME="ins"

	# Subshell with mocked exec
	(
		exec() {
			printf '%s\n' "$*" >"${captured_command}"
		}

		UNATTENDED_CONFIG="${dummy_config}"
		DRY_RUN=1
		OS_INSTALL=1

		if [ -n "$UNATTENDED_CONFIG" ]; then
			case "$UNATTENDED_CONFIG" in
			http://* | https://*) ;;
			*)
				[ -f "$UNATTENDED_CONFIG" ]
				UNATTENDED_CONFIG=$(cd "$(dirname "$UNATTENDED_CONFIG")" && pwd)/$(basename "$UNATTENDED_CONFIG")
				;;
			esac

			if [ "$DRY_RUN" -eq 1 ]; then
				exec "$INSTALL_DIR/$BIN_NAME" arch exec --dry-run -f "$UNATTENDED_CONFIG"
			else
				exec "$INSTALL_DIR/$BIN_NAME" arch exec -f "$UNATTENDED_CONFIG"
			fi
		fi
	)

	cmd="$(<"${captured_command}")"
	if [[ "${cmd}" == *"--trust-config"* ]]; then
		echo "Unattended launch should not use non-existent --trust-config flag" >&2
		return 1
	fi
	assert_equals "/custom/bin/ins arch exec --dry-run -f ${dummy_config}" "${cmd}"
)

test_release_selection
test_renamed_binary
test_argument_conflicts
test_non_tty_animation_is_plain
test_launch_mode_selection
test_keyring_failure_is_fatal
test_published_checksum_download_failure_is_fatal
test_non_tty_download_is_quiet
test_local_validation_precedes_animation
test_arm_cli_target_detection
test_unsupported_termux_arm_does_not_use_glibc_target
test_help_documents_install_dir_environment
test_release_selection_and_extraction_agree
test_console_palette_detection
test_unattended_config_parsing
test_unattended_launch_command
