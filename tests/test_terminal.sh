#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"
prepare_ins_binary
setup_test_env
trap cleanup_test_env EXIT

mkdir -p "${TEST_ROOT}/bin" "${HOME}/.config/instantos/default"
export TERMINAL_RECORD="${TEST_ROOT}/terminal-argv"
unset TERMINAL
cat >"${TEST_ROOT}/bin/foot" <<'TERMINAL_SCRIPT'
#!/bin/sh
printf '%s\n' "$@" > "$TERMINAL_RECORD"
exit "${TERMINAL_STATUS:-0}"
TERMINAL_SCRIPT
chmod +x "${TEST_ROOT}/bin/foot"
cp "${TEST_ROOT}/bin/foot" "${TEST_ROOT}/bin/alacritty"
alias_path="${HOME}/.config/instantos/default/terminal"
ln -s "${TEST_ROOT}/bin/foot" "${alias_path}"

# Exercise both early dispatch and normal CLI dispatch with a global option.
for prefix in early normal; do
	args=(terminal --app-id scratchpad_notes --title 'Notes with spaces' -- printf 'hello world')
	if [[ "${prefix}" = normal ]]; then args=(--no-color "${args[@]}"); fi
	env PATH="${TEST_ROOT}/bin" WAYLAND_DISPLAY=test "${INS_BIN}" "${args[@]}"
	expected=$'--app-id\nscratchpad_notes\n--title\nNotes with spaces\n--\nprintf\nhello world'
	[[ "$(cat "${TERMINAL_RECORD}")" = "${expected}" ]]
done

# A settings change takes effect on the next invocation, including its dialect.
ln -sf "${TEST_ROOT}/bin/alacritty" "${alias_path}"
"${INS_BIN}" terminal --app-id notes -- printf 'hello world'
[[ "$(cat "${TERMINAL_RECORD}")" = $'--class\nnotes\n-e\nprintf\nhello world' ]]

# A dangling preference falls back, without modifying the saved preference.
ln -sf "${TEST_ROOT}/absent" "${alias_path}"
env PATH="${TEST_ROOT}/bin" WAYLAND_DISPLAY=test "${INS_BIN}" terminal -- printf fallback
[[ "$(cat "${TERMINAL_RECORD}")" = $'--\nprintf\nfallback' ]]
[[ "$(readlink "${alias_path}")" = "${TEST_ROOT}/absent" ]]

# Explicit setup owns persistent repair.
env PATH="${TEST_ROOT}/bin" WAYLAND_DISPLAY=test "${INS_BIN}" terminal --ensure-default
[[ "$(readlink "${alias_path}")" = "${TEST_ROOT}/bin/foot" ]]

status=0
env PATH="${TEST_ROOT}/bin" WAYLAND_DISPLAY=test TERMINAL_STATUS=127 "${INS_BIN}" terminal || status=$?
[[ "${status}" = 127 ]]
status=0
"${INS_BIN}" terminal --terminal "${TEST_ROOT}/absent" 2>/dev/null || status=$?
[[ "${status}" = 127 ]]
printf 'Terminal launch integration checks passed\n'
