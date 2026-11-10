#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"
prepare_ins_binary
setup_test_env
trap cleanup_test_env EXIT

mkdir -p "${TEST_ROOT}/bin"
export FLATPAK_TEST_ROOT="${TEST_ROOT}"
export PATH="${TEST_ROOT}/bin:${PATH}"

cat >"${TEST_ROOT}/bin/flatpak" <<'PY'
#!/usr/bin/env python3
import json
import os
import pathlib
import sys

root = pathlib.Path(os.environ['FLATPAK_TEST_ROOT'])
args = sys.argv[1:]
if args[0] == 'list':
    print('org.example.One\tFirst App\t1.0\tflathub\t1 MB')
    print('org.example.Two\tSecond App\t2.0\tflathub\t2 MB')
    # The same ID can appear in more than one installation.
    print('org.example.One\tFirst App\t1.0\tflathub\t1 MB')
else:
    assert args[0] in ('uninstall', 'run'), args
    with (root / 'calls').open('a') as log:
        log.write(json.dumps(args) + '\n')
    failed = root / 'failed'
    if os.environ['FLATPAK_TEST_SCENARIO'] in ('retry', 'quit') and not failed.exists():
        failed.touch()
        print('Simulated Flatpak transaction failure', file=sys.stderr)
        sys.exit(1)
PY

cat >"${TEST_ROOT}/bin/fzf" <<'PY'
#!/usr/bin/env python3
import json
import os
import pathlib
import sys

root = pathlib.Path(os.environ['FLATPAK_TEST_ROOT'])
scenario = os.environ['FLATPAK_TEST_SCENARIO']
args = sys.argv[1:]
text = sys.stdin.read()
header = args[args.index('--header') + 1] if '--header' in args else ''
with (root / 'dialogs').open('a') as log:
    log.write(json.dumps(header) + '\n')
if '--read0' not in args:
    # Test the real streaming selector, including enabling multi-select.
    assert '--multi' in args, args
    selected = root / 'selected'
    if selected.exists() or scenario == 'escape':
        sys.exit(130)
    selected.touch()
    rows = text.splitlines()
    assert len(rows) == 3, rows
    if scenario == 'empty':
        sys.exit(0)
    print('\n'.join(rows[:1] if scenario.startswith('single') else rows))
elif 'Uninstall ' in header:
    assert 'org.example.One' in header, header
    if not scenario.startswith('single'):
        assert 'Uninstall 2 Flatpak packages?' in header, header
        assert 'org.example.Two' in header, header
    if scenario == 'cancel_escape':
        sys.exit(130)
    print('submit\n' + ('1' if scenario == 'cancel_no' else '0'))
elif 'could not complete the operation' in header:
    assert scenario in ('retry', 'quit'), header
    print('submit\n' + ('1' if scenario == 'quit' else '0'))
else:
    assert scenario in ('single_run', 'single_uninstall', 'single_cancel'), header
    if scenario == 'single_cancel':
        sys.exit(130)
    print('submit\n' + ('0' if scenario == 'single_run' else '1'))
PY
chmod +x "${TEST_ROOT}/bin/flatpak" "${TEST_ROOT}/bin/fzf"

for scenario in batch cancel_no cancel_escape escape empty retry quit single_run single_uninstall single_cancel; do
	export FLATPAK_TEST_SCENARIO="${scenario}"
	rm -f "${TEST_ROOT}/calls" "${TEST_ROOT}/dialogs" "${TEST_ROOT}/selected" "${TEST_ROOT}/failed"
	ins settings internal-apply --setting-id system.manage_installed_flatpaks >"${TEST_ROOT}/output" 2>&1
	python3 - <<'PY'
import json
import os
import pathlib

root = pathlib.Path(os.environ['FLATPAK_TEST_ROOT'])
scenario = os.environ['FLATPAK_TEST_SCENARIO']
path = root / 'calls'
calls = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
batch = ['uninstall', '-y', 'org.example.One', 'org.example.Two']
expected = {
    'batch': [batch],
    'retry': [batch, batch],
    'quit': [batch],
    'single_run': [['run', 'org.example.One']],
    'single_uninstall': [['uninstall', '-y', 'org.example.One']],
}.get(scenario, [])
assert calls == expected, (scenario, calls, expected)
headers = [json.loads(line) for line in (root / 'dialogs').read_text().splitlines()]
confirmations = [header for header in headers if 'Uninstall ' in header]
assert len(confirmations) == (1 if scenario in ('batch', 'retry', 'quit', 'cancel_no', 'cancel_escape', 'single_uninstall') else 0), (scenario, headers)
PY
done

printf 'Flatpak multi-select, confirmation, retry and single-app actions passed\n'
