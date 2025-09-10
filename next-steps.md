# Repository Management CLI Improvement Plan

## Current State Analysis

### Issues with Current CLI Structure

1. **Monolithic mod.rs (489 lines)**: The main dotfile module has grown too large, mixing repository management, dotfile operations, and CLI logic.

2. **Scattered Repository Commands**: Repository-related operations are spread across different command structures:
   - `clone` - Add new repositories
   - `remove` - Remove repositories  
   - `update` - Update all repositories
   - `status` - Check repository status
   - `list-subdirs/set-subdirs/show-subdirs` - Subdirectory management

4. **Missing Repository Operations**: No direct way to:
   - View repository details
   - Enable/disable repositories

## Proposed CLI Structure

### New Repository Subcommand Group

The `update` command should remain separate and update all repositories then apply changes. After adding a repo, it should also be immediately applied.

```
instant dot repo <subcommand>
```

#### Repository Management Commands
- `instant dot repo add <url> [--name] [--branch]` - Add a new repository 
- `instant dot repo remove <name> [--files]` - Remove a repository
- `instant dot repo list` - List all configured repositories
- `instant dot repo info <name>` - Show detailed repository information, list subdirs and display which ones are active, as well as status info

#### Repository Operations
- `instant dot repo enable <name>` - Enable a disabled repository
- `instant dot repo disable <name>` - Disable a repository temporarily

#### Subdirectory Management (improved)
- `instant dot repo subdirs set <name> <subdirs...>` - Set active subdirectories
- `instant dot repo subdirs list <name> [--active]` - List available subdirectories, optionally only active ones

### Backward Compatibility
Breaking changes are allowed. The plan eliminates the need for backward compatibility to clean up the CLI interface.

## Code Refactoring Plan

### 1. Module Reorganization

```
src/dot/
       mod.rs                    # Main orchestration (simplified)
       repo/                     # New repository management module
            mod.rs               # Repository module exports
            manager.rs           # Repository manager struct
            commands.rs          # Repository command implementations
            cli.rs               # Repository CLI definitions
       dotfile/                 # Dotfile operations (existing)
       config.rs                # Configuration management (existing)
       db.rs                    # Database operations (existing)
       git.rs                   # Git operations (existing)
       localrepo.rs             # Local repository representation (existing)
       meta.rs                  # Repository metadata (existing)
       utils.rs                 # Utility functions (existing)
```

### 2. Repository Manager Structure

Create a `RepositoryManager` struct that respects existing dependency injection patterns and uses existing structs:

### RepositoryManager Design Analysis

After analyzing the codebase, here are the key findings:

**Is RepositoryManager needed?** **Yes, but with a different scope:**

1. **Existing Iteration Patterns Found**:
   - `get_active_dotfile_dirs()` iterates over `config.repos` 
   - `update_all()` in `git.rs` iterates over `config.repos`
   - `status_all()` in `git.rs` iterates over `config.repos`
   - Multiple `LocalRepo::new()` calls throughout the codebase

2. **Current Duplication**: Each function that works with repositories repeats the same pattern:
   ```rust
   for repo in &config.repos {
       let local_repo = LocalRepo::new(config, repo.name.clone())?;
       // ... do something with local_repo
   }
   ```

3. **RepositoryManager Value**: Instead of eliminating iteration, the RepositoryManager should:
   - **Centralize the iteration logic** that's currently duplicated
   - **Handle enable/disable filtering** consistently
   - **Provide a single source of truth** for repository operations
   - **Allow existing functions** to be refactored to use it

**Enable/Disable Implementation Plan**:

1. **Add `enabled` field to `Repo` struct** in `config.rs`:
   ```rust
   #[derive(Serialize, Deserialize, Debug, Clone)]
   pub struct Repo {
       pub url: String,
       pub name: String,
       pub branch: Option<String>,
       #[serde(default = "default_active_subdirs")]
       pub active_subdirectories: Vec<String>,
       #[serde(default = "default_enabled")]
       pub enabled: bool,
   }
   
   fn default_enabled() -> bool { true }
   ```

2. **Update Core Functions** to respect the `enabled` flag:
   - `get_active_dotfile_dirs()` should skip disabled repos
   - `get_all_dotfiles()` should skip disabled repos
   - `update_all()` should skip disabled repos but show they were skipped
   - `status_all()` should show disabled repos with appropriate status

3. **Visibility**: Disabled repos should appear in listing commands with clear indicators but be excluded from operations like apply/fetch.

**Dependency Injection Analysis**: You're absolutely right! The existing codebase consistently passes `&Config` and `&Database` references to functions rather than having structs own copies. Looking at patterns like:

```rust
pub fn apply_all(config: &Config, db: &Database) -> Result<()>
pub fn reset_modified(config: &Config, db: &Database, path: &str) -> Result<()>
pub fn add_dotfile(config: &Config, db: &Database, path: &str) -> Result<()>
```

