#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"

# These checks exercise test infrastructure without building or invoking ins.
test_session_isolation() (
	# Environment-only check: explicitly provide a harmless prepared binary.
	export INS_BIN=/bin/true INS_PREPARED=1
	local parent_root
	parent_root="$(mktemp -d)"
	trap 'rm -rf "${parent_root}"' EXIT
	mkdir -p "${parent_root}/hooks"
	cat >"${parent_root}/hooks/pre-commit" <<EOF_HOOK
#!/bin/sh
touch '${parent_root}/host-hook-ran'
EOF_HOOK
	chmod +x "${parent_root}/hooks/pre-commit"
	printf '[core]\n\thooksPath = %s\n' "${parent_root}/hooks" >"${parent_root}/gitconfig"
	export GIT_CONFIG_GLOBAL="${parent_root}/gitconfig"
	export XDG_STATE_HOME="${parent_root}/host-state" XDG_RUNTIME_DIR="${parent_root}/host-runtime"
	export INS_PREVIEW_CACHE_DIR="${parent_root}/host-cache" INS_GAME_GUI=1
	export DISPLAY=:test WAYLAND_DISPLAY=test INSTANTWM=1
	export DBUS_SESSION_BUS_ADDRESS=autolaunch: DBUS_SYSTEM_BUS_ADDRESS=unix:path=/host-bus

	setup_test_env
	trap 'cleanup_test_env; rm -rf "${parent_root}"' EXIT
	[[ -z "${DISPLAY+x}${WAYLAND_DISPLAY+x}${INSTANTWM+x}" ]]
	[[ "${INS_GAME_GUI}" = 0 ]]
	[[ "$(stat -c %a "${XDG_RUNTIME_DIR}")" = 700 ]]
	for path in "${HOME}" "${XDG_CONFIG_HOME}" "${XDG_DATA_HOME}" "${XDG_CACHE_HOME}" \
		"${XDG_STATE_HOME}" "${XDG_RUNTIME_DIR}" "${INS_PREVIEW_CACHE_DIR}" \
		"${RESTIC_CACHE_DIR}" "${GNUPGHOME}" "${PASSWORD_STORE_DIR}" "${TMPDIR}"; do
		[[ "${path}" = "${TEST_ROOT}/"* ]]
	done
	[[ "${DBUS_SESSION_BUS_ADDRESS}" = "unix:path=${TEST_ROOT}/"* ]]
	[[ "${DBUS_SYSTEM_BUS_ADDRESS}" = "unix:path=${TEST_ROOT}/"* ]]

	# A real commit must not execute the inherited global hook.
	create_sample_dot_repo "${TEST_ROOT}/repo"
	[[ ! -e "${parent_root}/host-hook-ran" ]]
	[[ ! -e "${parent_root}/host-state" && ! -e "${parent_root}/host-runtime" && ! -e "${parent_root}/host-cache" ]]
	local root="${TEST_ROOT}"
	cleanup_test_env
	[[ ! -e "${root}" ]]
)

test_failed_build_does_not_prepare_a_stale_binary() (
	unset INS_PREPARED TEST_ROOT
	INS_BIN=/bin/true
	cargo() { return 23; }
	if setup_test_env; then
		echo 'A failed build was accepted' >&2
		return 1
	fi
	[[ -z "${INS_BIN:-}${INS_PREPARED:-}${TEST_ROOT:-}" ]]
)

test_preparation_uses_the_executable_reported_by_cargo() (
	unset INS_BIN INS_PREPARED
	local target_dir
	target_dir="$(mktemp -d)"
	trap 'rm -rf "${target_dir}"' EXIT
	export CARGO_TARGET_DIR="${target_dir}/custom target"
	local executable="${CARGO_TARGET_DIR}/debug/ins"
	mkdir -p "${CARGO_TARGET_DIR}/debug"
	ln -s /bin/true "${executable}"
	cargo() {
		jq -nc --arg executable "${executable}" \
			'{reason: "compiler-artifact", target: {name: "ins"}, executable: $executable}'
		printf '%s\n' '{"reason":"build-finished","success":true}'
	}

	prepare_ins_binary
	[[ "${INS_BIN}" = "${executable}" && "${INS_PREPARED}" = 1 ]]
)

test_session_isolation
test_failed_build_does_not_prepare_a_stale_binary
test_preparation_uses_the_executable_reported_by_cargo
printf 'Test environment isolation and binary preparation checks passed\n'
