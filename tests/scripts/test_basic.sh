#!/bin/bash
set -e

# Test basic functionality: clone, apply, status
# This script tests the core workflow of InstantCLI

echo "=== Testing Basic InstantCLI Functionality ==="

# Setup test environment
TEST_DIR="/tmp/instant-test-$$"
CONFIG_FILE="$TEST_DIR/instant.toml"
DB_FILE="$TEST_DIR/instant.db"
HOME_DIR="$TEST_DIR/home"
REPO_DIR="$TEST_DIR/repo"

# Cleanup function
cleanup() {
    echo "Cleaning up test directory: $TEST_DIR"
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# Create test environment
echo "Setting up test environment..."
mkdir -p "$HOME_DIR"
mkdir -p "$(dirname "$CONFIG_FILE")"
mkdir -p "$(dirname "$DB_FILE")"
mkdir -p "$REPO_DIR"
REPOS_DIR="$TEST_DIR/repos"
mkdir -p "$REPOS_DIR"

# Create initial config with custom repos_dir
cat > "$CONFIG_FILE" << EOF
repos_dir = "$REPOS_DIR"
clone_depth = 1
EOF

# Test functions
# TODO: stuff like this should be shared between test scripts using a utils script which gets sourced by them
# Same goes for creating test dotfile repos
run_instant() {
    local instant_dir="$1"
    shift
    if [ "$DEBUG" = "1" ]; then
        echo "Running: cd \"$instant_dir\" && cargo run -- --config \"$CONFIG_FILE\" --database \"$DB_FILE\" $@"
    fi
    cd "$instant_dir" && cargo run -- --config "$CONFIG_FILE" --database "$DB_FILE" "$@"
}

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTANT_DIR="$(dirname "$SCRIPT_DIR")"

# Create a simple test repository
echo "Creating test repository..."
cd "$REPO_DIR"
git init
mkdir -p dots/.config/test-app
echo "test configuration content" > dots/.config/test-app/config.txt
echo "# Test shell configuration" > dots/.bashrc

# Create instantdots.toml
cat > instantdots.toml << EOF
name = "test-basic-repo"
description = "Basic test repository"
EOF

# Add and commit files
git add .
git commit -m "Initial commit"
git branch -m main

# Get the repo URL for cloning
REPO_URL="file://$REPO_DIR"

# Test 1: Clone repository
echo ""
echo "=== Test 1: Clone Repository ==="
cd "$TEST_DIR"
run_instant "$INSTANT_DIR" dot clone "$REPO_URL"

echo "✓ Repository cloned successfully"

# Test 2: Check status
echo ""
echo "=== Test 2: Check Status ==="
run_instant "$INSTANT_DIR" dot status

echo "✓ Status command completed"

# Test 3: Apply dotfiles
echo ""
echo "=== Test 3: Apply Dotfiles ==="
run_instant "$INSTANT_DIR" dot apply

echo "✓ Apply command completed"

# Test 4: Verify files were created
echo ""
echo "=== Test 4: Verify Files ==="
if [ -f "$HOME_DIR/.config/test-app/config.txt" ]; then
    echo "✓ Config file created: $HOME_DIR/.config/test-app/config.txt"
    echo "Content: $(cat "$HOME_DIR/.config/test-app/config.txt")"
else
    echo "✗ Config file not created"
    exit 1
fi

if [ -f "$HOME_DIR/.bashrc" ]; then
    echo "✓ Bashrc file created: $HOME_DIR/.bashrc"
    echo "Content: $(cat "$HOME_DIR/.bashrc")"
else
    echo "✗ Bashrc file not created"
    exit 1
fi

# Test 5: Test help command
echo ""
echo "=== Test 5: Help Command ==="
run_instant "$INSTANT_DIR" --help

echo "✓ Help command works"

# Test 6: Test dot help command
echo ""
echo "=== Test 6: Dot Help Command ==="
run_instant "$INSTANT_DIR" dot --help

echo "✓ Dot help command works"

echo ""
echo "=== All Basic Tests Passed! ==="
echo "Test directory: $TEST_DIR"
echo "You can inspect the files before cleanup:"
