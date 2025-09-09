#!/bin/bash

# Shared utilities for InstantCLI test scripts
# This script provides common functions used across all test scripts

set -e

# Test environment setup
setup_test_env() {
    local test_dir="$1"
    
    # Create test directory structure
    export TEST_DIR="$test_dir"
    export CONFIG_FILE="$TEST_DIR/instant.toml"
    export DB_FILE="$TEST_DIR/instant.db"
    # TODO: instant always uses the real home directory, do not use a fake one
    # just make sure the tests do not modify dotfiles of real applications
    export HOME_DIR="$TEST_DIR/home"
    export REPO_DIR="$TEST_DIR/repo"
    export REPOS_DIR="$TEST_DIR/repos"
    
    # Create directories
    mkdir -p "$HOME_DIR"
    mkdir -p "$(dirname "$CONFIG_FILE")"
    mkdir -p "$(dirname "$DB_FILE")"
    mkdir -p "$REPO_DIR"
    mkdir -p "$REPOS_DIR"
    
    # Create initial config with custom repos_dir
    cat > "$CONFIG_FILE" << EOF
repos_dir = "$REPOS_DIR"
clone_depth = 1
EOF
    
    echo "Test environment created in: $TEST_DIR"
}

# Cleanup function
cleanup_test_env() {
    if [ -n "$TEST_DIR" ] && [ -d "$TEST_DIR" ]; then
        echo "Cleaning up test directory: $TEST_DIR"
        rm -rf "$TEST_DIR"
    fi
}


# Get InstantCLI directory - simple approach that finds the repo and compiles the binary
get_instant_dir() {
    local script_dir
    local instant_dir
    
    # Find the script location and navigate to repo root
    script_dir="$(cd "$(dirname "${BASH_SOURCE[1]}")" && pwd)"
    instant_dir="$(dirname "$script_dir")"  # from tests/scripts to tests
    instant_dir="$(dirname "$instant_dir")"  # from tests to repo root
    
    # Verify this is the InstantCLI repo
    if [ -f "$instant_dir/Cargo.toml" ] && grep -q "name = \"instant\"" "$instant_dir/Cargo.toml" 2>/dev/null; then
        echo "$instant_dir"
        return 0
    fi
    
    # Fallback: search upwards from current directory
    local current_dir
    current_dir="$(pwd)"
    
    while [ "$current_dir" != "/" ]; do
        if [ -f "$current_dir/Cargo.toml" ] && grep -q "name = \"instant\"" "$current_dir/Cargo.toml" 2>/dev/null; then
            echo "$current_dir"
            return 0
        fi
        current_dir="$(dirname "$current_dir")"
    done
    
    echo "ERROR: Could not find InstantCLI directory" >&2
    return 1
}

# Compile and get the absolute path to the instant binary
get_instant_binary() {
    local instant_dir
    instant_dir="$(get_instant_dir)"
    
    if [ "$instant_dir" = "ERROR: Could not find InstantCLI directory" ]; then
        echo "ERROR: Could not find InstantCLI directory" >&2
        return 1
    fi
    
    local binary_path="$instant_dir/target/debug/instant"
    
    # Compile if binary doesn't exist or is outdated
    if [ ! -f "$binary_path" ] || [ "$instant_dir/src/main.rs" -nt "$binary_path" ]; then
        if [ "$DEBUG" = "1" ]; then
            echo "Compiling InstantCLI..."
        fi
        (cd "$instant_dir" && cargo build --quiet)
    fi
    
    echo "$binary_path"
}

# Run InstantCLI command using the compiled binary
run_instant() {
    local binary_path
    binary_path="$(get_instant_binary)"
    
    if [ "$DEBUG" = "1" ]; then
        echo "Running: HOME=\"$HOME_DIR\" $binary_path --config \"$CONFIG_FILE\" --database \"$DB_FILE\" $@"
    fi
    
    HOME="$HOME_DIR" "$binary_path" --config "$CONFIG_FILE" --database "$DB_FILE" "$@"
}

# Create a simple test repository
create_test_repo() {
    local repo_dir="$1"
    local repo_name="${2:-test-repo}"
    
    cd "$repo_dir"
    git init >/dev/null 2>&1
    
    # Create basic dotfiles structure with made up applications
    mkdir -p dots/.config/instanttest
    echo "test configuration content" > dots/.config/instanttest/config.txt
    echo "another config file" > dots/.config/instanttest/settings.conf
    
    # Create instantdots.toml
    cat > instantdots.toml << EOF
name = "$repo_name"
description = "Test repository for InstantCLI"
EOF
    
    # Add and commit files
    git add . >/dev/null 2>&1
    git commit -m "Initial commit" >/dev/null 2>&1
    git branch -m main >/dev/null 2>&1
    
    echo "Test repository created in: $repo_dir"
}

# Verify file exists and show content
verify_file() {
    local file_path="$1"
    local expected_content="${2:-}"
    
    if [ -f "$file_path" ]; then
        echo "✓ File exists: $file_path"
        if [ -n "$expected_content" ]; then
            local actual_content
            actual_content=$(cat "$file_path")
            if [ "$actual_content" = "$expected_content" ]; then
                echo "✓ Content matches expected: $expected_content"
            else
                echo "⚠ Content differs. Expected: '$expected_content', Actual: '$actual_content'"
            fi
        else
            echo "Content: $(cat "$file_path")"
        fi
        return 0
    else
        echo "✗ File not found: $file_path"
        return 1
    fi
}

# Run command and check if it succeeds
run_cmd() {
    local cmd="$1"
    local description="${2:-Command}"
    
    echo "Running: $description"
    if eval "$cmd"; then
        echo "✓ $description succeeded"
        return 0
    else
        echo "✗ $description failed"
        return 1
    fi
}

# Print test header
print_test_header() {
    local test_name="$1"
    echo ""
    echo "=== $test_name ==="
}

# Print test result
print_test_result() {
    local result="$1"
    local message="${2:-Test}"
    
    if [ "$result" -eq 0 ]; then
        echo "✓ $message passed"
    else
        echo "✗ $message failed"
        return 1
    fi
}

# Trap cleanup on exit
trap cleanup_test_env EXIT

# Export functions
export -f setup_test_env cleanup_test_env run_instant get_instant_dir
export -f create_test_repo verify_file run_cmd print_test_header print_test_result
