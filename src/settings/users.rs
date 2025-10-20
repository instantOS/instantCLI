use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    process::Command,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::context::{SettingsContext, format_icon, select_one_with_style};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UsersFile {
    #[serde(default)]
    users: BTreeMap<String, UserSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UserSpec {
    #[serde(default = "default_shell")]
    shell: String,
    #[serde(default)]
    groups: Vec<String>,
}

impl UserSpec {
    fn sanitized(&self) -> UserSpec {
        let shell = if self.shell.trim().is_empty() {
            default_shell()
        } else {
            self.shell.clone()
        };

        let mut groups: Vec<String> = self
            .groups
            .iter()
            .map(|group| group.trim().to_string())
            .filter(|group| !group.is_empty())
            .collect();

        groups.sort();
        groups.dedup();

        UserSpec { shell, groups }
    }
}

fn default_shell() -> String {
    "/bin/bash".to_string()
}

struct UserStore {
    path: PathBuf,
    data: UsersFile,
}

impl UserStore {
    fn load() -> Result<Self> {
        let path = users_file_path()?;
        if !path.exists() {
            return Ok(Self {
                path,
                data: UsersFile::default(),
            });
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("reading user settings from {}", path.display()))?;
        let data = toml::from_str(&contents)
            .with_context(|| format!("parsing user settings at {}", path.display()))?;

        Ok(Self { path, data })
    }

    fn save(&self) -> Result<()> {
        let contents =
            toml::to_string_pretty(&self.data).context("serializing user settings to toml")?;
        fs::write(&self.path, contents)
            .with_context(|| format!("writing user settings to {}", self.path.display()))
    }

    fn iter(&self) -> impl Iterator<Item = (&String, &UserSpec)> {
        self.data.users.iter()
    }

    fn get(&self, username: &str) -> Option<&UserSpec> {
        self.data.users.get(username)
    }

    fn insert(&mut self, username: &str, spec: UserSpec) {
        self.data.users.insert(username.to_string(), spec);
    }

    fn remove(&mut self, username: &str) {
        self.data.users.remove(username);
    }
}

fn users_file_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("unable to determine user config directory")?
        .join("instant");
    fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating config directory {}", config_dir.display()))?;
    Ok(config_dir.join("users.toml"))
}

#[derive(Clone)]
enum ManageMenuItem {
    User {
        username: String,
        shell: String,
        groups: Vec<String>,
        in_toml: bool,
    },
    Add,
    Back,
}

impl FzfSelectable for ManageMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            ManageMenuItem::User {
                username,
                shell,
                groups,
                in_toml,
            } => {
                let label = if groups.is_empty() {
                    "no groups".to_string()
                } else {
                    groups.join(", ")
                };
                let status = if *in_toml { "managed" } else { "system" };
                format!(
                    "{} {} ({}) [{}] ({})",
                    format_icon(NerdFont::User),
                    username,
                    shell,
                    label,
                    status
                )
            }
            ManageMenuItem::Add => format!("{} Add user", format_icon(NerdFont::Plus)),
            ManageMenuItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            ManageMenuItem::User {
                username,
                shell,
                groups,
                in_toml,
            } => {
                let status = if *in_toml {
                    "Managed in TOML configuration"
                } else {
                    "System user with home directory"
                };
                let mut lines = vec![
                    format!("{} User: {}", char::from(NerdFont::Info), username),
                    format!("{} Status: {}", char::from(NerdFont::Tag), status),
                    format!("{} Shell: {}", char::from(NerdFont::Terminal), shell),
                ];
                if groups.is_empty() {
                    lines.push(format!("{} Groups: (none)", char::from(NerdFont::List)));
                } else {
                    lines.push(format!(
                        "{} Groups: {}",
                        char::from(NerdFont::List),
                        groups.join(", ")
                    ));
                }
                FzfPreview::Text(lines.join("\n"))
            }
            ManageMenuItem::Add => FzfPreview::Text("Create a new managed user entry".to_string()),
            ManageMenuItem::Back => FzfPreview::Text("Return to settings".to_string()),
        }
    }
}

#[derive(Clone)]
enum UserActionItem {
    Apply,
    ChangeShell,
    ChangePassword,
    ManageGroups,
    Remove,
    Back,
}

