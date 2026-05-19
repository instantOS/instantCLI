use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use fre::args::SortMethod;
use fre::store::{FrecencyStore, read_store, write_store};
use walkdir::WalkDir;

use super::types::{EXPORT_THRESHOLD_BYTES, PASS_OTP_DEP, PassEntry};
use crate::assist::deps::{GPG, PASS};
use crate::assist::deps::{LIBNOTIFY, WL_CLIPBOARD, XCLIP};
use crate::assist::utils::{copy_to_clipboard, show_notification};
use crate::common::display_server::DisplayServer;
use crate::common::package::{Dependency, InstallResult, ensure_all};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper};

pub(super) fn prompt_password_with_confirmation(prompt: &str) -> Result<String> {
    let password = match FzfWrapper::builder()
        .prompt(prompt)
        .password()
        .with_confirmation()
        .password_dialog()?
    {
        FzfResult::Selected(value) => value,
        _ => bail!("Password entry cancelled"),
    };

    if password.is_empty() {
        bail!("Password cannot be empty");
    }

    Ok(password)
}

pub(super) fn prompt_text_value(
    prompt: &str,
    header: Option<&str>,
    allow_empty: bool,
    ghost: Option<&str>,
    initial: Option<&str>,
) -> Result<Option<String>> {
    let mut base = FzfWrapper::builder().prompt(prompt);

    if let Some(header) = header {
        base = base.header(header);
    }
    if let Some(initial) = initial {
        base = base.query(initial);
    }

    let mut builder = base.input();

    if let Some(ghost) = ghost {
        builder = builder.ghost(ghost);
    }

    match builder.input_result()? {
        FzfResult::Selected(value) => {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() && !allow_empty {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        _ => Ok(None),
    }
}

pub(super) fn resolve_entry_name(
    name: Option<String>,
    prefix: Option<&str>,
    header: &str,
) -> Result<String> {
    match name {
        Some(name) => apply_prefix_to_entry_name(&sanitize_entry_name(&name)?, prefix),
        None => prompt_text_value(
            "Entry name",
            Some(header),
            false,
            Some(prefix.unwrap_or("examples/github or email/work")),
            None,
        )?
        .ok_or_else(|| anyhow!("Entry creation cancelled"))
        .and_then(|value| sanitize_entry_name(&value))
        .and_then(|value| apply_prefix_to_entry_name(&value, prefix)),
    }
}

pub(super) fn apply_prefix_to_entry_name(name: &str, prefix: Option<&str>) -> Result<String> {
    match prefix {
        Some(prefix) if !prefix.is_empty() && !name.contains('/') => {
            sanitize_entry_name(&format!("{prefix}/{name}"))
        }
        _ => Ok(name.to_string()),
    }
}

pub(super) fn sanitize_entry_name(name: &str) -> Result<String> {
    let trimmed = name.trim().trim_matches('/');
    if trimmed.is_empty() {
        bail!("Entry name cannot be empty");
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        bail!("Entry name cannot contain newlines");
    }
    if trimmed.contains("..") {
        bail!("Entry name cannot contain `..`");
    }
    Ok(trimmed.to_string())
}

pub(super) fn normalize_otp_name(name: &str) -> String {
    if name.ends_with(".otp") {
        name.to_string()
    } else {
        format!("{name}.otp")
    }
}

pub(super) fn resolve_otp_key(entry: &PassEntry) -> Result<String> {
    entry
        .otp_key
        .clone()
        .ok_or_else(|| anyhow!("Entry '{}' has no OTP data", entry.display_name))
}

pub(super) fn first_secret_line(output: &[u8]) -> Option<String> {
    std::str::from_utf8(output)
        .ok()?
        .lines()
        .next()
        .map(|line| line.trim_end())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
}

pub(super) fn should_export_output(entry: &PassEntry, output: &[u8]) -> bool {
    if entry.should_offer_export() {
        return true;
    }

    if output.len() > EXPORT_THRESHOLD_BYTES {
        return true;
    }

    std::str::from_utf8(output).is_err()
}

pub(super) fn confirm_export_instead(display_name: &str) -> Result<bool> {
    Ok(matches!(
        FzfWrapper::builder()
            .confirm(format!(
                "'{}' looks like a large or file-like entry.\n\nExport it to a file instead of copying it to the clipboard?",
                display_name
            ))
            .yes_text("Export")
            .no_text("Clipboard")
            .confirm_dialog()?,
        ConfirmResult::Yes
    ))
}

pub(super) fn prompt_export_destination(
    display_name: &str,
    path: Option<String>,
) -> Result<PathBuf> {
    let suggested = display_name.rsplit('/').next().unwrap_or(display_name);

    if let Some(path) = path {
        return Ok(PathBuf::from(path));
    }

    let value = prompt_text_value(
        "Export path",
        Some("Enter the destination file path"),
        false,
        Some(suggested),
        None,
    )?
    .ok_or_else(|| anyhow!("Export cancelled"))?;

    Ok(PathBuf::from(value))
}

pub(super) fn write_export_file(destination: &Path, output: &[u8]) -> Result<()> {
    if let Some(parent) = destination.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create export directory {}", parent.display()))?;
    }

    if destination.exists()
        && !matches!(
            FzfWrapper::confirm(&format!(
                "{} already exists.\n\nOverwrite it?",
                destination.display()
            ))?,
            ConfirmResult::Yes
        )
    {
        bail!("Export cancelled");
    }

    fs::write(destination, output)
        .with_context(|| format!("Failed to write export file {}", destination.display()))
}

pub(super) fn ensure_core_dependencies() -> Result<()> {
    ensure_dependencies(&[&PASS, &GPG], "pass")
}

pub(super) fn ensure_clipboard_dependencies() -> Result<()> {
    let mut deps: Vec<&'static Dependency> = vec![&PASS, &GPG];
    match DisplayServer::detect() {
        DisplayServer::Wayland => deps.push(&WL_CLIPBOARD),
        DisplayServer::X11 => deps.push(&XCLIP),
        DisplayServer::Unknown => {}
    }

    ensure_dependencies(&deps, "clipboard integration for pass")
}

pub(super) fn ensure_otp_dependency() -> Result<()> {
    ensure_dependencies(&[&PASS, &GPG, &PASS_OTP_DEP], "pass-otp")
}

fn ensure_dependencies(deps: &[&'static Dependency], label: &str) -> Result<()> {
    match ensure_all(deps)? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(()),
        InstallResult::Declined => Err(anyhow!("{label} installation cancelled")),
        InstallResult::NotAvailable { hint, .. } => {
            Err(anyhow!("{label} is not available on this system: {hint}"))
        }
        InstallResult::Failed { reason } => Err(anyhow!("{label} installation failed: {reason}")),
    }
}

pub(super) fn maybe_confirm_overwrite(key: &str) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let path = store_dir.join(format!("{key}.gpg"));
    if !path.exists() {
        return Ok(());
    }

    let confirm = FzfWrapper::confirm(&format!("Entry '{key}' already exists.\n\nOverwrite it?"))?;
    if matches!(confirm, ConfirmResult::Yes) {
        Ok(())
    } else {
        bail!("Overwrite cancelled for '{key}'");
    }
}

