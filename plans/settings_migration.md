# Settings migration tracker

This document keeps track of the migration effort from the legacy `settings.sh`
implementation to the new `instant settings` Rust subcommand.

## ✅ Done in this iteration

- [x] Scaffolding for `instant settings` using the shared `FzfWrapper`
- [x] Persist settings in `~/.config/instant/settings.toml`
- [x] Added initial settings: autotheming toggle, animation toggle, clipboard
  manager toggle (with background process management), default layout chooser

## 🔄 Pending categories

| Category | Status | Notes |
| --- | --- | --- |
| Sound | ⬜️ not started | Requires translating notification sound handling, custom audio selection, and mute logic relying on external tools (`zenity`, `mpv`). |
| Display | ⬜️ not started | Needs integration with autorandr, brightness assist scripts, HIDPI handling, and lock timeout toggling. |
| Appearance (wallpaper) | ⬜️ not started | Legacy workflow depends on `instantwallpaper`, custom color generation, and logo toggles. |
| Network | ⬜️ not started | Port network applet autostart logic, IP diagnostics, and speed test helpers. |
| Applications | ⬜️ not started | Default application selection reads from shared data files and updates `xdg-mime`. |
| Mouse & Keyboard | ⬜️ not started | Requires slider UI replacements and integration with `instantmouse` helpers. |
| Users & Accounts | ⬜️ not started | Heavily dependent on `passwd`, `useradd`, and privilege escalation wrappers. |
| Advanced | ⬜️ not started | Various package installs (gufw, tlpui, grub-customizer) and service toggles need privileged workflows. |
| Language & Time | ⬜️ not started | Relies on `instantARCH` tooling, git cloning, and `timedatectl`. |
| Storage | ⬜️ not started | udiskie toggling and gnome-disks launcher still shell-based. |

## ❓ Open questions / clarifications needed

- Decide whether to preserve legacy `iconf` semantics or migrate downstream
  components to read from `settings.toml`.
- Determine strategy for long-running GUI launches (`arandr`, `pavucontrol`,
  etc.) so they do not block the CLI while still using ergonomic helpers.
- Clarify which package install pathways should be handled automatically vs
  prompting users (legacy scripts use `instantinstall`).

## 📌 Follow-up ideas

- Provide helper traits/builders to register new settings inline next to their
  implementation modules (e.g. `Toggle::new` builder) to reduce boilerplate.
- Consider telemetry/logging hooks so other components can react to setting
  changes without polling the TOML file.
