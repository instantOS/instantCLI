#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
    if ! require_restic; then
        echo "Skipping game manager test because restic is not installed."
        return 0
    fi
    setup_test_env
    trap cleanup_test_env EXIT

    local restic_repo="${TEST_ROOT}/restic_repo"
    local game_name="Test Game"
    local save_path="${HOME}/.local/share/test-game/saves"

    mkdir -p "${restic_repo}" "${save_path}"
    echo "initial save" > "${save_path}/save1.txt"

    instant game init --repo "${restic_repo}" --password testpass

    instant game add \
        --name "${game_name}" \
        --description "Integration test game" \
        --launch-command "echo Launching ${game_name}" \
        --save-path "${save_path}" \
        --create-save-path

    instant game list
    instant game show "${game_name}"
    instant game sync --force "${game_name}"

    local backup_output
    backup_output="$(instant_output game backup "${game_name}")"
    echo "${backup_output}"

    local snapshot_id
    snapshot_id="$(printf '%s\n' "${backup_output}" | sed -n 's/.*snapshot: \([0-9a-f]\{8,\}\).*/\1/p' | head -n1)"

    if [[ -z "${snapshot_id:-}" ]]; then
        echo "Failed to determine snapshot id from backup output" >&2
        exit 1
    fi

    rm -rf "${save_path}"
    mkdir -p "${save_path}"

    instant game restore --force "${game_name}" "${snapshot_id}"

    local restored_file
    restored_file="$(find "${save_path}" -type f -name 'save1.txt' | head -n1)"
    if [[ -z "${restored_file}" ]]; then
        echo "Restore did not recreate expected save file" >&2
        exit 1
    fi

    assert_file_equals "${restored_file}" "initial save"

    instant game restic snapshots >/dev/null
    instant game launch "${game_name}"
    instant game setup
    instant game remove "${game_name}" --force
    instant game list

    echo "Game manager end-to-end flow succeeded"
}

main "$@"