**RepositoryManager should follow the same pattern**:

```rust
// RepositoryManager is a temporary helper, not a long-lived owner
pub struct RepositoryManager<'a> {
    config: &'a Config,
    db: &'a Database,
}

impl<'a> RepositoryManager<'a> {
    pub fn new(config: &'a Config, db: &'a Database) -> Self {
        Self { config, db }
    }
    
    // All methods use the borrowed references
    pub fn for_each_enabled_repo<F>(&self, mut callback: F) -> Result<()>
    where F: FnMut(&config::Repo, &LocalRepo) -> Result<()>;
}

impl<'a> RepositoryManager<'a> {
    // Core operations
    pub fn add_repository(&self, url: &str, name: Option<String>, branch: Option<String>) -> Result<()>;
    pub fn remove_repository(&self, name: &str, remove_files: bool) -> Result<()>;
    
    // Enable/disable functionality  
    pub fn enable_repository(&self, name: &str) -> Result<()>;
    pub fn disable_repository(&self, name: &str) -> Result<()>;
    
    // Centralized iteration with filtering
    pub fn for_each_enabled_repo<F>(&self, mut callback: F) -> Result<()> 
    where F: FnMut(&config::Repo, &LocalRepo) -> Result<()>;
    
    pub fn for_all_repos<F>(&self, mut callback: F) -> Result<()> 
    where F: FnMut(&config::Repo, &LocalRepo) -> Result<()>;
    
    // Repository listing and info
    pub fn list_repositories(&self) -> Result<Vec<(config::Repo, LocalRepo)>>;
    pub fn get_repository_info(&self, name: &str) -> Result<LocalRepo>;
    
    // Subdirectory management
    pub fn list_subdirectories(&self, name: &str) -> Result<Vec<String>>;
    pub fn set_subdirectories(&self, name: &str, subdirs: Vec<String>) -> Result<()>;
}
```

**Key Design Change**: Instead of returning `LocalRepo` vectors, return tuples of `(config::Repo, LocalRepo)` to provide both configuration and runtime information. The iterator-style methods (`for_each_enabled_repo`, `for_all_repos`) eliminate duplication in existing functions.

### 3. Enhanced Repository Information

Instead of creating new structs, enhance the existing `LocalRepo` struct to provide comprehensive information. The `LocalRepo` already contains:
- `url`, `name`, `branch` fields
- `dotfile_dirs: Vec<DotfileDir>` with active/inactive status
- `meta: RepoMetaData` with repository metadata
- Methods for git operations and path resolution

Additional helper functions can be added to provide status information without creating duplicate data structures.

### 4. CLI Structure Improvements

#### Main CLI Structure (simplified)
```rust
#[derive(Subcommand, Debug)]
enum DotCommands {
    // Repository operations (new structure)
    Repo {
        #[command(subcommand)]
        command: RepoCommands,
    },
    
    // Core dotfile operations
    Apply,
    Fetch { path: Option<String>, dry_run: bool },
    Reset { path: String },
    Add { path: String },
    
    // Global update command (updates all repos and applies changes)
    Update,
    
    // Other operations
    Status { path: Option<String> },
    Init { name: Option<String>, non_interactive: bool },
}
```

#### Repository Subcommands
```rust
#[derive(Subcommand, Debug)]
enum RepoCommands {
    /// List all configured repositories
    List,
    /// Add a new repository (and immediately apply)
    Add { 
        url: String, 
        #[arg(long)]
        name: Option<String>, 
        #[arg(long, short = 'b')]
        branch: Option<String> 
    },
    /// Remove a repository
    Remove { 
        name: String, 
        #[arg(short, long)]
        files: bool 
    },
    /// Show detailed repository information
    Info { name: String },
    /// Enable a disabled repository
    Enable { name: String },
    /// Disable a repository temporarily
    Disable { name: String },
    /// Subdirectory management
    Subdirs {
        #[command(subcommand)]
        command: SubdirCommands,
    },
}

#[derive(Subcommand, Debug)]
enum SubdirCommands {
    /// List available subdirectories
    List { 
        name: String,
        #[arg(long)]
        active: bool 
    },
    /// Set active subdirectories
    Set { 
        name: String, 
        subdirs: Vec<String> 
    },
}
```

## Key Design Principles

1. **Respect Existing Patterns**: Use existing dependency injection with `&Config` and `&Database` references, not ownership
2. **Leverage Existing Structs**: Utilize `LocalRepo` and `DotfileDir` instead of creating redundant data structures  
3. **Minimal Abstraction**: RepositoryManager is a temporary helper for centralizing iteration, not a long-lived owner of data
4. **Consistent Error Handling**: Follow existing error handling patterns throughout the codebase
5. **Preserve Functionality**: Ensure all existing capabilities are maintained during refactoring, although breaking backward compatibility in terms of interfaces is allowed
6. **Borrowed References**: All RepositoryManager methods use borrowed references to maintain consistency with existing codebase patterns

