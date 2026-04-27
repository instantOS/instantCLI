# InstantCLI

[![License: GPL v2](https://img.shields.io/badge/License-GPL%20v2-blue.svg)](https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

# DOCS LIVE AT [instantos.io/docs/ins](https://instantos.io/docs/ins.html)

A powerful, Rust-based command-line tool for managing dotfiles,
system diagnostics, WM keychords, game saves and much more.

## Features

- dotfile management
- system diagnostics
- WM keychord management
- game save management
- video editing (yes, I know it's random)

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

#### Arch

```bash
sudo pacman -Sy git fzf restic sqlite --needed
```

#### Ubuntu

```bash
sudo apt update; sudo apt install -y git fzf restic sqlite3 pkg-config libssl-dev libgit2-dev libsqlite3-dev
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

