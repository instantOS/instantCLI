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
	mkdir -p "${config_dir}" "${tools}"
	# Config order is neither alphabetical nor recent. Neither game has save
	# backups on this device, so launching needs no restic.
	cat >"${config_dir}/games.toml" <<EOF
repo = '${TEST_ROOT}/repo'

[[games]]
name = 'Zeta Game'
launch_command = 'true'

[[games]]
name = 'Alpha Game'
launch_command = 'true'
EOF
	: >"${config_dir}/installations.toml"
	cat >"${tools}/fzf" <<'PY'
#!/usr/bin/env python3
import os, re, sys

root = os.environ['TEST_ROOT']


def respond():
    args = sys.argv[1:]
    text = sys.stdin.read()
    header = args[args.index('--header') + 1] if '--header' in args else ''
    plain = lambda value: re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', value)

    if any(arg.startswith('ctrl-s:') for arg in args):
        # The game menu: the only dialog bound to the sort key.
        count_path = os.path.join(root, 'menu-count')
        count = int(open(count_path).read()) if os.path.exists(count_path) else 0
        open(count_path, 'w').write(str(count + 1))
        rows = text.split('\n')
        assert len(rows) == 8, rows  # six actions, two games
        assert 'Ctrl S' in plain(header) and 'sort' in plain(header), header
        if count == 0:
            # Games default to the most recently played; Zeta was launched.
            assert 'Sort: Recently played' in plain(header), header
            assert 'Zeta Game' in rows[6] and 'Alpha Game' in rows[7], rows
            print('ctrl-s')
            print(rows[6])  # Zeta is focused while Ctrl+S is pressed
        elif count == 1:
            # Alphabetical moved Alpha above Zeta; the cursor followed Zeta.
            assert 'Sort: Alphabetical' in plain(header), header
            assert 'Alpha Game' in rows[6] and 'Zeta Game' in rows[7], rows
            assert [arg for arg in args if arg.startswith('load:pos(')] == ['load:pos(8)'], args
            print(rows[6])  # open Alpha Game
        else:
            # The chosen order survives returning from a game's action menu.
            assert 'Sort: Alphabetical' in plain(header), header
            assert 'Alpha Game' in rows[6] and 'Zeta Game' in rows[7], rows
            sys.exit(130)
    elif '--read0' in args:
        if 'Sort Games' in plain(header):
            assert 'Recently played' in text and 'current' in text.split('\0')[0], text
            print('submit\n2')  # Alphabetical
        elif 'Backups not configured for' in plain(header):
            print('submit\n0')  # Launch without backups
        elif 'Set up save backups for' in plain(header):
            print('submit\n1')  # Later
        else:
            sys.exit(130)  # action menu: back
    else:
        raise AssertionError('Unexpected dialog: ' + plain(header))


try:
    respond()
except AssertionError as error:
    # ins treats statuses other than 0, 1, 130 and 143 as renderer failures
    # and reports stderr verbatim, so failed expectations reach the test log.
    sys.stderr.write('scripted fzf: %s\n' % error)
    sys.exit(2)
PY
	chmod +x "${tools}/fzf"
	export PATH="${tools}:${PATH}"

	# A launch that ran is remembered, and nothing is recorded beforehand.
	local history="${XDG_CACHE_HOME}/instant/game-frecency.json"
	[[ ! -e "${history}" ]]
	if ! ins game launch 'Zeta Game' >"${TEST_ROOT}/launch" 2>&1; then
		cat "${TEST_ROOT}/launch" >&2
		return 1
	fi
	assert_output_contains "$(cat "${TEST_ROOT}/launch")" 'Launch command completed for Zeta Game'
	grep -q '"Zeta Game"' "${history}"
	if grep -q '"Alpha Game"' "${history}"; then
		echo "Launching Zeta recorded Alpha" >&2
		return 1
	fi

	# The scripted fzf asserts the row order and cursor of every game menu.
	if ! ins game menu >"${TEST_ROOT}/menu" 2>&1; then
		cat "${TEST_ROOT}/menu" >&2
		return 1
	fi
	assert_file_equals "${TEST_ROOT}/menu-count" 3
	[[ ! -e "${TEST_ROOT}/repo" ]]
	echo "Game menu sorting, frecency default, and launch history checks succeeded"
}

main "$@"
