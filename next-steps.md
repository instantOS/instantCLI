# InstantCLI End-to-End Testing Plan

## Overview

This document outlines a comprehensive end-to-end testing strategy for InstantCLI, a Rust-based command-line tool for managing dotfiles and instantOS configurations. The testing approach focuses on validating the complete user workflow from repository management through dotfile application and modification tracking.

## Current Testing State

**Existing Tests:**
- Basic path resolution tests in `src/dot/path_tests.rs`
- Limited unit test coverage
- No integration or end-to-end tests
- Manual testing through CLI commands

**Testing Gaps:**
- No integration tests between components
- No end-to-end workflow validation
- No multi-repository scenario testing
- No error condition handling tests
- No configuration edge case testing

## Testing Philosophy

1. **Isolation**: Each test runs in a clean, isolated environment
2. **Deterministic**: Tests produce consistent results across runs
3. **Comprehensive**: Cover all major workflows and edge cases
4. **Realistic**: Use actual git repositories and file operations
5. **Cleanup**: Properly clean up all test artifacts

## Testing Architecture

### Test Structure
```
tests/
  e2e/
    main.rs              # Test runner and setup
    fixtures/            # Test fixtures and helpers
    workflows/           # End-to-end workflow tests
    utils/               # Test utilities
    common/              # Shared test utilities
```

### Test Categories

1. **End-to-End Workflows**: Complete user scenarios
3. **Error Handling**: Edge case and error condition testing
4. **Performance**: Large repository and file count testing

## Test Environment Setup - Simplified Approach

### Centralized Home Directory Override

Since all paths in InstantCLI are derived from the home directory, we can implement a simple, centralized solution by adding just **one environment variable override**:

**Environment Variable: `INSTANT_TEST_HOME_DIR`**

When this environment variable is set, all paths will be derived from it instead of the real home directory:
- Config: `$INSTANT_TEST_HOME_DIR/.config/instant/instant.toml`
- Database: `$INSTANT_TEST_HOME_DIR/.local/share/instantos/instant.db`
- Repos: `$INSTANT_TEST_HOME_DIR/.local/share/instantos/dots`

### Test File Paths
- Use unique paths that won't conflict with real user files
- Format: `~/.config/instantdottest/<test-specific-path>`
- Examples:
  - `~/.config/instantdottest/basic-config.txt`
  - `~/.config/instantdottest/theme/colors.conf`
  - `~/.config/instantdottest/app/settings.toml`

## Test Scenarios

### 1. Basic Repository Management

#### Test: Repository Clone and Apply

```rust
#[tokio::test]
async fn test_clone_and_apply_basic_repo() {
    // Setup: Create a git repo with basic dotfiles
    // Test: Clone repository and apply dotfiles
    // Verify: Files are correctly placed in home directory
    // Verify: Database tracks file hashes correctly
}
```

#### Test: Repository Removal
```rust
#[tokio::test]
async fn test_repository_removal() {
    // Setup: Add a repository to configuration
    // Test: Remove repository from configuration
    // Verify: Repository is removed from config
    // Verify: Local files are optionally cleaned up
}
```

### 2. Multi-Repository Scenarios

#### Test: Multiple Repositories with Priority
```rust
#[tokio::test]
async fn test_multiple_repositories_priority() {
    // Setup: Create 3 repositories with overlapping files
    // Test: Apply all repositories in order
    // Verify: Later repositories override earlier ones correctly
    // Verify: Priority system works as expected
}
```

### 3. File Modification Tracking

#### Test: User Modification Detection
```rust
#[tokio::test]
async fn test_user_modification_detection() {
    // Setup: Apply dotfiles, then modify some files
    // Test: Run status command to detect modifications
    // Verify: Modified files are correctly identified
    // Verify: Hash comparison works correctly
}
```

#### Test: Fetch Modified Files
```rust
#[tokio::test]
async fn test_fetch_modified_files() {
    // Setup: Apply dotfiles, modify files, then fetch
    // Test: Fetch modified files back to repository
    // Verify: Files are correctly updated in repository
    // Verify: Database entries are updated
}
```

