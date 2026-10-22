#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	setup_test_env
	trap cleanup_test_env EXIT
	local native="${XDG_CONFIG_HOME}/PCSX2"
	local flatpak="${HOME}/.var/app/net.pcsx2.PCSX2/config/PCSX2"
	local images="${TEST_ROOT}/images" cache="${TEST_ROOT}/external-cache"
	mkdir -p "${native}/inis" "${flatpak}/inis" "${images}/deep" "${images}/skip" "${cache}" "${native}/memcards" "${TEST_ROOT}/bin"
	touch "${images}/disc.iso" "${images}/other.iso" "${images}/deep/nested.chd" "${images}/skip/excluded.iso"
	printf card >"${native}/memcards/shared.ps2"
	# No emulator subprocess is needed to discover games.
	printf '#!/bin/sh\nexit 99\n' >"${TEST_ROOT}/bin/pcsx2-qt"
	chmod +x "${TEST_ROOT}/bin/pcsx2-qt"
	export PATH="${TEST_ROOT}/bin:${PATH}"
	cat >"${native}/inis/PCSX2.ini" <<TOML
[Folders]
Cache = ${cache}
[GameList]
Paths = ${images}
RecursivePaths = ${images}
ExcludedPaths = ${images}/skip
TOML
	printf '[GameList]\nPaths = %s\n' "${images}" >"${flatpak}/inis/PCSX2.ini"
	python3 - "${cache}/gamelist.cache" "${images}/disc.iso" <<'PY'
import pathlib, struct, sys
path = pathlib.Path(sys.argv[2])
strings = [str(path), 'SLUS-12345', 'Database Title', 'sort', 'English title']
blob = struct.pack('<II', 0x45434c47, 34)
for value in strings:
    value = value.encode()
    blob += struct.pack('<I', len(value)) + value
blob += struct.pack('<BBQQIB', 0, 0, 0, int(path.stat().st_mtime), 0x12345678, 5)
pathlib.Path(sys.argv[1]).write_bytes(blob)
PY
	local result
	result="$(ins --output json game discover --source ps2)"
	python3 - "${result}" <<'PY'
import json, sys
rows = json.loads(sys.argv[1])['data']['games']
assert len(rows) == 5, rows  # 3 native, 2 flatpak; shared images remain separate.
assert all(r['save_path'] is None and r['game_path'] for r in rows)
assert len({r['unique_key'] for r in rows}) == 5
assert any(r['name'] == 'Database Title' and r['launch_hint']['source'] == 'native' for r in rows)
assert any(r['name'] == 'disc' and r['launch_hint']['source'] == 'flatpak' for r in rows)
assert not any('excluded' in r['game_path'] for r in rows)
PY
	# An unsupported cache is harmless and is never modified by discovery.
	printf future-cache >"${cache}/gamelist.cache"
	result="$(ins --output json game discover --source ps2)"
	assert_file_equals "${cache}/gamelist.cache" future-cache
	python3 - "${result}" <<'PY'
import json, sys
rows = json.loads(sys.argv[1])['data']['games']
assert len(rows) == 5
assert not any(r['name'] == 'Database Title' for r in rows)
PY
	# Configuration changes must take effect with ins's normal discovery cache enabled.
	printf '[GameList]\nPaths = %s\nExcludedPaths = %s\n' "${images}" "${images}/disc.iso" >"${native}/inis/PCSX2.ini"
	result="$(ins --output json game discover --source ps2)"
	python3 - "${result}" <<'PY'
import json, sys
rows = json.loads(sys.argv[1])['data']['games']
assert len(rows) == 3, rows
assert sum(r['launch_hint']['source'] == 'native' for r in rows) == 1
assert all(r['name'] == 'other' for r in rows if r['launch_hint']['source'] == 'native')
PY
	echo "PCSX2 image discovery, cache fallback, and installation identity succeeded"
}

main "$@"
