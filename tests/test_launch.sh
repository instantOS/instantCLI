#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"
prepare_ins_binary
setup_test_env
trap cleanup_test_env EXIT

bin_dir="${TEST_ROOT}/bin"
mkdir -p "${bin_dir}" "${XDG_DATA_HOME}/applications" "${TEST_ROOT}/system"
export XDG_DATA_DIRS="${TEST_ROOT}/system"
export MENU_ITEMS="${TEST_ROOT}/menu-items"
export MENU_ARGS="${TEST_ROOT}/menu-args"
export LAUNCH_RECORD="${TEST_ROOT}/launched"
export TERMINAL_RECORD="${TEST_ROOT}/terminal-args"
export TERMINAL="${bin_dir}/foot"

cat >"${bin_dir}/instantmenu" <<'MENU'
#!/bin/sh
printf '%s\n' "$@" > "$MENU_ARGS"
/bin/cat > "$MENU_ITEMS"
printf '%s\n' "$MENU_SELECTION"
MENU
cat >"${bin_dir}/desktop-program" <<'APP'
#!/bin/sh
printf launched > "$LAUNCH_RECORD"
APP
cat >"${bin_dir}/foot" <<'TERMINAL'
#!/bin/sh
printf '%s\n' "$@" > "$TERMINAL_RECORD.tmp"
/bin/mv "$TERMINAL_RECORD.tmp" "$TERMINAL_RECORD"
TERMINAL
cat >"${bin_dir}/fzf" <<'FZF'
#!/bin/sh
printf '%s\n' "$@" > "$MENU_ARGS"
/bin/cat > "$MENU_ITEMS"
exit 130
FZF
cp "${bin_dir}/desktop-program" "${bin_dir}/tui-program"
chmod +x "${bin_dir}/instantmenu" "${bin_dir}/desktop-program" "${bin_dir}/tui-program" "${bin_dir}/foot" "${bin_dir}/fzf"

cat >"${XDG_DATA_HOME}/applications/demo.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Desktop Demo
Exec=${bin_dir}/desktop-program
DESKTOP
cat >"${XDG_DATA_HOME}/applications/tui.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Terminal Demo
Terminal=true
Exec=${bin_dir}/tui-program "argument with spaces"
DESKTOP

wait_for_record() {
	local path="$1"
	for _ in {1..100}; do
		[[ -s "$path" ]] && return
		sleep 0.01
	done
	printf 'Timed out waiting for %s\n' "$path" >&2
	return 1
}

# Default mode lists desktop entries, forwards prefill, and executes selection.
env PATH="${bin_dir}" MENU_SELECTION=desktop:demo.desktop "${INS_BIN}" launch --backend instantmenu --query 'Demo query'
wait_for_record "${LAUNCH_RECORD}"
[[ "$(cat "${LAUNCH_RECORD}")" = launched ]]
grep -Fq '{value="desktop:demo.desktop"}' "${MENU_ITEMS}"
! grep -Fq '{value="path:' "${MENU_ITEMS}"
grep -Fxq 'Demo query' "${MENU_ARGS}"

# Terminal=true desktop entries go through the shared terminal launcher.
env PATH="${bin_dir}" MENU_SELECTION=desktop:tui.desktop "${INS_BIN}" launch --backend instantmenu
wait_for_record "${TERMINAL_RECORD}"
[[ "$(cat "${TERMINAL_RECORD}")" = "$(printf '%s\n' -- "${bin_dir}/tui-program" 'argument with spaces')" ]]

# Explicit terminal mode lists PATH commands without desktop entries.
: >"${TERMINAL_RECORD}"
env PATH="${bin_dir}" MENU_SELECTION=path:tui-program "${INS_BIN}" launch --terminal --backend instantmenu
wait_for_record "${TERMINAL_RECORD}"
[[ "$(cat "${TERMINAL_RECORD}")" = $'--\ntui-program' ]]
! grep -Fq '{value="desktop:' "${MENU_ITEMS}"
grep -Fxq launch-terminal "${MENU_ARGS}"

# Mixed mode includes both types. Cancellation executes neither.
printf unchanged >"${LAUNCH_RECORD}"
status=0
env PATH="${bin_dir}" MENU_SELECTION= "${INS_BIN}" launch --include-path --backend instantmenu || status=$?
[[ "$status" = 1 ]]
[[ "$(cat "${LAUNCH_RECORD}")" = unchanged ]]
grep -Fq '{value="path:tui-program"}' "${MENU_ITEMS}"
grep -Fq '{value="desktop:demo.desktop"}' "${MENU_ITEMS}"

# TUI choices must pass the query to the renderer, including spaces.
status=0
env PATH="${bin_dir}" "${INS_BIN}" launch --backend tui -q 'Demo query' || status=$?
[[ "$status" = 1 ]]
grep -Fxq -- '--query' "${MENU_ARGS}"
grep -Fxq 'Demo query' "${MENU_ARGS}"

# Default setup links the installed application menu.
for name in kitty yazi nautilus firefox neovide appmenu ilock missioncenter; do
	cp "${bin_dir}/desktop-program" "${bin_dir}/${name}"
done
env PATH="${bin_dir}" "${INS_BIN}" settings ensure-defaults
[[ "$(readlink "${HOME}/.config/instantos/default/appmenu")" = "${bin_dir}/appmenu" ]]
printf 'Application launcher integration checks passed\n'
