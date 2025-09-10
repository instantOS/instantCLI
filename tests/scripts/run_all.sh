#!/bin/bash
set -e

# Master test runner for InstantCLI shell script tests
# This script runs all individual test scripts

echo "=== InstantCLI Test Suite ==="
echo "Running all test scripts..."

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Test results tracking
PASSED=0
FAILED=0

# Function to run a test script
run_test() {
    local test_script="$1"
    local test_name="$(basename "$test_script" .sh)"
    
    echo ""
    echo "=== Running Test: $test_name ==="
    echo "Script: $test_script"
    
    # Set timeout for the test (5 minutes)
    timeout 300 "$test_script" || {
        echo "✗ Test $test_name failed or timed out"
        FAILED=$((FAILED + 1))
        return 1
    }
    
    echo "✓ Test $test_name passed"
    PASSED=$((PASSED + 1))
}

# Find and run all test scripts
echo "Searching for test scripts in $SCRIPT_DIR..."
for test_script in "$SCRIPT_DIR"/test_*.sh; do
    if [ -f "$test_script" ] && [ -x "$test_script" ]; then
        run_test "$test_script"
    else
        echo "Warning: Skipping $test_script (not executable or not a file)"
    fi
done

# Print summary
echo ""
echo "=== Test Suite Summary ==="
echo "Total tests passed: $PASSED"
echo "Total tests failed: $FAILED"

if [ $FAILED -eq 0 ]; then
    echo "🎉 All tests passed!"
    exit 0
else
    echo "❌ $FAILED test(s) failed!"
    exit 1
fi
