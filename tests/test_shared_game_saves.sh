#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	require_restic || return 0
	setup_test_env
	trap cleanup_test_env EXIT
	# Record only command names, never repository passwords or other arguments.
	export TEST_REAL_RESTIC="$(command -v restic)"
	mkdir -p "${TEST_ROOT}/restic-bin"
	cat >"${TEST_ROOT}/restic-bin/restic" <<'EOF'
#!/bin/bash
if [[ -n "${TEST_RESTIC_CALLS:-}" ]]; then
    for arg in "$@"; do
        case "$arg" in
            snapshots|backup|restore|ls) printf '%s\n' "$arg" >> "$TEST_RESTIC_CALLS"; break ;;
        esac
    done
fi
exec "$TEST_REAL_RESTIC" "$@"
EOF
	chmod +x "${TEST_ROOT}/restic-bin/restic"
	export PATH="${TEST_ROOT}/restic-bin:${PATH}"
	local repo="${TEST_ROOT}/repo"
	local a="${TEST_ROOT}/a" b="${TEST_ROOT}/b"
	mkdir -p "${a}/cards" "${b}/cards"
	RESTIC_PASSWORD=testpass restic -r "${repo}" init >/dev/null
	local device
	for device in "${a}" "${b}"; do
		mkdir -p "${device}/config/instant/games"
		cat >"${device}/config/instant/games/games.toml" <<EOF
repo = "${repo}"
repo_password = "testpass"
[[games]]
name = "A"
save_group = "cards"
[[games]]
name = "B"
save_group = "cards"
[[save_groups]]
id = "cards"
name = "Shared PS2 cards"
EOF
		cat >"${device}/config/instant/games/installations.toml" <<EOF
[[save_groups]]
id = "cards"
save_path = "${device}/cards"
save_path_type = "directory"
EOF
	done
	# A fresh empty local directory must download existing progress, even though
	# its directory timestamp is newer than the remote save files.
	printf initial >"${a}/cards/card.ps2"
	stamp "${a}/cards" 600
	TEST_RESTIC_CALLS="${TEST_ROOT}/upload-calls" device_ins "${a}" game sync A
	assert_restic_calls "${TEST_ROOT}/upload-calls" snapshots=2 backup=1
	TEST_RESTIC_CALLS="${TEST_ROOT}/restore-calls" device_ins "${b}" game sync B
	assert_restic_calls "${TEST_ROOT}/restore-calls" snapshots=1 restore=1
	assert_file_equals "${b}/cards/card.ps2" initial

	# Both machines edit the same baseline offline. B's contents were written later,
	# even though A reconnects later and creates the later recovery backup.
	printf deck-progress >"${a}/cards/card.ps2"
	stamp "${a}/cards" 400
	printf pc-progress >"${b}/cards/card.ps2"
	stamp "${b}/cards" 200
	device_ins "${b}" game sync B
	local resolved
	resolved="$(TEST_RESTIC_CALLS="${TEST_ROOT}/conflict-calls" device_ins "${a}" game sync A)"
	assert_restic_calls "${TEST_ROOT}/conflict-calls" snapshots=3 backup=2 restore=1
	assert_output_contains "${resolved}" "newest saves"
	assert_file_equals "${a}/cards/card.ps2" pc-progress
	device_ins "${b}" game sync B
	assert_file_equals "${b}/cards/card.ps2" pc-progress
	# Inspect the repository contents, not only a reported success status.
	python3 - "${repo}" <<'PY'
import json, os, subprocess, sys
repo = sys.argv[1]
env = dict(os.environ, RESTIC_PASSWORD='testpass')
snapshots = json.loads(subprocess.check_output(['restic', '-r', repo, 'snapshots', '--json'], env=env))
versions = []
for snapshot in snapshots:
    if 'instantgame-group' in snapshot.get('tags', []):
        versions.append(subprocess.check_output(['restic', '-r', repo, 'dump', snapshot['id'], snapshot['paths'][0] + '/card.ps2'], env=env))
