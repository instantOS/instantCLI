#!/bin/bash

# Test script to demonstrate the improved test utilities
# This script shows how to use the new wrapper-based testing approach

# Source shared utilities
source "$(dirname "${BASH_SOURCE[0]}")/test_utils.sh"

echo "=== Testing Improved Test Utilities ==="

# Setup test environment
setup_test_env "/tmp/instant-test-new-$$"

# Create a simple test repository
echo "Creating test repository..."
create_test_repo "$REPO_DIR" "new-utils-test-repo"

# Get the repo URL for cloning
REPO_URL="file://$REPO_DIR"

# Test 1: Demonstrate wrapper enforcement (this should fail)
echo ""
echo "=== Test 1: Wrapper Enforcement ==="
echo "This test shows that commands must be run within the wrapper"
echo "Running direct command (should fail):"
if ensure_test_wrapper; then
    echo "ERROR: ensure_test_wrapper should have failed but didn't"
else
    echo "✓ ensure_test_wrapper correctly detected missing wrapper"
fi

# Test 2: Demonstrate proper wrapper usage
echo ""
echo "=== Test 2: Proper Wrapper Usage ==="
echo "Creating test repo with proper wrapper:"

# Use run_with_test_home for git commands
run_with_test_home git clone "$REPO_URL" "$HOME_DIR/test-repo"

# Test 3: Demonstrate new instant CLI usage
echo ""
echo "=== Test 3: Instant CLI with New Utils ==="
echo "Adding repository with instant CLI:"
run_instant dot repo add "$REPO_URL" --name "new-utils-test-repo"

echo ""
echo "Checking status (should show clean files):"
run_instant dot status

# Test 4: Demonstrate new verification utilities
echo ""
echo "=== Test 4: New Verification Utilities ==="

echo "Testing keyword checking:"
output=$(run_instant dot status)
check_output_contains "$output" "clean" "Status output contains 'clean'"
check_output_not_contains "$output" "No dotfiles found" "Status output doesn't contain 'No dotfiles found'"

echo ""
echo "Testing file verification:"
verify_file "$HOME_DIR/.config/instanttest/config.txt" "test configuration content"

# Test 5: Demonstrate file comparison utilities
echo ""
echo "=== Test 5: File Comparison Utilities ==="

# Create a modified file
echo "modified content" > "$HOME_DIR/.config/instanttest/config_modified.txt"

echo "Testing file difference:"
check_files_different "$HOME_DIR/.config/instanttest/config.txt" "$HOME_DIR/.config/instanttest/config_modified.txt" "Original and modified files are different"

# Create identical file
cp "$HOME_DIR/.config/instanttest/config.txt" "$HOME_DIR/.config/instanttest/config_copy.txt"

echo "Testing file sameness:"
check_files_same "$HOME_DIR/.config/instanttest/config.txt" "$HOME_DIR/.config/instanttest/config_copy.txt" "Original and copied files are identical"

# Test 6: Demonstrate run_and_capture utility
echo ""
echo "=== Test 6: Run and Capture Utility ==="
run_and_capture "run_instant dot apply" "Apply dotfiles command"

# Disable cleanup for inspection
trap - EXIT
echo ""
echo "=== New Utils Test Complete ==="
echo "Test directory: $TEST_DIR"
echo "You can inspect the files before cleanup."
echo ""
echo "Key improvements demonstrated:"
echo "✓ Wrapper enforcement prevents accidental real home directory usage"
echo "✓ Simplified config - no need for custom config/database paths"
echo "✓ Better output verification utilities"
echo "✓ File comparison utilities"
echo "✓ Command capture utility with detailed output"