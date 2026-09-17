//! Guided selection of an rclone account and a backup folder within it.

use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};

use crate::common::package::{
    Dependency, InstallResult, PackageDefinition, PackageManager, ensure_all,
};
use crate::common::requirements::InstallTest;
use crate::menu_utils::{DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::nerd_font::NerdFont;

static RCLONE: Dependency = Dependency {
    name: "rclone",
    packages: &[
        PackageDefinition::new("rclone", PackageManager::Pacman),
        PackageDefinition::new("rclone", PackageManager::Apt),
        PackageDefinition::new("rclone", PackageManager::Dnf),
        PackageDefinition::new("rclone", PackageManager::Zypper),
    ],
    tests: &[InstallTest::WhichSucceeds("rclone")],
};

const DEFAULT_FOLDER: &str = "instant-games";

/// Choose a restic URL without creating a folder or probing repository existence.
/// Escaping a selection/input dialog or declining installation cancels setup.
pub(super) fn choose_repository() -> Result<Option<String>> {
    match ensure_all(&[&RCLONE])? {
        InstallResult::AlreadyInstalled | InstallResult::Installed => {}
        InstallResult::Declined => return Ok(None),
        InstallResult::NotAvailable { name, hint } => bail!("{name} is required. {hint}"),
        InstallResult::Failed { reason } => bail!("Could not install rclone: {reason}"),
    }
    choose_with(&mut SystemRclone)
}

trait RcloneBackend {
    fn list_remotes(&mut self) -> Result<Vec<String>>;
    fn configure(&mut self) -> Result<()>;
    fn check_root(&mut self, remote: &str) -> Result<()>;
}

struct SystemRclone;

impl RcloneBackend for SystemRclone {
    fn list_remotes(&mut self) -> Result<Vec<String>> {
        let output = Command::new("rclone")
            .arg("listremotes")
            .stdin(Stdio::null())
            .output()
            .context("Could not run rclone listremotes")?;
        ensure!(
            output.status.success(),
            "rclone listremotes failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        parse_remotes(std::str::from_utf8(&output.stdout).context("Remote names are not UTF-8")?)
    }

    fn configure(&mut self) -> Result<()> {
        let status = Command::new("rclone")
            .arg("config")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("Could not start rclone config")?;
        ensure!(status.success(), "rclone config exited with {status}");
        Ok(())
    }

    fn check_root(&mut self, remote: &str) -> Result<()> {
        let root = remote_root(remote)?;
        // The chosen folder may not exist yet. Check only the account's root,
        // never interpret this directory listing as a restic repository check.
        let output = Command::new("rclone")
            .args(["lsd", "--", &root])
            .stdin(Stdio::null())
            .output()
            .context("Could not run rclone lsd")?;
        ensure!(
            output.status.success(),
            "Could not list {root} ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    Remote(String),
    Manage,
    Retry,
    Back,
}

impl FzfSelectable for Choice {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Remote(name) => format!("{name}: — configured remote/account"),
            Self::Manage => "Create or manage remotes (rclone config)".into(),
            Self::Retry => "Retry".into(),
            Self::Back => "Back".into(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Remote(name) => format!("remote:{name}"),
            Self::Manage => "manage".into(),
            Self::Retry => "retry".into(),
            Self::Back => "back".into(),
        }
    }
}

fn choose_with(backend: &mut impl RcloneBackend) -> Result<Option<String>> {
    'remotes: loop {
        let remotes = match backend.list_remotes() {
            Ok(remotes) => remotes,
            Err(error) => {
                match recovery("Could not list configured remotes", &error)? {
                    Some(Choice::Retry) => {}
                    Some(Choice::Manage) => manage(backend)?,
                    _ => return Ok(None),
                }
                continue;
            }
        };
        let mut header = HeaderBuilder::new(NerdFont::Folder, "Choose an rclone remote")
            .subtitle("A remote identifies a storage account or connection, not the backup folder.")
            .subtitle("Next, choose a folder inside that account for your game backups.");
        if remotes.is_empty() {
            header =
                header.subtitle("No remotes configured. Create one using rclone config below.");
        }
        let mut choices: Vec<_> = remotes.into_iter().map(Choice::Remote).collect();
        choices.extend([Choice::Manage, Choice::Back]);
        let remote = match FzfWrapper::builder()
            .header(header)
            .items(choices)
            .select_one()?
        {
            DialogOutcome::Submitted(Choice::Remote(remote)) => remote,
            DialogOutcome::Submitted(Choice::Manage) => {
                manage(backend)?;
                continue;
            }
            _ => return Ok(None),
        };

        loop {
            match backend.check_root(&remote) {
                Ok(()) => break,
                Err(error) => match recovery("Could not access the remote/account root", &error)? {
                    Some(Choice::Retry) => continue,
                    Some(Choice::Manage) => {
                        manage(backend)?;
                        // Config may rename or delete the selected remote.
                        continue 'remotes;
                    }
                    Some(Choice::Back) => continue 'remotes,
                    _ => return Ok(None),
                },
            }
        }
        return prompt_folder(&remote);
    }
}

fn manage(backend: &mut impl RcloneBackend) -> Result<()> {
    println!("Configure the storage remote/account in rclone, then quit to return here.");
    if let Err(error) = backend.configure() {
        FzfWrapper::message(&format!(
            "{error:#}\nReturning to the refreshed remote list."
        ))?;
    }
    Ok(())
}

fn recovery(title: &str, error: &anyhow::Error) -> Result<Option<Choice>> {
    let result = FzfWrapper::builder()
        .header(
            HeaderBuilder::new(NerdFont::Folder, title)
                .subtitle(format!("{error:#}"))
                .subtitle("Check the connection and rclone configuration, then retry.")
                .subtitle("This does not determine whether a backup repository exists."),
        )
        .items(vec![Choice::Retry, Choice::Manage, Choice::Back])
        .select_one()?;
    Ok(match result {
        DialogOutcome::Submitted(choice) => Some(choice),
        DialogOutcome::Cancelled => None,
    })
}

fn prompt_folder(remote: &str) -> Result<Option<String>> {
    let mut query = DEFAULT_FOLDER.to_string();
    loop {
        let outcome = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::Folder, "Choose the backup folder")
                    .field("Remote/account", format!("{remote}:"))
                    .subtitle("Use the SAME account and folder on your other devices.")
                    .subtitle(
                        "Remote names may differ between devices; the storage location must match.",
                    )
                    .subtitle("Enter a relative folder, e.g. instant-games or backups/games.")
                    .subtitle("The folder need not exist yet. Empty input uses instant-games."),
            )
            .query(&query)
            .input()
            .input_dialog()?;
        match outcome {
            DialogOutcome::Cancelled => return Ok(None),
            DialogOutcome::Submitted(folder) => {
                match build_repository(remote, &folder) {
                    Ok(repository) => return Ok(Some(repository)),
                    Err(error) => FzfWrapper::message(&error.to_string())?,
                }
                query = folder;
            }
        }
    }
}

