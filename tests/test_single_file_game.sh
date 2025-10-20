#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
    if ! require_restic; then
        echo "Skipping single file game test because restic is not installed."
        return 0
    fi
    setup_test_env
    trap cleanup_test_env EXIT

    local restic_repo="${TEST_ROOT}/restic_repo"
    local game_name="Test Single File Game"
    local save_dir="${HOME}/.local/share/test-single-game"
    local save_file="${save_dir}/save.dat"
    local other_file="${save_dir}/other_file.txt"

    # Create directory with multiple files
    mkdir -p "${save_dir}"
    echo "initial save data" > "${save_file}"
    echo "other important data" > "${other_file}"

    # Initialize restic repository
    ins game init --repo "${restic_repo}" --password testpass

    # Add game with single file save
    ins game add \
        --name "${game_name}" \
        --description "Integration test game for single file saves" \
        --launch-command "echo Launching ${game_name}" \
        --save-path "${save_file}" \
        --create-save-path

    # Verify the game was added with correct configuration
    list_json="$(ins --output json game list)"
    echo "${list_json}" | jq -e '.code == "game.list" and (.data.count | tonumber) >= 1' >/dev/null
    echo "${list_json}" | jq -e --arg name "${game_name}" '.data.games[].name == $name' >/dev/null

    # Verify show command returns correct game info
    show_json="$(ins --output json game show "${game_name}")"
    echo "${show_json}" | jq -e --arg name "${game_name}" 'select(.code=="game.show.details") | .data.game.name == $name' >/dev/null

    # Backup the single file
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

    # Test 1: Verify restic only backed up the specific file, not the entire directory
    echo "Testing restic backup contents..."
    local restic_files
    restic_files="$(RESTIC_PASSWORD=testpass restic -r "${restic_repo}" ls "${snapshot_id}" | grep -E '^\w+' | tail -n +2)"

    # Should contain the save file but not the other file
    if ! echo "${restic_files}" | grep -q "save.dat"; then
        echo "Expected save.dat to be in restic backup" >&2
        echo "Restic files: ${restic_files}" >&2
        exit 1
    fi

    if echo "${restic_files}" | grep -q "other_file.txt"; then
        echo "Expected other_file.txt NOT to be in restic backup" >&2
        echo "Restic files: ${restic_files}" >&2
        exit 1
    fi

    # Test 2: Modify the save file and backup again
    echo "modified save data v2" > "${save_file}"
    ins game backup "${game_name}"

    local backup_output2
    backup_output2="$(ins_output game backup "${game_name}")"
    echo "${backup_output2}"

    local snapshot_id2
    snapshot_id2="$(printf '%s\n' "${backup_output2}" | sed -n 's/.*snapshot: \([0-9a-f]\{8,\}\).*/\1/p' | head -n1)"

    # Test 3: Remove save file and restore from snapshot
    rm -f "${save_file}"
    if [[ -f "${save_file}" ]]; then
        echo "Failed to remove save file before restore test" >&2
        exit 1
    fi

    # The other file should remain untouched
    if [[ ! -f "${other_file}" ]]; then
        echo "Other file was unexpectedly removed" >&2
        exit 1
    fi

    ins game restore --force "${game_name}" "${snapshot_id}"

    # Verify only the save file was restored, other files untouched
    if [[ ! -f "${save_file}" ]]; then
        echo "Save file was not restored" >&2
        exit 1
    fi

    assert_file_equals "${save_file}" "initial save data"
    assert_file_equals "${other_file}" "other important data"

    # Test 4: Test dependency with single file
    echo "Testing single file dependencies..."
    local dep_source="${TEST_ROOT}/dependency"
    local dep_target="${save_dir}/dependency.dat"
    echo "dependency content" > "${dep_source}"

    # Add dependency
    ins game dependency add "${game_name}" --source "${dep_source}" --install-path "${dep_target}"

    # Backup dependency
    ins game dependency backup "${game_name}"

    # Remove dependency and restore
    rm -f "${dep_target}"
    ins game dependency restore "${game_name}"

    if [[ ! -f "${dep_target}" ]]; then
        echo "Dependency was not restored" >&2
        exit 1
    fi

    assert_file_equals "${dep_target}" "dependency content"

    # Test 5: Verify other files in directory are preserved during dependency restore
    if [[ ! -f "${other_file}" ]]; then
        echo "Other file was lost during dependency restore" >&2
        exit 1
    fi

    assert_file_equals "${other_file}" "other important data"

    # Test 6: Test restore to different location
    echo "Testing restore to different location..."
    local alternate_location="${TEST_ROOT}/alternate_restored"
    rm -rf "${alternate_location}"
    mkdir -p "${alternate_location}"

    # Remove original file first
    rm -f "${save_file}"

    # Restore to alternate location
    ins game restore --force "${game_name}" "${snapshot_id2}" --to "${alternate_location}"

    local expected_restored_file="${alternate_location}/save.dat"
    if [[ ! -f "${expected_restored_file}" ]]; then
        echo "File was not restored to alternate location" >&2
        exit 1
    fi

    assert_file_equals "${expected_restored_file}" "modified save data v2"

    # Test 7: Verify game functionality still works
    ins game restic snapshots >/dev/null
    ins game launch "${game_name}"
    ins game setup

    # Clean up
    ins game remove "${game_name}" --force

    # Ensure the game is no longer listed
    list_after_remove_json="$(ins --output json game list)"
    if echo "${list_after_remove_json}" | jq -e --arg name "${game_name}" '.data.games[].name == $name' >/dev/null; then
        echo "Game still present in list after removal" >&2
        exit 1
    fi

    echo "Single file game end-to-end flow succeeded"
}

main "$@"