# GEMINI.md

This file provides guidance to Gemini when working with code in this repository.

## Project Overview

InstantCLI is a Rust-based command-line tool (v0.1.10) for managing dotfiles, game saves, system settings, and instantOS configurations. It provides a decentralized approach to dotfile management that respects user modifications while enabling easy theme and configuration switching, along with comprehensive system management capabilities.

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

## Architecture

### Core Components

1. **Main CLI** (`src/main.rs`): Command-line interface using clap, handles parsing and dispatching to subcommands

2. **Completions** (`src/completions/`): Shell completion generation
   - `mod.rs`: Completion generation logic

3. **Dotfile Management** (`src/dot/`): Core dotfile functionality
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

4. **Game Save Management** (`src/game/`): Game save backup and synchronization using restic
   - `cli.rs`: Game command definitions
   - `commands.rs`: Game command handlers
   - `config.rs`: Game configuration management
   - `games/`: Game management logic
   - `operations/`: Game operations (launch, sync)
   - `repository/`: Repository management for game saves
   - `restic/`: Restic wrapper for backup operations

5. **System Settings** (`src/settings/`): System configuration management
   - `commands.rs`: Settings CLI command definitions
   - `registry.rs`: Settings definitions and metadata
   - `ui/`: Settings user interface components
   - `actions.rs`: Setting application logic
   - `store.rs`: Settings persistence

6. **Interactive Menus** (`src/menu/`): FZF-based interactive menu system
   - `mod.rs`: Menu command definitions and handlers
   - `server.rs`: Menu server for GUI integration
   - `client.rs`: Menu client for server communication
   - `protocol.rs`: Communication protocol definitions

7. **System Diagnostics** (`src/doctor/`): System health checks and diagnostics
   - `mod.rs`: Doctor trait definitions and orchestration
   - `checks.rs`: Individual health check implementations
   - `command.rs`: Doctor command handlers

8. **Scratchpad Management** (`src/scratchpad/`): Terminal scratchpad functionality
   - `mod.rs`: Scratchpad command definitions
   - `operations.rs`: Scratchpad operations (show, hide, toggle)
   - `terminal.rs`: Terminal management

9. **Application Launcher** (`src/launch/`): Desktop application launcher
   - `mod.rs`: Launch command handlers
   - `desktop.rs`: Desktop file parsing
   - `execute.rs`: Application execution

10. **Development Tools** (`src/dev/`): Development utilities
    - `mod.rs`: Dev command definitions
    - `clone.rs`: Repository cloning utilities
    - `install.rs`: Installation helpers

11. **Restic Wrapper** (`src/restic/`): Backup system integration
    - `wrapper.rs`: Restic command wrapper
    - `error.rs`: Error handling for restic operations
    - `logging.rs`: Restic command logging

12. **Video Editing** (`src/video/`): Video editing tools
    - `cli.rs`: Video command definitions
    - `commands.rs`: Video command handlers
    - `convert.rs`: Transcript to markdown conversion
    - `render.rs`: Markdown to video rendering
    - `transcribe.rs`: Video to transcript generation

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

**Modification Detection Logic**:
The `is_target_unmodified()` function determines if a target file can be safely overwritten:

**Returns true (safe to overwrite) if**:
1. **File doesn't exist** - Can be safely created
2. **File was created by instantCLI** - Hash matches any source file hash in database
3. **File matches current source** - Hash matches current source file hash

**Returns false (user modification detected) if**:
- File exists, has been modified by user, and doesn't match current source

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

**No Git Commits**: Do NOT create git commits. Never ever create git commits. The repository has strict policies against automated commits.
**Never execute git commands**: Do NOT execute git commands.

**Compile After Changes**: Always compile the code after making changes to verify correctness with the compiler. Use `cargo check` for quick syntax checks or `cargo build` for full compilation.

**Hash-Based Safety**: All file operations respect the hash-based modification detection system. Never bypass this system as it protects user modifications. The system distinguishes between source files (repository copies) and target files (home directory installations) using the `source_file` database field.

