use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use fre::args::SortMethod;
use fre::store::{FrecencyStore, read_store, write_store};
use walkdir::WalkDir;

use crate::assist::deps::{LIBNOTIFY, WL_CLIPBOARD, XCLIP};
use crate::assist::utils::{copy_to_clipboard, show_notification};
use crate::common::display_server::DisplayServer;
use crate::common::package::{Dependency, InstallResult, ensure_all};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper};

use super::types::{GPG_DEP, PASS_DEP, PASS_OTP_DEP, PassEntry};

pub(super) fn prompt_password_with_confirmation(prompt: &str) -> Result<String> {
    let first = match FzfWrapper::builder()
        .prompt(prompt)
        .password()
        .password_dialog()?
    {
        FzfResult::Selected(value) => value,
        _ => bail!("Password entry cancelled"),
    };

    if first.is_empty() {
        bail!("Password cannot be empty");
    }

    let second = match FzfWrapper::builder()
        .prompt("Confirm password")
        .password()
        .password_dialog()?
    {
        FzfResult::Selected(value) => value,
        _ => bail!("Password confirmation cancelled"),
    };

    if first != second {
        bail!("Passwords do not match");
    }

    Ok(first)
}

pub(super) fn prompt_text_value_prefilled(
    prompt: &str,
    header: Option<&str>,
    current: &str,
    allow_empty: bool,
    ghost: Option<&str>,
) -> Result<Option<String>> {
    let mut builder = FzfWrapper::builder().input().prompt(prompt).query(current);

    if let Some(header) = header {
        builder = builder.header(header);
    }

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

pub(super) fn prompt_text_value(
    prompt: &str,
    header: Option<&str>,
    allow_empty: bool,
    ghost: Option<&str>,
) -> Result<Option<String>> {
    let mut builder = FzfWrapper::builder().input().prompt(prompt);

    if let Some(header) = header {
        builder = builder.header(header);
    }

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
        .or_else(|| {
            entry
                .secret_key
                .as_ref()
                .map(|secret| format!("{secret}.otp"))
        })
        .ok_or_else(|| anyhow!("Entry '{}' has no OTP data", entry.display_name))
}

pub(super) fn first_secret_line(output: &[u8]) -> Option<String> {
    String::from_utf8(output.to_vec())
        .ok()?
        .lines()
        .next()
        .map(|line| line.trim_end().to_string())
        .filter(|line| !line.is_empty())
}

pub(super) fn should_export_output(entry: &PassEntry, output: &[u8]) -> bool {
    if entry.should_offer_export() {
        return true;
    }

    if output.len() > 100 * 1024 {
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
    ensure_dependencies(&[&PASS_DEP, &GPG_DEP], "pass")
}

pub(super) fn ensure_clipboard_dependencies() -> Result<()> {
    let mut deps: Vec<&'static Dependency> = vec![&PASS_DEP, &GPG_DEP];
    match DisplayServer::detect() {
        DisplayServer::Wayland => deps.push(&WL_CLIPBOARD),
        DisplayServer::X11 => deps.push(&XCLIP),
        DisplayServer::Unknown => {}
    }

    ensure_dependencies(&deps, "clipboard integration for pass")
}

pub(super) fn ensure_otp_dependency() -> Result<()> {
    ensure_dependencies(&[&PASS_DEP, &GPG_DEP, &PASS_OTP_DEP], "pass-otp")
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
    let mut grouped: BTreeMap<String, PassEntry> = BTreeMap::new();

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

        let (display_name, is_otp) = match without_ext.strip_suffix(".otp") {
            Some(base) => (base.to_string(), true),
            None => (without_ext.to_string(), false),
        };

        let grouped_entry = grouped
            .entry(display_name.clone())
            .or_insert_with(|| PassEntry {
                display_name,
                secret_key: None,
                otp_key: None,
                secret_path: None,
                otp_path: None,
            });

        if is_otp {
            grouped_entry.otp_key = Some(without_ext.to_string());
            grouped_entry.otp_path = Some(path.to_path_buf());
        } else {
            grouped_entry.secret_key = Some(without_ext.to_string());
            grouped_entry.secret_path = Some(path.to_path_buf());
        }
    }

    let mut entries: Vec<_> = grouped.into_values().collect();
    entries.sort_by_key(|entry| entry.display_name.to_lowercase());
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
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(env!("CARGO_BIN_NAME"));
    fs::create_dir_all(&cache_dir).context("Failed to create cache directory for pass")?;
    Ok(cache_dir.join("pass_frecency_store.json"))
}

pub(super) fn run_pass_stdout<const N: usize>(args: [&str; N]) -> Result<Vec<u8>> {
    let output = Command::new("pass")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run `pass {}`", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("`pass {}` failed", args.join(" "));
        } else {
            bail!("`pass {}` failed: {}", args.join(" "), stderr);
        }
    }

    Ok(output.stdout)
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
