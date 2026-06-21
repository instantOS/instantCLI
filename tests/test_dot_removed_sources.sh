#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

main() {
	setup_test_env
	trap cleanup_test_env EXIT

	local primary_repo="${TEST_ROOT}/primary-repo"
	create_sample_dot_repo "${primary_repo}" "primary"
	ins dot repo clone "file://${primary_repo}" --name primary >/dev/null

	local clean_target="${HOME}/.config/instanttest/config.txt"
	local modified_target="${HOME}/.config/instanttest/settings.conf"
	assert_path_exists "${clean_target}"
	assert_path_exists "${modified_target}"

	# A temporary working-tree deletion must not make `dot apply` destructive.
	local cloned_repo="${XDG_DATA_HOME}/instant/dots/primary"
	rm "${cloned_repo}/dots/.config/instanttest/config.txt"
	ins dot apply >/dev/null
	assert_file_equals "${clean_target}" "test configuration content"
	git -C "${cloned_repo}" restore dots/.config/instanttest/config.txt

	rm "${primary_repo}/dots/.config/instanttest/config.txt"
	(
		cd "${primary_repo}"
		git add -u
		git commit -qm "Remove clean dotfile"
	)

	local clean_update_output
	clean_update_output="$(ins dot update 2>&1)"
	if [[ -e "${clean_target}" ]]; then
		echo "Expected unmodified target to be removed after source deletion: ${clean_target}" >&2
		return 1
	fi
	assert_output_contains "${clean_update_output}" "Removed: ~/.config/instanttest/config.txt"

	echo "locally modified" >"${modified_target}"
	rm "${primary_repo}/dots/.config/instanttest/settings.conf"
	(
		cd "${primary_repo}"
		git add -u
		git commit -qm "Remove modified dotfile source"
	)

	local modified_update_output
	modified_update_output="$(ins dot update 2>&1)"
	assert_file_equals "${modified_target}" "locally modified"
	assert_output_contains "${modified_update_output}" "Preserved modified file: ~/.config/instanttest/settings.conf"

	# Once preserved, the target becomes unmanaged. Repeated apply operations
	# must not reconsider or delete it.
	ins dot apply >/dev/null
	assert_file_equals "${modified_target}" "locally modified"

	local fallback_high="${TEST_ROOT}/fallback-high"
	local fallback_low="${TEST_ROOT}/fallback-low"
	create_single_file_repo "${fallback_high}" "fallback-high" "high priority"
	create_single_file_repo "${fallback_low}" "fallback-low" "low priority"
	ins dot repo clone "file://${fallback_high}" --name fallback-high >/dev/null
	ins dot repo clone "file://${fallback_low}" --name fallback-low >/dev/null

	local fallback_target="${HOME}/.config/fallback/value.txt"
	assert_path_exists "${fallback_target}"

	rm "${fallback_high}/dots/.config/fallback/value.txt"
	(
		cd "${fallback_high}"
		git add -u
		git commit -qm "Remove high-priority provider"
	)

	ins dot update >/dev/null
	assert_file_equals "${fallback_target}" "low priority"

	echo "✓ Removed-source reconciliation passed"
}

create_single_file_repo() {
	local repo_dir="$1"
	local repo_name="$2"
	local content="$3"

	mkdir -p "${repo_dir}/dots/.config/fallback"
	(
		cd "${repo_dir}"
		git init -q
		git config user.email "tests@example.com"
		git config user.name "InstantCLI Tests"
	)
	echo "${content}" >"${repo_dir}/dots/.config/fallback/value.txt"
	cat >"${repo_dir}/instantdots.toml" <<EOF
name = "${repo_name}"
description = "Removed source test repository"
dots_dirs = ["dots"]
EOF
	(
		cd "${repo_dir}"
		git add .
		git commit -qm "Initial commit"
		git branch -m main
	)
}

main "$@"
