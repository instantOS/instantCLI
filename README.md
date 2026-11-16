# InstantCLI

[![License: GPL v2](https://img.shields.io/badge/License-GPL%20v2-blue.svg)](https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html)

**A Swiss Army knife of [instantOS](https://instantos.io) utilities.**

A single utility to manage all your Linux needs (and some more).

It includes an Arch Linux installer, dotfile management, a cross-distro and
cross-WM settings TUI, a clipboard manager, a password manager based on
[pass](https://www.passwordstore.org/) and much more, all in a single small and
fast executable. 


[Documentation](https://instantos.io/docs/ins.html) ·
[Releases](https://github.com/instantOS/instantCLI/releases)

## Installation

### Recommended: install script

Download and install a prebuilt binary for your system:

```sh
curl -fsSL https://raw.githubusercontent.com/instantOS/instantCLI/main/scripts/install.sh | sh
ins --help
```
[Inspect the script](scripts/install.sh) before running it. 

On Arch/instantOS install live media, the script also launches the OS installer. To
install only the CLI there, pass `--cli-only` to `sh`:

```sh
curl -fsSL https://raw.githubusercontent.com/instantOS/instantCLI/main/scripts/install.sh | sh -s -- --cli-only
```

### Other installation options

**Arch Linux (AUR):**

```sh
yay -S ins
```

**Cargo:** requires a Rust toolchain and native build dependencies (see
[Development](#development)).

```sh
cargo install --git https://github.com/instantOS/instantCLI --locked ins
```

**npm:** Linux x64 or ARM64 with Node.js 18 or newer. This installs the binary;
external tools and desktop integration files must be installed separately.

```sh
npm install -g @instantos/cli
```

See the [npm README](npm/README.md) for details.

**Manual download:** prebuilt binaries and Debian/Alpine packages are available
from [GitHub releases](https://github.com/instantOS/instantCLI/releases).


## Usage

Run `ins --help` to see all commands, or add `--help` to a command for its options:

```sh
ins dot --help
ins game --help
ins video --help
```

| Command | What it does |
| --- | --- |
| `ins dot` | Manage and apply dotfile repositories |
| `ins doctor` | Check for system problems and offer fixes |
| `ins settings` | Browse desktop and system settings |
| `ins game` | Back up and sync game saves |
| `ins pass` | Pick passwords and generate one-time codes |
| `ins notify` | Browse notification history |
| `ins clip` | Browse clipboard history |
| `ins launch` | Search and launch applications |
| `ins open FILE` | Choose an application to open a file |
| `ins open --instantmenu FILE` | Choose an application in a compact graphical menu |
| `ins display` | Choose a display layout with instantWM |
| `ins keyhelp` | Explore instantWM keybindings |
| `ins assist` | Open quick desktop actions |
| `ins menu` | Build interactive menus for shell scripts |
| `ins terminal` | Launch your configured terminal |
| `ins arch install` | Install Arch Linux or instantOS |
| `ins video` | Edit video through text transcripts |

`ins open --instantmenu FILE` requires `instantmenu` and an X11 or Wayland session.
It shares application ranking and history with the terminal picker: Enter opens,
Ctrl+D immediately sets the selected application as the default for the file type,
Ctrl+E chooses a custom executable using the hosted Yazi picker, and Ctrl+X
makes the opened file executable for its owner and runs it directly (including
AppImages and scripts). Unrecognized formats, unavailable script interpreters,
and binaries for other CPU architectures require confirmation before permissions
change. Ctrl+X is available for regular files only. The graphical
menu omits previews; `--gui` opens the full terminal picker instead.

The `desktop/ins-open.desktop` entry is installed by `just install`, `just rootinstall`,
and the Arch package. It makes **INS Open** available in file managers' “Open With” dialogs for
common file types and folders. To install the entry manually:

```sh
install -Dm644 desktop/ins-open.desktop ~/.local/share/applications/ins-open.desktop
update-desktop-database ~/.local/share/applications
```

Choose INS Open as the default in your file manager, or run
`xdg-mime default ins-open.desktop text/plain` for a specific MIME type. Choosing
another default with Ctrl+D replaces that association. The chooser excludes its
own desktop entry to avoid opening itself recursively.

For guides, see the [documentation](https://instantos.io/docs/ins.html),
[password and OTP guide](https://instantos.io/docs/pass), and
[Arch installation prerequisites](ARCH_INSTALLATION.md).

### Runtime dependencies

Requirements vary by feature: terminal menus use fzf, dotfile management uses Git,
and game-save backups use restic. Commands can offer to install missing tools on
supported distributions. Desktop features may also require instantMENU, clipboard
utilities, or a notification service.

Rust is only needed when building from source or installing through Cargo.

## Development

Use a Rust toolchain supporting edition 2024, Git, a C toolchain, pkg-config,
and OpenSSL/SQLite development libraries. On Debian or Ubuntu:

```sh
sudo apt update
sudo apt install build-essential git pkg-config libssl-dev libsqlite3-dev
```

On Alpine/postmarketOS, also install the static SQLite library:

```sh
sudo apk add build-base git pkgconf openssl-dev sqlite-dev sqlite-static
```

Clone and build:

```sh
git clone https://github.com/instantOS/instantCLI.git
cd instantCLI
cargo build --bin ins
cargo run -- --help
```

With `just` installed, `just install` installs the debug binary and desktop
integration for the current user. `just rootinstall` installs the debug binary
to `/usr/local/bin` using sudo.

Check changes and run tests:

```sh
cargo fmt --check
cargo check
cargo test
just test
```

See [AGENTS.md](AGENTS.md) for repository conventions,
[installer sources](scripts/install-src/README.md) for work on the install script,
[release maintenance](RELEASING.md),
and [npm publishing](npm/PUBLISHING.md).

## License

[GPL-2.0-only](LICENSE).
