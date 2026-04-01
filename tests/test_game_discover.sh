#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	setup_test_env
	trap cleanup_test_env EXIT

	local bin_dir="${TEST_ROOT}/bin"
	local install_dir="${HOME}/Games/epic/Sable"
	local steam_root="${HOME}/.local/share/Steam"
	local steam_prefix="${steam_root}/steamapps/compatdata/12345/pfx"
	local steam_save_dir="${steam_prefix}/drive_c/users/steamuser/Documents/Test Steam Game"
	local wine_prefix="${HOME}/.wine"
	local wine_save_dir="${wine_prefix}/drive_c/users/testuser/Documents/Test Wine Game"
	mkdir -p "${bin_dir}" "${install_dir}"
	export PATH="${bin_dir}:${PATH}"

	touch "${install_dir}/Sable.exe"
	mkdir -p "${steam_save_dir}" "${wine_save_dir}" "${steam_root}/steamapps" "${XDG_CACHE_HOME}/instant"
	echo "save" >"${steam_save_dir}/save1.sav"
	echo "save" >"${wine_save_dir}/profile1.sav"

	cat >"${XDG_CACHE_HOME}/instant/ludusavi-manifest.yaml" <<'EOF'
"Test Steam Game":
  files:
    "<winDocuments>/Test Steam Game":
      tags:
        - save
      when:
        - os: windows
"Test Wine Game":
  files:
    "<winDocuments>/Test Wine Game":
      tags:
        - save
      when:
        - os: windows
EOF

	cat >"${steam_root}/steamapps/libraryfolders.vdf" <<EOF
"libraryfolders"
{
  "0"
  {
    "path" "${steam_root}"
  }
}
EOF

	cat >"${steam_root}/steamapps/appmanifest_12345.acf" <<'EOF'
"AppState"
{
  "appid" "12345"
  "name" "Test Steam Game"
}
EOF

	cat >"${bin_dir}/legendary" <<EOF
#!/usr/bin/env sh
set -eu
if [ "\${1:-}" = "list-installed" ] && [ "\${2:-}" = "--json" ]; then
	cat <<'JSON'
[{
  "app_name": "9b48cbb1a0cf4a73b87ccbf4cde04b26",
  "install_path": "${install_dir}",
  "title": "Sable",
  "executable": "Sable.exe",
  "launch_parameters": ""
}]
JSON
else
	exit 1
fi
EOF
	chmod +x "${bin_dir}/legendary"

	local discover_json
	discover_json="$(ins --output json game discover)"
	echo "${discover_json}" | jq -e '
		.code == "game.discover" and
		(.data.count | tonumber) >= 1 and
		(.data.games | map(select(.name == "Sable" and .platform_short == "Epic")) | length) == 1
	' >/dev/null

	local epic_only_json
	epic_only_json="$(ins --output json game discover --source epic)"
	echo "${epic_only_json}" | jq -e '
		.code == "game.discover" and
		(.data.games | length) == 1 and
		.data.games[0].platform_short == "Epic"
	' >/dev/null

	local steam_only_json
	steam_only_json="$(ins --output json game discover --source steam)"
	echo "${steam_only_json}" | jq -e '
		.code == "game.discover" and
		(.data.games | map(select(.name == "Test Steam Game" and .platform_short == "Steam")) | length) == 1
	' >/dev/null

	local wine_only_json
	wine_only_json="$(ins --output json game discover --source wine)"
	echo "${wine_only_json}" | jq -e '
		.code == "game.discover" and
		(.data.games | map(select(.name == "Test Wine Game" and .platform_short == "Wine")) | length) == 1
	' >/dev/null

	local menu_output
	menu_output="$(ins_output game discover --menu --source epic)"
	assert_output_contains "${menu_output}" $'manual\tmanual\t'
	assert_output_contains "${menu_output}" "Sable (Epic)"

	echo "Game discovery CLI succeeded"
}

main "$@"