impl FzfSelectable for UserActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            UserActionItem::Apply => format!("{} Apply changes", format_icon(NerdFont::Check)),
            UserActionItem::ChangeShell => {
                format!("{} Change shell", format_icon(NerdFont::Terminal))
            }
            UserActionItem::ChangePassword => {
                format!("{} Change password", format_icon(NerdFont::Key))
            }
            UserActionItem::ManageGroups => {
                format!("{} Manage groups", format_icon(NerdFont::List))
            }
            UserActionItem::Remove => format!("{} Remove entry", format_icon(NerdFont::Trash)),
            UserActionItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let text = match self {
            UserActionItem::Apply => "Apply the stored configuration to the system",
            UserActionItem::ChangeShell => "Update the user's default shell",
            UserActionItem::ChangePassword => "Change the user's password",
            UserActionItem::ManageGroups => "Add or remove supplementary groups",
            UserActionItem::Remove => "Stop managing this user (does not delete the account)",
            UserActionItem::Back => "Return to the previous menu",
        };
        FzfPreview::Text(text.to_string())
    }
}

#[derive(Clone)]
enum GroupMenuItem {
    ExistingGroup(String),
    AddGroup,
    Back,
}

impl FzfSelectable for GroupMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            GroupMenuItem::ExistingGroup(name) => {
                format!("{} {}", char::from(NerdFont::List), name)
            }
            GroupMenuItem::AddGroup => format!("{} Add group", format_icon(NerdFont::Plus)),
            GroupMenuItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let text = match self {
            GroupMenuItem::ExistingGroup(name) => {
                format!("Group: {}\n\nSelect to manage this group membership", name)
            }
            GroupMenuItem::AddGroup => "Add a new supplementary group to the user".to_string(),
            GroupMenuItem::Back => "Return to user management".to_string(),
        };
        FzfPreview::Text(text)
    }
}

#[derive(Clone)]
enum GroupActionItem {
    RemoveGroup,
    Back,
}

impl FzfSelectable for GroupActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            GroupActionItem::RemoveGroup => {
                format!("{} Remove group", format_icon(NerdFont::Minus))
            }
            GroupActionItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let text = match self {
            GroupActionItem::RemoveGroup => "Remove this group from the user",
            GroupActionItem::Back => "Return to group list",
        };
        FzfPreview::Text(text.to_string())
    }
}

#[derive(Clone)]
struct GroupItem {
    name: String,
}

impl FzfSelectable for GroupItem {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", char::from(NerdFont::List), self.name)
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(format!("Group: {}", self.name))
    }
}

#[derive(Clone)]
struct ShellItem {
    path: String,
}

impl FzfSelectable for ShellItem {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", char::from(NerdFont::Terminal), self.path)
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(format!("Shell: {}", self.path))
    }
}

pub(super) fn manage_users(ctx: &mut SettingsContext) -> Result<()> {
    let mut store = UserStore::load()?;
    let mut dirty = false;

    loop {
        // Get all users: from TOML + system users with home directories
        let system_users = get_system_users_with_home()?;
        let mut all_usernames = BTreeSet::new();

        // Add users from TOML
        for username in store.iter().map(|(name, _)| name.clone()) {
            all_usernames.insert(username);
        }

        // Add system users with home directories
        for username in system_users {
            all_usernames.insert(username);
        }

        let mut items: Vec<ManageMenuItem> = all_usernames
            .into_iter()
            .filter_map(|username| {
                let in_toml = store.get(&username).is_some();

                // Try to get info from store first, then from system
                let (shell, groups) = if let Some(spec) = store.get(&username) {
                    (spec.shell.clone(), spec.groups.clone())
                } else if let Ok(Some(info)) = get_user_info(&username) {
                    (info.shell, info.groups)
                } else {
                    return None;
                };

                Some(ManageMenuItem::User {
                    username,
                    shell,
                    groups,
                    in_toml,
                })
            })
            .collect();

        items.sort_by(|a, b| match (a, b) {
            (
                ManageMenuItem::User { username: lhs, .. },
                ManageMenuItem::User { username: rhs, .. },
            ) => lhs.cmp(rhs),
            (ManageMenuItem::User { .. }, _) => std::cmp::Ordering::Less,
            (_, ManageMenuItem::User { .. }) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        });
        items.push(ManageMenuItem::Add);
        items.push(ManageMenuItem::Back);

        match select_one_with_style(items)? {
            Some(ManageMenuItem::Add) => {
                if add_user(ctx, &mut store)? {
                    dirty = true;
                }
            }
            Some(ManageMenuItem::User { username, .. }) => {
                if handle_user(ctx, &mut store, &username)? {
                    dirty = true;
                }
            }
            _ => break,
        }
    }

    if dirty {
        store.save()?;
        ctx.emit_success("settings.users.saved", "User configuration updated.");
    } else {
        ctx.emit_info("settings.users.noop", "No changes made to users.");
    }

    Ok(())
}

