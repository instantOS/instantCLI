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

### Running Individual Tests

```bash
# Run basic functionality test
./tests/scripts/test_basic.sh

# Run init functionality test
./tests/scripts/test_init.sh

# Run with debug output
DEBUG=1 ./tests/scripts/test_basic.sh
```

### Running from Different Directories

Since InstantCLI is a dotfile manager, you might want to run tests from other directories:

```bash
# From home directory
cd ~ && /path/to/instantCLI/tests/scripts/test_basic.sh

# From any directory
cd /some/other/path && /path/to/instantCLI/tests/scripts/test_basic.sh
```

## Test Environment

Each test script creates a isolated test environment:

- **Test Directory**: `/tmp/instant-test-$$` (where `$$` is the process ID)
- **Config File**: `$TEST_DIR/instant.toml`
- **Database File**: `$TEST_DIR/instant.db`
- **Home Directory**: `$TEST_DIR/home`
- **Repositories**: Created in `$TEST_DIR/repo` or similar

The test environment is automatically cleaned up after each test run.

## Test Coverage

### Basic Functionality (`test_basic.sh`)
- Repository cloning
- Dotfile application
- Status checking
- File verification
- Help commands

### Repository Initialization (`test_init.sh`)
- Non-interactive init with custom name
- Non-interactive init with default name (directory name)
- Error handling for non-git directories
- Error handling for existing instantdots.toml
- Full workflow with actual dotfiles

## Debugging

### Enable Debug Output

```bash
DEBUG=1 ./tests/scripts/test_basic.sh
```

This shows the exact `cargo run` commands being executed.

### Manual Inspection

Tests pause before cleanup, allowing you to inspect files:

```bash
# Run test and inspect files before cleanup
./tests/scripts/test_basic.sh
# Look at the output for the test directory path
cd /tmp/instant-test-XXXXX  # Use the actual path shown
ls -la
```

### Test Specific Functionality

You can modify individual test scripts to focus on specific areas:

1. Edit the test script
2. Comment out other test cases
3. Run the specific test

### Test Directory Not Cleaned Up

If a test is interrupted, temporary directories might remain:

```bash
# Clean up all test directories
rm -rf /tmp/instant-test-*
```

## Current Limitations

The current test implementation has some limitations due to the architecture:

1. **Config Persistence**: The config file is saved to the default location even when using custom config paths. This is a known limitation of the current architecture.

2. **Repository Management**: Tests demonstrate repository cloning and basic operations, but the config persistence issue affects some operations.

3. **Isolation**: While tests use custom database paths and custom repository directories, the config file persistence is not fully isolated.

Despite these limitations, the tests successfully demonstrate:
- The non-interactive init feature works correctly
- Repository cloning to custom directories works
- Basic CLI functionality is operational
- Error handling works as expected

## Future Enhancements

Possible improvements:

1. **More Test Scripts**: Add tests for other features (multiple repos, subdirectories, etc.)
2. **Test Data**: Create more realistic test repositories
3. **Error Testing**: Add tests for error conditions
4. **Performance Testing**: Add timing measurements
5. **CI Integration**: Set up automated testing in CI/CD
6. **Architecture Improvements**: Address config persistence limitations for better test isolation
