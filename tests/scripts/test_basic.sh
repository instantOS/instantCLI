#!/bin/bash

# Test basic functionality: clone, apply, status
# This script tests the core workflow of InstantCLI

# Source shared utilities
source "$(dirname "${BASH_SOURCE[0]}")/test_utils.sh"

echo "=== Testing Basic InstantCLI Functionality ==="

# Setup test environment
setup_test_env "/tmp/instant-test-$$"

# Create a simple test repository
echo "Creating test repository..."
create_test_repo "$REPO_DIR" "test-basic-repo"

# Get the repo URL for cloning
REPO_URL="file://$REPO_DIR"

# Test 1: Clone repository
print_test_header "Test 1: Clone Repository"
cd "$TEST_DIR"
run_instant dot repo add "$REPO_URL" --name "test-basic-repo"

print_test_result 0 "Repository clone"

# Test 2: Check status
print_test_header "Test 2: Check Status"
run_instant dot status

print_test_result 0 "Status command"

# Test 3: Apply dotfiles
print_test_header "Test 3: Apply Dotfiles"
run_instant dot apply

print_test_result 0 "Apply command"

# Test 4: Verify files were created in test home directory
print_test_header "Test 4: Verify Files"

verify_file "$HOME_DIR/.config/instanttest/config.txt" "test configuration content"
verify_file "$HOME_DIR/.config/instanttest/settings.conf" "another config file"

# Test 5: Test help command
print_test_header "Test 5: Help Command"
run_instant --help

print_test_header "All Basic Tests Passed!"
echo "Test directory: $TEST_DIR"
echo "You can inspect the files before cleanup:"
