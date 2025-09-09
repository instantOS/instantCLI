# InstantCLI Shell Script Tests

This directory contains simple shell script tests for InstantCLI, replacing the complex Rust e2e test suite.

## Overview

These tests provide basic functionality verification for InstantCLI using shell scripts. They are designed to be:
- Simple to understand and modify
- Easy to debug
- Isolated from user data
- Manually verifiable

## Test Scripts

### Individual Test Scripts

- `test_basic.sh` - Tests basic clone/apply/status workflow
- `test_init.sh` - Tests repository initialization (non-interactive mode)

### Master Test Runner

- `run_all.sh` - Runs all test scripts and reports results

## Usage

### Running All Tests

```bash
# Run all tests
./tests/scripts/run_all.sh

# Run with debug output (shows cargo commands)
DEBUG=1 ./tests/scripts/run_all.sh
```


## Test Environment

Each test script creates a isolated test environment:

- **Test Directory**: `/tmp/instant-test-$$` (where `$$` is the process ID)
- **Config File**: `$TEST_DIR/instant.toml`
- **Database File**: `$TEST_DIR/instant.db`
- **Home Directory**: `$TEST_DIR/home`
- **Repositories**: Created in `$TEST_DIR/repo` or similar

The test environment is automatically cleaned up after each test run.


## Future Enhancements

Possible improvements:

1. **More Test Scripts**: Add tests for other features (multiple repos, subdirectories, etc.)
2. **Test Data**: Create more realistic test repositories
3. **Error Testing**: Add tests for error conditions
4. **Performance Testing**: Add timing measurements
5. **CI Integration**: Set up automated testing in CI/CD
6. **Architecture Improvements**: Address config persistence limitations for better test isolation
