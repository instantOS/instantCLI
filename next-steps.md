# Next Steps: Replace E2E Testing with Simple Shell Scripts

## Current Problem

The current e2e testing suite is overly complex and difficult to debug:
- Located in `tests/e2e_test.rs` with supporting modules in `tests/e2e/`
- Uses complex Rust test framework with temporary directories, environment variables, and mutex locking
- Requires deep understanding of the testing infrastructure to debug issues
- Too much complexity for basic functionality verification

## Proposed Solution

Replace the complex Rust e2e tests with simple shell scripts that:
- Use existing CLI arguments for custom config/database paths
- Provide basic functionality verification
- Allow manual inspection of outputs
- Are easy to understand and modify

## CLI Arguments Available

The CLI already supports custom paths:
- `--config <path>` or `-c <path>`: Custom config file path
- `--database <path>`: Custom database file path
- `--debug`: Enable debug output

## Required Features

### 1. Non-Interactive Init Flag

Add a `--non-interactive` flag to `instant dot init`:
- Uses provided name without prompting
- Uses directory name as default if no name provided
- Skips all interactive prompts

### 2. Shell Script Test Suite

Create simple shell scripts in `tests/scripts/`:

#### Basic Test Structure
```bash
#!/bin/bash
set -e

# Setup test environment
TEST_DIR="/tmp/instant-test-$$"
CONFIG_FILE="$TEST_DIR/instant.toml"
DB_FILE="$TEST_DIR/instant.db"
HOME_DIR="$TEST_DIR/home"

# Cleanup function
cleanup() {
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# Create test environment
mkdir -p "$HOME_DIR"
mkdir -p "$(dirname "$CONFIG_FILE")"
mkdir -p "$(dirname "$DB_FILE")"

# Test functions
run_instant() {
    cargo run -- --config "$CONFIG_FILE" --database "$DB_FILE" "$@"
}
# TODO: come up with a way to run the program when outside of the repo
# this is a dotfile manager, so running it from other directories (cd
# otherstuff) is to be expected

# Test cases
echo "=== Testing basic functionality ==="
run_instant dot clone <test-repo-url>
run_instant dot apply
run_instant dot status
```

#### Test Scripts to Create

1. `test_basic.sh` - Basic clone/apply/status operations
2. `test_multiple_repos.sh` - Multiple repository handling
3. `test_modification_detection.sh` - User modification detection
4. `test_fetch_reset.sh` - Fetch and reset operations
5. `test_subdirectories.sh` - Multiple subdirectories support
6. `test_init.sh` - Repository initialization (non-interactive)

## Implementation Plan

### Phase 1: Add Non-Interactive Init
1. Modify `src/dot/meta.rs` to add `--non-interactive` flag
2. Update `src/main.rs` to pass the flag through
3. Skip prompts when flag is set

### Phase 2: Create Shell Script Tests
1. Create `tests/scripts/` directory
2. Write basic test scripts with common setup
3. Each script tests one major feature area
4. Include cleanup and error handling

### Phase 3: Documentation and Usage
1. Add `README.md` in `tests/scripts/` explaining usage
2. Document how to run individual tests
3. Explain expected outputs for manual verification

### Phase 4: Remove Old Tests
1. Remove `tests/e2e_test.rs`
2. Remove `tests/e2e/` directory
3. Update `Cargo.toml` if needed

## Benefits

- **Simplicity**: Shell scripts are easy to understand and modify
- **Debugging**: Easy to step through and debug issues
- **Flexibility**: Can easily add new test cases
- **Manual Verification**: User can inspect outputs directly
- **No Complex Framework**: No need for Rust testing infrastructure
- **Isolation**: Uses custom config/db paths, won't affect user data

## Test Coverage

The shell scripts will cover:
- Basic repository cloning and applying
- Multiple repository management
- User modification detection
- Fetch and reset operations
- Subdirectory management
- Repository initialization
- Error handling for invalid inputs

## Expected Output

Each test script will:
- Print clear section headers
- Show command outputs
- Indicate success/failure
- Clean up after itself
- Allow manual inspection of files created

## Usage

```bash
# Run all tests
./tests/scripts/run_all.sh

# Run specific test
./tests/scripts/test_basic.sh

# Run with debug output
DEBUG=1 ./tests/scripts/test_basic.sh
```

This approach provides a much simpler testing solution that focuses on basic functionality verification while being easy to debug and maintain.
