#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	prepare_ins_binary
	setup_test_env
	trap cleanup_test_env EXIT
	export INS_GAME_GUI=0
	local config_dir="${XDG_CONFIG_HOME}/instant/games"
	local tools="${TEST_ROOT}/tools"
	mkdir -p "${config_dir}" "${tools}" "${TEST_ROOT}/saves"
	cat >"${config_dir}/games.toml" <<EOF
repo = 'rclone:drive:existing/backups'
repo_password = 'testpass'
[[games]]
name = 'Setup Game'
launch_command = 'touch ${TEST_ROOT}/launched'
EOF
	cat >"${config_dir}/installations.toml" <<EOF
[[installations]]
game_name = 'Setup Game'
save_path = '${TEST_ROOT}/saves'
EOF

	# Noninteractive launches give guidance instead of attempting installation.
	local output
	if output="$(PATH="${tools}" ins game launch 'Setup Game' </dev/null 2>&1)"; then
		echo "Launch unexpectedly succeeded without restic" >&2
		return 1
	fi
	assert_output_contains "${output}" 'Install or upgrade restic'
	assert_output_contains "${output}" 'Retry launching from a terminal'
	[[ ! -e "${TEST_ROOT}/launched" ]]

	# An interactive launch uses the shared dependency prompt. Declining must
	# cancel cleanly, without running an installer or the game.
	export TEST_FZF_ARGS="${TEST_ROOT}/dependency-dialog"
	cat >"${tools}/fzf" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" > "$TEST_FZF_ARGS"
printf 'submit\n1\n'
EOF
	chmod +x "${tools}/fzf"
	python3 - "${INS_BIN}" "${tools}" <<'PY'
import os, pty, subprocess, sys
master, slave = pty.openpty()
try:
    result = subprocess.run(
        [sys.argv[1], '--no-color', 'game', 'launch', 'Setup Game'],
        stdin=slave, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        env=dict(os.environ, PATH=sys.argv[2]), timeout=15,
    )
    assert result.returncode == 0, result.stdout.decode()
finally:
    os.close(slave)
    os.close(master)
PY
	assert_output_contains "$(cat "${TEST_FZF_ARGS}")" 'restic'
	assert_output_contains "$(cat "${TEST_FZF_ARGS}")" 'rclone'
	assert_output_contains "$(cat "${TEST_FZF_ARGS}")" 'Package Installations'
	[[ ! -e "${TEST_ROOT}/launched" ]]

	cat >"${tools}/restic" <<'EOF'
#!/bin/sh
case "$*" in
    *version*) echo 'restic 0.18.0';;
    *) echo 'sync was attempted' >&2; exit 1;;
esac
EOF
	chmod +x "${tools}/restic"
	if output="$(PATH="${tools}" ins game launch 'Setup Game' </dev/null 2>&1)"; then
		return 1
	fi
	assert_output_contains "${output}" 'rclone is not installed'
	assert_output_contains "${output}" 'ins game init'
	[[ ! -e "${TEST_ROOT}/launched" ]]

	cat >"${tools}/rclone" <<'EOF'
#!/bin/sh
echo 'other-account:'
EOF
	chmod +x "${tools}/rclone"
	if output="$(PATH="${tools}" ins game launch 'Setup Game' </dev/null 2>&1)"; then
		return 1
	fi
	assert_output_contains "${output}" "remote 'drive' is not configured"
	assert_output_contains "${output}" 'rclone config'
	[[ ! -e "${TEST_ROOT}/launched" ]]

	# A configured remote reaches sync, whose failure must still block launch.
	cat >"${tools}/rclone" <<'EOF'
#!/bin/sh
echo 'drive:'
EOF
	if output="$(PATH="${tools}" ins game launch 'Setup Game' </dev/null 2>&1)"; then
		return 1
	fi
	assert_output_contains "${output}" 'sync was attempted'
	assert_output_contains "${output}" 'The game was not launched'
	[[ ! -e "${TEST_ROOT}/launched" ]]
	echo "Launch setup guidance and sync failure checks succeeded"
}

main "$@"
