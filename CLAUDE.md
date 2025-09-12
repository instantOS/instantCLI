# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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

**Key Concepts**:
- **Source Files**: Files in `~/.local/share/instantos/dots/` (the repository copies)
- **Target Files**: Files in `~/` (the installed dotfiles in home directory)
- **Lazy Hash Computation**: Hashes are computed on-demand and cached with timestamp validation
- **Source File Flag**: Each hash entry explicitly tracks whether it came from source (`source_file=true`) or target (`source_file=false`)

**Modification Detection Logic**:
A target file is considered **unmodified** (safe to override) if either:
1. Its hash matches any source file hash in the database (indicating it was created by instantCLI)
2. Its hash matches the current source file hash (indicating it's in sync with current source)

A target file is considered **modified** (user-touched) only if:
- Its hash doesn't match any known source file hash
- AND it doesn't match the current source file hash

**Hash Management**:
- Hashes are computed lazily when needed
- Database cache is validated against file modification timestamps
- Target files always stored with `source_file=false`
- Source files always stored with `source_file=true`

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

**Hash-Based Safety**: All file operations respect the hash-based modification detection system. Never bypass this system as it protects user modifications. The system distinguishes between source files (repository copies) and target files (home directory installations) using the `source_file` database field.

**Config Locations**: 
- Config: `~/.config/instant/instant.toml`
- Database: `~/.local/share/instantos/instant.db` 
- Repos: `~/.local/share/instantos/dots/`

## Key Commands

- `instant dot clone <url>`: Add a new dotfile repository
- `instant dot apply`: Apply all dotfiles from configured repos
- `instant dot fetch [<path>]`: Fetch modified files from home directory back to repos
- `instant dot reset <path>`: Reset modified files to original state
- `instant dot update`: Update all configured repositories
- `instant dot status [<path>]`: Check repository status
- `instant dot init`: Initialize current directory as a dotfile repo

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

- `instant dot list-subdirs <repo>`: List available subdirectories in a repository
- `instant dot set-subdirs <repo> <subdirs...>`: Set active subdirectories for a repository
- `instant dot show-subdirs <repo>`: Show currently active subdirectories for a repository

### Default Behavior

- If `dots_dirs` is not specified in `instantdots.toml`, defaults to `["dots"]`
- If `active_subdirs` is not specified in global config, defaults to `["dots"]`
- Only the first subdirectory is active by default to maintain backward compatibility
- Later repositories override earlier ones for the same file paths (overlay system)
