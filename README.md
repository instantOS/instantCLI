# InstantCLI

[![License: GPL v2](https://img.shields.io/badge/License-GPL%20v2-blue.svg)](https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

A powerful, Rust-based command-line tool for managing dotfiles, game saves,
system diagnostics, and instantOS configurations. InstantCLI provides a
decentralized approach to dotfile management that respects user modifications
while enabling easy theme and configuration switching.

## Features

### 🗂️ **Dotfile Management**
- **Multi-repository support** with priority-based overlaying
- **Smart modification detection** using hashes to protect user changes
- **Subdirectory management** for organizing different configuration sets like themes

### 🎮 **Game Save Management**
- Centralized game save backup and restore
- Automatic save location detection

### 🩺 **System Diagnostics**
- Comprehensive system health checks
- Automated fixes for common issues
- InstantOS-specific optimizations

### 🚀 **Application Launcher**
- Fast application discovery and launching
- Integration with system applications

### 📋 **Interactive Menus**
- FZF-powered interactive dialogs
- Confirmation prompts and selection menus
- Shell script integration utilities

### 🖥️ **Scratchpad Terminal**
- Toggle-able floating terminal windows
- Named scratchpads for different workflows
- Custom terminal and sizing options

## Installation

### Cargo

```bash
cargo install ins
```

### AUR

```bash
yay -S ins
```

### From Source

```bash
# Clone the repository
git clone https://github.com/your-username/instantCLI.git
cd instantCLI

# Build and install locally
just install

# Or install system-wide (requires sudo)
just rootinstall
```

### Prerequisites

- Rust
- Git
- FZF
- Restic
- SQLite3

## Quick Start

### Initialize Dotfile Management

```bash
# Add a dotfile repository
ins dot repo add https://github.com/your-username/dotfiles.git

# Apply dotfiles to your system
ins dot apply

# Check status of your dotfiles
ins dot status
```

### Basic Usage Examples

```bash
# Show all available commands
ins --help

# Run system diagnostics
ins doctor

# Toggle scratchpad terminal
ins scratchpad toggle

# Launch an application
ins launch

# Show interactive confirmation dialog
ins menu confirm --message "Proceed with operation?"
```

## Configuration

InstantCLI uses a TOML configuration file located at `~/.config/instant/instant.toml`:

## Commands Reference

### Dotfile Commands

| Command | Description |
|---------|-------------|
| `ins dot apply` | Apply all dotfiles from configured repositories |
| `ins dot fetch [path]` | Fetch modified files from home directory back to repos |
| `ins dot reset <path>` | Reset modified files to original state |
| `ins dot update` | Update all configured repositories |
| `ins dot status [path]` | Check repository and file status |
| `ins dot init` | Initialize current directory as a dotfile repository |
| `ins dot add <path>` | Add new dotfiles to tracking |
| `ins dot diff [path]` | Show differences between files |

### Repository Management

| Command | Description |
|---------|-------------|
| `ins dot repo add <url>` | Add a new dotfile repository |
| `ins dot repo list` | List all configured repositories |
| `ins dot repo remove <name>` | Remove a repository |
| `ins dot repo info <name>` | Show detailed repository information |
| `ins dot repo enable/disable <name>` | Enable/disable a repository |

### Subdirectory Management

| Command | Description |
|---------|-------------|
| `ins dot repo subdirs list <name>` | List available subdirectories in a repository |
| `ins dot repo subdirs set <name> <subdirs...>` | Set active subdirectories in a repository |

### Scratchpad Terminal

| Command | Description |
|---------|-------------|
| `ins scratchpad toggle` | Toggle scratchpad visibility |
| `ins scratchpad show` | Show scratchpad terminal |
| `ins scratchpad hide` | Hide scratchpad terminal |
| `ins scratchpad status` | Check scratchpad status |

#### Scratchpad Options

- `--name <NAME>`: Create named scratchpads (default: "instantscratchpad")
- `--command <COMMAND>`: Run specific command inside terminal
- `--terminal <TERMINAL>`: Specify terminal application (default: "kitty")
- `--width-pct <WIDTH>`: Terminal width as percentage (default: 50)
- `--height-pct <HEIGHT>`: Terminal height as percentage (default: 60)

### Interactive Menu System

| Command | Description |
|---------|-------------|
| `ins menu confirm --message "text"` | Show confirmation dialog |
| `ins menu choice --prompt "text" --multi` | Show selection menu |
| `ins menu input --prompt "text"` | Show text input dialog |

### Other Commands

| Command | Description |
|---------|-------------|
| `ins doctor` | Run system diagnostics and fixes |
| `ins game` | Game save management commands |
| `ins launch` | Application launcher |
| `ins dev` | Development utilities |
| `ins completions` | Shell completion helpers |

## How It Works

### Dotfile Structure

InstantCLI expects dotfile repositories to have a specific structure:

```
your-dotfiles-repo/
├── instantdots.toml          # Repository metadata
├── dots/                     # Main dotfiles directory
│   ├── .config/
│   │   ├── kitty/
│   │   │   └── kitty.conf
│   │   └── nvim/
│   │       └── init.vim
│   └── .bashrc
├── themes/                   # Optional: theme-specific configs
│   └── .config/
│       └── kitty/
│           └── theme.conf
└── configs/                  # Optional: additional configurations
    └── ...
```

### Multi-Repository Support

- **Priority-based overlaying**: Later repositories override earlier ones for conflicting files
- **Selective activation**: Choose which subdirectories are active per repository
- **Independent updates**: Each repository can be updated and managed separately

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run with debug logging
cargo run -- --debug <command>
```

### Testing

```bash
# Run all tests
cargo test

# Run integration tests
just test
```

