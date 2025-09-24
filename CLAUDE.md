# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

InstantCLI is a Rust-based command-line tool (v0.1.1) for managing dotfiles and instantOS configurations. It provides a decentralized approach to dotfile management that respects user modifications while enabling easy theme and configuration switching.

## Common Development Commands

### Building and Installation
```bash
# Build the project
cargo build

# Install locally (builds and copies to ~/.local/bin/)
just install

# Run tests
cargo test

# Run with debug output
cargo run -- --debug <command>
```

### Testing
```bash
# Run all tests
cargo test
just test

# Run specific test
cargo test test_apply_and_fetch

# Install locally for user testing (builds and copies to ~/.local/bin/)
just install
```

### User Testing
For testing purposes, you can install the CLI locally and use it as a normal user would:

```bash
# Install the CLI (builds and installs to ~/.local/bin/)
just install

# Test from home directory (simulates real user usage)
cd ~ && instant <command>

# Example test commands
cd ~ && instant dot status
cd ~ && instant --debug dot apply
cd ~ && instant dot reset .config
```

**Note**: Using `cd ~ && instant <command>` is the preferred testing method as it simulates how real users will interact with the CLI, avoiding working directory concerns and ensuring the tool behaves correctly in normal usage scenarios.

**Important Note**: Commands involving fzf interactive menus (such as `instant menu` commands and any dotfile operations that require user selection) should be run by the user directly. AI agents are not capable of interacting with fzf's interactive interface, so these commands must be executed manually by a human user.

**FZF Wrapper Enhancement**: The FzfWrapper has been enhanced to support multi-line messages via the `--header` argument and includes builder patterns for more ergonomic usage. Key improvements:

- **Multi-line support**: Added `header` field to `FzfOptions` struct, enabling multi-line text in dialogs
- **Builder patterns**: Added `FzfWrapperBuilder`, `ConfirmationDialogBuilder`, and `MessageDialogBuilder` for fluent configuration
- **Enhanced APIs**:
  - `FzfWrapper::confirm_builder()` - Create customizable confirmation dialogs with multi-line messages
  - `FzfWrapper::message_builder()` - Create message dialogs with titles and multi-line content
  - `FzfWrapper::builder()` - Build custom FZF configurations with fluent API
- **Backward compatibility**: All existing APIs continue to work unchanged
- **Ergonomic improvements**: Chain methods for configuration, custom button text, titles, and multi-line content

Example usage:
```rust
// Multi-line confirmation with custom styling
FzfWrapper::confirm_builder()
    .message("Are you sure you want to remove this game?\n\nThis action cannot be undone.")
    .yes_text("Remove Game")
    .no_text("Keep Game")
    .show()?

// Message with title
FzfWrapper::message_builder()
    .message("Operation completed successfully!")
    .title("Success")
    .show()?
```

## Architecture

### Core Components

1. **Main CLI** (`src/main.rs`): Command-line interface using clap, handles parsing and dispatching to subcommands

2. **Dotfile Management** (`src/dot/`): Core dotfile functionality
   - `mod.rs`: Main orchestration logic for dotfile operations
   - `config.rs`: Configuration management (TOML-based, stored in `~/.config/instant/instant.toml`)
   - `db.rs`: SQLite database for tracking file hashes and modifications
   - `dotfile.rs`: Core `Dotfile` struct with apply/fetch/reset operations
   - `git.rs`: Git repository operations (cloning, updating, status checking)
   - `localrepo.rs`: Local repository representation and management
   - `meta.rs`: Repository initialization and metadata handling
   - `repo/`: Repository management subdirectory
     - `cli.rs`: Repository CLI command definitions
     - `commands.rs`: Repository command handlers
     - `manager.rs`: Repository management logic
   - `utils.rs`: Utility functions
   - `path_serde.rs`: Path serialization/deserialization
   - `path_tests.rs`: Path-related tests

3. **System Diagnostics** (`src/doctor/`): System health checks and diagnostics
   - `mod.rs`: Doctor trait definitions and orchestration
   - `checks.rs`: Individual health check implementations

### Key Concepts

**Dotfile Structure**: 
- Repositories contain a `dots/` directory mirroring home directory structure
- Example: `dots/.config/kitty/kitty.conf` installs to `~/.config/kitty/kitty.conf`

**Multi-Repository Support**:
- Multiple dotfile repos can be configured with priority-based overlaying
- Later repos override earlier ones for the same file paths

**User Modification Protection**:
- SHA256 hashes track file states in SQLite database with explicit source/target distinction
- User-modified files are never overwritten automatically
- Files are only updated if they are determined to be unmodified using hash comparison

### Hash System Architecture

The hash system distinguishes between **source files** (in dotfile repositories) and **target files** (in home directory):

**Database Schema**:
```sql
CREATE TABLE file_hashes (
    created TEXT NOT NULL,
    hash TEXT NOT NULL, 
    path TEXT NOT NULL,
    source_file INTEGER NOT NULL,  -- true=source file, false=target file
    PRIMARY KEY (hash, path)
)
```

**DotFileType Enum**:
- `SourceFile` (serializes as `true`): Files in the dotfile repository (`~/.local/share/instantos/dots/`)
- `TargetFile` (serializes as `false`): Files in the home directory (`~/`)

**Key Concepts**:
- **Source Files**: Files in the dotfile repository (`~/.local/share/instantos/dots/`)
- **Target Files**: Files in the home directory (`~/`)
- **DotFileType Enum**: Explicitly tracks hash origin with clear semantics
- **Lazy Hash Computation**: Hashes are computed on-demand and cached with timestamp validation

**Modification Detection Logic**:
The `is_target_unmodified()` function determines if a target file can be safely overwritten:

