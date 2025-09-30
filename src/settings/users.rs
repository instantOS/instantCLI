use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    process::Command,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::fzf_wrapper::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::{SettingsContext, select_one_with_style};

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
            } => {
                let label = if groups.is_empty() {
                    "no groups".to_string()
                } else {
                    groups.join(", ")
                };
                format!(
                    "{} {} ({}) [{}]",
                    super::format_icon(Fa::User),
                    username,
                    shell,
                    label
                )
            }
            ManageMenuItem::Add => format!("{} Add user", super::format_icon(Fa::Plus)),
            ManageMenuItem::Back => format!("{} Back", super::format_icon(Fa::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            ManageMenuItem::User {
                username,
                shell,
                groups,
            } => {
                let mut lines = vec![
                    format!("{} User: {}", char::from(Fa::InfoCircle), username),
                    format!("{} Shell: {}", char::from(Fa::Terminal), shell),
                ];
                if groups.is_empty() {
                    lines.push(format!("{} Groups: (none)", char::from(Fa::List)));
                } else {
                    lines.push(format!(
                        "{} Groups: {}",
                        char::from(Fa::List),
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
    EditGroups,
    Remove,
    Back,
}

impl FzfSelectable for UserActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            UserActionItem::Apply => format!("{} Apply changes", super::format_icon(Fa::Check)),
            UserActionItem::ChangeShell => {
                format!("{} Change shell", super::format_icon(Fa::Terminal))
            }
            UserActionItem::EditGroups => format!("{} Edit groups", super::format_icon(Fa::List)),
            UserActionItem::Remove => format!("{} Remove entry", super::format_icon(Fa::TrashO)),
            UserActionItem::Back => format!("{} Back", super::format_icon(Fa::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let text = match self {
            UserActionItem::Apply => "Apply the stored configuration to the system",
            UserActionItem::ChangeShell => "Update the user's default shell",
            UserActionItem::EditGroups => "Set supplementary groups (comma-separated list)",
            UserActionItem::Remove => "Stop managing this user (does not delete the account)",
            UserActionItem::Back => "Return to the previous menu",
        };
        FzfPreview::Text(text.to_string())
    }
}

pub(super) fn manage_users(ctx: &mut SettingsContext) -> Result<()> {
    let mut store = UserStore::load()?;
    let mut dirty = false;

    loop {
        let mut items: Vec<ManageMenuItem> = store
            .iter()
            .map(|(username, spec)| ManageMenuItem::User {
                username: username.clone(),
                shell: spec.shell.clone(),
                groups: spec.groups.clone(),
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
        .show_input()?;
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

    let shell = FzfWrapper::builder()
        .prompt("Shell (default /bin/bash)")
        .input()
        .show_input()?;

    let groups = FzfWrapper::builder()
        .prompt("Groups (comma separated)")
        .input()
        .show_input()?;

    let spec = UserSpec {
        shell,
        groups: parse_groups(&groups),
    }
    .sanitized();

    store.insert(username, spec.clone());
    apply_user_spec(ctx, username, &spec)?;

    ctx.emit_success(
        "settings.users.added",
        &format!("{} registered for management.", username),
    );

    Ok(true)
}

fn handle_user(ctx: &mut SettingsContext, store: &mut UserStore, username: &str) -> Result<bool> {
    let mut changed = false;

    loop {
        let Some(spec) = store.get(username).cloned() else {
            return Ok(changed);
        };

        let actions = vec![
            UserActionItem::Apply,
            UserActionItem::ChangeShell,
            UserActionItem::EditGroups,
            UserActionItem::Remove,
            UserActionItem::Back,
        ];

        match select_one_with_style(actions)? {
            Some(UserActionItem::Apply) => {
                apply_user_spec(ctx, username, &spec)?;
            }
            Some(UserActionItem::ChangeShell) => {
                let input = FzfWrapper::builder()
                    .prompt("Shell")
                    .header("Enter new shell path")
                    .input()
                    .show_input()?;
                if input.trim().is_empty() {
                    continue;
                }
                let mut updated = spec.clone();
                updated.shell = input;
                let updated = updated.sanitized();
                store.insert(username, updated.clone());
                apply_user_spec(ctx, username, &updated)?;
                changed = true;
            }
            Some(UserActionItem::EditGroups) => {
                let input = FzfWrapper::builder()
                    .prompt("Groups")
                    .header("Comma separated group names")
                    .input()
                    .show_input()?;
                let mut updated = spec.clone();
                updated.groups = parse_groups(&input);
                let updated = updated.sanitized();
                store.insert(username, updated.clone());
                apply_user_spec(ctx, username, &updated)?;
                changed = true;
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

fn parse_groups(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|segment| segment.trim().to_string())
        .filter(|segment| !segment.is_empty())
        .collect()
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
            char::from(Fa::ExclamationCircle),
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
