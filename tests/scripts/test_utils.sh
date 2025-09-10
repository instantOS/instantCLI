#!/bin/bash

# Shared utilities for InstantCLI test scripts
# This script provides common functions used across all test scripts

set -e

# Global flag to track if we're in a test wrapper
export IN_TEST_WRAPPER=""

# Test environment setup
setup_test_env() {
    local test_dir="$1"
    
    # Create test directory structure
    export TEST_DIR="$test_dir"
    export HOME_DIR="$TEST_DIR/home"
    export REPO_DIR="$TEST_DIR/repo"
    
    # Create directories
    mkdir -p "$HOME_DIR"
    mkdir -p "$REPO_DIR"
    
    echo "Test environment created in: $TEST_DIR"
}

# Ensure test is running within wrapper - exit if not
ensure_test_wrapper() {
    if [ -z "$IN_TEST_WRAPPER" ]; then
        echo "ERROR: Test must be run within run_with_test_home wrapper" >&2
        exit 1
    fi
}

# Run command with test home directory wrapper
run_with_test_home() {
    local cmd="$1"
    shift
    
    # Set the wrapper flag
    local old_wrapper="$IN_TEST_WRAPPER"
    export IN_TEST_WRAPPER="1"
    export HOME="$HOME_DIR"
    
    # Run the command
    if [ "$DEBUG" = "1" ]; then
        echo "Running with HOME=$HOME_DIR: $cmd $@"
    fi
    
    # Execute command and capture output
    local output
    output=$("$cmd" "$@")
    local exit_code=$?
    
    # Restore wrapper flag
    export IN_TEST_WRAPPER="$old_wrapper"
    export HOME="$old_wrapper"
    
    # Always show output for review
    echo "$output"
    
    return $exit_code
}

# Cleanup function
cleanup_test_env() {
    if [ -n "$TEST_DIR" ] && [ -d "$TEST_DIR" ]; then
        echo "Cleaning up test directory: $TEST_DIR"
        rm -rf "$TEST_DIR"
    fi
}


# Get InstantCLI binary path
get_instant_binary() {
    local script_dir
    local instant_dir
    
    # Find the script location and navigate to repo root
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    instant_dir="$(dirname "$script_dir")"  # from tests/scripts to tests
    instant_dir="$(dirname "$instant_dir")"  # from tests to repo root
    
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

# Run InstantCLI command using the wrapper
run_instant() {
    ensure_test_wrapper
    local binary_path
    binary_path="$(get_instant_binary)"
    run_with_test_home "$binary_path" "$@"
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

# Output verification utilities
check_output_contains() {
    local output="$1"
    local keyword="$2"
    local description="${3:-Check if output contains keyword}"
    
    echo "Checking: $description"
    echo "Looking for: '$keyword' in output"
    
    if echo "$output" | grep -q "$keyword"; then
        echo "✓ Found keyword: '$keyword'"
        return 0
    else
        echo "✗ Keyword not found: '$keyword'"
        echo "Output was:"
        echo "$output"
        return 1
    fi
}

check_output_not_contains() {
    local output="$1"
    local keyword="$2"
    local description="${3:-Check if output does not contain keyword}"
    
    echo "Checking: $description"
    echo "Looking for absence of: '$keyword' in output"
    
    if echo "$output" | grep -q "$keyword"; then
        echo "✗ Found unwanted keyword: '$keyword'"
        echo "Output was:"
        echo "$output"
        return 1
    else
        echo "✓ Keyword not found (as expected): '$keyword'"
        return 0
    fi
}

check_files_different() {
    local file1="$1"
    local file2="$2"
    local description="${3:-Check if files are different}"
    
    echo "Checking: $description"
    
    if [ ! -f "$file1" ]; then
        echo "✗ File not found: $file1"
        return 1
    fi
    
    if [ ! -f "$file2" ]; then
        echo "✗ File not found: $file2"
        return 1
    fi
    
    if cmp -s "$file1" "$file2"; then
        echo "✗ Files are identical: $file1 and $file2"
        return 1
    else
        echo "✓ Files are different: $file1 and $file2"
        return 0
    fi
}

check_files_same() {
    local file1="$1"
    local file2="$2"
    local description="${3:-Check if files are the same}"
    
    echo "Checking: $description"
    
    if [ ! -f "$file1" ]; then
        echo "✗ File not found: $file1"
        return 1
    fi
    
    if [ ! -f "$file2" ]; then
        echo "✗ File not found: $file2"
        return 1
    fi
    
    if cmp -s "$file1" "$file2"; then
        echo "✓ Files are identical: $file1 and $file2"
        return 0
    else
        echo "✗ Files are different: $file1 and $file2"
        return 1
    fi
}

# Run command and capture output for verification
run_and_capture() {
    local cmd="$1"
    local description="${2:-Command}"
    
    echo "Running: $description"
    if [ "$DEBUG" = "1" ]; then
        echo "Command: $cmd"
    fi
    
    local output
    output=$(eval "$cmd")
    local exit_code=$?
    
    echo "Output:"
    echo "$output"
    echo ""
    
    if [ $exit_code -eq 0 ]; then
        echo "✓ $description succeeded"
    else
        echo "✗ $description failed (exit code: $exit_code)"
    fi
    
    # Return both exit code and output for further processing
    return $exit_code
}

# Export functions
export setup_test_env run_instant create_test_repo verify_file
export ensure_test_wrapper run_with_test_home
export check_output_contains check_output_not_contains
export check_files_different check_files_same run_and_capture
