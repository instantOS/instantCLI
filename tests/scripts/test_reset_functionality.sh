#!/bin/bash

# Test script to verify reset functionality
# This script tests that reset properly restores dotfiles to unmodified state

# Source shared utilities
source "$(dirname "${BASH_SOURCE[0]}")/test_utils.sh"

echo "=== Testing Reset Functionality ==="

# Setup test environment
setup_test_env "/tmp/instant-test-reset-$$"

# Create a simple test repository
echo "Creating test repository..."
create_test_repo "$REPO_DIR" "reset-test-repo"

# Get the repo URL for cloning
REPO_URL="file://$REPO_DIR"

# Add the repository
cd "$TEST_DIR"
run_instant dot repo add "$REPO_URL" --name "reset-test-repo"

# Test 1: Modify a file and verify it shows as modified
echo ""
echo "=== Test 1: Modify file and check status ==="
echo "modified content" > "$HOME_DIR/.config/instanttest/config.txt"
run_instant dot status

# Test 2: Reset the .config directory
echo ""
echo "=== Test 2: Reset .config directory ==="
cd "$HOME_DIR"
run_instant dot reset .config
run_instant dot status

# Test 3: Verify the file content was restored
echo ""
echo "=== Test 3: Verify file content was restored ==="
echo "Expected content: 'test configuration content'"
echo "Actual content:"
cat "$HOME_DIR/.config/instanttest/config.txt"

# Test 4: Reset specific file
echo ""
echo "=== Test 4: Reset specific file ==="
echo "modified again" > "$HOME_DIR/.config/instanttest/settings.conf"
run_instant dot status
cd "$HOME_DIR"
run_instant dot reset .config/instanttest/settings.conf
run_instant dot status

# Test 5: Verify only the specified file was reset
echo ""
echo "=== Test 5: Verify only specified file was reset ==="
echo "settings.conf content:"
cat "$HOME_DIR/.config/instanttest/settings.conf"
echo "config.txt content (should still be 'test configuration content'):"
cat "$HOME_DIR/.config/instanttest/config.txt"

# Disable cleanup for inspection
trap - EXIT
echo ""
echo "=== Reset Test Complete ==="
echo "Test directory: $TEST_DIR"
echo "You can inspect the files before cleanup:"
echo ""
echo "=== Debug Info ==="
echo "Database file exists:"
ls -la "$DB_FILE"
echo ""
echo "Running status with debug output:"
HOME="$HOME_DIR" /home/benjamin/workspace/instantCLI/target/debug/instant --config "$CONFIG_FILE" --debug dot status