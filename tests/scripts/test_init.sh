#!/bin/bash
set -e

# Test repository initialization functionality
# This script tests the init command with both interactive and non-interactive modes

echo "=== Testing Repository Initialization ==="

# Setup test environment
TEST_DIR="/tmp/instant-test-init-$$"
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

# Test functions
run_instant() {
    if [ "$DEBUG" = "1" ]; then
        echo "Running: cargo run -- --config \"$CONFIG_FILE\" --database \"$DB_FILE\" $@"
    fi
    cargo run -- --config "$CONFIG_FILE" --database "$DB_FILE" "$@"
}

# Test 1: Non-interactive init with custom name
echo ""
echo "=== Test 1: Non-Interactive Init with Custom Name ==="
cd "$REPO_DIR"
git init
run_instant dot init --non-interactive "test-custom-name"

if [ -f "$REPO_DIR/instantdots.toml" ]; then
    echo "✓ instantdots.toml created"
    echo "Content:"
    cat "$REPO_DIR/instantdots.toml"
else
    echo "✗ instantdots.toml not created"
    exit 1
fi

# Verify the content
if grep -q "name = \"test-custom-name\"" "$REPO_DIR/instantdots.toml"; then
    echo "✓ Custom name set correctly"
else
    echo "✗ Custom name not set correctly"
    exit 1
fi

# Test 2: Non-interactive init with default name (directory name)
echo ""
echo "=== Test 2: Non-Interactive Init with Default Name ==="
cd "$TEST_DIR"
mkdir test-default-repo
cd test-default-repo
git init
run_instant dot init --non-interactive

if [ -f "instantdots.toml" ]; then
    echo "✓ instantdots.toml created"
    echo "Content:"
    cat instantdots.toml
else
    echo "✗ instantdots.toml not created"
    exit 1
fi

# Verify the content (should use directory name)
if grep -q "name = \"test-default-repo\"" instantdots.toml; then
    echo "✓ Default name (directory name) set correctly"
else
    echo "✗ Default name not set correctly"
    exit 1
fi

# Test 3: Test that init fails in non-git directory
echo ""
echo "=== Test 3: Init Fails in Non-Git Directory ==="
cd "$TEST_DIR"
mkdir non-git-dir
cd non-git-dir

if run_instant dot init --non-interactive "test-non-git" 2>/dev/null; then
    echo "✗ Init should have failed in non-git directory"
    exit 1
else
    echo "✓ Init correctly failed in non-git directory"
fi

# Test 4: Test that init fails when instantdots.toml already exists
echo ""
echo "=== Test 4: Init Fails When instantdots.toml Exists ==="
cd "$REPO_DIR"
# We already have instantdots.toml from Test 1

if run_instant dot init --non-interactive "test-existing" 2>/dev/null; then
    echo "✗ Init should have failed when instantdots.toml exists"
    exit 1
else
    echo "✓ Init correctly failed when instantdots.toml exists"
fi

# Test 5: Create a repository with actual dotfiles and test the full workflow
echo ""
echo "=== Test 5: Full Workflow with Init ==="
cd "$TEST_DIR"
mkdir full-test-repo
cd full-test-repo
git init

# Create some dotfiles
mkdir -p dots/.config/test-app
echo "app configuration" > dots/.config/test-app/config.conf
echo "test alias" > dots/.bash_aliases

# Initialize the repository
run_instant dot init --non-interactive "full-test-repo"

# Verify instantdots.toml exists and has correct content
if [ -f "instantdots.toml" ]; then
    echo "✓ instantdots.toml created"
    echo "Content:"
    cat instantdots.toml
else
    echo "✗ instantdots.toml not created"
    exit 1
fi

# Add files to git
git add .
git commit -m "Add dotfiles and instantdots.toml"

# Now test if we can clone and apply from this repository
cd "$TEST_DIR"
CLONE_DIR="$TEST_DIR/cloned-repo"
run_instant dot clone "file://$full-test-repo"

if [ -d "$CLONE_DIR" ]; then
    echo "✓ Repository cloned successfully"
else
    echo "✗ Repository clone failed"
    exit 1
fi

# Apply the dotfiles
run_instant dot apply

# Verify files were applied
if [ -f "$HOME_DIR/.config/test-app/config.conf" ]; then
    echo "✓ Config file applied: $HOME_DIR/.config/test-app/config.conf"
    echo "Content: $(cat "$HOME_DIR/.config/test-app/config.conf")"
else
    echo "✗ Config file not applied"
    exit 1
fi

if [ -f "$HOME_DIR/.bash_aliases" ]; then
    echo "✓ Bash aliases applied: $HOME_DIR/.bash_aliases"
    echo "Content: $(cat "$HOME_DIR/.bash_aliases")"
else
    echo "✗ Bash aliases not applied"
    exit 1
fi

echo ""
echo "=== All Init Tests Passed! ==="
echo "Test directory: $TEST_DIR"
echo "You can inspect the files before cleanup"