pub(super) fn ensure_password_store_dir() -> Result<PathBuf> {
    let dir = password_store_dir()?;
    if !dir.exists() {
        bail!(
            "No password store found at {}. Run `pass init <gpg-id>` first.",
            dir.display()
        );
    }
    Ok(dir)
}

pub(super) fn password_store_dir() -> Result<PathBuf> {
    if let Ok(dir) = env::var("PASSWORD_STORE_DIR") {
        return Ok(PathBuf::from(dir));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("Failed to determine home directory"))?;
    Ok(home.join(".password-store"))
}

pub(super) fn load_entries(store_dir: &Path) -> Result<Vec<PassEntry>> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(store_dir).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("Warning: failed to read password-store entry: {err}");
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gpg") {
            continue;
        }

        let relative = path
            .strip_prefix(store_dir)
            .with_context(|| format!("Failed to strip store prefix from {}", path.display()))?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        let without_ext = relative.strip_suffix(".gpg").unwrap_or(&relative);
        let key = without_ext.to_string();
        let is_otp = key.ends_with(".otp");

        entries.push(PassEntry {
            display_name: key.clone(),
            secret_key: (!is_otp).then_some(key.clone()),
            otp_key: is_otp.then_some(key),
            secret_path: (!is_otp).then_some(path.to_path_buf()),
            otp_path: is_otp.then_some(path.to_path_buf()),
        });
    }

    entries.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    Ok(entries)
}

pub(super) fn sort_entries_by_frecency(entries: &mut [PassEntry]) -> Result<()> {
    let path = frecency_store_path()?;
    let store: FrecencyStore = read_store(&path).unwrap_or_default();
    let sorted_items = store.sorted(SortMethod::Frecent);
    let frecency_rank: std::collections::HashMap<_, _> = sorted_items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.item.as_str().to_owned(), index))
        .collect();

    entries.sort_by(|left, right| {
        let left_index = frecency_rank
            .get(left.display_name.as_str())
            .copied()
            .unwrap_or(usize::MAX);
        let right_index = frecency_rank
            .get(right.display_name.as_str())
            .copied()
            .unwrap_or(usize::MAX);

        left_index.cmp(&right_index).then_with(|| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
        })
    });

    Ok(())
}

pub(super) fn record_frecency(item: &str) -> Result<()> {
    let path = frecency_store_path()?;
    let mut store: FrecencyStore = read_store(&path).unwrap_or_default();
    store.add(item);
    write_store(store, &path).context("Failed to save pass frecency store")
}

fn frecency_store_path() -> Result<PathBuf> {
    // Use the platform's temp dir as a fallback so Termux (which exposes
    // $PREFIX/tmp via $TMPDIR rather than /tmp) gets a writable location.
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(env!("CARGO_BIN_NAME"));
    fs::create_dir_all(&cache_dir).context("Failed to create cache directory for pass")?;
    Ok(cache_dir.join("pass_frecency_store.json"))
}

fn run_pass_output<const N: usize>(args: [&str; N]) -> Result<std::process::Output> {
    let display_cmd = args.join(" ");
    let output = Command::new("pass")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run `pass {display_cmd}`"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("`pass {display_cmd}` failed");
        } else {
            bail!("`pass {display_cmd}` failed: {stderr}");
        }
    }

    Ok(output)
}

pub(super) fn run_pass_stdout<const N: usize>(args: [&str; N]) -> Result<Vec<u8>> {
    Ok(run_pass_output(args)?.stdout)
}

pub(super) fn run_pass_status<const N: usize>(args: [&str; N]) -> Result<()> {
    run_pass_output(args)?;
    Ok(())
}

pub(super) fn copy_secret_to_clipboard(data: &[u8]) -> Result<()> {
    let display_server = DisplayServer::detect();
    copy_to_clipboard(data, &display_server)?;
    Ok(())
}

pub(super) fn maybe_notify(title: &str, message: &str) {
    if LIBNOTIFY.is_installed() {
        let _ = show_notification(title, message);
    }
}
