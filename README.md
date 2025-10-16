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

### Dependencies

- Rust
- Git
- FZF
- Restic
- SQLite3

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

