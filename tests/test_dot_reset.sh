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
	echo "user change" >"${target_file}"

	ins dot reset "${target_file}"

	assert_file_equals "${target_file}" "test configuration content"
	echo "Dot reset restored the original content"

	# A tracked file whose target is missing is restored from the repo,
	# even when referenced by basename only (like `ins dot reset models.json`
	# from ~/.prime/agent).
	local missing_target="${HOME}/.config/instanttest/settings.conf"
	rm -f "${missing_target}"

	ins dot reset "settings.conf"
	assert_file_equals "${missing_target}" "another config file"
	echo "Dot reset restored a missing target by basename"

	rm -f "${missing_target}"
	ins dot reset "${missing_target}"
	assert_file_equals "${missing_target}" "another config file"
	echo "Dot reset restored a missing target by full path"
}

main "$@"
