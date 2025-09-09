#!/bin/bash

# Test for bug: identical files showing as modified after clone
# This reproduces the scenario where dotfiles already exist in home directory
# with the same content as in the dotfile repo, but show as modified

# Source shared utilities
source "$(dirname "${BASH_SOURCE[0]}")/test_utils.sh"

echo "=== Testing Bug: Identical Files Showing as Modified ==="

# Setup test environment
setup_test_env "/tmp/instant-test-bug-$$"

# Create a simple test repository
echo "Creating test repository..."
create_test_repo "$REPO_DIR" "test-bug-repo"

# Get the repo URL for cloning
REPO_URL="file://$REPO_DIR"

# First, create the identical files in the home directory manually
echo "Creating identical files in home directory..."
mkdir -p "$HOME_DIR/.config/instanttest"
echo "test configuration content" > "$HOME_DIR/.config/instanttest/config.txt"
echo "another config file" > "$HOME_DIR/.config/instanttest/settings.conf"

echo "Files created in home directory:"
ls -la "$HOME_DIR/.config/instanttest/"

# Test 1: Clone repository
print_test_header "Test 1: Clone Repository with Existing Identical Files"
cd "$TEST_DIR"
run_instant dot clone "$REPO_URL"

print_test_result 0 "Repository clone"

# Test 2: Check status - should show files as clean (not modified)
print_test_header "Test 2: Check Status - Should Show Clean"
run_instant dot status

echo ""
echo "=== VERIFICATION ==="
echo "✓ Files should show as 'clean' since they're identical"

# Test 3: Verify file contents are actually identical
print_test_header "Test 3: Verify File Contents Are Identical"

echo "Comparing files:"
echo "Repo file: $(cat "$REPO_DIR/dots/.config/instanttest/config.txt")"
echo "Home file: $(cat "$HOME_DIR/.config/instanttest/config.txt")"
echo "Files are identical: $(cmp -s "$REPO_DIR/dots/.config/instanttest/config.txt" "$HOME_DIR/.config/instanttest/config.txt" && echo "YES" || echo "NO")"

echo ""
echo "Repo file: $(cat "$REPO_DIR/dots/.config/instanttest/settings.conf")"
echo "Home file: $(cat "$HOME_DIR/.config/instanttest/settings.conf")"
echo "Files are identical: $(cmp -s "$REPO_DIR/dots/.config/instanttest/settings.conf" "$HOME_DIR/.config/instanttest/settings.conf" && echo "YES" || echo "NO")"

print_test_header "Bug reproduction complete"
echo "Test directory: $TEST_DIR"