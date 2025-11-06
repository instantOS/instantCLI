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

    ins game init --repo "${restic_repo}" --password testpass

    ins game add \
        --name "${game_name}" \
        --description "Integration test game" \
        --launch-command "echo Launching ${game_name}" \
        --save-path "${save_path}" \
        --create-save-path

    # Verify list uses JSON and contains the game
    list_json="$(ins --output json game list)"
    echo "${list_json}" | jq -e '.code == "game.list" and (.data.count | tonumber) >= 1' >/dev/null
    echo "${list_json}" | jq -e --arg name "${game_name}" '.data.games[].name == $name' >/dev/null

    # Verify info uses JSON and returns correct game name
    info_json="$(ins --output json game info "${game_name}")"
    echo "${info_json}" | jq -e --arg name "${game_name}" 'select(.code=="game.show.details") | .data.game.name == $name' >/dev/null

    ins game sync --force "${game_name}"

    local backup_output
    backup_output="$(ins_output game backup "${game_name}")"
    echo "${backup_output}"

    local snapshot_id
    snapshot_id="$(printf '%s\n' "${backup_output}" | sed -n 's/.*snapshot: \([0-9a-f]\{8,\}\).*/\1/p' | head -n1)"

    if [[ -z "${snapshot_id:-}" ]]; then
        echo "Failed to determine snapshot id from backup output" >&2
        exit 1
    fi

    rm -rf "${save_path}"
    mkdir -p "${save_path}"

    ins game restore --force "${game_name}" "${snapshot_id}"

    local expected_file="${save_path}/save1.txt"
    mapfile -t restored_files < <(find "${save_path}" -type f -name 'save1.txt' | sort)
    if [[ "${#restored_files[@]}" -ne 1 ]]; then
        echo "Expected exactly one restored save file, found ${#restored_files[@]}" >&2
        printf 'Restored files:\n' >&2
        printf '  %s\n' "${restored_files[@]}" >&2
        exit 1
    fi

    if [[ "${restored_files[0]}" != "${expected_file}" ]]; then
        echo "Restore placed save at unexpected path" >&2
        echo "Expected: ${expected_file}" >&2
        echo "Actual:   ${restored_files[0]}" >&2
        exit 1
    fi

    assert_file_equals "${expected_file}" "initial save"

    ins game restic snapshots >/dev/null
    ins game launch "${game_name}"
    ins game setup
    ins game remove "${game_name}" --force

    # Ensure the game is no longer listed
    list_after_remove_json="$(ins --output json game list)"
    if echo "${list_after_remove_json}" | jq -e --arg name "${game_name}" '.data.games[].name == $name' >/dev/null; then
        echo "Game still present in list after removal" >&2
        exit 1
    fi

    echo "Game manager end-to-end flow succeeded"
}

main "$@"
