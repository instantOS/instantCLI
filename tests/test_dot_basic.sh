#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

# Helper function to assert JSON field value
assert_json_field() {
    local json="$1"
    local field="$2"
    local expected="$3"

    # Filter out warning lines and extract the last JSON line
    local clean_json
    clean_json="$(echo "${json}" | grep -v '^warning:' | tail -1)"

    local actual
    actual="$(echo "${clean_json}" | jq -r "${field}")"

    if [[ "${actual}" != "${expected}" ]]; then
        echo "Expected JSON field ${field} to be: ${expected}" >&2
        echo "Actual value: ${actual}" >&2
        echo "Full JSON output:" >&2
        echo "${json}" >&2
        return 1
    fi
}

# Helper function to assert JSON field contains value
assert_json_field_contains() {
    local json="$1"
    local field="$2"
    local expected="$3"

    local actual
    actual="$(echo "${json}" | jq -r "${field}")"

    if ! grep -Fq -- "${expected}" <<<"${actual}"; then
        echo "Expected JSON field ${field} to contain: ${expected}" >&2
        echo "Actual value: ${actual}" >&2
        echo "Full JSON output:" >&2
        echo "${json}" >&2
        return 1
    fi
}

main() {
    setup_test_env
    trap cleanup_test_env EXIT

    local repo_dir="${TEST_ROOT}/dotrepo"
    create_sample_dot_repo "${repo_dir}" "basic-test"

    local repo_url="file://${repo_dir}"

    # Test dot repo list JSON output (empty initially)
    echo "Testing empty repo list JSON output..."
    local empty_list_json
    empty_list_json="$(ins_output --output json dot repo list 2>/dev/null)"
    assert_json_field "${empty_list_json}" ".data.count" "0"
    assert_json_field "${empty_list_json}" ".data.repos" "[]"

    # Add repository
    echo "Adding repository..."
    ins dot repo add "${repo_url}" --name basic-test >/dev/null 2>&1

    # Test dot repo list JSON output (after adding repo)
    echo "Testing repo list JSON output after adding repo..."
    local repo_list_json
    repo_list_json="$(ins_output --output json dot repo list 2>/dev/null)"
    assert_json_field "${repo_list_json}" ".data.count" "1"
    assert_json_field "${repo_list_json}" ".data.repos[0].name" "basic-test"
    assert_json_field "${repo_list_json}" ".data.repos[0].enabled" "true"
    assert_json_field "${repo_list_json}" ".data.repos[0].active_subdirectories[0]" "dots"

    # Test dot status JSON output (before apply)
    echo "Testing dot status JSON output before apply..."
    local status_json
    status_json="$(ins_output --output json dot status 2>/dev/null)"
    assert_json_field "${status_json}" ".data.total_files" "2"
    assert_json_field "${status_json}" ".data.modified_count" "0"
    assert_json_field "${status_json}" ".data.outdated_count" "0"
    assert_json_field "${status_json}" ".data.clean_count" "2"  # Files are already applied (clean) due to auto-apply during 'dot repo add'

    # Apply dotfiles
    echo "Applying dotfiles..."
    ins dot apply >/dev/null 2>&1

    # Test dot status JSON output (after apply)
    echo "Testing dot status JSON output after apply..."
    local applied_status_json
    applied_status_json="$(ins_output --output json dot status 2>/dev/null)"
    assert_json_field "${applied_status_json}" ".data.total_files" "2"
    assert_json_field "${applied_status_json}" ".data.clean_count" "2"
    assert_json_field "${applied_status_json}" ".data.modified_count" "0"
    assert_json_field "${applied_status_json}" ".data.outdated_count" "0"

    # Verify actual file contents
    echo "Verifying file contents..."
    assert_file_equals "${HOME}/.config/instanttest/config.txt" "test configuration content"
    assert_file_equals "${HOME}/.config/instanttest/settings.conf" "another config file"

    # Test individual file status JSON output
    echo "Testing individual file status JSON output..."
    local file_status_json
    file_status_json="$(ins_output --output json dot status .config/instanttest/config.txt 2>/dev/null)"
    assert_json_field "${file_status_json}" ".data.tracked" "true"
    assert_json_field "${file_status_json}" ".data.repo" "basic-test"
    assert_json_field "${file_status_json}" ".data.dotfile_dir" "dots"

    echo "✓ All JSON output tests passed"
    echo "✓ Basic dot apply flow succeeded"
}

main "$@"