**Config Locations**: 
- Config: `~/.config/instant/instant.toml`
- Database: `~/.local/share/instantos/instant.db` 
- Repos: `~/.local/share/instantos/dots/`

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
- `ins dot ignore add <path>`: Add a path to the ignore list (prevents apply/update)
- `ins dot ignore remove <path>`: Remove a path from the ignore list
- `ins dot ignore list`: List all currently ignored paths

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

### Interactive Menu Commands
- `ins menu confirm --message "Are you sure?"`: Show confirmation dialog
- `ins menu choice --prompt "Select an item:" --multi`: Show selection menu
- `ins menu input --prompt "Type a value:"`: Show text input dialog
- `ins menu password --prompt "Enter password:"`: Show password input dialog
- `ins menu server launch`: Launch menu server
- `ins menu server stop`: Stop menu server

### Scratchpad Commands
- `ins scratchpad toggle`: Toggle scratchpad terminal visibility
- `ins scratchpad show`: Show scratchpad terminal
- `ins scratchpad hide`: Hide scratchpad terminal
- `ins scratchpad status`: Check if scratchpad terminal is currently visible

### Application Launcher Commands
- `ins launch`: Launch applications interactively
- `ins launch --list`: List available applications

### System Diagnostics Commands
- `ins doctor`: Run system diagnostics and fixes

### Completion Commands
- `ins completion generate <shell>`: Generate shell completions for a given shell
- `ins completion install <shell>`: Install shell completions for a given shell

### Development Commands
- `ins dev clone`: Clone development repositories
- `ins dev install`: Install development tools

### Video Editing Commands
- `ins video convert <video> [--transcript <transcript>] [--out-file <out-file>]`: Convert a timestamped transcript into editable video markdown
- `ins video transcribe <video> [--compute-type <type>] [--device <device>] [--model <model>]`: Generate a transcript for a video using WhisperX
- `ins video render <markdown> [--out-file <out-file>] [--force] [--precache-titlecards] [--dry-run]`: Render a video according to edits in a markdown file
- `ins video titlecard <markdown> [--out-file <out-file>]`: Generate a title card image from a markdown file
- `ins video stats <markdown>`: Display statistics about how a markdown file will be rendered

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

### Default Behavior

- If `dots_dirs` is not specified in `instantdots.toml`, defaults to `["dots"]`
- If `active_subdirs` is not specified in global config, defaults to `["dots"]`
- Only the first subdirectory is active by default to maintain backward compatibility
- Later repositories override earlier ones for the same file paths (overlay system)

## Path Ignoring

Sometimes you may want to prevent certain dotfiles from being applied on a specific machine, even if they exist in your dotfile repositories. The ignore functionality allows you to maintain a local list of paths that should be skipped during `ins dot apply`.

### Use Cases

- **Machine-specific exclusions**: Ignore dotfiles that don't make sense on a particular machine (e.g., ignore GUI configs on a headless server)
- **Prevent overwrites**: Exclude files you've intentionally deleted locally and don't want restored
- **Temporary exclusions**: Temporarily ignore specific configs while testing alternatives

### Managing Ignored Paths

```bash
# Add a path to ignore list (supports both files and directories)
ins dot ignore add ~/.config/nvim
ins dot ignore add .bashrc

# Remove a path from ignore list
ins dot ignore remove ~/.config/nvim

# List all ignored paths
ins dot ignore list
```

### Path Formats

The ignore command accepts paths in multiple formats:
- Tilde notation: `~/.config/nvim` or `~/.bashrc`
- Relative paths: `.config/nvim` or `.bashrc` (automatically prefixed with `~/`)
- Absolute paths: `/home/user/.config/nvim` (converted to tilde notation)

### Behavior

- Ignored paths are stored in your dotfiles configuration (`~/.config/instant/dots.toml`)
- When you run `ins dot apply`, any dotfiles matching ignored paths are skipped
- Directory ignores apply recursively (ignoring `~/.config/nvim` ignores all files under that directory)
- Ignored paths are local to each machine and not synced with your dotfile repositories