fn add_user(ctx: &mut SettingsContext, store: &mut UserStore) -> Result<bool> {
    let username = FzfWrapper::builder()
        .prompt("New username")
        .input()
        .input_dialog()?;
    let username = username.trim();
    if username.is_empty() {
        ctx.emit_info("settings.users.add.cancelled", "Creation cancelled.");
        return Ok(false);
    }

    if store.get(username).is_some() {
        ctx.emit_info(
            "settings.users.add.exists",
            &format!("{} already managed.", username),
        );
        return Ok(false);
    }

    // Select shell from /etc/shells
    let available_shells = get_available_shells()?;
    let shell_items: Vec<ShellItem> = available_shells
        .into_iter()
        .map(|path| ShellItem { path })
        .collect();

    let shell = if shell_items.is_empty() {
        ctx.emit_info(
            "settings.users.shell",
            "No shells found in /etc/shells, using default",
        );
        default_shell()
    } else {
        let selected = FzfWrapper::builder()
            .prompt("Select shell")
            .header("Choose a shell from /etc/shells (Esc for default)")
            .select(shell_items)?;

        match selected {
            crate::menu_utils::FzfResult::Selected(item) => item.path,
            _ => default_shell(),
        }
    };

    // Use group selection menu instead of comma-separated input
    let all_groups = get_all_system_groups()?;
    let group_items: Vec<GroupItem> = all_groups
        .into_iter()
        .map(|name| GroupItem { name })
        .collect();

    let mut selected_groups = Vec::new();
    if !group_items.is_empty() {
        let result = FzfWrapper::builder()
            .prompt("Select groups")
            .header("Use Tab to select multiple, Enter to confirm, Esc to skip")
            .args(["--multi"])
            .select(group_items)?;

        match result {
            crate::menu_utils::FzfResult::Selected(item) => {
                selected_groups.push(item.name);
            }
            crate::menu_utils::FzfResult::MultiSelected(items) => {
                selected_groups.extend(items.into_iter().map(|item| item.name));
            }
            _ => {}
        }
    }

    let spec = UserSpec {
        shell,
        groups: selected_groups,
    }
    .sanitized();

    store.insert(username, spec.clone());
    apply_user_spec(ctx, username, &spec)?;

    // Prompt for password
    if let Some(password) = prompt_password_with_confirmation(ctx, "Set password for user")? {
        set_user_password(ctx, username, &password)?;
    }

    ctx.emit_success(
        "settings.users.added",
        &format!("{} registered for management.", username),
    );

    Ok(true)
}

fn handle_user(ctx: &mut SettingsContext, store: &mut UserStore, username: &str) -> Result<bool> {
    let mut changed = false;

    // Load or initialize user spec
    let mut current_spec = if let Some(spec) = store.get(username) {
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
    };

    loop {
        let actions = vec![
            UserActionItem::Apply,
            UserActionItem::ChangeShell,
            UserActionItem::ChangePassword,
            UserActionItem::ManageGroups,
            UserActionItem::Remove,
            UserActionItem::Back,
        ];

        match select_one_with_style(actions)? {
            Some(UserActionItem::Apply) => {
                apply_user_spec(ctx, username, &current_spec)?;
                // Save to TOML after successful apply
                store.insert(username, current_spec.clone());
                changed = true;
            }
            Some(UserActionItem::ChangeShell) => {
                let available_shells = get_available_shells()?;
                let shell_items: Vec<ShellItem> = available_shells
                    .into_iter()
                    .map(|path| ShellItem { path })
                    .collect();

                if shell_items.is_empty() {
                    ctx.emit_info("settings.users.shell", "No shells found in /etc/shells");
                    continue;
                }

                let selected = FzfWrapper::builder()
                    .prompt("Select shell")
                    .header("Choose a shell from /etc/shells")
                    .select(shell_items)?;

                if let crate::menu_utils::FzfResult::Selected(item) = selected {
                    current_spec.shell = item.path;
                    current_spec = current_spec.sanitized();
                    store.insert(username, current_spec.clone());
                    apply_user_spec(ctx, username, &current_spec)?;
                    changed = true;
                }
            }
            Some(UserActionItem::ChangePassword) => {
                if let Some(password) = prompt_password_with_confirmation(ctx, "New password")? {
                    set_user_password(ctx, username, &password)?;
                }
            }
            Some(UserActionItem::ManageGroups) => {
                if manage_user_groups(ctx, store, username, &mut current_spec)? {
                    changed = true;
                }
            }
            Some(UserActionItem::Remove) => {
                store.remove(username);
                ctx.emit_info(
                    "settings.users.removed",
                    &format!("{} removed from management.", username),
                );
                changed = true;
                break;
            }
            _ => break,
        }
    }

    Ok(changed)
}

