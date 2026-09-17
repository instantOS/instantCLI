#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	if ! require_restic; then
		return 0
	fi
	setup_test_env
	trap cleanup_test_env EXIT

	local repo="${TEST_ROOT}/backups"
	local config="${XDG_CONFIG_HOME}/instant/games/games.toml"
	local output

	# The built-in password remains the zero-friction multi-device default.
	ins game init --repo "${repo}"
	assert_path_exists "${repo}/config"
	assert_output_contains "$(cat "${config}")" 'repo_password = "instantgamepassword"'
	[[ "$(stat -c %a "${config}")" == 600 ]]
	cp "${config}" "${TEST_ROOT}/original.toml"

	# Re-running create is deliberately not an implicit connect/reconfiguration.
	if output="$(ins game init --repo "${repo}" 2>&1)"; then
		echo "Creating over an existing repository unexpectedly succeeded" >&2
		return 1
	fi
	assert_output_contains "${output}" "Backups already exist"
	cmp "${config}" "${TEST_ROOT}/original.toml"

	# A bad password must never trigger repository creation or change settings.
	if output="$(ins game init --repo "${repo}" --existing --password wrong 2>&1)"; then
		echo "Connecting with the wrong password unexpectedly succeeded" >&2
		return 1
	fi
	assert_output_contains "${output}" "Invalid password"
	cmp "${config}" "${TEST_ROOT}/original.toml"

	if output="$(ins game init --repo "${TEST_ROOT}/missing" --existing 2>&1)"; then
		echo "Connecting to a missing repository unexpectedly succeeded" >&2
		return 1
	fi
	assert_output_contains "${output}" "No backup repository was found"
	[[ ! -e "${TEST_ROOT}/missing/config" ]]
	cmp "${config}" "${TEST_ROOT}/original.toml"

	# Simulate another device with no game configuration, but the same storage.
	export XDG_CONFIG_HOME="${TEST_ROOT}/second-device-config"
	ins game init --repo "${repo}" --existing
	ins game restic snapshots >/dev/null
	config="${XDG_CONFIG_HOME}/instant/games/games.toml"
	assert_output_contains "$(cat "${config}")" 'repo_password = "instantgamepassword"'
	[[ "$(stat -c %a "${config}")" == 600 ]]

	# Explicit custom passwords are saved too, with no secret in normal output.
	output="$(ins game init --repo "${TEST_ROOT}/custom" --password private-test-password)"
	if [[ "${output}" == *private-test-password* ]]; then
		echo "Password leaked into setup output" >&2
		return 1
	fi
	assert_output_contains "$(cat "${config}")" 'repo_password = "private-test-password"'
	ins game restic snapshots >/dev/null
	[[ "$(stat -c %a "${config}")" == 600 ]]

	echo "Game repository setup and reconnect checks succeeded"
}

main "$@"
