#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	prepare_ins_binary
	setup_test_env
	trap cleanup_test_env EXIT
	local database="${XDG_DATA_HOME}/instant/notifications.db"
	local tools="${TEST_ROOT}/tools"
	mkdir -p "$(dirname "${database}")" "${tools}"
	# Let ins create the database with its current schema, then add
	# notifications in arrival order. Spotify and the earlier Discord message
	# were read; neither app name nor read state matches the arrival order.
	ins notify count >/dev/null
	python3 - "${database}" <<'PY'
import sqlite3, sys

connection = sqlite3.connect(sys.argv[1])
connection.executemany(
    'INSERT INTO notifications (timestamp, app_name, title, body, read) VALUES (?, ?, ?, ?, ?)',
    [
        ('2026-11-24 09:00', 'Slack', 'Build finished', 'main passed', 0),
        ('2026-11-24 09:05', 'Discord', 'Second message', 'anyone there?', 1),
        ('2026-11-24 09:10', 'Discord', 'First message', 'hello', 0),
        ('2026-11-24 09:15', 'Spotify', 'Now playing', 'Song - Artist', 1),
    ],
)
connection.commit()
PY
	assert_output_contains "$(ins notify count)" 2
	cat >"${tools}/fzf" <<'PY'
#!/usr/bin/env python3
import os, re, sys

root = os.environ['TEST_ROOT']


def respond():
    args = sys.argv[1:]
    wrap_log = os.path.join(root, 'wrap-log')
    wraps = [arg.split('=', 1)[1].split(':')[-1]
             for arg in args if arg.startswith('--preview-window=')]
    assert wraps and all(option == wraps[0] for option in wraps), wraps
    wrap = {'wrap-word': 'word', 'wrap': 'plain'}[wraps[0]]
    with open(wrap_log, 'a') as log:
        log.write(wrap + '\n')
    if os.environ['TEST_FZF_LEGACY_WRAP'] == '1' and wrap == 'word':
        print('invalid preview window option: wrap-word', file=sys.stderr)
        sys.exit(2)

    text = sys.stdin.read()
    header = args[args.index('--header') + 1] if '--header' in args else ''
    plain = lambda value: re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', value)
    count_path = os.path.join(root, 'menu-count')
    count = int(open(count_path).read()) if os.path.exists(count_path) else 0
    position = [arg for arg in args if arg.startswith('load:pos(')]

    if any(arg.startswith('ctrl-s:') for arg in args):
        # The notification center: the only dialog bound to the sort key.
        open(count_path, 'w').write(str(count + 1))
        rows = text.split('\n')
        # Close, Options, Enable and start capture, then four notifications.
        assert len(rows) == 7, rows
        ids = [row.split('\x1f')[2].removeprefix('notif:') for row in rows[3:]]
        assert 'Ctrl S' in plain(header) and 'sort' in plain(header), header
        if count == 0:
            # Notifications default to the newest first.
            assert 'Sort: Newest first' in plain(header), header
            assert '2 unread' in plain(header), header
            assert ids == ['4', '3', '2', '1'], ids
            print('ctrl-s')
            print(rows[3])  # Spotify is focused while Ctrl+S is pressed
        elif count == 1:
            # Unread first moved the read Spotify notification below the
            # unread ones; the cursor followed it.
            assert 'Sort: Unread first' in plain(header), header
            assert ids == ['3', '1', '4', '2'], ids
            assert position == ['load:pos(6)'], args
            print('ctrl-s')
            print(rows[5])  # Spotify is still focused
        elif count == 2:
            # By application lists Discord above Slack and Spotify.
            assert 'Sort: By application' in plain(header), header
            assert '2 unread' in plain(header), header
            assert ids == ['3', '2', '1', '4'], ids
            assert position == ['load:pos(7)'], args
            print(rows[6])  # open Spotify, which was read already
        else:
            # The chosen order and unread count survive returning from the
            # notification, which is still focused.
            assert 'Sort: By application' in plain(header), header
            assert '2 unread' in plain(header), header
            assert ids == ['3', '2', '1', '4'], ids
            assert position == ['load:pos(7)'], args
            sys.exit(130)
    elif '--read0' in args:
        if 'Sort Notifications' in plain(header):
            records = text.split('\0')
            if count == 1:
                # Back leads the chooser; it opens on the current order.
                assert 'Back' in records[0], records[0]
                assert 'Newest first' in records[1] and 'current' in records[1], text
                assert position == ['load:pos(2)'], args
                print('submit\n3')  # Unread first
            else:
                assert 'Back' in records[0], records[0]
                assert 'Unread first' in records[3] and 'current' in records[3], text
                assert position == ['load:pos(4)'], args
                print('submit\n4')  # By application
        elif 'Back' in text:
            print('submit\n0')  # notification details: back
        else:
            raise AssertionError('Unexpected dialog: ' + plain(header))
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

	# The scripted fzf asserts the row order and cursor of every notification list.
	local legacy
	for legacy in 0 1; do
		rm -f "${TEST_ROOT}/menu-count" "${TEST_ROOT}/wrap-log"
		if ! TEST_FZF_LEGACY_WRAP="${legacy}" ins notify >"${TEST_ROOT}/menu" 2>&1; then
			cat "${TEST_ROOT}/menu" >&2
			return 1
		fi
		assert_file_equals "${TEST_ROOT}/menu-count" 4
		if [[ "${legacy}" == 0 ]]; then
			assert_file_equals "${TEST_ROOT}/wrap-log" $'word\nword\nword\nword\nword\nword\nword'
		else
			assert_file_equals "${TEST_ROOT}/wrap-log" $'word\nplain\nplain\nplain\nplain\nplain\nplain\nplain'
		fi
	done
	echo "Notification center sorting and cursor checks succeeded"
}

main "$@"
