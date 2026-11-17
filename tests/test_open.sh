#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helpers.sh"
prepare_ins_binary
setup_test_env
trap cleanup_test_env EXIT
export INS_BIN
python3 "${SCRIPT_DIR}/test_open.py"
