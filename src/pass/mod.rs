use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use fre::args::SortMethod;
use fre::store::{FrecencyStore, read_store, write_store};
use walkdir::WalkDir;

use crate::assist::deps::{LIBNOTIFY, WL_CLIPBOARD, XCLIP};
use crate::assist::utils::{copy_to_clipboard, show_notification};
use crate::common::display_server::DisplayServer;
use crate::common::package::{
    Dependency, InstallResult, PackageDefinition, PackageManager, ensure_all,
};
use crate::common::requirements::InstallTest;
use crate::menu::client::MenuClient;
use crate::menu::protocol::{FzfPreview, SerializableMenuItem};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper};

static PASS_DEP: Dependency = Dependency {
    name: "pass",
    packages: &[
        PackageDefinition::new("pass", PackageManager::Pacman),
        PackageDefinition::new("pass", PackageManager::Apt),
        PackageDefinition::new("pass", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("pass")],
};

static GPG_DEP: Dependency = Dependency {
    name: "gpg",
    packages: &[
        PackageDefinition::new("gnupg", PackageManager::Pacman),
        PackageDefinition::new("gnupg", PackageManager::Apt),
        PackageDefinition::new("gnupg2", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("gpg")],
};

static PASS_OTP_DEP: Dependency = Dependency {
    name: "pass-otp",
    packages: &[
        PackageDefinition::new("pass-otp", PackageManager::Pacman),
        PackageDefinition::new("pass-extension-otp", PackageManager::Apt),
        PackageDefinition::new("pass-otp", PackageManager::Dnf),
        PackageDefinition::new("pass-otp", PackageManager::Aur),
    ],
    tests: &[InstallTest::CommandSucceeds {
        program: "pass",
        args: &["otp", "help"],
    }],
};

#[derive(Subcommand, Debug, Clone)]
pub enum PassCommands {
    /// Insert a password or OTP entry
    Add {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
        /// Store an OTP URI instead of a password
        #[arg(long)]
        otp: bool,
    },
    /// Generate a random password and store it in pass
    Generate {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
        /// Generated password length
        #[arg(long, default_value_t = 20)]
        length: usize,
    },
    /// Delete a pass entry
    Delete {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
    },
    /// Copy the OTP code for an entry
    Otp {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
    },
    /// Export a decrypted entry to a file instead of copying it
    Export {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
        /// Output path (optional, prompts if omitted)
        path: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PassEntry {
    display_name: String,
    secret_key: Option<String>,
    otp_key: Option<String>,
    secret_path: Option<PathBuf>,
    otp_path: Option<PathBuf>,
}

impl PassEntry {
    fn kind_label(&self) -> &'static str {
        match (self.secret_key.is_some(), self.otp_key.is_some()) {
            (true, true) => "password + otp",
            (true, false) => "password",
            (false, true) => "otp",
            (false, false) => "empty",
        }
    }

    fn primary_action_label(&self) -> &'static str {
        if self.secret_key.is_some() {
            "Copy password"
        } else {
            "Copy OTP code"
        }
    }

    fn has_secret(&self) -> bool {
        self.secret_key.is_some()
    }

    fn has_otp(&self) -> bool {
        self.otp_key.is_some()
    }

    fn primary_key(&self) -> Result<&str> {
        self.secret_key
            .as_deref()
            .or(self.otp_key.as_deref())
            .ok_or_else(|| anyhow!("Entry '{}' has no secret or OTP data", self.display_name))
    }

    fn primary_file_path(&self) -> Option<&Path> {
        self.secret_path.as_deref().or(self.otp_path.as_deref())
    }

    fn preview_text(&self) -> String {
        let mut lines = vec![
            format!("Entry: {}", self.display_name),
            format!("Type: {}", self.kind_label()),
            format!("Primary action: {}", self.primary_action_label()),
        ];

        if self.has_otp() && self.has_secret() {
            lines.push("OTP companion is available via `ins pass otp <entry>`.".to_string());
        }

        if self.should_offer_export() {
            lines.push(
                "Large or file-like entry: you will be offered export instead of clipboard."
                    .to_string(),
            );
        }

        lines.join("\n")
    }

    fn should_offer_export(&self) -> bool {
        self.display_name.ends_with(".file")
            || self
                .primary_file_path()
                .and_then(|path| fs::metadata(path).ok())
                .map(|metadata| metadata.len() > 100 * 1024)
                .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeleteMode {
    Secret,
    Otp,
    Both,
}

pub fn pass_entry_names() -> Vec<String> {
    let Ok(store_dir) = password_store_dir() else {
        return Vec::new();
    };
    let Ok(entries) = load_entries(&store_dir) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .map(|entry| entry.display_name)
        .collect()
}

pub fn handle_pass_command(list_only: bool, command: Option<PassCommands>) -> Result<i32> {
    match command {
        Some(PassCommands::Add { name, otp }) => {
            ensure_core_dependencies()?;
            if otp {
                ensure_otp_dependency()?;
                insert_otp_entry(name)?;
            } else {
                insert_password_entry(name)?;
            }
            Ok(0)
        }
        Some(PassCommands::Generate { name, length }) => {
            ensure_core_dependencies()?;
            generate_password_entry(name, length)?;
            Ok(0)
        }
        Some(PassCommands::Delete { name }) => {
            ensure_core_dependencies()?;
            delete_entry_flow(name)?;
            Ok(0)
        }
        Some(PassCommands::Otp { name }) => {
            ensure_core_dependencies()?;
            ensure_otp_dependency()?;
            copy_otp_flow(name)?;
            Ok(0)
        }
        Some(PassCommands::Export { name, path }) => {
            ensure_core_dependencies()?;
            export_entry_flow(name, path)?;
            Ok(0)
        }
        None if list_only => {
            ensure_core_dependencies()?;
            let store_dir = ensure_password_store_dir()?;
            for entry in load_entries(&store_dir)? {
                println!("{}", entry.display_name);
            }
            Ok(0)
        }
        None => interactive_pass_menu(),
    }
}

fn interactive_pass_menu() -> Result<i32> {
    ensure_core_dependencies()?;

    let client = MenuClient::new();
    let server_client = client.clone();
    let server_ready = std::thread::spawn(move || server_client.ensure_server_running());

    let store_dir = ensure_password_store_dir()?;
    let mut entries = load_entries(&store_dir)?;
    sort_entries_by_frecency(&mut entries)?;
    let menu_items = prepare_menu_items(&entries);

    server_ready
        .join()
        .map_err(|_| anyhow!("menu server thread panicked"))??;

    let selected = client.choice("Pass".to_string(), menu_items, false)?;
    if selected.is_empty() {
        return Ok(1);
    }

    let metadata = selected[0]
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("Selected menu item missing metadata"))?;

    match metadata.get("kind").map(String::as_str) {
        Some("action") => match metadata.get("action").map(String::as_str) {
            Some("add-password") => insert_password_entry(None)?,
            Some("generate-password") => generate_password_entry(None, 20)?,
            Some("add-otp") => {
                ensure_otp_dependency()?;
                insert_otp_entry(None)?;
            }
            other => bail!("Unknown action selection: {:?}", other),
        },
        Some("entry") => {
            let index = metadata
                .get("index")
                .ok_or_else(|| anyhow!("Selected entry is missing index metadata"))?
                .parse::<usize>()
                .context("Selected entry index is invalid")?;
            let entry = entries
                .get(index)
                .ok_or_else(|| anyhow!("Selected entry index out of bounds: {index}"))?;
            copy_primary_entry(entry)?;
            record_frecency(&entry.display_name)?;
        }
        other => bail!("Unknown menu item kind: {:?}", other),
    }

    Ok(0)
}

fn prepare_menu_items(entries: &[PassEntry]) -> Vec<SerializableMenuItem> {
    let mut items = Vec::with_capacity(entries.len() + 3);

    items.push(action_item(
        "Create password",
        "add-password",
        "Insert a password manually. Empty input is rejected and a confirmation step is shown.",
    ));
    items.push(action_item(
        "Generate password",
        "generate-password",
        "Create a new pass entry with a generated password. Default length is 20 characters.",
    ));
    items.push(action_item(
        "Create OTP entry",
        "add-otp",
        "Store an otpauth:// URI using pass-otp. If the base password exists, this becomes a companion OTP entry.",
    ));

    for (index, entry) in entries.iter().enumerate() {
        let mut metadata = HashMap::new();
        metadata.insert("kind".to_string(), "entry".to_string());
        metadata.insert("index".to_string(), index.to_string());

        let label = match (entry.has_secret(), entry.has_otp()) {
            (true, true) => format!("{}  [password + otp]", entry.display_name),
            (false, true) => format!("{}  [otp]", entry.display_name),
            _ => entry.display_name.clone(),
        };

        items.push(SerializableMenuItem {
            display_text: label,
            preview: FzfPreview::Text(entry.preview_text()),
            metadata: Some(metadata),
        });
    }

    items
}

fn action_item(display_text: &str, action: &str, preview: &str) -> SerializableMenuItem {
    let mut metadata = HashMap::new();
    metadata.insert("kind".to_string(), "action".to_string());
    metadata.insert("action".to_string(), action.to_string());

    SerializableMenuItem {
        display_text: display_text.to_string(),
        preview: FzfPreview::Text(preview.to_string()),
        metadata: Some(metadata),
    }
}

fn insert_password_entry(name: Option<String>) -> Result<()> {
    let entry_name = resolve_entry_name(name, "New pass entry")?;
    let password = prompt_password_with_confirmation("New password")?;
    insert_password(&entry_name, &password)?;
    maybe_notify("Pass", &format!("Stored password for {entry_name}"));
    Ok(())
}

fn generate_password_entry(name: Option<String>, length: usize) -> Result<()> {
    if length == 0 {
        bail!("Password length must be greater than zero");
    }

    let entry_name = resolve_entry_name(name, "Generated pass entry")?;
    maybe_confirm_overwrite(&entry_name)?;

    let status = Command::new("pass")
        .args(["generate", "-f", &entry_name, &length.to_string()])
        .status()
        .with_context(|| format!("Failed to run `pass generate` for '{entry_name}'"))?;

    if !status.success() {
        bail!("`pass generate` failed for '{entry_name}'");
    }

    maybe_notify(
        "Pass",
        &format!("Generated {length}-character password for {entry_name}"),
    );
    Ok(())
}

fn insert_otp_entry(name: Option<String>) -> Result<()> {
    let raw_name = resolve_entry_name(name, "New OTP entry")?;
    let entry_name = normalize_otp_name(&raw_name);
    maybe_confirm_overwrite(&entry_name)?;

    let otp_uri = prompt_text_value(
        "OTP URI",
        Some("Paste an otpauth:// URI"),
        false,
        Some("otpauth://totp/example?secret=..."),
    )?
    .ok_or_else(|| anyhow!("OTP creation cancelled"))?;

    if !otp_uri.starts_with("otpauth://") {
        bail!("OTP entries must start with `otpauth://`");
    }

    let status = Command::new("pass")
        .args(["otp", "insert", "-f", &entry_name, &otp_uri])
        .status()
        .with_context(|| format!("Failed to store OTP entry '{entry_name}'"))?;

    if !status.success() {
        bail!("`pass otp insert` failed for '{entry_name}'");
    }

    maybe_notify("Pass", &format!("Stored OTP entry for {raw_name}"));
    Ok(())
}

fn copy_primary_entry(entry: &PassEntry) -> Result<()> {
    if entry.has_secret() {
        copy_password_entry(entry)
    } else {
        copy_otp_entry(entry)
    }
}

fn copy_password_entry(entry: &PassEntry) -> Result<()> {
    ensure_clipboard_dependencies()?;

    let key = entry
        .secret_key
        .as_deref()
        .ok_or_else(|| anyhow!("Entry '{}' has no password data", entry.display_name))?;
    let output = run_pass_stdout(["show", key])?;

    if should_export_output(entry, &output) && confirm_export_instead(&entry.display_name)? {
        let destination = prompt_export_destination(&entry.display_name, None)?;
        write_export_file(&destination, &output)?;
        maybe_notify(
            "Pass",
            &format!(
                "Exported '{}' to {}",
                entry.display_name,
                destination.display()
            ),
        );
        return Ok(());
    }

    let secret = first_secret_line(&output).ok_or_else(|| {
        anyhow!(
            "Entry '{}' does not contain a password line",
            entry.display_name
        )
    })?;

    copy_secret_to_clipboard(secret.as_bytes())?;
    maybe_notify(
        "Pass",
        &format!("Copied password for {}", entry.display_name),
    );
    Ok(())
}

fn copy_otp_entry(entry: &PassEntry) -> Result<()> {
    ensure_clipboard_dependencies()?;
    ensure_otp_dependency()?;

    let otp_key = resolve_otp_key(entry)?;
    let output = run_pass_stdout(["otp", &otp_key])?;
    let code = String::from_utf8(output)
        .context("OTP output is not valid UTF-8")?
        .trim()
        .to_string();

    if code.is_empty() {
        bail!("OTP output for '{}' was empty", entry.display_name);
    }

    copy_secret_to_clipboard(code.as_bytes())?;
    maybe_notify(
        "Pass",
        &format!("Copied OTP code for {}", entry.display_name),
    );
    Ok(())
}

fn copy_otp_flow(name: Option<String>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let entries = load_entries(&store_dir)?;
    let entry = select_entry(name, &entries, true)?.ok_or_else(|| anyhow!("OTP copy cancelled"))?;
    copy_otp_entry(&entry)?;
    record_frecency(&entry.display_name)?;
    Ok(())
}

fn export_entry_flow(name: Option<String>, path: Option<String>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let entries = load_entries(&store_dir)?;
    let entry = select_entry(name, &entries, false)?.ok_or_else(|| anyhow!("Export cancelled"))?;

    let key = entry.primary_key()?.to_string();
    let output = run_pass_stdout(["show", &key])?;
    let destination = prompt_export_destination(&entry.display_name, path)?;
    write_export_file(&destination, &output)?;

    maybe_notify(
        "Pass",
        &format!(
            "Exported '{}' to {}",
            entry.display_name,
            destination.display()
        ),
    );
    record_frecency(&entry.display_name)?;
    Ok(())
}

fn delete_entry_flow(name: Option<String>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let entries = load_entries(&store_dir)?;
    let entry = select_entry(name, &entries, false)?.ok_or_else(|| anyhow!("Delete cancelled"))?;

    let mode = choose_delete_mode(&entry)?;
    let confirm_message = match mode {
        DeleteMode::Secret => format!("Delete password entry '{}'?", entry.display_name),
        DeleteMode::Otp => format!("Delete OTP companion for '{}'?", entry.display_name),
        DeleteMode::Both => format!(
            "Delete password entry '{}' and its OTP companion?",
            entry.display_name
        ),
    };

    if !matches!(FzfWrapper::confirm(&confirm_message)?, ConfirmResult::Yes) {
        return Ok(());
    }

    match mode {
        DeleteMode::Secret => {
            let key = entry
                .secret_key
                .as_deref()
                .ok_or_else(|| anyhow!("Entry '{}' has no password data", entry.display_name))?;
            remove_pass_entry(key)?;
        }
        DeleteMode::Otp => {
            let key = resolve_otp_key(&entry)?;
            remove_pass_entry(&key)?;
        }
        DeleteMode::Both => {
            if let Some(key) = entry.secret_key.as_deref() {
                remove_pass_entry(key)?;
            }
            if let Some(key) = entry.otp_key.as_deref() {
                remove_pass_entry(key)?;
            }
        }
    }

    maybe_notify("Pass", &format!("Deleted '{}'", entry.display_name));
    Ok(())
}

fn choose_delete_mode(entry: &PassEntry) -> Result<DeleteMode> {
    match (entry.has_secret(), entry.has_otp()) {
        (true, false) => Ok(DeleteMode::Secret),
        (false, true) => Ok(DeleteMode::Otp),
        (true, true) => {
            #[derive(Clone)]
            struct DeleteChoice {
                label: &'static str,
                mode: DeleteMode,
                preview: &'static str,
            }

            impl crate::menu_utils::FzfSelectable for DeleteChoice {
                fn fzf_display_text(&self) -> String {
                    self.label.to_string()
                }

                fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
                    crate::menu::protocol::FzfPreview::Text(self.preview.to_string())
                }

                fn fzf_key(&self) -> String {
                    self.label.to_string()
                }
            }

            let choices = vec![
                DeleteChoice {
                    label: "Delete password only",
                    mode: DeleteMode::Secret,
                    preview: "Remove the main password entry and keep the OTP companion.",
                },
                DeleteChoice {
                    label: "Delete OTP only",
                    mode: DeleteMode::Otp,
                    preview: "Remove only the OTP companion entry.",
                },
                DeleteChoice {
                    label: "Delete both",
                    mode: DeleteMode::Both,
                    preview: "Remove both the password entry and the OTP companion.",
                },
            ];

            match FzfWrapper::builder()
                .prompt("Delete mode")
                .header("Choose what to remove")
                .select(choices)?
            {
                FzfResult::Selected(choice) => Ok(choice.mode),
                _ => Err(anyhow!("Delete cancelled")),
            }
        }
        (false, false) => bail!("Entry '{}' has nothing to delete", entry.display_name),
    }
}

fn remove_pass_entry(key: &str) -> Result<()> {
    let status = Command::new("pass")
        .args(["rm", "-f", key])
        .status()
        .with_context(|| format!("Failed to delete pass entry '{key}'"))?;

    if !status.success() {
        bail!("`pass rm` failed for '{key}'");
    }

    Ok(())
}

fn select_entry(
    name: Option<String>,
    entries: &[PassEntry],
    otp_required: bool,
) -> Result<Option<PassEntry>> {
    if let Some(name) = name {
        return resolve_entry_by_name(entries, &name, otp_required).map(Some);
    }

    let candidates: Vec<PassEntry> = if otp_required {
        entries
            .iter()
            .filter(|entry| entry.has_otp())
            .cloned()
            .collect()
    } else {
        entries.to_vec()
    };

    if candidates.is_empty() {
        return Ok(None);
    }

    match FzfWrapper::builder()
        .prompt(if otp_required {
            "OTP entry"
        } else {
            "Pass entry"
        })
        .header(if otp_required {
            "Select an entry with OTP support"
        } else {
            "Select a pass entry"
        })
        .select(candidates)?
    {
        FzfResult::Selected(entry) => Ok(Some(entry)),
        _ => Ok(None),
    }
}

impl crate::menu_utils::FzfSelectable for PassEntry {
    fn fzf_display_text(&self) -> String {
        self.display_name.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview_text())
    }

    fn fzf_key(&self) -> String {
        self.display_name.clone()
    }
}

fn resolve_entry_by_name(
    entries: &[PassEntry],
    name: &str,
    otp_required: bool,
) -> Result<PassEntry> {
    let normalized = name.trim().trim_end_matches(".otp");

    entries
        .iter()
        .find(|entry| {
            let matches_name = entry.display_name == normalized
                || entry.secret_key.as_deref() == Some(name.trim())
                || entry.otp_key.as_deref() == Some(name.trim());
            matches_name && (!otp_required || entry.has_otp())
        })
        .cloned()
        .ok_or_else(|| anyhow!("No pass entry matched '{}'", name.trim()))
}

fn prompt_password_with_confirmation(prompt: &str) -> Result<String> {
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

fn prompt_text_value(
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

fn resolve_entry_name(name: Option<String>, header: &str) -> Result<String> {
    match name {
        Some(name) => sanitize_entry_name(&name),
        None => prompt_text_value(
            "Entry name",
            Some(header),
            false,
            Some("examples/github or email/work"),
        )?
        .ok_or_else(|| anyhow!("Entry creation cancelled"))
        .and_then(|value| sanitize_entry_name(&value)),
    }
}

fn sanitize_entry_name(name: &str) -> Result<String> {
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

fn normalize_otp_name(name: &str) -> String {
    if name.ends_with(".otp") {
        name.to_string()
    } else {
        format!("{name}.otp")
    }
}

fn ensure_core_dependencies() -> Result<()> {
    ensure_dependencies(&[&PASS_DEP, &GPG_DEP], "pass")
}

fn ensure_clipboard_dependencies() -> Result<()> {
    let mut deps: Vec<&'static Dependency> = vec![&PASS_DEP, &GPG_DEP];
    match DisplayServer::detect() {
        DisplayServer::Wayland => deps.push(&WL_CLIPBOARD),
        DisplayServer::X11 => deps.push(&XCLIP),
        DisplayServer::Unknown => {}
    }

    ensure_dependencies(&deps, "clipboard integration for pass")
}

fn ensure_otp_dependency() -> Result<()> {
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

fn maybe_confirm_overwrite(key: &str) -> Result<()> {
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

fn insert_password(key: &str, password: &str) -> Result<()> {
    maybe_confirm_overwrite(key)?;

    let mut child = Command::new("pass")
        .args(["insert", "-m", "-f", key])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start `pass insert` for '{key}'"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(password.as_bytes())
            .context("Failed to write password to `pass insert`")?;
        stdin.write_all(b"\n").ok();
    }

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for `pass insert` for '{key}'"))?;

    if !status.success() {
        bail!("`pass insert` failed for '{key}'");
    }

    Ok(())
}

fn ensure_password_store_dir() -> Result<PathBuf> {
    let dir = password_store_dir()?;
    if !dir.exists() {
        bail!(
            "No password store found at {}. Run `pass init <gpg-id>` first.",
            dir.display()
        );
    }
    Ok(dir)
}

fn password_store_dir() -> Result<PathBuf> {
    if let Ok(dir) = env::var("PASSWORD_STORE_DIR") {
        return Ok(PathBuf::from(dir));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("Failed to determine home directory"))?;
    Ok(home.join(".password-store"))
}

fn load_entries(store_dir: &Path) -> Result<Vec<PassEntry>> {
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

fn sort_entries_by_frecency(entries: &mut [PassEntry]) -> Result<()> {
    let path = frecency_store_path()?;
    let store: FrecencyStore = read_store(&path).unwrap_or_default();
    let sorted_items = store.sorted(SortMethod::Frecent);
    let frecency_rank: HashMap<_, _> = sorted_items
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

fn record_frecency(item: &str) -> Result<()> {
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

fn run_pass_stdout<const N: usize>(args: [&str; N]) -> Result<Vec<u8>> {
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

fn resolve_otp_key(entry: &PassEntry) -> Result<String> {
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

fn first_secret_line(output: &[u8]) -> Option<String> {
    String::from_utf8(output.to_vec())
        .ok()?
        .lines()
        .next()
        .map(|line| line.trim_end().to_string())
        .filter(|line| !line.is_empty())
}

fn should_export_output(entry: &PassEntry, output: &[u8]) -> bool {
    if entry.should_offer_export() {
        return true;
    }

    if output.len() > 100 * 1024 {
        return true;
    }

    std::str::from_utf8(output).is_err()
}

fn confirm_export_instead(display_name: &str) -> Result<bool> {
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

fn prompt_export_destination(display_name: &str, path: Option<String>) -> Result<PathBuf> {
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

fn write_export_file(destination: &Path, output: &[u8]) -> Result<()> {
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

fn copy_secret_to_clipboard(data: &[u8]) -> Result<()> {
    let display_server = DisplayServer::detect();
    copy_to_clipboard(data, &display_server)?;
    Ok(())
}

fn maybe_notify(title: &str, message: &str) {
    if LIBNOTIFY.is_installed() {
        let _ = show_notification(title, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_otp_names() {
        assert_eq!(normalize_otp_name("email/github"), "email/github.otp");
        assert_eq!(normalize_otp_name("email/github.otp"), "email/github.otp");
    }

    #[test]
    fn first_secret_line_uses_first_nonempty_line_only() {
        let output = b"topsecret\nusername: demo\n";
        assert_eq!(first_secret_line(output).as_deref(), Some("topsecret"));
    }

    #[test]
    fn groups_password_and_otp_paths_under_one_display_name() {
        let mut entry = PassEntry {
            display_name: "mail/work".to_string(),
            secret_key: Some("mail/work".to_string()),
            otp_key: Some("mail/work.otp".to_string()),
            secret_path: None,
            otp_path: None,
        };

        assert!(entry.has_secret());
        assert!(entry.has_otp());
        assert_eq!(entry.kind_label(), "password + otp");
        assert_eq!(entry.primary_action_label(), "Copy password");

        entry.secret_key = None;
        assert_eq!(entry.kind_label(), "otp");
        assert_eq!(entry.primary_action_label(), "Copy OTP code");
    }

    #[test]
    fn sanitizes_bad_entry_names() {
        assert!(sanitize_entry_name("").is_err());
        assert!(sanitize_entry_name("../foo").is_err());
        assert!(sanitize_entry_name("foo\nbar").is_err());
        assert_eq!(sanitize_entry_name("/work/github/").unwrap(), "work/github");
    }
}