### 4. Subdirectory Management

#### Test: Multiple Subdirectories
```rust
#[tokio::test]
async fn test_multiple_subdirectories() {
    // Setup: Create repo with multiple subdirectories
    // Test: List, set, and activate subdirectories
    // Verify: Only active subdirectories are processed
    // Verify: Subdirectory switching works correctly
}
```

### 5. Error Handling

#### Test: Invalid Repository URL
```rust
#[tokio::test]
async fn test_invalid_repository_url() {
    // Test: Attempt to clone invalid repository
    // Verify: Proper error handling and messages
    // Verify: System state remains consistent
}
```

#### Test: Path Outside Home Directory
```rust
#[tokio::test]
async fn test_path_outside_home_directory() {
    // Test: Attempt operations on paths outside home
    // Verify: Operations are properly rejected
    // Verify: Security boundaries are enforced
}
```


### Test Utilities

#### Repository Creation Helper
```rust
pub fn create_test_repo(
    env: &TestEnvironment,
    name: &str,
    files: &[(&str, &str)], // (path, content) pairs
    subdirs: &[&str],
) -> Result<PathBuf> {
    let repo_path = // homestuff
    
    // Create git repository
    std::process::Command::new("git")
        .args(&["init", "--bare", repo_path.to_str().unwrap()])
        .output()?;
    
    // Create temporary working directory for adding files
    let work_dir = tempfile::tempdir()?;
    
    // Clone bare repo to working directory
    std::process::Command::new("git")
        .args(&["clone", repo_path.to_str().unwrap(), work_dir.path().to_str().unwrap()])
        .output()?;
    
    // Add files to specified subdirectories
    for (subdir, files_in_subdir) in subdirs.iter().zip(files.chunks(files.len() / subdirs.len())) {
        let subdir_path = work_dir.path().join(subdir);
        fs::create_dir_all(&subdir_path)?;
        
        for (file_path, content) in files_in_subdir {
            let full_path = subdir_path.join(file_path);
            fs::create_dir_all(full_path.parent().unwrap())?;
            fs::write(&full_path, content)?;
        }
    }
    
    // Commit and push back to bare repo
    std::process::Command::new("git")
        .args(&["add", "."])
        .current_dir(work_dir.path())
        .output()?;
    
    std::process::Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(work_dir.path())
        .output()?;
    
    std::process::Command::new("git")
        .args(&["push", "origin", "main"])
        .current_dir(work_dir.path())
        .output()?;
    
    Ok(repo_path)
}
```

#### Command Execution Helper
```rust
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn run_instant_command(
    env: &TestEnvironment,
    args: &[&str],
) -> Result<CommandOutput> {
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(&["run", "--bin", "instant", "--"])
        .args(args);
    // make sure this can be run from different dirs, cargo run needs to know
    // where the project lives
    let output = cmd.output()?;
    
    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
```


### Test Data Management

1. **Static Test Fixtures**
   - Pre-created repository templates
   - Common configuration patterns
   - Sample dotfile collections

2. **Dynamic Test Data**
   - Generated repositories with random content
   - Variable file sizes and counts
   - Different git repository structures

## Test Cleanup Strategy

### Automatic Cleanup
```rust
impl Drop for TestEnvironment {
    fn drop(&mut self) {
        // Remove temporary directories
        // Kill any lingering processes
        // Restore system state
    }
}
```

### Cleanup Verification
- Verify all temporary files are removed
- Verify no processes remain running
- Verify system state is restored
- Verify no side effects on real user data

## Test Reporting and Metrics

### Test Output Format
```rust
pub struct TestResult {
    pub test_name: String,
    pub passed: bool,
    pub duration: Duration,
    pub output: CommandOutput,
    pub artifacts: Vec<PathBuf>,
}
```

### Metrics Collection
- Test execution time
- Memory usage
- File system operations
- Network operations (for git operations)
- Database operations

