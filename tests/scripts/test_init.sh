#!/bin/bash

# Test repository initialization functionality
# This script tests the init command with both interactive and non-interactive modes

# Source shared utilities
source "$(dirname "${BASH_SOURCE[0]}")/test_utils.sh"

echo "=== Testing Repository Initialization ==="

# Setup test environment
setup_test_env "/tmp/instant-test-init-$$"

# Test 1: Non-interactive init with custom name
print_test_header "Test 1: Non-Interactive Init with Custom Name"
cd "$REPO_DIR"
git init
run_instant dot init --non-interactive "test-custom-name"

verify_file "$REPO_DIR/instantdots.toml"

# Verify the content
if grep -q "name = \"test-custom-name\"" "$REPO_DIR/instantdots.toml"; then
    echo "✓ Custom name set correctly"
else
    echo "✗ Custom name not set correctly"
    exit 1
fi

# Test 2: Non-interactive init with default name (directory name)
print_test_header "Test 2: Non-Interactive Init with Default Name"
cd "$TEST_DIR"
mkdir test-default-repo
cd test-default-repo
git init
run_instant dot init --non-interactive

verify_file "instantdots.toml"

# Verify the content (should use directory name)
if grep -q "name = \"test-default-repo\"" instantdots.toml; then
    echo "✓ Default name (directory name) set correctly"
else
    echo "✗ Default name not set correctly"
    exit 1
fi

# Test 3: Test that init fails in non-git directory
print_test_header "Test 3: Init Fails in Non-Git Directory"
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
print_test_header "Test 4: Init Fails When instantdots.toml Exists"
cd "$REPO_DIR"
# We already have instantdots.toml from Test 1

if run_instant dot init --non-interactive "test-existing" 2>/dev/null; then
    echo "✗ Init should have failed when instantdots.toml exists"
    exit 1
else
    echo "✓ Init correctly failed when instantdots.toml exists"
fi

# Test 5: Create a repository with actual dotfiles and test the full workflow
print_test_header "Test 5: Full Workflow with Init"
cd "$TEST_DIR"
mkdir full-test-repo
cd full-test-repo
git init

# Create some dotfiles with made up applications
mkdir -p dots/.config/instanttest
echo "app configuration" > dots/.config/instanttest/app.conf
echo "test settings" > dots/.config/instanttest/settings.ini

# Initialize the repository
run_instant dot init --non-interactive "full-test-repo"

# Verify instantdots.toml exists
verify_file "instantdots.toml"

# Add files to git
git add . >/dev/null 2>&1
git commit -m "Add dotfiles and instantdots.toml" >/dev/null 2>&1

# Now test if we can clone and apply from this repository
cd "$TEST_DIR"
CLONE_DIR="$TEST_DIR/cloned-repo"
run_instant dot clone "file://full-test-repo"

if [ -d "$CLONE_DIR" ]; then
    echo "✓ Repository cloned successfully"
else
    echo "✗ Repository clone failed"
    exit 1
fi

# Apply the dotfiles
run_instant dot apply

# Verify files were applied in real home directory
echo "Note: InstantCLI uses the real home directory, not a test directory"
echo "Checking if files were created in real home directory..."

verify_file "$HOME/.config/instanttest/app.conf" "app configuration"
verify_file "$HOME/.config/instanttest/settings.ini" "test settings"

print_test_header "All Init Tests Passed!"
echo "Test directory: $TEST_DIR"
echo "You can inspect the files before cleanup"
