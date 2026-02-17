use anyhow::Result;
use std::fmt;

use crate::menu_utils::{FzfResult, FzfWrapper};

use super::super::context::SettingsContext;
use super::menu_items::{GroupItem, ShellItem};
use super::models::default_shell;
use super::system::{get_all_system_groups, get_available_shells, partition_groups};

/// Prompt for a password with confirmation
pub(super) fn prompt_password_with_confirmation(
    ctx: &SettingsContext,
    prompt: &str,
) -> Result<Option<String>> {
    let password_result = FzfWrapper::builder()
        .prompt(prompt)
        .password()
        .with_confirmation()
        .password_dialog()?;

    let password = match password_result {
        FzfResult::Selected(s) => s,
        _ => return Ok(None),
    };

    if password.trim().is_empty() {
        ctx.emit_info("settings.users.password", "Password cannot be empty.");
        return Ok(None);
    }

    Ok(Some(password))
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

/// Create a new system user
pub(super) fn create_user(
    ctx: &mut SettingsContext,
    username: &str,
    shell: &str,
    groups: &[String],
) -> Result<()> {
    let (valid_groups, missing_groups) = partition_groups(groups)?;

    if !missing_groups.is_empty() {
        ctx.emit_info(
            "settings.users.groups",
            &format!("Skipping unknown group(s): {}", missing_groups.join(", ")),
        );
    }

    ctx.emit_info(
        "settings.users.create",
        &format!("Creating system user {}", username),
    );
    ctx.run_command_as_root("useradd", ["-m", "-s", shell, username])?;

    if !valid_groups.is_empty() {
        let joined = valid_groups.join(",");
        ctx.run_command_as_root("usermod", ["-a", "-G", &joined, username])?;
    }

    Ok(())
}

/// Change a user's shell
pub(super) fn change_user_shell(
    ctx: &mut SettingsContext,
    username: &str,
    shell: &str,
) -> Result<()> {
    ctx.emit_info(
        "settings.users.shell",
        &format!("Setting shell for {} to {}", username, shell),
    );
    ctx.run_command_as_root("chsh", ["-s", shell, username])?;
    ctx.emit_success(
        "settings.users.shell",
        &format!("Shell updated for {}", username),
    );
    Ok(())
}

/// Add a user to a group
pub(super) fn add_user_to_group(
    ctx: &mut SettingsContext,
    username: &str,
    group: &str,
) -> Result<()> {
    ctx.run_command_as_root("usermod", ["-a", "-G", group, username])?;
    ctx.emit_success(
        "settings.users.groups",
        &format!("Added {} to group {}", username, group),
    );
    Ok(())
}

/// Remove a user from a group
pub(super) fn remove_user_from_group(
    ctx: &mut SettingsContext,
    username: &str,
    group: &str,
) -> Result<()> {
    ctx.run_command_as_root("gpasswd", ["-d", username, group])?;
    Ok(())
}

/// Delete a user account and their home directory
pub(super) fn delete_user(ctx: &mut SettingsContext, username: &str) -> Result<()> {
    ctx.emit_info(
        "settings.users.delete",
        &format!("Deleting user {}...", username),
    );
    ctx.run_command_as_root("userdel", ["-r", username])?;
    Ok(())
}

/// Prompt for a new group name
pub(super) fn prompt_group_name() -> Result<String> {
    let group_name = FzfWrapper::builder()
        .prompt("New group")
        .input()
        .input_dialog()?;
    Ok(group_name.trim().to_string())
}

/// Create a new system group
pub(super) fn create_group(ctx: &mut SettingsContext, group_name: &str) -> Result<()> {
    ctx.emit_info(
        "settings.users.groups",
        &format!("Creating group {}", group_name),
    );
    ctx.run_command_as_root("groupadd", [group_name])?;
    ctx.emit_success(
        "settings.users.groups",
        &format!("Group {} created.", group_name),
    );
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum UnixNameValidationError {
    Empty,
    TooLong,
    InvalidStart,
    InvalidChar,
}

impl UnixNameValidationError {
    fn display_with_label(&self, label: &str) -> String {
        match self {
            UnixNameValidationError::Empty => format!("{} cannot be empty.", label),
            UnixNameValidationError::TooLong => {
                format!("{} must be at most 32 characters long.", label)
            }
            UnixNameValidationError::InvalidStart => {
                format!("{} must start with a lowercase letter.", label)
            }
            UnixNameValidationError::InvalidChar => format!(
                "{} may only contain lowercase letters, digits, hyphens, or underscores.",
                label
            ),
        }
    }
}

impl fmt::Display for UnixNameValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_with_label("Name"))
    }
}

fn validate_unix_name(name: &str) -> Result<(), UnixNameValidationError> {
    if name.is_empty() {
        return Err(UnixNameValidationError::Empty);
    }

    if name.chars().count() > 32 {
        return Err(UnixNameValidationError::TooLong);
    }

    let mut chars = name.chars();
    let first = chars.next().ok_or(UnixNameValidationError::Empty)?;
    if !first.is_ascii_lowercase() {
        return Err(UnixNameValidationError::InvalidStart);
    }

    if chars.any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' && c != '_') {
        return Err(UnixNameValidationError::InvalidChar);
    }

    Ok(())
}

pub(super) type UsernameValidationError = UnixNameValidationError;
pub(super) type GroupNameValidationError = UnixNameValidationError;

pub(super) fn validate_username(username: &str) -> Result<(), UsernameValidationError> {
    validate_unix_name(username)
}

pub(super) fn validate_group_name(group_name: &str) -> Result<(), GroupNameValidationError> {
    validate_unix_name(group_name)
}

#[cfg(test)]
mod tests {
    use super::{UsernameValidationError, validate_username};

    #[test]
    fn accepts_valid_username() {
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("alice1").is_ok());
        assert!(validate_username("a_l-i_c-e").is_ok());
    }

    #[test]
    fn rejects_empty_username() {
        assert_eq!(validate_username(""), Err(UsernameValidationError::Empty));
    }

    #[test]
    fn rejects_too_long_username() {
        let long = "a".repeat(33);
        assert_eq!(
            validate_username(&long),
            Err(UsernameValidationError::TooLong)
        );
    }

    #[test]
    fn rejects_invalid_start() {
        assert_eq!(
            validate_username("1alice"),
            Err(UsernameValidationError::InvalidStart)
        );
        assert_eq!(
            validate_username("-alice"),
            Err(UsernameValidationError::InvalidStart)
        );
        assert_eq!(
            validate_username("Alice"),
            Err(UsernameValidationError::InvalidStart)
        );
    }

    #[test]
    fn rejects_invalid_chars() {
        assert_eq!(
            validate_username("alice!"),
            Err(UsernameValidationError::InvalidChar)
        );
        assert_eq!(
            validate_username("alice."),
            Err(UsernameValidationError::InvalidChar)
        );
    }
}
