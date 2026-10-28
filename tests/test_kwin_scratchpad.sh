#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"
prepare_ins_binary
setup_test_env
trap cleanup_test_env EXIT
export INS_BIN

# All compositor commands and terminals are stubs; this never contacts KWin.
python3 - <<'PY'
import json, os, pathlib, subprocess, time

root = pathlib.Path(os.environ['TEST_ROOT'])
bin_dir = root / 'bin'
bin_dir.mkdir()
scripts = {
    'dbus-send': '''#!/bin/sh
case "$*" in
    *loadScript*)
        for argument in "$@"; do
            case "$argument" in
                string:*)
                    while IFS= read -r line; do
                        case "$line" in
                            *callDBus*) : > "$KWIN_QUERY_MARKER" ;;
                        esac
                    done < "${argument#string:}"
                    ;;
            esac
        done
        printf 'method return\\n   int32 1\\n'
        ;;
    *Script.run*)
        if [ "$KWIN_FAIL_QUERY" = 1 ] && [ -f "$KWIN_QUERY_MARKER" ]; then
            echo 'Simulated KWin script failure' >&2
            exit 1
        fi
        ;;
esac
''',
    'dbus-monitor': '''#!/bin/sh
printf '%s' "$$" > "$KWIN_MONITOR_PID"
if [ "$KWIN_SILENT" != 1 ]; then
    printf '   string "%s"\\n' "$KWIN_RESPONSE"
fi
exec /bin/sleep 10
''',
    'audit-terminal': '''#!/bin/sh
printf launched > "$KWIN_TERMINAL_MARKER"
''',
}
for name, contents in scripts.items():
    path = bin_dir / name
    path.write_text(contents)
    path.chmod(0o755)

base_env = dict(os.environ, PATH=str(bin_dir), XDG_SESSION_DESKTOP='kde',
                KWIN_MONITOR_PID=str(root / 'monitor-pid'),
                KWIN_QUERY_MARKER=str(root / 'query'),
                KWIN_TERMINAL_MARKER=str(root / 'terminal'))
base_env.pop('INSTANTWM', None)
existing = json.dumps([dict(name='instantscratchpad',
                           **{'class': 'scratchpad_instantscratchpad'},
                           title='Notes', visible=True)])
for name, command, response, silent, failed, expected_status, launches in [
    ('first show', 'show', '[]', False, False, 0, True),
    ('first toggle', 'toggle', '[]', False, False, 0, True),
    ('existing window', 'show', existing, False, False, 0, False),
    ('silent monitor', 'show', '', True, False, 1, False),
    ('failed status script', 'show', '[]', False, True, 1, False),
]:
    for filename in ('monitor-pid', 'query', 'terminal'):
        (root / filename).unlink(missing_ok=True)
    env = dict(base_env, KWIN_RESPONSE=response, KWIN_SILENT=str(int(silent)),
               KWIN_FAIL_QUERY=str(int(failed)))
    started = time.monotonic()
    result = subprocess.run([os.environ['INS_BIN'], '--no-color', 'scratchpad', command,
                             '--terminal', str(bin_dir / 'audit-terminal')],
                            env=env, capture_output=True, text=True, timeout=4)
    assert result.returncode == expected_status, (name, result)
    assert (root / 'terminal').exists() == launches, (name, result)
    assert time.monotonic() - started < 3, name
    pid = int((root / 'monitor-pid').read_text())
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        pass
    else:
        raise AssertionError(f'{name}: monitor {pid} was not reaped')
    if silent:
        assert 'Timeout waiting for KWin status' in result.stderr, result
    if failed:
        assert 'Simulated KWin script failure' in result.stderr, result
print('KWin scratchpad creation, status timeout, and monitor cleanup passed')
PY
