#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
    if ! require_restic; then
        echo "Skipping single file basic test because restic is not installed."
        return 0
    fi
    setup_test_env
    trap cleanup_test_env EXIT

    local restic_repo="${TEST_ROOT}/restic_repo"
    local game_name="Test Single File Game"
    local save_dir="${HOME}/.local/share/test-single-game"
    local save_file="${save_dir}/save.dat"

    # Create directory with multiple files
    mkdir -p "${save_dir}"
    echo "initial save data" > "${save_file}"
    echo "other important data" > "${save_dir}/other_file.txt"

    # Initialize restic repository
    ins game init --repo "${restic_repo}" --password testpass

    # Add game with single file save
    ins game add \
        --name "${game_name}" \
        --description "Integration test game for single file saves" \
        --launch-command "echo Launching ${game_name}" \
        --save-path "${save_file}" \
        --create-save-path

    echo "✓ Game added successfully"

    # Try to backup
    ins game sync --force "${game_name}"

    echo "✓ Single file basic test passed"
}

main "$@"