#!/usr/bin/env bash
set -euo pipefail

if ! command -v restic >/dev/null 2>&1; then
    echo "restic is required to run this script" >&2
    exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

tmp_root="$(mktemp -d)"
cleanup() {
    rm -rf "${tmp_root}"
}
trap cleanup EXIT

export HOME="${tmp_root}/home"
export XDG_CONFIG_HOME="${HOME}/.config"
export XDG_DATA_HOME="${HOME}/.local/share"
export XDG_CACHE_HOME="${HOME}/.cache"

mkdir -p "${HOME}" "${XDG_CONFIG_HOME}" "${XDG_DATA_HOME}" "${XDG_CACHE_HOME}"

restic_repo="${tmp_root}/restic_repo"
mkdir -p "${restic_repo}"

game_name="Test Game"
save_path="${HOME}/.local/share/test-game/saves"
mkdir -p "${save_path}"
echo "initial save" > "${save_path}/save1.txt"

cd "${repo_root}"

cargo build --quiet

instant_cmd=(cargo run --quiet --)

run_instant() {
    "${instant_cmd[@]}" "$@"
}

run_instant game init --repo "${restic_repo}" --password testpass

run_instant game add \
    --name "${game_name}" \
    --description "Integration test game" \
    --launch-command "echo Launching ${game_name}" \
    --save-path "${save_path}" \
    --create-save-path

run_instant game list

run_instant game show "${game_name}"

run_instant game sync --force "${game_name}"

backup_output="$(run_instant game backup "${game_name}")"
echo "${backup_output}"
snapshot_id="$(printf '%s\n' "${backup_output}" | sed -n 's/.*snapshot: \([0-9a-f]\{8,\}\).*/\1/p' | head -n1)"

if [[ -z "${snapshot_id:-}" ]]; then
    echo "Failed to determine snapshot id from backup output" >&2
    exit 1
fi

rm -rf "${save_path}"
mkdir -p "${save_path}"

run_instant game restore --force "${game_name}" "${snapshot_id}"

restored_file="$(find "${save_path}" -type f -name 'save1.txt' | head -n1)"

if [[ -z "${restored_file}" ]]; then
    echo "Restore did not recreate expected save file" >&2
    exit 1
fi

if [[ "$(cat "${restored_file}")" != "initial save" ]]; then
    echo "Restored save has unexpected contents" >&2
    exit 1
fi

run_instant game restic snapshots > /dev/null

run_instant game launch "${game_name}"

run_instant game setup

run_instant game remove "${game_name}" --force

run_instant game list

echo "Game management end-to-end test completed successfully."
