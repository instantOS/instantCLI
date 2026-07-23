//! Executable aliases used by instantOS desktop integrations.
//!
//! Unlike MIME defaults, terminal applications do not have an XDG association
//! that other programs can query.  instantOS therefore exposes stable command
//! paths below `~/.config/instantos/default`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefaultCommand {
    Terminal,
    TerminalFileManager,
}

impl DefaultCommand {
    pub const fn alias(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::TerminalFileManager => "termfilemanager",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal Emulator",
            Self::TerminalFileManager => "Terminal File Manager",
        }
    }

    pub const fn candidates(self) -> &'static [&'static str] {
        match self {
            Self::Terminal => &[
                "kitty",
                "ghostty",
                "wezterm",
                "foot",
                "alacritty",
                "xterm",
                "st",
            ],
            Self::TerminalFileManager => &["yazi", "ranger", "lf", "nnn", "vifm", "mc"],
        }
    }
}

const DESKTOP_DEFAULTS: &[(&str, &[&str])] = &[
    (
        "filemanager",
        &["nautilus", "dolphin", "thunar", "pcmanfm", "pcmanfm-qt"],
    ),
    (
        "browser",
        &["firefox", "chromium", "google-chrome", "brave"],
    ),
    ("editor", &["nvim-qt", "neovide", "code", "gedit", "kate"]),
    ("appmenu", &["appmenu", "instantmenu_smartrun"]),
    (
        "lockscreen",
        &["ilock", "swaylock", "gtklock", "slock", "i3lock"],
    ),
    (
        "systemmonitor",
        &[
            "missioncenter",
            "mate-system-monitor",
            "gnome-system-monitor",
        ],
    ),
];

/// Create missing default-command aliases and repair broken symlinks.
///
/// This mutates the user's configuration and therefore belongs in explicit
/// setup/repair or session-apply paths, not in read-only UI initialization.
pub fn ensure_default_links() -> Result<()> {
    let default_dir = default_dir()?;
    fs::create_dir_all(&default_dir)
        .with_context(|| format!("creating {}", default_dir.display()))?;

    for command in [
        DefaultCommand::Terminal,
        DefaultCommand::TerminalFileManager,
    ] {
        ensure_alias(&default_dir, command.alias(), command.candidates())?;
    }
    for (alias, candidates) in DESKTOP_DEFAULTS {
        ensure_alias(&default_dir, alias, candidates)?;
    }

    Ok(())
}

pub fn ensure_default_links_complete() -> Result<()> {
    ensure_default_links()?;
    let default_dir = default_dir()?;
    let missing: Vec<&str> = [
        DefaultCommand::Terminal.alias(),
        DefaultCommand::TerminalFileManager.alias(),
    ]
    .into_iter()
    .chain(DESKTOP_DEFAULTS.iter().map(|(alias, _)| *alias))
    .filter(|alias| !is_executable(&default_dir.join(alias)))
    .collect();

    if !missing.is_empty() {
        bail!(
            "no installed default application could be found for: {}",
            missing.join(", ")
        );
    }
    Ok(())
}

pub fn installed_candidates(command: DefaultCommand) -> Vec<(String, PathBuf)> {
    command
        .candidates()
        .iter()
        .filter_map(|candidate| {
            which::which(candidate)
                .ok()
                .map(|path| ((*candidate).to_string(), path))
        })
        .collect()
}

pub fn current_command(command: DefaultCommand) -> Option<PathBuf> {
    let link = default_dir().ok()?.join(command.alias());
    fs::read_link(&link).ok().map(|target| {
        if target.is_absolute() {
            target
        } else {
            link.parent().unwrap_or_else(|| Path::new("")).join(target)
        }
    })
}

pub fn set_default_command(command: DefaultCommand, executable: &Path) -> Result<()> {
    if !executable.is_absolute() {
        bail!("default command target must be an absolute path");
    }

    let default_dir = default_dir()?;
    fs::create_dir_all(&default_dir)
        .with_context(|| format!("creating {}", default_dir.display()))?;
    replace_symlink(&default_dir.join(command.alias()), executable)
}

fn default_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("unable to determine home directory")?;
    Ok(home.join(".config/instantos/default"))
}

fn ensure_alias(default_dir: &Path, alias: &str, candidates: &[&str]) -> Result<()> {
    let link = default_dir.join(alias);
    match fs::symlink_metadata(&link) {
        Ok(metadata) if metadata.file_type().is_symlink() && !link.exists() => {
            fs::remove_file(&link)
                .with_context(|| format!("removing broken alias {}", link.display()))?;
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("inspecting alias {}", link.display()));
        }
    }

    if let Some(target) = candidates
        .iter()
        .find_map(|candidate| which::which(candidate).ok())
    {
        replace_symlink(&link, &target)?;
    }
    Ok(())
}

fn replace_symlink(link: &Path, target: &Path) -> Result<()> {
    match fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            bail!("cannot replace directory {}", link.display());
        }
        Ok(_) => fs::remove_file(link)
            .with_context(|| format!("removing existing alias {}", link.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("inspecting alias {}", link.display()));
        }
    }

    symlink(target, link).with_context(|| {
        format!(
            "linking default command {} to {}",
            link.display(),
            target.display()
        )
    })
}

fn is_executable(path: &Path) -> bool {
    match path.metadata() {
        Ok(metadata) if metadata.is_file() => metadata.permissions().mode() & 0o111 != 0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_alias_preserves_an_explicit_existing_entry() {
        let temp = tempfile::tempdir().unwrap();
        let explicit = temp.path().join("custom-terminal");
        fs::write(&explicit, "").unwrap();
        let alias = temp.path().join("terminal");
        symlink(&explicit, &alias).unwrap();

        ensure_alias(temp.path(), "terminal", &["definitely-not-a-real-command"]).unwrap();

        assert_eq!(fs::read_link(alias).unwrap(), explicit);
    }

    #[test]
    fn replace_symlink_updates_an_existing_alias() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::write(&first, "").unwrap();
        fs::write(&second, "").unwrap();
        let alias = temp.path().join("terminal");
        symlink(&first, &alias).unwrap();

        replace_symlink(&alias, &second).unwrap();

        assert_eq!(fs::read_link(alias).unwrap(), second);
    }

    #[test]
    fn ensure_alias_repairs_a_broken_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("terminal");
        fs::write(&executable, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let alias = temp.path().join("default-terminal");
        symlink(temp.path().join("missing"), &alias).unwrap();

        ensure_alias(
            temp.path(),
            "default-terminal",
            &[executable.to_str().unwrap()],
        )
        .unwrap();

        assert_eq!(fs::read_link(alias).unwrap(), executable);
    }
}
