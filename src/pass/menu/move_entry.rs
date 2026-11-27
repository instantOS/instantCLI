use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::menu_utils::{DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;

use super::super::types::PassEntry;
use super::super::utils::{maybe_notify, prompt_text_value, run_pass_status};

#[derive(Clone)]
enum Destination {
    Here,
    Folder(String),
    NewFolder,
    Back,
    Cancel,
}

impl FzfSelectable for Destination {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Here => format!(
                "{} Move Here",
                format_icon_colored(NerdFont::Check, colors::GREEN)
            ),
            Self::Folder(name) => format!(
                "{}{name}/",
                format_icon_colored(NerdFont::Folder, colors::BLUE)
            ),
            Self::NewFolder => format!(
                "{} New Folder",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            Self::Back => format!("{} Parent Folder", format_back_icon()),
            Self::Cancel => format!("{} Cancel", format_back_icon()),
        }
    }
}

pub(super) fn run(store: &Path, entry: &PassEntry) -> Result<Option<String>> {
    let key = entry.primary_key()?;
    let (source, name) = key.rsplit_once('/').unwrap_or(("", key));
    let source = Path::new(source);
    let mut folder = source.to_path_buf();

    loop {
        let directory = checked_folder(store, &folder)?;
        let mut items = Vec::new();
        if folder != source {
            items.push(Destination::Here);
        }
        items.extend(
            child_folders(&directory)?
                .into_iter()
                .map(Destination::Folder),
        );
        items.push(Destination::NewFolder);
        if folder.parent().is_some() {
            items.push(Destination::Back);
        }
        items.push(Destination::Cancel);

        let location = format!("/{}", folder.display());
        let selection = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::Folder, "Move Password Entry")
                    .field("Entry", &entry.display_name)
                    .field("Folder", &location)
                    .build(),
            )
            .prompt("Destination")
            .responsive_layout()
            .items(items)
            .select_one()?;

        match selection {
            DialogOutcome::Submitted(Destination::Folder(name)) => folder.push(name),
            DialogOutcome::Submitted(Destination::Back) => {
                folder.pop();
            }
            DialogOutcome::Submitted(Destination::NewFolder) => {
                if let Some(name) = prompt_text_value(
                    &format!("Folder name in {location}"),
                    None,
                    false,
                    None,
                    None,
                )? {
                    match create_folder(store, &folder, &name) {
                        Ok(()) => folder.push(name),
                        Err(err) => FzfWrapper::message(&format!("{err:#}"))?,
                    }
                }
            }
            DialogOutcome::Submitted(Destination::Here) => {
                match move_to_folder(store, entry, &folder, name) {
                    Ok(target) => {
                        maybe_notify(
                            "Pass",
                            &format!("Moved '{}' to '{target}'", entry.display_name),
                        );
                        return Ok(Some(target));
                    }
                    Err(err) => FzfWrapper::message(&format!("{err:#}"))?,
                }
            }
            DialogOutcome::Cancelled | DialogOutcome::Submitted(Destination::Cancel) => {
                return Ok(None);
            }
        }
    }
}

fn valid_folder_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

fn checked_folder(store: &Path, relative: &Path) -> Result<PathBuf> {
    let mut directory = store.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            bail!("Select a folder inside the password store");
        };
        directory.push(name);
        let metadata = directory
            .symlink_metadata()
            .with_context(|| format!("Could not open folder /{}", relative.display()))?;
        if !metadata.is_dir() {
            bail!(
                "/{} must be a folder inside the password store",
                relative.display()
            );
        }
    }
    Ok(directory)
}

fn child_folders(directory: &Path) -> Result<Vec<String>> {
    let mut folders = Vec::new();
    for entry in directory
        .read_dir()
        .with_context(|| format!("Could not read {}", directory.display()))?
    {
        let entry = entry?;
        if let Ok(name) = entry.file_name().into_string()
            && valid_folder_name(&name)
            && entry.file_type()?.is_dir()
        {
            folders.push(name);
        }
    }
    folders.sort_unstable();
    Ok(folders)
}