fn manage_user_groups(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
    spec: &mut UserSpec,
) -> Result<bool> {
    let mut changed = false;

    loop {
        // Build menu with existing groups + Add Group option
        let mut items: Vec<GroupMenuItem> = spec
            .groups
            .iter()
            .map(|name| GroupMenuItem::ExistingGroup(name.clone()))
            .collect();

        items.push(GroupMenuItem::AddGroup);
        items.push(GroupMenuItem::Back);

        match select_one_with_style(items)? {
            Some(GroupMenuItem::ExistingGroup(group_name)) => {
                // Show submenu for this group
                if manage_single_group(ctx, store, username, spec, &group_name)? {
                    changed = true;
                }
            }
            Some(GroupMenuItem::AddGroup) => {
                // Get all system groups
                let all_groups = get_all_system_groups()?;
                if all_groups.is_empty() {
                    ctx.emit_info("settings.users.groups", "No system groups found.");
                    continue;
                }

                // Filter out groups already assigned
                let current_groups: BTreeSet<_> = spec.groups.iter().cloned().collect();
                let available_groups: Vec<GroupItem> = all_groups
                    .into_iter()
                    .filter(|g| !current_groups.contains(g))
                    .map(|name| GroupItem { name })
                    .collect();

                if available_groups.is_empty() {
                    ctx.emit_info(
                        "settings.users.groups",
                        "User is already in all available groups.",
                    );
                    continue;
                }

                // Show selection menu with multi-select
                let selected = FzfWrapper::builder()
                    .prompt("Select groups to add")
                    .header("Use Tab to select multiple, Enter to confirm")
                    .args(["--multi"])
                    .select(available_groups)?;

                match selected {
                    crate::menu_utils::FzfResult::Selected(item) => {
                        if !spec.groups.contains(&item.name) {
                            spec.groups.push(item.name);
                        }
                        *spec = spec.sanitized();
                        store.insert(username, spec.clone());
                        apply_user_spec(ctx, username, spec)?;
                        changed = true;
                    }
                    crate::menu_utils::FzfResult::MultiSelected(group_items) => {
                        for item in group_items {
                            if !spec.groups.contains(&item.name) {
                                spec.groups.push(item.name);
                            }
                        }
                        *spec = spec.sanitized();
                        store.insert(username, spec.clone());
                        apply_user_spec(ctx, username, spec)?;
                        changed = true;
                    }
                    _ => {}
                }
            }
            _ => break,
        }
    }

    Ok(changed)
}

fn manage_single_group(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
    spec: &mut UserSpec,
    group_name: &str,
) -> Result<bool> {
    let actions = vec![GroupActionItem::RemoveGroup, GroupActionItem::Back];

    match select_one_with_style(actions)? {
        Some(GroupActionItem::RemoveGroup) => {
            // Get primary group to prevent removal
            let primary_group = get_user_info(username)?.and_then(|info| info.primary_group);

            // Don't remove primary group
            if Some(group_name) == primary_group.as_deref() {
                ctx.emit_info(
                    "settings.users.groups",
                    &format!("Cannot remove primary group: {}", group_name),
                );
                return Ok(false);
            }

            spec.groups.retain(|g| g != group_name);
            *spec = spec.sanitized();
            store.insert(username, spec.clone());
            apply_user_spec(ctx, username, spec)?;

            ctx.emit_success(
                "settings.users.groups",
                &format!("Removed {} from {}", group_name, username),
            );

            Ok(true)
        }
        _ => Ok(false),
    }
}

