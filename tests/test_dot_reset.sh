#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
    setup_test_env
    trap cleanup_test_env EXIT

    local repo_dir="${TEST_ROOT}/dotrepo"
    create_sample_dot_repo "${repo_dir}" "reset-test"

    local repo_url="file://${repo_dir}"

    ins dot repo clone "${repo_url}" --name reset-test
    ins dot apply

    local target_file="${HOME}/.config/instanttest/config.txt"
    echo "user change" > "${target_file}"

    ins dot reset "${target_file}"

    assert_file_equals "${target_file}" "test configuration content"
    echo "Dot reset restored the original content"
}

main "$@"
