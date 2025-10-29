use std::collections::BTreeSet;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::ui::prelude::*;

use super::super::context::SettingsContext;
use super::menu_items::{GroupItem, ShellItem};
use super::models::{default_shell, UserSpec};
use super::store::UserStore;
use super::system::{get_all_system_groups, get_available_shells, get_user_info, partition_groups};

/// Prompt for a password with confirmation
pub(super) fn prompt_password_with_confirmation(
    ctx: &SettingsContext,
    prompt: &str,
) -> Result<Option<String>> {
    let password1 = FzfWrapper::builder()
        .prompt(prompt)
        .password()
        .show_password()?;

    if password1.trim().is_empty() {
        ctx.emit_info("settings.users.password", "Password cannot be empty.");
        return Ok(None);
    }

    let password2 = FzfWrapper::builder()
        .prompt("Confirm password")
        .password()
        .show_password()?;

    if password1 != password2 {
        ctx.emit_info("settings.users.password", "Passwords do not match.");
        return Ok(None);
    }

    Ok(Some(password1))
}

/// Set a user's password using chpasswd
pub(super) fn set_user_password(
    ctx: &mut SettingsContext,
    username: &str,
    password: &str,
) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = if ctx.is_privileged() {
        std::process::Command::new("chpasswd")
            .stdin(Stdio::piped())
            .spawn()
    } else {
        std::process::Command::new("sudo")
            .arg("chpasswd")
            .stdin(Stdio::piped())
            .spawn()
    }?;

    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "{}:{}", username, password)?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("Failed to set password for user {}", username);
    }

    ctx.emit_success(
        "settings.users.password",
        &format!("Password updated for {}", username),
    );

    Ok(())
}

/// Apply a user specification to the system
pub(super) fn apply_user_spec(
    ctx: &mut SettingsContext,
    username: &str,
    spec: &UserSpec,
) -> Result<()> {
    let desired = spec.sanitized();
    let (valid_groups, missing_groups) = partition_groups(&desired.groups)?;

    if !missing_groups.is_empty() {
        let arch_hint = if missing_groups.iter().any(|group| group == "sudo") {
            " Arch-based systems typically use the 'wheel' group for sudo access."
        } else {
            ""
        };

        let message = format!(
            "{} Skipping unknown group(s): {}.{arch_hint}",
            char::from(NerdFont::Warning),
            missing_groups.join(", ")
        );
        emit(Level::Warn, "settings.users.missing_groups", &message, None);
    }

    let info = get_user_info(username)?;

    if info.is_none() {
        create_new_user(ctx, username, &desired, &valid_groups)?;
        return Ok(());
    }

    let info = info.unwrap();
    update_existing_user(ctx, username, &desired, &valid_groups, &info)?;

    Ok(())
}

/// Create a new system user
fn create_new_user(
    ctx: &mut SettingsContext,
    username: &str,
    spec: &UserSpec,
    valid_groups: &[String],
) -> Result<()> {
    ctx.emit_info(
        "settings.users.create",
        &format!("Creating system user {}", username),
    );
    ctx.run_command_as_root("useradd", ["-m", "-s", spec.shell.as_str(), username])?;

    if !valid_groups.is_empty() {
        let joined = valid_groups.join(",");
        ctx.run_command_as_root("usermod", ["-a", "-G", &joined, username])?;
    }

    Ok(())
}

/// Update an existing system user
fn update_existing_user(
    ctx: &mut SettingsContext,
    username: &str,
    spec: &UserSpec,
    valid_groups: &[String],
    info: &super::models::UserInfo,
) -> Result<()> {
    // Update shell if different
    if info.shell != spec.shell {
        ctx.emit_info(
            "settings.users.shell",
            &format!("Setting shell for {} to {}", username, spec.shell),
        );
        ctx.run_command_as_root("chsh", ["-s", spec.shell.as_str(), username])?;
    }

    // Update groups
    let desired_set: BTreeSet<_> = valid_groups.iter().cloned().collect();
    let current_set: BTreeSet<_> = info.groups.iter().cloned().collect();

    // Add missing groups
    for group in desired_set.difference(&current_set) {
        ctx.run_command_as_root("usermod", ["-a", "-G", group, username])?;
    }

    // Remove extra groups (except primary group)
    for group in current_set.difference(&desired_set) {
        if Some(group.as_str()) == info.primary_group.as_deref() {
            continue;
        }
        ctx.run_command_as_root("gpasswd", ["-d", username, group])?;
    }

    Ok(())
}

/// Prompt user to select a shell
pub(super) fn select_shell(ctx: &SettingsContext, prompt: &str) -> Result<Option<String>> {
    let available_shells = get_available_shells()?;
    let shell_items: Vec<ShellItem> = available_shells
        .into_iter()
        .map(|path| ShellItem { path })
        .collect();

    if shell_items.is_empty() {
        ctx.emit_info(
            "settings.users.shell",
            "No shells found in /etc/shells, using default",
        );
        return Ok(Some(default_shell()));
    }

    let selected = FzfWrapper::builder()
        .prompt(prompt)
        .header("Choose a shell from /etc/shells (Esc for default)")
        .select(shell_items)?;

    match selected {
        FzfResult::Selected(item) => Ok(Some(item.path)),
        _ => Ok(Some(default_shell())),
    }
}

/// Prompt user to select groups (multi-select)
pub(super) fn select_groups(header: &str) -> Result<Vec<String>> {
    let all_groups = get_all_system_groups()?;
    let group_items: Vec<GroupItem> = all_groups
        .into_iter()
        .map(|name| GroupItem { name })
        .collect();

    if group_items.is_empty() {
        return Ok(Vec::new());
    }

    let result = FzfWrapper::builder()
        .prompt("Select groups")
        .header(header)
        .args(["--multi"])
        .select(group_items)?;

    let mut selected_groups = Vec::new();
    match result {
        FzfResult::Selected(item) => {
            selected_groups.push(item.name);
        }
        FzfResult::MultiSelected(items) => {
            selected_groups.extend(items.into_iter().map(|item| item.name));
        }
        _ => {}
    }

    Ok(selected_groups)
}

/// Get or initialize a user spec from the store or system
pub(super) fn get_or_init_user_spec(store: &UserStore, username: &str) -> UserSpec {
    if let Some(spec) = store.get(username) {
        spec.clone()
    } else if let Ok(Some(info)) = get_user_info(username) {
        // User exists on system but not in TOML - create initial spec from system
        UserSpec {
            shell: info.shell,
            groups: info.groups,
        }
    } else {
        // User doesn't exist anywhere - use defaults
        UserSpec::default()
    }
}

