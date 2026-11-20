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
- Arch Linux and Artix Linux installer
- video editing (yes, I know it's random)

### Artix Linux support

`ins arch install` supports Artix Linux with OpenRC, runit, s6 and dinit. You can install
from the Artix live ISO or from a running Artix system onto a different disk; the running
system's pacman configuration is never rewritten. `ins arch setup` works on an existing
Artix system.

s6 installations require the current `s6-frontend` tooling. Legacy s6 service wrappers
are not supported.

Minimal mode installs vanilla Artix. The full instantOS profile uses
`artix-archlinux-support` with the Arch `extra` and `multilib` repositories. The installer does
not configure Plymouth on Artix.

### Reviewing possible non-speech in video transcripts

`ins video convert` and `ins video append` compare transcript cues against a
local Silero speech-activity detector. Cues of at least half a second with little
detected speech get a review comment in the editable markdown:

```markdown
<!-- REVIEW: possible non-speech (background noise or silence); listen before keeping this cue. -->
`a#17@00:12.0-00:14.2` possibly garbled transcription
```

This is a hint to listen, not a verdict: quiet speech can be flagged, and some
noise can still look like speech. Nothing is automatically removed or muted;
comments do not enter subtitles or change cue IDs, word timings, or cuts.

The detector downloads a pinned model of about 1.2 MB on first use and runs on
CPU through the same `uv`/`transcribe-cpp` environment as Granite. Audio stays
local. Analysis is cached per recording and also works with existing or imported
transcripts, without rerunning speech-to-text. If analysis is unavailable, a
warning is logged and conversion continues without review comments. Existing
markdown is not modified automatically; regenerate with `convert --force` to
add comments (this overwrites that markdown, so preserve any edits first).

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
| `ins arch install` | Install Arch Linux, Artix Linux or instantOS |
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

Custom executables chosen with Ctrl+E are remembered per MIME type after a successful
launch and appear alongside desktop applications in the recommended list. Reusing
one updates its frecency score. Entries expire after 30 days without use; the cache
keeps at most 20 per MIME type and 100 overall, evicting the least recently used.
Missing or non-executable programs are pruned when the picker opens. Ctrl+D requires
a desktop entry.

The `desktop/ins-open.desktop` entry is installed by `just install`, `just rootinstall`,
and the Arch package. It makes **INS Open** available in file managers' “Open With” dialogs for
common file types and folders. To install the entry manually:

```sh
install -Dm644 desktop/ins-open.desktop ~/.local/share/applications/ins-open.desktop
update-desktop-database ~/.local/share/applications
```

Choose INS Open as the default in your file manager, or run
`xdg-mime default ins-open.desktop text/plain` for a specific MIME type. Choosing
another default with Ctrl+D replaces that association. INS Open also appears as
a normal choice in its own picker, so Ctrl+D can make it the default for a file
type that should always ask which application to use.

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