fn create_folder(store: &Path, folder: &Path, name: &str) -> Result<()> {
    if !valid_folder_name(name) {
        bail!("Enter a single folder name without a leading dot, slashes, or control characters");
    }
    let path = checked_folder(store, folder)?.join(name);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .with_context(|| format!("Could not create folder '{}'", path.display()))
}

fn move_to_folder(store: &Path, entry: &PassEntry, folder: &Path, name: &str) -> Result<String> {
    let key = entry.primary_key()?;
    let source = Path::new(key).parent().context("Invalid entry path")?;
    let source_file = checked_folder(store, source)?.join(format!("{name}.gpg"));
    if entry.primary_file_path() != Some(source_file.as_path())
        || !source_file
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_file())
    {
        bail!(
            "Could not resolve the store path for '{}'",
            entry.display_name
        );
    }

    let directory = checked_folder(store, folder)?;
    let target = folder.join(name);
    let target = target.to_str().context("Invalid destination path")?;
    // A bare directory at the target makes pass interpret the key as a folder.
    for path in [directory.join(name), directory.join(format!("{name}.gpg"))] {
        match path.symlink_metadata() {
            Ok(_) => bail!("The destination already contains '{}'", path.display()),
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("Could not check '{}'", path.display()));
            }
        }
    }

    run_pass_status(["mv", "--", key, target])?;
    Ok(target.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};
    use crate::pass::utils::load_entries;

    #[test]
    fn picker_can_create_a_folder_and_cancel_the_move() -> Result<()> {
        let store = tempfile::tempdir()?;
        let original = store.path().join("account.otp.gpg");
        fs::write(&original, b"test entry")?;
        let entries = load_entries(store.path())?;
        let _mock = MockQueue::new()
            .select_index(0) // New Folder at the source
            .input_string("Work accounts")
            .cancel_selection()
            .guard();

        assert_eq!(run(store.path(), &entries[0])?, None);
        assert_eq!(scripted_responses_remaining(), 0);
        assert!(store.path().join("Work accounts").is_dir());
        assert!(original.is_file());
        Ok(())
    }

    #[test]
    fn rejects_destination_collisions_before_calling_pass() -> Result<()> {
        let store = tempfile::tempdir()?;
        let destination = Path::new("work");
        fs::create_dir(store.path().join(destination))?;
        let original = store.path().join("account.file.gpg");
        fs::write(&original, b"original")?;
        let collision = store.path().join(destination).join("account.file.gpg");
        fs::write(&collision, b"existing")?;
        let entries = load_entries(store.path())?;

        let err = move_to_folder(store.path(), &entries[0], destination, "account.file")
            .expect_err("existing entry should block the move");
        assert!(err.to_string().contains("already contains"));
        assert_eq!(fs::read(&original)?, b"original");
        assert_eq!(fs::read(&collision)?, b"existing");

        fs::remove_file(collision)?;
        fs::create_dir(store.path().join(destination).join("account.file"))?;
        let err = move_to_folder(store.path(), &entries[0], destination, "account.file")
            .expect_err("same-named directory should block the move");
        assert!(err.to_string().contains("already contains"));
        assert_eq!(fs::read(&original)?, b"original");
        Ok(())
    }

    #[test]
    fn folders_exclude_store_metadata_and_paths_outside_the_store() -> Result<()> {
        let store = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        fs::create_dir(store.path().join(".git"))?;
        fs::create_dir(store.path().join("empty"))?;
        std::os::unix::fs::symlink(outside.path(), store.path().join("linked"))?;

        assert_eq!(child_folders(store.path())?, ["empty"]);
        assert!(checked_folder(store.path(), Path::new("linked")).is_err());
        assert!(create_folder(store.path(), Path::new("linked"), "new").is_err());
        assert!(create_folder(store.path(), Path::new(""), "../escaped").is_err());
        assert!(!outside.path().join("new").exists());
        Ok(())
    }
}