assert b'deck-progress' in versions and b'pc-progress' in versions, versions
PY

	# A game and group with the same spelling have disjoint histories.
	mkdir -p "${a}/private"
	printf private >"${a}/private/save"
	device_ins "${a}" game add --name cards --save-path "${a}/private"
	device_ins "${a}" game sync cards
	# A no-op sync shares one repository read across private and shared owners.
	TEST_RESTIC_CALLS="${TEST_ROOT}/noop-calls" device_ins "${a}" game sync
	assert_restic_calls "${TEST_ROOT}/noop-calls" snapshots=1
	device_ins "${a}" game snapshots cards >"${TEST_ROOT}/private-history"
	if grep -q '\[group:cards\]' "${TEST_ROOT}/private-history"; then
		echo "Private game selected group history" >&2
		return 1
	fi
	# Portable membership requires no per-game installation or copied checkpoint.
	device_ins "${a}" game add --name C --save-group cards --launch-command true
	python3 - "${a}/config/instant/games/installations.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], 'rb') as f: config = tomllib.load(f)
assert all(i['game_name'] not in ['A', 'B', 'C'] for i in config.get('installations', []))
assert len(config['save_groups']) == 1
PY

	# Reject directory/card overlap before any restore can change data.
	cp "${a}/config/instant/games/installations.toml" "${TEST_ROOT}/bindings"
	cat >>"${a}/config/instant/games/installations.toml" <<EOF
[[installations]]
game_name = "Nested"
save_path = "${a}/cards/card.ps2"
save_path_type = "file"
EOF
	if device_ins "${a}" game group sync cards >"${TEST_ROOT}/overlap" 2>&1; then
		echo "Overlapping ownership was accepted" >&2
		return 1
	fi
	assert_output_contains "$(cat "${TEST_ROOT}/overlap")" "Conflicting save ownership"
	cp "${TEST_ROOT}/bindings" "${a}/config/instant/games/installations.toml"

	# Direct Steam shortcut launches hold the shared lock through gameplay.
	# No fzf or interactive terminal is available in this test.
	mkdir -p "${TEST_ROOT}/bin" "${a}/config/PCSX2/inis"
	cat >"${a}/config/PCSX2/inis/PCSX2.ini" <<EOF
[Folders]
MemoryCards = ${a}/cards
[MemoryCards]
Slot1_Filename = card.ps2
Slot2_Enable = false
EOF
	cat >"${TEST_ROOT}/bin/pcsx2-qt" <<'EOF'
#!/bin/sh
case " $* " in
*" -testconfig "*) printf 'DataRoot Directory: %s\n' "$TEST_PCSX2_ROOT"; exit 0 ;;
esac
: > "$TEST_STARTED"
cp "$TEST_CARD" "$TEST_LAUNCH_INPUT"
sleep 2
printf played-from-shortcut > "$TEST_CARD"
EOF
	cat >"${TEST_ROOT}/bin/zenity" <<'EOF'
#!/bin/sh
case "$1" in
--progress) cat >/dev/null ;;
*) echo "Unexpected interactive dialog: $*" >> "$TEST_DIALOGS"; exit 1 ;;
esac
EOF
	cat >"${TEST_ROOT}/bin/notify-send" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$TEST_NOTIFICATIONS"
EOF
	cat >"${TEST_ROOT}/bin/fzf" <<'EOF'
