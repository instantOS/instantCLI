#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
    if ! require_restic; then
        echo "Skipping game relocation test because restic is not installed."
        return 0
    fi


    setup_test_env
    trap cleanup_test_env EXIT

    local restic_repo="${TEST_ROOT}/restic_repo"
    local game_name="Relocation Game"
    local original_save_path="${HOME}/.local/share/reloc-game/saves"

    mkdir -p "${restic_repo}" "${original_save_path}"
    echo "initial save" > "${original_save_path}/save1.txt"

    instant game init --repo "${restic_repo}" --password testpass

    instant game add \
        --name "${game_name}" \
        --save-path "${original_save_path}" \
        --create-save-path

    instant game sync --force "${game_name}"

    local backup_output
    backup_output="$(instant_output game backup "${game_name}")"
    local snapshot_id
    snapshot_id="$(printf '%s\n' "${backup_output}" | sed -n 's/.*snapshot: \([0-9a-f]\{8,\}\).*/\1/p' | head -n1)"

    if [[ -z "${snapshot_id:-}" ]]; then
        echo "Failed to determine snapshot id from backup output" >&2
        exit 1
    fi

    local new_save_path="${HOME}/.local/share/reloc-game/new-saves"
    mkdir -p "${new_save_path}"

    cat > "${XDG_CONFIG_HOME}/instant/games/installations.toml" <<EOF
[[installations]]
game_name = "${game_name}"
save_path = "${new_save_path}"
nearest_checkpoint = "${snapshot_id}"
EOF

    if [[ -z "${new_save_path}" ]]; then
        echo "Error: new_save_path is empty. Aborting to prevent accidental deletion." >&2
        exit 1
    fi
    rm -rf -- "${new_save_path}"/*

    instant game restore --force "${game_name}" "${snapshot_id}"

    local relocated_expected="${new_save_path}/save1.txt"
    mapfile -t relocated_files < <(find "${new_save_path}" -type f -name 'save1.txt' | sort)
    if [[ "${#relocated_files[@]}" -ne 1 ]]; then
        echo "Expected exactly one restored save file after relocation, found ${#relocated_files[@]}" >&2
        printf 'Restored files:\n' >&2
        printf '  %s\n' "${relocated_files[@]}" >&2
        exit 1
    fi

    if [[ "${relocated_files[0]}" != "${relocated_expected}" ]]; then
        echo "Restore placed save at unexpected path after relocation" >&2
        echo "Expected: ${relocated_expected}" >&2
        echo "Actual:   ${relocated_files[0]}" >&2
        exit 1
    fi

    assert_file_equals "${relocated_expected}" "initial save"

    echo "Game restore relocation behavior succeeded"
}

main "$@"