fn remote_root(remote: &str) -> Result<String> {
    ensure!(
        !remote.is_empty()
            && remote.trim() == remote
            && !remote.contains([':', '/', '\\'])
            && !remote.chars().any(char::is_control),
        "Invalid rclone remote name"
    );
    Ok(format!("{remote}:"))
}

fn parse_remotes(output: &str) -> Result<Vec<String>> {
    let mut remotes = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let remote = line
            .strip_suffix(':')
            .context("Unexpected rclone listremotes output: expected a name ending in ':'")?;
        remote_root(remote)?;
        if !remotes.iter().any(|name| name == remote) {
            remotes.push(remote.to_string());
        }
    }
    Ok(remotes)
}

fn build_repository(remote: &str, folder: &str) -> Result<String> {
    let root = remote_root(remote)?;
    // Do not silently trim/normalize names: different devices must address the
    // exact same folder, and traversal should be rejected rather than resolved.
    let folder = if folder.is_empty() {
        DEFAULT_FOLDER
    } else {
        folder
    };
    ensure!(
        folder.trim() == folder
            && !folder.contains([':', '\\'])
            && !folder.chars().any(char::is_control)
            && !folder.starts_with('~')
            && folder
                .split('/')
                .all(|part| !matches!(part, "" | "." | "..")),
        "Use a relative folder without colons, control characters, backslashes, empty components, '.' or '..' (e.g. instant-games)."
    );
    Ok(format!("rclone:{root}{folder}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::MockQueue;

    #[derive(Default)]
    struct FakeRclone {
        list_calls: usize,
        config_calls: usize,
        roots: Vec<String>,
        fail_list_once: bool,
        fail_root_once: bool,
    }

    impl RcloneBackend for FakeRclone {
        fn list_remotes(&mut self) -> Result<Vec<String>> {
            self.list_calls += 1;
            if std::mem::take(&mut self.fail_list_once) {
                bail!("list failed");
            }
            Ok(vec!["drive".into()])
        }

        fn configure(&mut self) -> Result<()> {
            self.config_calls += 1;
            Ok(())
        }

        fn check_root(&mut self, remote: &str) -> Result<()> {
            self.roots.push(remote_root(remote)?);
            if std::mem::take(&mut self.fail_root_once) {
                bail!("offline");
            }
            Ok(())
        }
    }

    #[test]
    fn parses_remotes_and_deduplicates() -> Result<()> {
        assert_eq!(
            parse_remotes("drive:\r\n\nmy account:\r\ndrive:\n")?,
            vec!["drive", "my account"]
        );
        assert!(parse_remotes("\n")?.is_empty());
        for malformed in ["drive", "drive:folder:", ":", "bad/name:", "bad\tname:"] {
            assert!(parse_remotes(malformed).is_err(), "{malformed:?}");
        }
        Ok(())
    }

    #[test]
    fn builds_default_and_nested_folders() -> Result<()> {
        assert_eq!(build_repository("drive", "")?, "rclone:drive:instant-games");
        assert_eq!(
            build_repository("my account", "backups/game saves")?,
            "rclone:my account:backups/game saves"
        );
        assert_eq!(remote_root("drive")?, "drive:");
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_or_unsafe_folders() {
        for folder in [
            "/games",
            "../games",
            "games/../other",
            "./games",
            "games/.",
            "games//save",
            "games/",
            "C:games",
            "a\nb",
            "a\rb",
            "a\0b",
            "a\\b",
            "~/games",
            " ",
            " games",
        ] {
            assert!(build_repository("drive", folder).is_err(), "{folder:?}");
        }
        assert!(build_repository("drive:folder", "games").is_err());
    }

    #[test]
    fn selection_cancellation_does_not_check_root() -> Result<()> {
        let _guard = MockQueue::new().cancel_selection().guard();
        let mut backend = FakeRclone::default();
        assert_eq!(choose_with(&mut backend)?, None);
        assert!(backend.roots.is_empty());
        assert_eq!(backend.config_calls, 0);
        Ok(())
    }

    #[test]
    fn folder_cancellation_is_clean() -> Result<()> {
        let _guard = MockQueue::new().select_index(0).input_cancelled().guard();
        assert_eq!(choose_with(&mut FakeRclone::default())?, None);
        Ok(())
    }

    #[test]
    fn checks_root_not_the_backup_folder() -> Result<()> {
        let _guard = MockQueue::new()
            .select_index(0)
            .input_string("new/folder")
            .guard();
        let mut backend = FakeRclone::default();
        assert_eq!(
            choose_with(&mut backend)?,
            Some("rclone:drive:new/folder".into())
        );
        assert_eq!(backend.roots, vec!["drive:"]);
        Ok(())
    }

    #[test]
    fn management_refreshes_remotes() -> Result<()> {
        let _guard = MockQueue::new().select_index(1).cancel_selection().guard();
        let mut backend = FakeRclone::default();
        assert_eq!(choose_with(&mut backend)?, None);
        assert_eq!(backend.config_calls, 1);
        assert_eq!(backend.list_calls, 2);
        Ok(())
    }

    #[test]
    fn list_failure_can_retry() -> Result<()> {
        let _guard = MockQueue::new().select_index(0).cancel_selection().guard();
        let mut backend = FakeRclone {
            fail_list_once: true,
            ..Default::default()
        };
        assert_eq!(choose_with(&mut backend)?, None);
        assert_eq!(backend.list_calls, 2);
        Ok(())
    }

    #[test]
    fn root_failure_can_retry_or_go_back() -> Result<()> {
        let _guard = MockQueue::new()
            .select_index(0)
            .select_index(0)
            .input_string("")
            .guard();
        let mut backend = FakeRclone {
            fail_root_once: true,
            ..Default::default()
        };
        assert_eq!(
            choose_with(&mut backend)?,
            Some("rclone:drive:instant-games".into())
        );
        assert_eq!(backend.roots, vec!["drive:", "drive:"]);
        drop(_guard);

        let _guard = MockQueue::new()
            .select_index(0)
            .select_index(2)
            .cancel_selection()
            .guard();
        let mut backend = FakeRclone {
            fail_root_once: true,
            ..Default::default()
        };
        assert_eq!(choose_with(&mut backend)?, None);
        assert_eq!(backend.list_calls, 2);
        Ok(())
    }

    #[test]
    fn invalid_folder_can_be_corrected() -> Result<()> {
        let _guard = MockQueue::new()
            .select_index(0)
            .input_string("../games")
            .message_ack()
            .input_string("games")
            .guard();
        assert_eq!(
            choose_with(&mut FakeRclone::default())?,
            Some("rclone:drive:games".into())
        );
        Ok(())
    }
}
