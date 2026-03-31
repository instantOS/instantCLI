#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	setup_test_env
	trap cleanup_test_env EXIT

	local bin_dir="${TEST_ROOT}/bin"
	local install_dir="${HOME}/Games/epic/Sable"
	mkdir -p "${bin_dir}" "${install_dir}"
	export PATH="${bin_dir}:${PATH}"

	touch "${install_dir}/Sable.exe"

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

	local menu_output
	menu_output="$(ins_output game discover --menu)"
	assert_output_contains "${menu_output}" $'manual\tmanual\t'
	assert_output_contains "${menu_output}" "Sable (Epic)"

	echo "Game discovery CLI succeeded"
}

main "$@"
