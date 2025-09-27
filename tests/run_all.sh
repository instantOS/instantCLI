#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "${SCRIPT_DIR}/helpers.sh"
prepare_ins_binary

export INS_BIN
export INS_PREPARED


status=0

for test_script in "${SCRIPT_DIR}"/test_*.sh; do
    [[ -x "${test_script}" ]] || continue
    echo "=== Running $(basename "${test_script}") ==="
    if ! "${test_script}"; then
        echo "✗ $(basename "${test_script}") failed"
        status=1
    else
        echo "✓ $(basename "${test_script}") passed"
    fi
    echo
done

exit "${status}"
