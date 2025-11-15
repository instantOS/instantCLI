# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

InstantCLI is a Rust-based command-line tool (v0.1.9) for managing dotfiles, game saves, system settings, and instantOS configurations. It provides a decentralized approach to dotfile management that respects user modifications while enabling easy theme and configuration switching, along with comprehensive system management capabilities.

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
cd ~ && ins <command>

# Example test commands
cd ~ && ins dot status
cd ~ && ins --debug dot apply
cd ~ && ins dot reset .config
```

**Note**: Using `cd ~ && ins <command>` is the preferred testing method as it simulates how real users will interact with the CLI, avoiding working directory concerns and ensuring the tool behaves correctly in normal usage scenarios.

**Important Note**: Commands involving fzf interactive menus (such as `ins menu` commands and any dotfile operations that require user selection) should be run by the user directly. AI agents are not capable of interacting with fzf's interactive interface, so these commands must be executed manually by a human user.

**FZF Wrapper Enhancement**: The FzfWrapper has been enhanced with a unified builder pattern for all FZF operations. Key improvements:

- **Unified Builder Pattern**: Single `FzfBuilder` handles all dialog types (selection, input, password, confirmation, message)
- **Multi-line support**: Full support for multi-line headers and messages
- **Enhanced APIs**:
  - `FzfWrapper::builder()` - Create customizable FZF configurations with fluent API
  - `FzfWrapper::select_one()` - Quick single selection with default options
  - `FzfWrapper::select_many()` - Quick multi-selection with default options
  - `FzfWrapper::input()` - Text input dialog
  - `FzfWrapper::confirm_dialog()` - Confirmation dialog
  - `FzfWrapper::message_dialog()` - Message dialog
- **Streaming Support**: `select_streaming()` allows FZF to start showing results before command completes
- **Preview Support**: Built-in preview functionality for complex data structures
- **Backward compatibility**: All existing APIs continue to work unchanged

Example usage:
```rust
// Multi-line confirmation with custom styling
let result = FzfWrapper::builder()
    .confirm("Are you sure you want to remove this game?\n\nThis action cannot be undone.")
    .yes_text("Remove Game")
    .no_text("Keep Game")
    .confirm_dialog()?;

// Message with title
FzfWrapper::builder()
    .message("Operation completed successfully!")
    .title("Success")
    .message_dialog()?;

// Streaming selection from command output
let result = FzfWrapper::builder()
    .multi_select(true)
    .args(["--preview", "pacman -Sii {}"])
    .select_streaming("pacman -Slq")?;
