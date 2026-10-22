#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

write_fixture() {
	local second_path="$1" pending="${2:-}"
	cat >"${XDG_CONFIG_HOME}/instant/games/games.toml" <<TOML
repo = "${TEST_ROOT}/repo"
repo_password = "testpass"
[[games]]
name = "A"
[[games]]
name = "B"
TOML
	cat >"${XDG_CONFIG_HOME}/instant/games/installations.toml" <<TOML
[[installations]]
game_name = "A"
save_path = "${TEST_ROOT}/memcards/shared.ps2"
save_path_type = "file"
nearest_checkpoint = "old-private-a"
launch_command = "echo local-override"
[[installations]]
game_name = "B"
save_path = "${second_path}"
save_path_type = "file"
nearest_checkpoint = "old-private-b"
${pending}
TOML
}

expect_unchanged_failure() {
	local message="$1"
	shift
	cp "${XDG_CONFIG_HOME}/instant/games/games.toml" "${TEST_ROOT}/before-games"
	cp "${XDG_CONFIG_HOME}/instant/games/installations.toml" "${TEST_ROOT}/before-local"
	if ins "$@" >"${TEST_ROOT}/failure" 2>&1; then
		echo "Expected failure: $*" >&2
		return 1
	fi
	assert_output_contains "$(cat "${TEST_ROOT}/failure")" "${message}"
	cmp "${TEST_ROOT}/before-games" "${XDG_CONFIG_HOME}/instant/games/games.toml"
	cmp "${TEST_ROOT}/before-local" "${XDG_CONFIG_HOME}/instant/games/installations.toml"
}

main() {
	setup_test_env
	trap cleanup_test_env EXIT
	mkdir -p "${XDG_CONFIG_HOME}/instant/games" "${TEST_ROOT}/memcards"
	printf shared-progress >"${TEST_ROOT}/memcards/shared.ps2"
	printf independent-progress >"${TEST_ROOT}/memcards/independent.ps2"

	write_fixture "${TEST_ROOT}/memcards/independent.ps2"
	expect_unchanged_failure "exactly the same save path" game group migrate shared --name "Shared card" A B
	write_fixture "${TEST_ROOT}/memcards/shared.ps2" 'pending_restore = "unfinished"'
	expect_unchanged_failure "incomplete restore" game group migrate shared --name "Shared card" A B

	# Joining must not bypass interrupted-restore protection either.
	write_fixture "${TEST_ROOT}/memcards/independent.ps2" 'pending_restore = "unfinished"'
	ins game group create shared --name "Shared card"
	expect_unchanged_failure "incomplete restore" game group join shared B --discard-private-binding

	local have_restic=false
	if require_restic; then
		have_restic=true
		RESTIC_PASSWORD=testpass restic -r "${TEST_ROOT}/repo" init >/dev/null
		local tag
		for tag in QQ== Qg==; do
			RESTIC_PASSWORD=testpass restic -r "${TEST_ROOT}/repo" backup --tag instantgame --tag "${tag}" "${TEST_ROOT}/memcards/shared.ps2" >/dev/null
		done
	fi

	# Convert an otherwise invalid duplicate-path configuration in one operation.
	write_fixture "${TEST_ROOT}/memcards/shared.ps2"
	ins game group migrate shared --name "Shared card" A B
	python3 - "${XDG_CONFIG_HOME}/instant/games" "${TEST_ROOT}/memcards/shared.ps2" <<'PY'
import pathlib, sys, tomllib
root = pathlib.Path(sys.argv[1])
config = tomllib.loads((root / 'games.toml').read_text())
local = tomllib.loads((root / 'installations.toml').read_text())
assert all(g['save_group'] == 'shared' for g in config['games'])
assert all('save_path' not in i and 'nearest_checkpoint' not in i for i in local['installations'])
assert local['installations'][0]['launch_command'] == 'echo local-override'
group = local['save_groups'][0]
assert group['save_path'] == sys.argv[2] and group['save_path_type'] == 'file'
assert 'nearest_checkpoint' not in group and 'baseline' not in group
PY
	assert_file_equals "${TEST_ROOT}/memcards/shared.ps2" shared-progress
	assert_file_equals "${TEST_ROOT}/memcards/independent.ps2" independent-progress

	# A per-card group must never roll back the sibling card. Keep legacy private
	# snapshots in the same repository to verify migration preserves their namespace.
	if "${have_restic}"; then
		RESTIC_PASSWORD=testpass restic -r "${TEST_ROOT}/repo" snapshots --json | python3 -c 'import json,sys; assert len(json.load(sys.stdin)) == 2'
		ins game group reconcile shared --local
		local snapshot
		snapshot="$(RESTIC_PASSWORD=testpass restic -r "${TEST_ROOT}/repo" snapshots --json | python3 -c 'import json,sys; print(next(s["id"] for s in json.load(sys.stdin) if "instantgame-group" in s.get("tags", [])))')"
		printf changed >"${TEST_ROOT}/memcards/shared.ps2"
		ins game group sync shared
		ins game group restore shared "${snapshot}" --force
		assert_file_equals "${TEST_ROOT}/memcards/shared.ps2" shared-progress
		assert_file_equals "${TEST_ROOT}/memcards/independent.ps2" independent-progress
		RESTIC_PASSWORD=testpass restic -r "${TEST_ROOT}/repo" snapshots --json | python3 -c 'import json,sys; assert sum("instantgame" in s.get("tags", []) for s in json.load(sys.stdin)) == 2'
	fi
	echo "Save group migration and per-card isolation succeeded"
}

main "$@"