**Returns true (safe to overwrite) if**:
1. **File doesn't exist** - Can be safely created
2. **File was created by instantCLI** - Hash matches any source file hash in database
3. **File matches current source** - Hash matches current source file hash

**Returns false (user modification detected) if**:
- File exists, has been modified by user, and doesn't match current source

**Purpose**: Protect user modifications while allowing safe updates of unmodified files and creation of new dotfiles.

**Hash Management**:
- Hashes are computed lazily when needed
- Database cache is validated against file modification timestamps
- Target files always stored with `DotFileType::TargetFile` (serializes as `false`)
- Source files always stored with `DotFileType::SourceFile` (serializes as `true`)

### Configuration Structure

TOML config at `~/.config/instant/instant.toml`:
```toml
clone_depth = 1  # Default git clone depth

[[repos]]
url = "https://github.com/user/dotfiles.git"
name = "my-dotfiles"
branch = "main"
```

## Important Agent Policies

**No Git Commits**: Do NOT create git commits. The repository has strict policies against automated commits. If changes need to be committed, ask the user for explicit permission.

**Compile After Changes**: Always compile the code after making changes to verify correctness with the compiler. Use `cargo check` for quick syntax checks or `cargo build` for full compilation.

**Hash-Based Safety**: All file operations respect the hash-based modification detection system. Never bypass this system as it protects user modifications. The system distinguishes between source files (repository copies) and target files (home directory installations) using the `source_file` database field.
You run in an environment where `ast-grep` is available; whenever a search requires syntax-aware or structural matching, default to `ast-grep --lang rust -p '<pattern>'` and avoid falling back to text-only tools like `rg` or `grep` unless I explicitly request a plain-text search.

**Web serach** always use firecrawl or fetch to do research on the web. 

**Config Locations**: 
- Config: `~/.config/instant/instant.toml`
- Database: `~/.local/share/instantos/instant.db` 
- Repos: `~/.local/share/instantos/dots/`

## Key Commands

### Dotfile Commands
- `instant dot apply`: Apply all dotfiles from configured repos
- `instant dot fetch [<path>]`: Fetch modified files from home directory back to repos
- `instant dot reset <path>`: Reset modified files to original state
- `instant dot update`: Update all configured repositories
- `instant dot status [<path>]`: Check repository status
- `instant dot init`: Initialize current directory as a dotfile repo
- `instant dot add <path>`: Add new dotfiles to tracking

### Repository Management Commands
- `instant dot repo add <url>`: Add a new dotfile repository
- `instant dot repo list`: List all configured repositories
- `instant dot repo remove <name>`: Remove a repository
- `instant dot repo info <name>`: Show detailed repository information
- `instant dot repo enable <name>`: Enable a disabled repository
- `instant dot repo disable <name>`: Disable a repository temporarily

### Subdirectory Management Commands
- `instant dot repo subdirs list <name>`: List available subdirectories
- `instant dot repo subdirs set <name> <subdirs...>`: Set active subdirectories

### Interactive Menu Commands
- `instant menu confirm --message "Are you sure?" --default "false"`: Show confirmation dialog
- `instant menu choice --prompt "Select an item:" --multi`: Show selection menu
- `instant menu input --prompt "Type a value:"`: Show text input dialog

### Scratchpad Commands
- `instant scratchpad toggle`: Toggle scratchpad terminal visibility (create if doesn't exist)
- `instant scratchpad show`: Show scratchpad terminal (create if doesn't exist)
- `instant scratchpad hide`: Hide scratchpad terminal
- `instant scratchpad status`: Check if scratchpad terminal is currently visible

**Named Scratchpads**: You can create multiple scratchpads with different names:
- `--name <NAME>`: Scratchpad name (default: "instantscratchpad"). Used as prefix for window class.

**Custom Commands**: Run specific applications inside the scratchpad:
- `--command <COMMAND>`: Command to run inside terminal (e.g., "fish", "ranger", "yazi")

**Configuration Options** (for toggle/show commands):
- `--terminal <TERMINAL>`: Terminal command to launch (default: "kitty")
- `--width-pct <WIDTH>`: Terminal width as percentage of screen (default: 50)
- `--height-pct <HEIGHT>`: Terminal height as percentage of screen (default: 60)

**Examples**:
```bash
# Default scratchpad
instant scratchpad show

# Named scratchpad with file manager
instant scratchpad show --name files --command ranger

# Multiple scratchpads
instant scratchpad show --name term1 --command fish
instant scratchpad show --name term2 --command zsh
instant scratchpad hide --name term1
```

### System Diagnostics
- `instant doctor`: Run system diagnostics and fixes

## Multiple Subdirectories Support

InstantCLI repositories can declare multiple subdirectories containing dotfiles, with configurable active subdirectories per repository.

### Repository Structure

Repositories can define multiple dots directories in their `instantdots.toml`:

```toml
name = "my-dotfiles"
description = "My personal dotfiles collection"
dots_dirs = ["dots", "themes", "configs"]
```

### Active Subdirectories Configuration

The global configuration can specify which subdirectories are active for each repository:

```toml
[[repos]]
url = "https://github.com/user/dotfiles.git"
name = "my-dotfiles"
active_subdirs = ["dots", "themes"]
```

### Subdirectory Management Commands

- `instant dot repo subdirs list <repo>`: List available subdirectories in a repository
- `instant dot repo subdirs set <repo> <subdirs...>`: Set active subdirectories for a repository

### Default Behavior

- If `dots_dirs` is not specified in `instantdots.toml`, defaults to `["dots"]`
- If `active_subdirs` is not specified in global config, defaults to `["dots"]`
- Only the first subdirectory is active by default to maintain backward compatibility
- Later repositories override earlier ones for the same file paths (overlay system)