```

## Architecture

### Core Components

1. **Main CLI** (`src/main.rs`): Command-line interface using clap, handles parsing and dispatching to subcommands

2. **Dotfile Management** (`src/dot/`): Core dotfile functionality
   - `mod.rs`: Main orchestration logic for dotfile operations
   - `config.rs`: Configuration management (TOML-based, stored in `~/.config/instant/dots.toml`)
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

3. **Game Save Management** (`src/game/`): Game save backup and synchronization using restic
   - `cli.rs`: Game command definitions
   - `commands.rs`: Game command handlers
   - `config.rs`: Game configuration management
   - `games/`: Game management logic
   - `operations/`: Game operations (launch, sync)
   - `repository/`: Repository management for game saves
   - `restic/`: Restic wrapper for backup operations
   - `setup.rs`: Game setup and configuration

4. **System Settings** (`src/settings/`): System configuration management
   - `mod.rs`: Settings module orchestration
   - `commands.rs`: Settings CLI command definitions
   - `registry.rs`: Settings definitions and metadata
   - `ui/`: Settings user interface components
   - `actions.rs`: Setting application logic
   - `store.rs`: Settings persistence
   - `apply.rs`: Settings application and reapplication

5. **Interactive Menus** (`src/menu/`): FZF-based interactive menu system
   - `mod.rs`: Menu command definitions and handlers
   - `server.rs`: Menu server for GUI integration
   - `client.rs`: Menu client for server communication
   - `protocol.rs`: Communication protocol definitions
   - `tui.rs`: Terminal user interface components

6. **System Diagnostics** (`src/doctor/`): System health checks and diagnostics
   - `mod.rs`: Doctor trait definitions and orchestration
   - `checks.rs`: Individual health check implementations
   - `command.rs`: Doctor command handlers
   - `registry.rs`: Health check registry

7. **Scratchpad Management** (`src/scratchpad/`): Terminal scratchpad functionality
   - `mod.rs`: Scratchpad command definitions
   - `operations.rs`: Scratchpad operations (show, hide, toggle)
   - `terminal.rs`: Terminal management
   - `visibility.rs`: Window visibility management

8. **Application Launcher** (`src/launch/`): Desktop application launcher
   - `mod.rs`: Launch command handlers
   - `desktop.rs`: Desktop file parsing
   - `execute.rs`: Application execution
   - `cache.rs`: Application cache management

9. **Development Tools** (`src/dev/`): Development utilities
   - `mod.rs`: Dev command definitions
   - `clone.rs`: Repository cloning utilities
   - `install.rs`: Installation helpers
   - `github.rs`: GitHub integration

10. **Restic Wrapper** (`src/restic/`): Backup system integration
    - `wrapper.rs`: Restic command wrapper
    - `error.rs`: Error handling for restic operations
    - `logging.rs`: Restic command logging

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
- `SourceFile` (serializes as `true`): Files in the dotfile repository (`~/.local/share/instant/dots/`)
- `TargetFile` (serializes as `false`): Files in the home directory (`~/`)

**Key Concepts**:
- **Source Files**: Files in the dotfile repository (`~/.local/share/instant/dots/`)
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

TOML config at `~/.config/instant/dots.toml`:
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
- Config: `~/.config/instant/dots.toml`
- Database: `~/.local/share/instant/instant.db`
- Repos: `~/.local/share/instant/dots/`

## Key Commands

### Dotfile Commands
- `ins dot apply`: Apply all dotfiles from configured repos
- `ins dot add <path>`: Add or update dotfiles
  - For a single file: If tracked, update the source file. If untracked, prompt to add it.
  - For a directory: Update all tracked files. Use `--all` to also add untracked files.
- `ins dot reset <path>`: Reset modified files to original state
- `ins dot update`: Update all configured repositories
- `ins dot status [<path>]`: Check repository status
- `ins dot init`: Initialize current directory as a dotfile repo
- `ins dot diff [<path>]`: Show differences between source and target files
- `ins dot repo clone <url>`: Clone a new dotfile repository
- `ins dot repo list`: List all configured repositories
- `ins dot repo remove <name>`: Remove a repository
- `ins dot repo info <name>`: Show detailed repository information
- `ins dot repo enable <name>`: Enable a disabled repository
- `ins dot repo disable <name>`: Disable a repository temporarily
- `ins dot repo subdirs list <name>`: List available subdirectories
- `ins dot repo subdirs set <name> <subdirs...>`: Set active subdirectories

### Game Save Management Commands
- `ins game init`: Initialize restic repository for game saves
- `ins game add`: Add a new game to track
- `ins game list`: List all tracked games
- `ins game remove <game>`: Remove a game from tracking
- `ins game backup [<game>]`: Backup game saves
- `ins game restore [<game>]`: Restore game saves from backup
- `ins game launch <game>`: Launch a game
- `ins game sync <game>`: Sync game saves (backup then restore latest)
- `ins game setup`: Set up games that have been added but not configured
- `ins game prune`: Clean up old backup snapshots

### System Settings Commands
- `ins settings`: Open interactive settings UI
- `ins settings apply`: Reapply settings that don't persist across reboots
- `ins settings list`: List available settings and categories
- `ins settings list --categories`: List only setting categories

### Interactive Menu Commands
- `ins menu confirm --message "Are you sure?"`: Show confirmation dialog
- `ins menu choice --prompt "Select an item:" --multi`: Show selection menu
- `ins menu input --prompt "Type a value:"`: Show text input dialog
- `ins menu password --prompt "Enter password:"`: Show password input dialog
- `ins menu server launch`: Launch menu server
- `ins menu server stop`: Stop menu server
- `ins menu status`: Get menu server status

### Scratchpad Commands
- `ins scratchpad toggle`: Toggle scratchpad terminal visibility (create if doesn't exist)
- `ins scratchpad show`: Show scratchpad terminal (create if doesn't exist)
- `ins scratchpad hide`: Hide scratchpad terminal
- `ins scratchpad status`: Check if scratchpad terminal is currently visible

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
ins scratchpad show
ins scratchpad show --name files --command ranger
ins scratchpad show --name term1 --command fish
ins scratchpad show --name term2 --command zsh
ins scratchpad hide --name term1
# Multiple scratchpads
ins scratchpad show --name term1 --command fish
ins scratchpad show --name term2 --command zsh
ins scratchpad hide --name term1
```

### Application Launcher Commands
- `ins launch`: Launch applications interactively
- `ins launch --list`: List available applications

### System Diagnostics Commands
- `ins doctor`: Run system diagnostics and fixes

### Development Commands
- `ins dev clone`: Clone development repositories
- `ins dev install`: Install development tools

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

- `ins dot repo subdirs list <repo>`: List available subdirectories in a repository
- `ins dot repo subdirs set <repo> <subdirs...>`: Set active subdirectories for a repository

### Default Behavior

- If `dots_dirs` is not specified in `instantdots.toml`, defaults to `["dots"]`
- If `active_subdirs` is not specified in global config, defaults to `["dots"]`
- Only the first subdirectory is active by default to maintain backward compatibility
- Later repositories override earlier ones for the same file paths (overlay system)
