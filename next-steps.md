# instant dev install Implementation Plan

## Overview

Reimplement the bash-based `install_old.sh` as a Rust `instant dev install` command. The command will compile and install instantOS packages from the `instantOS/extra` GitHub repository using fzf for package selection.

## Current Bash Implementation Analysis

### Key Functionality (from install_old.sh)
1. **Repository Management**:
   - Clone `instantOS/extra` to `~/.cache/instantos/extra` if not exists
   - Pull latest changes with depth 3
   - Handle local changes by stashing

2. **Package Discovery**:
   - Scan all directories in extra repo for `PKGBUILD` files
   - Extract package names from directory names
   - Filter out invalid entries (dots, single chars)

3. **Interactive Selection**:
   - Use fzf to present package list
   - Support direct package name argument or interactive selection

4. **Package Installation**:
   - Change to package directory
   - Run `makepkg -si` to build and install

## Rust Implementation Architecture

### 1. Dependencies to Add

```toml
[dependencies]
# Add to existing Cargo.toml
xshell = "0.2"  # For shell command execution
```



### 2. Module Structure

```
src/dev/
       mod.rs              # Update to include Install command
       install.rs          # New: Package installation logic
       package.rs          # New: Package discovery and management
       fuzzy.rs            # Existing: fzf wrapper
       github.rs           # Existing: GitHub API
       clone.rs            # Existing: Repository cloning
```

### 3. Implementation Steps

#### Step 1: Add Install Command to DevCommands enum
**File**: `src/dev/mod.rs`
- Add `Install` variant to `DevCommands` enum
- Update `handle_dev_command` to route to install handler

#### Step 2: Create Package Management Module
**File**: `src/dev/package.rs`
- Define `Package` struct with name, path, description
- Implement package discovery logic
- Handle directory scanning and PKGBUILD detection

#### Step 3: Create Install Module
**File**: `src/dev/install.rs`
- Repository management (clone/pull logic)
- Package selection interface
- Build and install execution
- Error handling and user feedback

#### Step 4: Integrate with Existing FZF Wrapper
**File**: `src/dev/fuzzy.rs`
- Create `select_package` function similar to `select_repository`  (maybe share
  code?)
- Handle package display formatting

#### Step 5: Add xshell Integration
**File**: `src/dev/install.rs`
- Use xshell for `git`, `makepkg`
- Handle command execution with proper error handling

### 4. Key Components

#### Package Discovery Logic
```rust
struct Package {
    name: String,
    path: PathBuf,
    description: Option<String>,
}

impl Package {
    fn from_directory(dir: &Path) -> Option<Self> {
        // Check for PKGBUILD, extract name, create Package
    }

    fn discover_packages(repo_path: &Path) -> Result<Vec<Package>> {
        // Scan directories, filter valid packages
    }
}
```

#### Repository Management
```rust
struct PackageRepo {
    path: PathBuf,  // ~/.cache/instantos/extra
    url: String,    // https://github.com/instantOS/extra
}

impl PackageRepo {
    fn new() -> Result<Self> {
        // Create directory structure if needed
    }

    fn ensure_updated(&self) -> Result<()> {
        // Clone or pull with proper error handling
    }

    fn handle_local_changes(&self) -> Result<()> {
        // Stash changes if instantwm is running
    }
}
```

#### Install Command Handler
```rust
pub async fn handle_install(debug: bool) -> Result<()> {
    let repo = PackageRepo::new()?;
    repo.ensure_updated()?;

    let packages = Package::discover_packages(&repo.path)?;
    let selected_package = select_package(packages)?;

    build_and_install_package(&selected_package, debug)?;

    Ok(())
}
```

### 5. Error Handling Strategy

- Use existing `anyhow::Result` pattern
- Define specific error types for package operations
- Provide user-friendly error messages
- Handle missing dependencies gracefully

### 6. User Experience Enhancements

- **Progress Indicators**: Use existing `indicatif` spinner
- **Debug Output**: Respect `--debug` flag
- **Colored Output**: Use existing `colored` crate
- **Error Recovery**: Handle git conflicts gracefully

### 7. Integration Points

#### Command Structure
```bash
instant dev install          # Interactive package selection
instant dev install <name>   # Install specific package
instant dev install --debug  # Verbose output
```

#### Configuration
- Use `~/.cache/instantos/extra` directory
- Maintain compatibility with bash version
- Respect existing file permissions


### 11. Success Criteria

- Interactive package selection with fzf
- Direct package installation by name
- Repository auto-update with conflict handling
- Colored output and progress indicators
- Proper error handling and user feedback
- Compatibility with existing bash workflow
- Debug mode for troubleshooting

## Implementation Notes

- **Simplicity Focus**: Keep implementation concise and maintainable
- **Leverage Existing Code**: Use existing error handling, progress indicators, and CLI patterns
- **Shell Commands**: Use xshell for git and makepkg operations instead of Rust Git libraries
- **Performance**: Cache repository locally, shallow clones for speed
