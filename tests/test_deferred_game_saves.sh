#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	require_restic || return 0
	setup_test_env
	trap cleanup_test_env EXIT
	export INS_GAME_GUI=0
	export TEST_REAL_RESTIC="$(command -v restic)"
	export TEST_RESTIC_CALLS="${TEST_ROOT}/restic-calls"
	export TEST_SAVE_PATH="${TEST_ROOT}/new-wine-saves"
	export TEST_LAUNCH_MARKER="${TEST_ROOT}/launched"
	export TEST_ALLOW_LAUNCH=0 TEST_SETUP_NOW=0
	local repo="${TEST_ROOT}/repo"
	mkdir -p "${TEST_ROOT}/bin"
	cat >"${TEST_ROOT}/bin/restic" <<'EOF'
#!/bin/bash
for arg in "$@"; do
    case "$arg" in
        snapshots|backup|restore) printf '%s\n' "$arg" >> "$TEST_RESTIC_CALLS"; break ;;
    esac
done
exec "$TEST_REAL_RESTIC" "$@"
EOF
	cat >"${TEST_ROOT}/bin/fzf" <<'PY'
#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
text = sys.stdin.read()
header = args[args.index('--header') + 1] if '--header' in args else ''
if '--print-query' in args:
    print(os.environ['TEST_SAVE_PATH'])
elif '--read0' in args:
    if "Backups not configured for" in header:
        choice = 0 if os.environ['TEST_ALLOW_LAUNCH'] == '1' else 1
    elif "Set up save backups for" in header:
        choice = 0 if os.environ['TEST_SETUP_NOW'] == '1' else 1
    elif 'Are you sure you want to use' in header:
        choice = 0
    else:
        raise AssertionError('Unexpected confirmation: ' + header)
    print('submit\n' + str(choice))
elif 'Choose the save path' in header:
    print(text.splitlines()[0])
else:
    raise AssertionError('Unexpected dialog: ' + header)
PY
	chmod +x "${TEST_ROOT}/bin/restic" "${TEST_ROOT}/bin/fzf"
	export PATH="${TEST_ROOT}/bin:${PATH}"
	ins game init --repo "${repo}" --password testpass >/dev/null
	local output
	output="$(ins game add --name 'New Wine Game' --skip-save-setup \
		--launch-command 'mkdir -p "$TEST_SAVE_PATH"; printf progress > "$TEST_SAVE_PATH/save.dat"; touch "$TEST_LAUNCH_MARKER"' 2>&1)"
	assert_output_contains "${output}" "Backups not configured"
	assert_output_contains "${output}" "Set up saves"
	python3 - "${XDG_CONFIG_HOME}/instant/games" <<'PY'
import pathlib, sys, tomllib
root = pathlib.Path(sys.argv[1])
assert tomllib.loads((root / 'games.toml').read_text())['games'][0]['launch_command']
assert not tomllib.loads((root / 'installations.toml').read_text()).get('installations')
PY
	# Declining the first-launch warning must not execute the launcher or create saves.
	ins game launch 'New Wine Game' >"${TEST_ROOT}/declined" 2>&1
	[[ ! -e "${TEST_LAUNCH_MARKER}" && ! -e "${TEST_SAVE_PATH}" ]]

	# Confirmed first launch runs without restic and keeps the reminder when setup is deferred.
	export TEST_ALLOW_LAUNCH=1
	ins game launch 'New Wine Game' >"${TEST_ROOT}/unbacked" 2>&1
	assert_file_equals "${TEST_SAVE_PATH}/save.dat" progress
	assert_output_contains "$(cat "${TEST_ROOT}/unbacked")" "Backups not configured"
	[[ ! -s "${TEST_RESTIC_CALLS}" ]]
	output="$(ins game sync 2>&1)"
	assert_output_contains "${output}" "Backups not configured for: New Wine Game"

	# Setup after the next launch binds the generated save directory and backs up its contents.
	export TEST_SETUP_NOW=1
	ins game launch 'New Wine Game' >"${TEST_ROOT}/configured" 2>&1
	python3 - "${XDG_CONFIG_HOME}/instant/games/installations.toml" "${TEST_SAVE_PATH}" "${repo}" <<'PY'
import json, os, pathlib, subprocess, sys, tomllib
config = pathlib.Path(sys.argv[1])
local = tomllib.loads(config.read_text())
assert local['installations'][0]['save_path'] == sys.argv[2]
assert local['installations'][0]['baseline']['remote_head']
env = dict(os.environ, RESTIC_PASSWORD='testpass')
snapshots = json.loads(subprocess.check_output([os.environ['TEST_REAL_RESTIC'], '-r', sys.argv[3], 'snapshots', '--json'], env=env))
assert len(snapshots) == 1
saved = subprocess.check_output([os.environ['TEST_REAL_RESTIC'], '-r', sys.argv[3], 'dump', snapshots[0]['id'], sys.argv[2] + '/save.dat'], env=env)
assert saved == b'progress'
# A broken existing binding must fail sync, never fall back to unbacked play.
config.write_text(config.read_text().replace('\n[[installations]]\n', '\n[[installations]]\npending_restore = "missing-snapshot"\n', 1))
assert tomllib.loads(config.read_text())['installations'][0]['pending_restore'] == 'missing-snapshot'
PY
	rm "${TEST_LAUNCH_MARKER}"
	if ins game launch 'New Wine Game' >"${TEST_ROOT}/broken" 2>&1; then
		echo "A failed save sync incorrectly allowed the game to launch" >&2
		return 1
	fi
	[[ ! -e "${TEST_LAUNCH_MARKER}" ]]
	assert_output_contains "$(cat "${TEST_ROOT}/broken")" "Pre-launch save sync failed"
	echo "Deferred save setup, first-launch consent, backup activation, and sync failure checks succeeded"
}

main "$@"
