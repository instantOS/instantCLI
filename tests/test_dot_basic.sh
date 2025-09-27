#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
    setup_test_env
    trap cleanup_test_env EXIT

    local repo_dir="${TEST_ROOT}/dotrepo"
    create_sample_dot_repo "${repo_dir}" "basic-test"

    local repo_url="file://${repo_dir}"

    instant dot repo add "${repo_url}" --name basic-test
    instant dot status
    instant dot apply

    assert_file_equals "${HOME}/.config/instanttest/config.txt" "test configuration content"
    assert_file_equals "${HOME}/.config/instanttest/settings.conf" "another config file"

    echo "Basic dot apply flow succeeded"
}

main "$@"
