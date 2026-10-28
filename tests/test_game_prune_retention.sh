#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	require_restic || return 0
	setup_test_env
	trap cleanup_test_env EXIT
	export TEST_REAL_RESTIC="$(command -v restic)"
	mkdir -p "${TEST_ROOT}/bin"
	cat >"${TEST_ROOT}/bin/restic" <<'EOF'
#!/bin/bash
for arg in "$@"; do
    case "$arg" in
        forget)
            if [[ "${INS_FAIL_FORGET:-0}" == 1 ]]; then
                echo 'simulated retention failure' >&2
                exit 1
            fi ;;
    esac
done
exec "$TEST_REAL_RESTIC" "$@"
EOF
	chmod +x "${TEST_ROOT}/bin/restic"
	export PATH="${TEST_ROOT}/bin:${PATH}"
	prune_case directory private
	prune_case file shared
	echo "Retention compaction, recovery pins, failure recovery and offline edits succeeded"
}

prune_case() {
	local kind="$1" mode="$2" root="${TEST_ROOT}/$1-$2"
	local repo="${root}/repo" config="${root}/config/instant/games"
	local save_path="${root}/saves" file="${root}/saves"
	export XDG_CONFIG_HOME="${root}/config"
	mkdir -p "${config}"
	if [[ "${kind}" == directory ]]; then
		mkdir -p "${save_path}"
		file="${save_path}/save.dat"
	fi
	RESTIC_PASSWORD=testpass restic -r "${repo}" init >/dev/null
	cat >"${config}/games.toml" <<EOF
repo = "${repo}"
repo_password = "testpass"
[retention_policy]
keep_last = 1
keep_daily = 0
keep_weekly = 0
keep_monthly = 0
keep_yearly = 0
[[games]]
name = "History"
EOF
	if [[ "${mode}" == shared ]]; then
		cat >>"${config}/games.toml" <<EOF
save_group = "cards"
[[save_groups]]
id = "cards"
name = "Shared history"
EOF
		printf '[[save_groups]]\nid = "cards"\n' >"${config}/installations.toml"
	else
		printf '[[installations]]\ngame_name = "History"\n' >"${config}/installations.toml"
	fi
	cat >>"${config}/installations.toml" <<EOF
save_path = "${save_path}"
save_path_type = "${kind}"
EOF
	local version
	for version in 0 1 2 3 4 5; do
		printf 'version-%s' "${version}" >"${file}"
		ins game backup History >/dev/null
	done
	# Pin an old checkpoint and record a recovery reference on a snapshot that
	# retention will later discard. Preserve the latest content baseline.
	python3 - "${config}/installations.toml" "${repo}" "${save_path}" "${mode}" <<'PY'
import base64,json,os,pathlib,re,subprocess,sys,tomllib
config,repo,save,mode = sys.argv[1:]
env = dict(os.environ, RESTIC_PASSWORD='testpass')
snapshots = json.loads(subprocess.check_output(['restic','-r',repo,'snapshots','--json'],env=env))
snapshots.sort(key=lambda s:s['time'])
path = pathlib.Path(config)
local = tomllib.loads(path.read_text())
binding = local['save_groups' if mode == 'shared' else 'installations'][0]
head = next(s for s in snapshots if s['id'] == binding['baseline']['remote_head'])
path.write_text(re.sub(r'nearest_checkpoint = "[^"]+"', 'nearest_checkpoint = "'+snapshots[0]['id']+'"', path.read_text()))
tags = ['instantgame-group' if mode == 'shared' else 'instantgame',
        base64.b64encode(('cards' if mode == 'shared' else 'History').encode()).decode(),
        'instantgame-parent:'+head['id'], 'instantgame-preserve:'+snapshots[1]['id'],
        'instantgame-content:'+binding['baseline']['content'],
        next(t for t in head['tags'] if t.startswith('instantgame-save-time:'))]
command = ['restic','-r',repo,'backup','--force']
for tag in tags: command += ['--tag',tag]
subprocess.run(command+[save],env=env,stdout=subprocess.DEVNULL,check=True)
path.with_suffix('.original.json').write_text(json.dumps(snapshots))
current = json.loads(subprocess.check_output(['restic','-r',repo,'snapshots','--json'],env=env))
superseded = {t.removeprefix('instantgame-parent:') for s in current for t in s['tags'] if t.startswith('instantgame-parent:')}
heads = [s for s in current if s['id'] not in superseded]
assert len(heads) == 1, heads
path.with_suffix('.source.json').write_text(json.dumps(heads[0]))
PY
	local before
	before="$(stat -c '%Y:%i' "${file}")"
	if INS_FAIL_FORGET=1 ins game prune --game-name History >"${root}/failure" 2>&1; then
		echo "Simulated pruning failure unexpectedly succeeded" >&2
		return 1
	fi
	assert_output_contains "$(cat "${root}/failure")" "simulated retention failure"
	# The metadata copy must not remove or retag any original snapshot, even if
	# deletion fails after publishing compacted history.
	python3 - "${repo}" "${config}/installations.original.json" <<'PY'
import json,os,pathlib,subprocess,sys
snapshots = json.loads(subprocess.check_output(['restic','-r',sys.argv[1],'snapshots','--json'],env=dict(os.environ,RESTIC_PASSWORD='testpass')))
original = json.loads(pathlib.Path(sys.argv[2]).read_text())
source = json.loads(pathlib.Path(sys.argv[2].replace('.original.json','.source.json')).read_text())
assert {s['id'] for s in original} <= {s['id'] for s in snapshots}
compacted = [s for s in snapshots if 'instantgame-compaction' in s['tags']]
assert len(compacted) == 1, snapshots
assert compacted[0]['paths'] == source['paths'], compacted
assert compacted[0]['tree'] == source['tree'], compacted
assert compacted[0]['hostname'] == source['hostname'], compacted
assert next(t for t in compacted[0]['tags'] if t.startswith('instantgame-save-time:')) == next(t for t in source['tags'] if t.startswith('instantgame-save-time:')), compacted
PY
	ins game prune --game-name History >/dev/null
	[[ "${before}" == "$(stat -c '%Y:%i' "${file}")" ]]
	assert_file_equals "${file}" version-5
	python3 - "${repo}" "${config}/installations.original.json" <<'PY'
import json,os,pathlib,subprocess,sys
snapshots = json.loads(subprocess.check_output(['restic','-r',sys.argv[1],'snapshots','--json'],env=dict(os.environ,RESTIC_PASSWORD='testpass')))
original = json.loads(pathlib.Path(sys.argv[2]).read_text())
ids = {s['id'] for s in snapshots}
assert len(snapshots) == 4, snapshots
assert {original[i]['id'] for i in [0,1,5]} <= ids
assert not {original[i]['id'] for i in [2,3,4]} & ids
superseded = {t.removeprefix('instantgame-parent:') for s in snapshots for t in s['tags'] if t.startswith('instantgame-parent:')}
heads = [s for s in snapshots if s['id'] not in superseded]
assert len(heads) == 1 and 'instantgame-compaction' in heads[0]['tags'], heads
pathlib.Path(sys.argv[2]+'.after').write_text(json.dumps(sorted(ids)))
PY
	# Repeated pruning must converge, rather than creating endless new anchors.
	ins game prune --game-name History >/dev/null
	python3 - "${repo}" "${config}/installations.original.json.after" <<'PY'
import json,os,pathlib,subprocess,sys
snapshots = json.loads(subprocess.check_output(['restic','-r',sys.argv[1],'snapshots','--json'],env=dict(os.environ,RESTIC_PASSWORD='testpass')))
assert sorted(s['id'] for s in snapshots) == json.loads(pathlib.Path(sys.argv[2]).read_text())
PY
	# A real local edit remains an upload when remote contents were unchanged by
	# compaction, even if the game preserved/backdated its modification time.
	printf offline-edit >"${file}"
	touch -t 200001010000 "${file}" "${save_path}"
	local output
	output="$(ins game sync History)"
	assert_output_contains "${output}" "Backed up saves"
	assert_output_contains "${output}" "Backed up: 1"
	assert_file_equals "${file}" offline-edit
	output="$(ins game sync History)"
	assert_output_contains "${output}" "Skipped:   1"
}

main "$@"
