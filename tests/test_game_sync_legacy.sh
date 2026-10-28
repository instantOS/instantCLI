#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	require_restic || return 0
	setup_test_env
	trap cleanup_test_env EXIT
	export TEST_REAL_RESTIC="$(command -v restic)"
	export SYNC_CALLS="${TEST_ROOT}/calls"
	mkdir -p "${TEST_ROOT}/bin" "${TEST_ROOT}/original/empty" "${TEST_ROOT}/local"
	cat >"${TEST_ROOT}/bin/restic" <<'EOF'
#!/bin/bash
for arg in "$@"; do
    case "$arg" in
        snapshots|backup|restore|ls) printf '%s\n' "$arg" >> "$SYNC_CALLS"; break ;;
    esac
done
if [[ "${SYNC_FAIL_BACKUP:-0}" == 1 && " $* " == *" backup "* ]]; then
    echo "Simulated backup failure" >&2
    exit 1
fi
exec "$TEST_REAL_RESTIC" "$@"
EOF
	chmod +x "${TEST_ROOT}/bin/restic"
	export PATH="${TEST_ROOT}/bin:${PATH}"
	local repo="${TEST_ROOT}/repo" config="${XDG_CONFIG_HOME}/instant/games"
	mkdir -p "${config}"
	printf progress >"${TEST_ROOT}/original/save"
	printf hidden >"${TEST_ROOT}/original/.hidden"
	ln -s save "${TEST_ROOT}/original/link"
	RESTIC_PASSWORD=testpass restic -r "${repo}" init >/dev/null
	# A real pre-baseline snapshot, without content, save-time or parent tags.
	RESTIC_PASSWORD=testpass restic -r "${repo}" backup --tag instantgame --tag TGVnYWN5 "${TEST_ROOT}/original" >/dev/null
	cp -a "${TEST_ROOT}/original/." "${TEST_ROOT}/local/"
	# Identical contents with different timestamps must not trigger a live restore.
	touch -t 202001010000 "${TEST_ROOT}/local/save"
	cat >"${config}/games.toml" <<EOF
repo = "${repo}"
repo_password = "testpass"
[[games]]
name = "Legacy"
EOF
	cat >"${config}/installations.toml" <<EOF
[[installations]]
game_name = "Legacy"
save_path = "${TEST_ROOT}/local"
save_path_type = "directory"
EOF
	local before after output
	before="$(stat -c '%Y:%i' "${TEST_ROOT}/local/save")"
	: >"${SYNC_CALLS}"
	output="$(ins game update)"
	assert_output_contains "${output}" "Legacy: Already in sync"
	assert_output_contains "${output}" "Sync Summary"
	assert_output_contains "${output}" "Backed up: 0"
	assert_output_contains "${output}" "Restored:  0"
	assert_output_contains "${output}" "Skipped:   1"
	assert_output_contains "${output}" "Errors:    0"
	[[ "${output}" != *$'\e['* ]]
	after="$(stat -c '%Y:%i' "${TEST_ROOT}/local/save")"
	[[ "${before}" == "${after}" ]]
	assert_calls snapshots=1 restore=1
	# Only the one-time comparison downloads data; the next run only lists history.
	: >"${SYNC_CALLS}"
	output="$(ins --output json game update)"
	assert_calls snapshots=1
	printf '%s\n' "${output}" | python3 -c '
import json,sys
events = [json.loads(line) for line in sys.stdin]
assert any(e["code"] == "game.sync.already_in_sync" for e in events), events
assert events[-1]["data"]["skipped"] == 1, events
assert not any("\x1b" in e["message"] for e in events)
'
	# A subsequent local edit must upload, rather than restore old remote data.
	printf updated >"${TEST_ROOT}/local/save"
	: >"${SYNC_CALLS}"
	output="$(ins --output json game update)"
	assert_output_contains "${output}" "Legacy: Backed up saves"
	printf '%s\n' "${output}" | python3 -c '
import json,sys
events = [json.loads(line) for line in sys.stdin]
backup = next(e for e in events if e["code"] == "game.sync.backup.completed")
assert len(backup["data"]["snapshot_id"]) == 64, backup
assert events[-1]["data"]["backed_up"] == 1, events
'
	assert_calls snapshots=2 backup=1
	assert_file_equals "${TEST_ROOT}/local/save" updated
	: >"${SYNC_CALLS}"
	# Unchanged games should be visually quiet even when color is enabled.
	output="$("${INS_BIN}" game update)"
	printf '%s\n' "${output}" | python3 -c '
import re,sys
output = sys.stdin.read()
line = next(line for line in output.splitlines() if "Already in sync" in line)
assert re.search(r"\x1b\[[0-9;]*2[;m]", line), repr(line)
assert not re.search(r"\x1b\[[0-9;]*32[;m]", line), repr(line)
'
	assert_calls snapshots=1
	# Failed uploads must remain visible in both the per-game error and summary.
	printf unsynced >"${TEST_ROOT}/local/save"
	: >"${SYNC_CALLS}"
	if output="$(SYNC_FAIL_BACKUP=1 ins game update Legacy 2>&1)"; then
		echo "Expected the simulated upload failure to fail the command" >&2
		return 1
	fi
	assert_output_contains "${output}" "Legacy:"
	assert_output_contains "${output}" "Failed to back up saves"
	assert_output_contains "${output}" "Backed up: 0"
	assert_output_contains "${output}" "Errors:    1"
	assert_calls snapshots=1 backup=1
	# Single-file legacy saves can live at a different filename on this device.
	printf file-progress >"${TEST_ROOT}/original.dat"
	RESTIC_PASSWORD=testpass restic -r "${repo}" backup --tag instantgame --tag TGVnYWN5IEZpbGU= "${TEST_ROOT}/original.dat" >/dev/null
	cp "${TEST_ROOT}/original.dat" "${TEST_ROOT}/local.dat"
	cat >>"${config}/games.toml" <<EOF
[[games]]
name = "Legacy File"
EOF
	cat >>"${config}/installations.toml" <<EOF
[[installations]]
game_name = "Legacy File"
save_path = "${TEST_ROOT}/local.dat"
save_path_type = "file"
EOF
	before="$(stat -c '%Y:%i' "${TEST_ROOT}/local.dat")"
	: >"${SYNC_CALLS}"
	output="$(ins game update 'Legacy File')"
	assert_output_contains "${output}" "Legacy File: Already in sync"
	[[ "${before}" == "$(stat -c '%Y:%i' "${TEST_ROOT}/local.dat")" ]]
	assert_calls snapshots=1 restore=1 ls=1
	: >"${SYNC_CALLS}"
	ins game update 'Legacy File' >/dev/null
	assert_calls snapshots=1
	echo "Legacy save sync adoption and repeat sync succeeded"
}

assert_calls() {
	python3 - "${SYNC_CALLS}" "$@" <<'PY'
import collections,pathlib,sys
actual = collections.Counter(pathlib.Path(sys.argv[1]).read_text().splitlines())
expected = collections.Counter({k: int(v) for k,v in (arg.split('=') for arg in sys.argv[2:])})
assert actual == expected, (actual, expected)
PY
}

main "$@"
