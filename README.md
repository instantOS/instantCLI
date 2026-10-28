# InstantCLI

[![License: GPL v2](https://img.shields.io/badge/License-GPL%20v2-blue.svg)](https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

# DOCS LIVE AT [instantos.io/docs/ins](https://instantos.io/docs/ins.html)

A powerful, Rust-based command-line tool for managing dotfiles,
system diagnostics, WM keychords, game saves and much more.

## Install with npm

On Linux x64 or ARM64 with Node.js 18 or newer:

```sh
npm install -g @instantos/cli
ins --help
```

See [npm packaging and CI setup](npm/PUBLISHING.md) for release configuration.

## Alpine and postmarketOS settings

`ins settings` uses apk on Alpine, postmarketOS, and distributions declaring
`ID_LIKE=alpine`. You can browse, install, preview, and remove system packages,
and install supported applications from the curated settings menus. Package
changes run directly as root, or through sudo/doas for regular users. On minimal
Alpine installations, settings installs Bash for its menu previews before opening.

## Alpine packages

Releases include `.apk` packages for x86_64 and ARM64 (aarch64), built from the
static musl binaries. Download the package and its `.sha256` file from
[GitHub releases](https://github.com/instantOS/instantCLI/releases), then verify
and install them (replace the version and architecture as needed):

```sh
sha256sum -c ins-0.14.23-r0-aarch64.apk.sha256
# As root:
apk add --no-cache --allow-untrusted ./ins-0.14.23-r0-aarch64.apk
```

These standalone packages use a temporary CI signing key, rather than a trusted
Alpine repository key, so local installation requires `--allow-untrusted`.
See [Alpine packaging](apk/README.md) for local build instructions.

## Features

- dotfile management
- system diagnostics
- WM keychord management
- game save management
- notification center
- [password and OTP management](https://instantos.io/docs/pass) via `ins pass`
- Arch Linux Installer
- video editing (yes, I know it's random)

### Display menu

`ins display` opens an instantMENU picker with Mirror, Extend, one “Only display”
entry per connected output, and Other… (wdisplays on Wayland, arandr on X11).
instantWM binds it to Super+Shift+P by default. Both instantWM backends are
supported; other compositors can be added through the display provider interface.

Requires instantMENU and an instantWM version with `monitor modes all`. Disabled
and mirrored outputs remain selectable. Presets use each display’s highest
resolution and refresh rate, normal orientation, and 100% scale; Extend arranges
displays left to right with the internal panel first. Changes apply to the current
session. X11 mirroring uses the largest resolution shared by all displays.
Escape closes the menu without changing the layout.

### Emoji picker

`ins assist run e` opens a searchable instantMENU emoji picker and copies the
selection to the clipboard. Enter selects; Ctrl-Enter adds more emoji before
Enter finishes; Escape cancels without changing the clipboard. Recent/frequent
selections rise to the top.

This replaces Emote outright. It requires the current Rust instantMENU (including
`value=` metadata and `--frecency-cache`), clipboard utilities (`wl-copy` on
Wayland, `xclip` on X11), and an emoji-capable font. Install instantMENU from its
[releases](https://github.com/instantOS/instantMENU/releases) or source if it is
not available through your package manager. There is no Emote/legacy-menu fallback.

The full Unicode 17 English-name catalog is embedded as ~157 KiB of plain UTF-8
(including its license), so no download is needed at build time or first use.
See [catalog provenance and regeneration](src/assist/actions/emoji/README.md).

### Notification history

`ins notify` browses notification history (`ins notify --gui` opens it in a
dedicated terminal window), while `ins notify list`, `count`,
`read`, `unread`, and `delete` provide scriptable access. History capture runs
as a separate session process:

```bash
# Packaged installs: supervised background capture and login autostart
ins notify enable

# Inspect or disable it later
ins notify status
ins notify disable

# Binary-only installs
ins notify daemon
```

When capture is not running, the interactive notification center also offers
an explicit action to enable and start the packaged service. `daemon` stays in
the foreground by design; background lifecycle and restart handling belong to
the systemd user service rather than a self-forking process.

The history database defaults to 1,000 entries and can be changed from the
interactive notification options menu. Transient notifications are not stored.

Notification actions (for example, Bluetooth pairing approval) must be invoked
while the original notification is live. `ins notify` records the advertised
actions and their live/expired state, but it does not replay expired actions:
the desktop notification protocol invalidates them when the notification
closes. Configure the notification daemon to invoke them directly:

```ini
# dunst: ~/.config/dunst/dunstrc.d/90-actions.conf
[global]
mouse_left_click = do_action,close_current
mouse_middle_click = context,close_current
# `context` uses the configured dmenu-compatible chooser.
```

```ini
# mako: ~/.config/mako/config
on-button-left=invoke-default-action
on-button-middle=exec makoctl menu -n "$id" -- wmenu -p 'Select action: '
```

Opening `ins notify --gui` from a separate key binding is useful for history,
but replacing an action click with it would discard the live application
callback. Reload dunst with `dunstctl reload`; reload mako with `makoctl reload`.

### Removed dotfiles

`ins dot apply` and `ins dot update` reconcile files that were previously
applied from a dotfile repository:

- If a source deletion is committed to the repository and no other active
  source provides the target, an unchanged target is removed.
- A locally modified target is preserved and becomes unmanaged.
- Staged or unstaged source deletions do not remove targets.
- Disabled, removed, unreadable, or failed-to-update repositories do not
  trigger target deletion.
- Normal `ins dot update --include-root` and `ins dot apply --include-root`
  delegate root reconciliation while root sources still exist. If every root
  source has already been removed, no sudo child is spawned solely for stale
  tracking records; run `ins dot apply --root-only` explicitly to reconcile
  those final root targets.

Tracking starts when a source and target are first confirmed identical after
upgrading. Sources that were already deleted before this tracking state was
recorded cannot be reconciled safely.

Dotfile status is content-based: a target containing a known previous source
version is reported as outdated whenever it differs from the effective source,
regardless of file modification times.

## Installation

### Quick install

```bash
curl -fsSL https://raw.githubusercontent.com/instantOS/instantCLI/main/scripts/install.sh | sh
```

Set `INSTALL_DIR` to override the destination directory. Non-root installs use
`~/.local/bin` or `~/bin` when either is already in `PATH`, otherwise
`~/.local/bin`. Root and instantOS live-system installs use `/usr/local/bin`.

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

#### Alpine / postmarketOS

On musl-based systems the default Rust target is statically linked, so the
static SQLite library is required in addition to the development headers:

```bash
sudo apk add git fzf restic sqlite sqlite-dev sqlite-static pkgconfig openssl-dev libgit2-dev
```

Missing optional tools can be installed through the usual dependency confirmation
prompt on Alpine and postmarketOS. APK uses `sudo`, or `doas` when `sudo` is
unavailable; root runs `apk` directly. Package mappings cover Assist tools, the
video pipeline, password-store/OTP, backups, and supported settings applications.
APK installs and removals wait up to 60 seconds for its database lock. If an
interactive dependency installation fails, choose Retry to rerun the failed
package group or Cancel to stop. Completed groups are not repeated; unattended
installs report failures without opening a recovery prompt.
Availability depends on the Alpine branch, architecture, and enabled repositories;
many desktop tools require `community`. Packages without a verified APK mapping
retain their existing fallback or are reported as unavailable.

To recheck all explicit APK package names against Alpine's published `main` and
`community` indexes without installing anything:

```sh
python3 scripts/verify-apk-dependencies.py
# Check another release or architecture:
python3 scripts/verify-apk-dependencies.py --branch edge --arch aarch64
```

The default check targets Alpine v3.24 on both aarch64 and x86_64. Non-obvious
mappings include `libqrencode-tools` for `qrencode`, `pandoc-cli` for `pandoc`,
`ntfs-3g-progs` for `ntfsresize`, and `tesseract-ocr-data-eng` for OCR with English
language data (which also installs the Tesseract engine).

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

## Terminal icons

Linux virtual consoles automatically use single-cell ASCII icon fallbacks,
including when running tmux on a console. Status and navigation keep meaningful
symbols (`+`, `x`, `!`, `<`, `>`); decorative icons use `*` beside their label.
This detection is independent of `INS_COLOR_MODE`.

Use `INS_ICON_MODE=ascii ins ...` to enable the fallback in any terminal without
Nerd Fonts, or `INS_ICON_MODE=nerd ins ...` to force the original glyphs.
Unset the variable (or use `auto`) for automatic detection.

## Menu action bindings

```sh
printf '%s\n' 'alpha' 'beta' |
  ins menu choice --bind 'ctrl-e:Edit' --bind 'alt-s:Save'
```

`ins menu choice --bind KEY:LABEL` registers a global action and a visible key
hint. Repeat the option for more actions. It works with native instantMENU,
local fzf (`--backend tui`), and the hosted terminal (`--backend scratchpad`),
including streamed input and `--multi`. `--items 'alpha beta'` also works.
Use `--frecency-cache NAME` to retain selection ranking; terminal menus with
frecency collect input before ranking, as usual.

With any bindings registered, stdout begins with the pressed key, or an empty
line for normal submission, followed by selected values. Ctrl-E on beta prints
`ctrl-e\nbeta\n`. Actions also work with no matches and then return only the key.
Escape returns exit status 2 without a result; success returns 0. The caller
handles the action; neither keys nor labels execute shell commands. Native
multi-selection uses Ctrl-Return to accumulate items before the final action.
Without `--bind`, the existing output format is unchanged.

Keys follow the fzf wrapper's validated menu vocabulary (`ctrl-e`, `alt-s`,
`ctrl-alt-r`, `f3`, `shift-left`, etc.). Submission, dismissal, and core navigation
keys are reserved. Labels may contain spaces and colons. Invalid labels, unsafe
key expressions, and duplicate bindings are rejected before launching a menu.
Install the updated instantMENU binary when using the native backend. This
change advances the hosted menu protocol to version 5; the client restarts an
older scratchpad server through the existing compatibility mechanism.

## Installer TTY fonts

`ins arch install` asks for a console font near the start, before the keyboard
layout. The standard 8x16 font is preselected and marked Default. Available
Sun and Terminus fonts appear as suggested choices; “All fonts” lists discovered
PSF/PSFU fonts, including gzip files, custom fonts, and fonts in subdirectories.
Sizes and glyph counts are read from the font files.

On a Linux virtual console (also through tmux), moving through the font menu
changes the font live. Enter reapplies and keeps the selected font for the
installation; cancelling restores the previous font and Unicode map. In Kitty
or another graphical terminal, previews describe the choices without changing
the terminal font.

The selected font and its Unicode table are saved with the installer answers.
The installer installs `kbd`, copies the selected font to
`/usr/share/kbd/consolefonts/ins-selected.psf`, and writes `FONT=ins-selected`
alongside `KEYMAP=` in the installed system's `/etc/vconsole.conf`. This also
works for custom fonts without an owning package and saved choices whose
original font files are no longer available. Reviews and support reports show
only the font name. Older plain-name answers still install their original font
package; older answers without a font choice use the default 8x16 font.

Compressed font discovery uses the system's `gzip` command. No additional Rust
dependencies are required.

## Default terminal

Terminal launches use [instantos-terminal](https://github.com/instantOS/instantOS-libs).
Choose a default in `ins settings`, or override it with `TERMINAL=foot` or
`TERMINAL=alacritty`. Scratchpads follow this setting unless `--terminal` is
explicitly supplied. The old hardcoded kitty default has been removed.

```sh
ins terminal
ins terminal --title Notes -- nvim notes.md
ins terminal --terminal foot -- sh -c 'printf "hello\n"; read answer'
ins terminal --ensure-default
```

An explicit terminal override is strict. Ordinary launches try the configured
alias directly and fall back only if execution reports a missing executable.
Fallback does not overwrite the saved preference. Terminal options and command
arguments are kept separate; use `--` before the command.

Scratchpad commands take program arguments after `--`. Shell interpretation is
explicit; the old `--command` shell-string option has been removed:

```sh
ins scratchpad show --terminal foot -- nvim 'notes with spaces.md'
ins scratchpad show --shell-command 'printf "hello\n"; read answer'
```

## Application launcher

`ins launch` discovers desktop applications and remembers selections by their
stable desktop-entry IDs. Entries with `Terminal=true` use the configured
terminal. Use `--include-path` to include executable commands, or `--terminal`
for a command-only menu that runs the selection in a terminal:

```sh
ins launch
ins launch --include-path --query firefox
ins launch --terminal --query nvim
ins launch --list
```

`--query` is supported by the instantMENU, TUI and scratchpad backends.
Use `--backend instantmenu`, `--backend tui` or `--backend scratchpad` to select
one explicitly. The hosted menu protocol is now version 9; restart an older
menu server when upgrading.
