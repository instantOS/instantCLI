#!/usr/bin/env bash
set -euo pipefail

# Shared helpers for shell-based end-to-end tests.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

setup_test_env() {
    TEST_ROOT="$(mktemp -d)"
    export TEST_ROOT

    export HOME="${TEST_ROOT}/home"
    export XDG_CONFIG_HOME="${HOME}/.config"
    export XDG_DATA_HOME="${HOME}/.local/share"
    export XDG_CACHE_HOME="${HOME}/.cache"

    mkdir -p "${HOME}" "${XDG_CONFIG_HOME}" "${XDG_DATA_HOME}" "${XDG_CACHE_HOME}"

    mkdir -p "${XDG_CONFIG_HOME}/instant"
    cat > "${XDG_CONFIG_HOME}/instant/instant.toml" <<'EOF'
clone_depth = 0
EOF
}

cleanup_test_env() {
    if [[ -n "${TEST_ROOT:-}" && -d "${TEST_ROOT}" ]]; then
        rm -rf "${TEST_ROOT}"
    fi
}

prepare_ins_binary() {
    if [[ -n "${INS_PREPARED:-}" && -n "${INS_BIN:-}" && -x "${INS_BIN}" ]]; then
        return
    fi

    echo "Compiling ins (debug) for tests..."
    cd "${REPO_ROOT}"

    # Optimizations for faster compilation in tests
    export CARGO_INCREMENTAL=1
    export CARGO_BUILD_JOBS=$(nproc)  # Use all available cores

    # Only build the ins binary, skip all other dependencies and examples
    cargo build --bin ins --message-format=human

    export INS_BIN="${REPO_ROOT}/target/debug/ins"
    export INS_PREPARED=1
}

ins() {
    if [[ -z "${INS_BIN:-}" || ! -x "${INS_BIN}" ]]; then
        prepare_ins_binary
    fi
    "${INS_BIN}" "$@"
}

ins_output() {
    if [[ -z "${INS_BIN:-}" || ! -x "${INS_BIN}" ]]; then
        prepare_ins_binary
    fi
    "${INS_BIN}" "$@"
}

create_sample_dot_repo() {
    local repo_dir="$1"
    local repo_name="${2:-sample-dot-repo}"

    mkdir -p "${repo_dir}"
    (cd "${repo_dir}" && git init -q)

    (cd "${repo_dir}" && git config user.email "tests@example.com" && git config user.name "InstantCLI Tests")

    mkdir -p "${repo_dir}/dots/.config/instanttest"
    cat > "${repo_dir}/dots/.config/instanttest/config.txt" <<'EOF'
test configuration content
EOF
    cat > "${repo_dir}/dots/.config/instanttest/settings.conf" <<'EOF'
another config file
EOF

    cat > "${repo_dir}/instantdots.toml" <<EOF
name = "${repo_name}"
description = "Sample repository for tests"
EOF

    (cd "${repo_dir}" && git add . >/dev/null && git commit -qm "Initial commit" && git branch -m main >/dev/null)
}

assert_path_exists() {
    local path="$1"
    if [[ ! -e "${path}" ]]; then
        echo "Expected path to exist: ${path}" >&2
        return 1
    fi
}

assert_file_equals() {
    local path="$1"
    local expected="$2"

    assert_path_exists "${path}"

    local actual
    actual="$(cat "${path}")"
    if [[ "${actual}" != "${expected}" ]]; then
        echo "Unexpected file contents for ${path}" >&2
        echo "Expected: ${expected}" >&2
        echo "Actual:   ${actual}" >&2
        return 1
    fi
}

assert_output_contains() {
    local output="$1"
    local needle="$2"

    if ! grep -Fq -- "${needle}" <<<"${output}"; then
        echo "Expected output to contain: ${needle}" >&2
        echo "Actual output:" >&2
        echo "${output}" >&2
        return 1
    fi
}

require_restic() {
    if ! command -v restic >/dev/null 2>&1; then
        echo "restic is required for this test" >&2
        return 1
    fi
    return 0
}
