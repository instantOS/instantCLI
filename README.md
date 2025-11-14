# InstantCLI

[![License: GPL v2](https://img.shields.io/badge/License-GPL%20v2-blue.svg)](https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

A powerful, Rust-based command-line tool for managing dotfiles, game saves,
system diagnostics, and instantOS configurations. InstantCLI provides a
decentralized approach to dotfile management that respects user modifications
while enabling easy theme and configuration switching.

## Features

### **Dotfile Management**
- **Multi-repository support** with priority-based overlaying
- **Smart modification detection** using hashes to protect user changes
- **Subdirectory management** for organizing different configuration sets like themes

### **Game Save Management**
- Centralized game save backup and restore
- Support for popular free cloud storage services
- Automatic save location detection

### **Application Launcher**
- Launch desktop applications
- Fuzzy finding
- Frecency (Higher ranking for frequently and recently used apps)
- Based on reusing Terminal windows using scratchpads


## Installation

### Quick install

```bash
curl -fsSL https://raw.githubusercontent.com/instantOS/instantCLI/main/scripts/install.sh | sh
```

Set `INSTALL_DIR` to override the destination directory (defaults to a writable user bin in your `PATH`, otherwise `/usr/local/bin`).

Check before you pipe `:)`

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

## Dotfile Management

### Dotfile Structure

`ins` expects dotfile repositories to have a specific structure:

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
├── mytheme/                   # Optional: theme-specific configs
│   └── .config/
│       └── kitty/
│           └── theme.conf
└── myothertheme/              # Optional: theme-specific configs
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

