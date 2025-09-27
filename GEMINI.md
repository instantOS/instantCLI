# GEMINI.md

This file provides guidance to Gemini when working with code in this repository.

## Project Overview

InstantCLI is a Rust-based command-line tool for managing dotfiles and instantOS configurations. It provides a decentralized approach to dotfile management that respects user modifications while enabling easy theme and configuration switching.

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

# Run specific test
cargo test test_apply_and_fetch
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

### Key Concepts

**Dotfile Structure**: 
- Repositories contain a `dots/` directory mirroring home directory structure
- Example: `dots/.config/kitty/kitty.conf` installs to `~/.config/kitty/kitty.conf`

**Multi-Repository Support**:
- Multiple dotfile repos can be configured with priority-based overlaying
- Later repos override earlier ones for the same file paths

**User Modification Protection**:
- SHA256 hashes track file states in SQLite database
- User-modified files are never overwritten automatically
- Files are only updated if they match known unmodified hashes

### Database Schema

SQLite database at `~/.local/share/instantos/instant.db`:
```sql
CREATE TABLE hashes (
    created TEXT NOT NULL,
    hash TEXT NOT NULL, 
    path TEXT NOT NULL,
    unmodified INTEGER NOT NULL,
    PRIMARY KEY (hash, path)
)
```

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

**Hash-Based Safety**: All file operations respect the hash-based modification detection system. Never bypass this system as it protects user modifications.

**Config Locations**: 
- Config: `~/.config/instant/instant.toml`
- Database: `~/.local/share/instantos/instant.db` 
- Repos: `~/.local/share/instantos/dots/`

## Key Commands

- `ins dot clone <url> [--name <name>] [--branch <branch>]`: Add a new dotfile repository
- `ins dot apply`: Apply all dotfiles from configured repos
- `ins dot fetch [<path>]`: Fetch modified files from home directory back to repos
- `ins dot reset <path>`: Reset modified files to original state
- `ins dot update`: Update all configured repositories
- `ins dot status [<path>]`: Check repository status
- `ins dot init [<name>]`: Initialize current directory as a dotfile repo
- `ins dot add <path>`: Add new dotfiles to tracking
- `ins dot remove <repo> [--files]`: Remove a repository from configuration
- `ins dot list-subdirs <repo>`: List available subdirectories in a repository
- `ins dot set-subdirs <repo> <subdirs...>`: Set active subdirectories for a repository
- `ins dot show-subdirs <repo>`: Show currently active subdirectories for a repository
- `ins dot list-subdirs <repo>`: List available subdirectories in a repository
- `ins dot set-subdirs <repo> <subdirs...>`: Set active subdirectories for a repository
- `ins dot show-subdirs <repo>`: Show currently active subdirectories for a repository
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

- `ins dot list-subdirs <repo>`: List available subdirectories in a repository
- `ins dot set-subdirs <repo> <subdirs...>`: Set active subdirectories for a repository
- `ins dot show-subdirs <repo>`: Show currently active subdirectories for a repository

### Default Behavior

- If `dots_dirs` is not specified in `instantdots.toml`, defaults to `["dots"]`
- If `active_subdirs` is not specified in global config, defaults to `["dots"]`
- Only the first subdirectory is active by default to maintain backward compatibility
- Later repositories override earlier ones for the same file paths (overlay system)
