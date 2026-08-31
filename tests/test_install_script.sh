#!/usr/bin/env bash
# shellcheck disable=SC1090,SC2034,SC2154,SC2329
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
INSTALL_SCRIPT="${REPO_ROOT}/scripts/install.sh"

"${REPO_ROOT}/scripts/build-install.sh" --check

# Load the installer functions without executing main.
source <(sed '$d' "${INSTALL_SCRIPT}")

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

test_release_selection
test_renamed_binary
test_argument_conflicts
test_non_tty_animation_is_plain
test_launch_mode_selection
test_keyring_failure_is_fatal