#!/bin/sh
exit 99
EOF
	chmod +x "${TEST_ROOT}/bin/"*
	export PATH="${TEST_ROOT}/bin:${PATH}"
	export TEST_STARTED="${TEST_ROOT}/started" TEST_CARD="${a}/cards/card.ps2" TEST_LAUNCH_INPUT="${TEST_ROOT}/launch-input" TEST_PCSX2_ROOT="${a}/config/PCSX2"
	export TEST_DIALOGS="${TEST_ROOT}/dialogs" TEST_NOTIFICATIONS="${TEST_ROOT}/notifications"
	device_ins "${a}" game add --name Shortcut --save-group cards --launch-command "pcsx2-qt -- '${TEST_ROOT}/game.iso'"
	printf shortcut-deck >"${a}/cards/card.ps2"
	stamp "${a}/cards" 120
	printf shortcut-cloud >"${b}/cards/card.ps2"
	stamp "${b}/cards" 60
	device_ins "${b}" game sync B
	# An uncovered per-game card override must fail before automatic restore.
	mkdir -p "${a}/config/PCSX2/gamesettings"
	printf '[Folders]\nMemoryCards = other-cards\n' >"${a}/config/PCSX2/gamesettings/override.ini"
	if device_ins "${a}" game launch Shortcut >"${TEST_ROOT}/mismatch" 2>&1; then
		echo "Uncovered PCSX2 card override was accepted" >&2
		return 1
	fi
	assert_output_contains "$(cat "${TEST_ROOT}/mismatch")" "PCSX2 launch mapping mismatch"
	assert_file_equals "${a}/cards/card.ps2" shortcut-deck
	[[ ! -e "${TEST_STARTED}" ]]
	rm "${a}/config/PCSX2/gamesettings/override.ini"
	INS_GAME_GUI=1 DISPLAY=:test device_ins "${a}" game launch Shortcut >"${TEST_ROOT}/launch" 2>&1 &
	local launch_pid=$!
	for _ in $(seq 1 200); do
		[[ -e "${TEST_STARTED}" ]] && break
		sleep 0.05
	done
	assert_path_exists "${TEST_STARTED}"
	if device_ins "${a}" game group sync cards >"${TEST_ROOT}/locked" 2>&1; then
		echo "Shared saves were synchronized while the game held their lock" >&2
		return 1
	fi
	assert_output_contains "$(cat "${TEST_ROOT}/locked")" "Another save operation"
	wait "${launch_pid}"
	assert_file_equals "${a}/cards/card.ps2" played-from-shortcut
	assert_file_equals "${TEST_LAUNCH_INPUT}" shortcut-cloud
	assert_output_contains "$(cat "${TEST_NOTIFICATIONS}")" "Newest saves selected automatically"
	[[ ! -e "${TEST_DIALOGS}" ]]
	device_ins "${b}" game sync B
	assert_file_equals "${b}/cards/card.ps2" played-from-shortcut

	# Interrupted restores retry the exact snapshot without letting damaged local
	# files win merely because their modification time is newer.
	python3 - "${a}/config/instant/games/installations.toml" <<'PYTEST'
import pathlib, sys, tomllib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
group = tomllib.loads(s)['save_groups'][0]
s = s.replace('[save_groups.baseline]', 'pending_restore = "' + group['nearest_checkpoint'] + '"\n\n[save_groups.baseline]', 1)
p.write_text(s)
PYTEST
	printf partial-restore >"${a}/cards/card.ps2"
	device_ins "${a}" game group sync cards
	assert_file_equals "${a}/cards/card.ps2" played-from-shortcut
	python3 - "${a}/config/instant/games/installations.toml" <<'PYTEST'
import sys, tomllib
with open(sys.argv[1], 'rb') as f: group = tomllib.load(f)['save_groups'][0]
assert 'pending_restore' not in group
assert group['baseline']['remote_head']
PYTEST

	# Removing a member retains the group and every other game's reference.
	device_ins "${a}" game remove A --force
	device_ins "${a}" game group list >"${TEST_ROOT}/groups"
	assert_output_contains "$(cat "${TEST_ROOT}/groups")" "Shared PS2 cards"
	if device_ins "${a}" game group remove cards; then
		echo "Referenced group was removed" >&2
		return 1
	fi
	# Pruning must preserve the alternatives retained during automatic resolution.
	device_ins "${a}" game prune --game-name B
	python3 - "${repo}" <<'PY'
import json, os, subprocess, sys
repo = sys.argv[1]
env = dict(os.environ, RESTIC_PASSWORD='testpass')
snapshots = json.loads(subprocess.check_output(['restic', '-r', repo, 'snapshots', '--json'], env=env))
versions = []
for snapshot in snapshots:
    if 'instantgame-group' in snapshot.get('tags', []):
        versions.append(subprocess.check_output(['restic', '-r', repo, 'dump', snapshot['id'], snapshot['paths'][0] + '/card.ps2'], env=env))
assert b'deck-progress' in versions and b'pc-progress' in versions, versions
PY
	echo "Shared game-save synchronization succeeded"
}

assert_restic_calls() {
	local log="$1"
	shift
	python3 - "$log" "$@" <<'PYTEST'
import collections, pathlib, sys
actual = collections.Counter(pathlib.Path(sys.argv[1]).read_text().splitlines())
expected = collections.Counter({k: int(v) for k, v in (a.split('=') for a in sys.argv[2:])})
assert actual == expected, (actual, expected)
PYTEST
}

device_ins() {
	local device_root="$1"
	shift
	XDG_CONFIG_HOME="${device_root}/config" INS_GAME_GUI="${INS_GAME_GUI:-0}" ins "$@"
}

stamp() {
	python3 - "$1" "$2" <<'PY'
import os, pathlib, sys, time
root = pathlib.Path(sys.argv[1])
t = time.time() - int(sys.argv[2])
for p in [root, *root.rglob('*')]: os.utime(p, (t, t))
PY
}

main "$@"