fn prompt_password_with_confirmation(
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

fn set_user_password(ctx: &mut SettingsContext, username: &str, password: &str) -> Result<()> {
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

fn apply_user_spec(ctx: &mut SettingsContext, username: &str, spec: &UserSpec) -> Result<()> {
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
        ctx.emit_info(
            "settings.users.create",
            &format!("Creating system user {}", username),
        );
        ctx.run_command_as_root("useradd", ["-m", "-s", desired.shell.as_str(), username])?;

        if !valid_groups.is_empty() {
            let joined = valid_groups.join(",");
            ctx.run_command_as_root("usermod", ["-a", "-G", &joined, username])?;
        }

        return Ok(());
    }

    let info = info.unwrap();

    if info.shell != desired.shell {
        ctx.emit_info(
            "settings.users.shell",
            &format!("Setting shell for {} to {}", username, desired.shell),
        );
        ctx.run_command_as_root("chsh", ["-s", desired.shell.as_str(), username])?;
    }

    let desired_set: BTreeSet<_> = valid_groups.iter().cloned().collect();
    let current_set: BTreeSet<_> = info.groups.iter().cloned().collect();

    for group in desired_set.difference(&current_set) {
        ctx.run_command_as_root("usermod", ["-a", "-G", group, username])?;
    }

    for group in current_set.difference(&desired_set) {
        if Some(group.as_str()) == info.primary_group.as_deref() {
            continue;
        }
        ctx.run_command_as_root("gpasswd", ["-d", username, group])?;
    }

    Ok(())
}

fn partition_groups(groups: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let mut valid = Vec::new();
    let mut missing = Vec::new();

    for group in groups {
        if group_exists(group)? {
            valid.push(group.clone());
        } else {
            missing.push(group.clone());
        }
    }

    Ok((valid, missing))
}

fn group_exists(name: &str) -> Result<bool> {
    let status = Command::new("getent")
        .arg("group")
        .arg(name)
        .status()
        .with_context(|| format!("checking group {name}"))?;
    Ok(status.success())
}

struct UserInfo {
    shell: String,
    primary_group: Option<String>,
    groups: Vec<String>,
}

fn get_user_info(username: &str) -> Result<Option<UserInfo>> {
    let passwd = Command::new("getent")
        .arg("passwd")
        .arg(username)
        .output()
        .with_context(|| format!("querying passwd entry for {}", username))?;

    if !passwd.status.success() {
        return Ok(None);
    }

    let line = String::from_utf8(passwd.stdout).context("parsing passwd entry")?;
    let mut fields = line.trim().split(':');
    let _name = fields.next();
    let _pw = fields.next();
    let _uid = fields.next();
    let _gid = fields.next();
    let _gecos = fields.next();
    let _home = fields.next();
    let shell = fields
        .next()
        .map(str::to_string)
        .unwrap_or_else(default_shell);

    let primary_group = Command::new("id")
        .arg("-gn")
        .arg(username)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        });

    let groups = Command::new("id")
        .arg("-nG")
        .arg(username)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(
                    String::from_utf8_lossy(&output.stdout)
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();

    Ok(Some(UserInfo {
        shell,
        primary_group,
        groups,
    }))
}

fn get_system_users_with_home() -> Result<Vec<String>> {
    let output = Command::new("getent")
        .arg("passwd")
        .output()
        .context("querying system users")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let passwd_data = String::from_utf8_lossy(&output.stdout);
    let users: Vec<String> = passwd_data
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 7 {
                let username = fields[0];
                let home = fields[5];
                if home.starts_with("/home/") {
                    return Some(username.to_string());
                }
            }
            None
        })
        .collect();

    Ok(users)
}

fn get_all_system_groups() -> Result<Vec<String>> {
    let output = Command::new("getent")
        .arg("group")
        .output()
        .context("querying system groups")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let group_data = String::from_utf8_lossy(&output.stdout);
    let groups: Vec<String> = group_data
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if !fields.is_empty() {
                Some(fields[0].to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(groups)
}

fn get_available_shells() -> Result<Vec<String>> {
    let shells_path = std::path::Path::new("/etc/shells");
    if !shells_path.exists() {
        return Ok(vec![default_shell()]);
    }

    let contents = fs::read_to_string(shells_path).context("reading /etc/shells")?;

    let shells: Vec<String> = contents
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect();

    if shells.is_empty() {
        Ok(vec![default_shell()])
    } else {
        Ok(shells)
    }
}
