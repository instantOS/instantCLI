use std::collections::BTreeSet;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfWrapper};

use super::super::context::SettingsContext;
use super::menu_items::{
    GroupActionItem, GroupItem, GroupMenuItem, ManageMenuItem, UserActionItem,
};
use super::store::UserStore;
use super::system::{
    WheelSudoStatus, get_all_system_groups, get_system_users_with_home, get_user_info,
    group_exists, wheel_sudo_status,
};
use super::utils::{
    add_user_to_group, change_user_shell, create_group, create_user, prompt_group_name,
    prompt_password_with_confirmation, remove_user_from_group, select_groups, select_shell,
    set_user_password, validate_group_name, validate_username,
};
use crate::menu_utils::select_one_with_style;

const WHEEL_GROUP: &str = "wheel";

/// Main entry point for user management
pub fn manage_users(ctx: &mut SettingsContext) -> Result<()> {
    let mut store = UserStore::load()?;
    let mut dirty = false;

    loop {
        let items = build_user_menu_items(&store)?;

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

/// Build the menu items for the main user management screen
fn build_user_menu_items(store: &UserStore) -> Result<Vec<ManageMenuItem>> {
    let system_users = get_system_users_with_home()?;
    let mut all_usernames = BTreeSet::new();

    // Add managed users from TOML
    for username in store.iter() {
        all_usernames.insert(username.clone());
    }

    // Add system users with home directories
    for username in system_users {
        all_usernames.insert(username);
    }

    let mut items: Vec<ManageMenuItem> = all_usernames
        .into_iter()
        .filter_map(|username| {
            let is_managed = store.is_managed(&username);

            // Always read current state from system
            let info = get_user_info(&username).ok()??;

            Some(ManageMenuItem::User {
                username,
                shell: info.shell,
                groups: info.groups,
                in_toml: is_managed,
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

    Ok(items)
}

/// Add a new user
fn add_user(ctx: &mut SettingsContext, store: &mut UserStore) -> Result<bool> {
    let username = prompt_username()?;
    if username.is_empty() {
        ctx.emit_info("settings.users.add.cancelled", "Creation cancelled.");
        return Ok(false);
    }

    if let Err(err) = validate_username(&username) {
        ctx.emit_info(
            "settings.users.add.invalid",
            &format!("Invalid username: {}", err),
        );
        return Ok(false);
    }

    if store.is_managed(&username) {
        ctx.emit_info(
            "settings.users.add.exists",
            &format!("{} already managed.", username),
        );
        return Ok(false);
    }

    let shell = select_shell(ctx, "Select shell")?.unwrap_or_else(super::models::default_shell);
    let selected_groups =
        select_groups("Use Tab to select multiple, Enter to confirm, Esc to skip")?;

    // Create the user on the system
    create_user(ctx, &username, &shell, &selected_groups)?;

    // Mark as managed
    store.add(&username);

    // Prompt for password
    if let Some(password) = prompt_password_with_confirmation(ctx, "Set password for user")? {
        set_user_password(ctx, &username, &password)?;
    }

    ctx.emit_success(
        "settings.users.added",
        &format!("{} registered for management.", username),
    );

    Ok(true)
}

/// Prompt for a username
fn prompt_username() -> Result<String> {
    let username = FzfWrapper::builder()
        .prompt("New username")
        .input()
        .input_dialog()?;
    Ok(username.trim().to_string())
}

/// Handle actions for a specific user
fn handle_user(ctx: &mut SettingsContext, store: &mut UserStore, username: &str) -> Result<bool> {
    let mut changed = false;

    loop {
        let user_info = match get_user_info(username)? {
            Some(info) => info,
            None => {
                ctx.emit_info("settings.users.user", "User not found on system.");
                return Ok(changed);
            }
        };

        let has_wheel = user_info.groups.iter().any(|group| group == WHEEL_GROUP);
        let wheel_warning = matches!(wheel_sudo_status(), WheelSudoStatus::Denied);
        let is_managed = store.is_managed(username);

        let actions = vec![
            UserActionItem::ChangeShell {
                current_shell: user_info.shell.clone(),
            },
            UserActionItem::ChangePassword,
            UserActionItem::ManageGroups {
                groups: user_info.groups.clone(),
                primary_group: user_info.primary_group.clone(),
            },
            UserActionItem::ToggleSudo {
                enabled: has_wheel,
                wheel_warning,
            },
            UserActionItem::Remove { is_managed },
            UserActionItem::Back,
        ];

        match select_one_with_style(actions)? {
            Some(UserActionItem::ChangeShell { .. }) => {
                if let Some(new_shell) = select_shell(ctx, "Select shell")? {
                    change_user_shell(ctx, username, &new_shell)?;
                    // Mark as managed if not already
                    if !store.is_managed(username) {
                        store.add(username);
                        changed = true;
                    }
                }
            }
            Some(UserActionItem::ChangePassword) => {
                if let Some(password) = prompt_password_with_confirmation(ctx, "New password")? {
                    set_user_password(ctx, username, &password)?;
                }
            }
            Some(UserActionItem::ManageGroups { .. }) => {
                if manage_user_groups(ctx, store, username)? {
                    changed = true;
                }
            }
            Some(UserActionItem::ToggleSudo { enabled, .. }) => {
                if toggle_user_sudo(ctx, store, username, enabled)? {
                    changed = true;
                }
            }
            Some(UserActionItem::Remove { .. }) => {
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

fn toggle_user_sudo(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
    currently_enabled: bool,
) -> Result<bool> {
    if !group_exists(WHEEL_GROUP)? {
        ctx.emit_info(
            "settings.users.sudo",
            "Wheel group not found on this system.",
        );
        return Ok(false);
    }

    if currently_enabled {
        if is_current_user(username) {
            let result = FzfWrapper::builder()
                .confirm(
                    "You are removing sudo access from the current user. This cannot be undone without another admin account.",
                )
                .yes_text("Remove sudo")
                .no_text("Cancel")
                .show_confirmation()?;

            if result != crate::menu_utils::ConfirmResult::Yes {
                return Ok(false);
            }
        }
        remove_user_from_group(ctx, username, WHEEL_GROUP)?;
        ctx.emit_success(
            "settings.users.sudo",
            &format!("Removed {} from wheel group.", username),
        );
    } else {
        add_user_to_group(ctx, username, WHEEL_GROUP)?;
        ctx.emit_success(
            "settings.users.sudo",
            &format!("Added {} to wheel group.", username),
        );
    }

    if !store.is_managed(username) {
        store.add(username);
    }

    if matches!(wheel_sudo_status(), WheelSudoStatus::Denied) {
        ctx.emit_info(
            "settings.users.sudo",
            "Warning: wheel group is not allowed to use sudo on this system.",
        );
    }

    Ok(true)
}

fn is_current_user(username: &str) -> bool {
    std::env::var("SUDO_USER")
        .ok()
        .or_else(|| std::env::var("USER").ok())
        .as_deref()
        == Some(username)
}

/// Manage groups for a user
fn manage_user_groups(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
) -> Result<bool> {
    let mut changed = false;

    loop {
        // Always read current groups from system
        let user_info = match get_user_info(username)? {
            Some(info) => info,
            None => {
                ctx.emit_info("settings.users.groups", "User not found on system.");
                return Ok(false);
            }
        };

        let items = build_group_menu_items(&user_info.groups, user_info.primary_group.as_deref());

        match select_one_with_style(items)? {
            Some(GroupMenuItem::ExistingGroup {
                name: group_name, ..
            }) => {
                if manage_single_group(ctx, store, username, &group_name, &user_info)? {
                    changed = true;
                }
            }
            Some(GroupMenuItem::AddGroup) => {
                if add_groups_to_user(ctx, store, username, &user_info.groups)? {
                    changed = true;
                }
            }
            Some(GroupMenuItem::CreateGroup) => {
                if create_group_for_user(ctx, store, username, &user_info.groups)? {
                    changed = true;
                }
            }
            _ => break,
        }
    }

    Ok(changed)
}

/// Build menu items for group management
fn build_group_menu_items(
    current_groups: &[String],
    primary_group: Option<&str>,
) -> Vec<GroupMenuItem> {
    let mut items: Vec<GroupMenuItem> = current_groups
        .iter()
        .map(|name| GroupMenuItem::ExistingGroup {
            name: name.clone(),
            is_primary: primary_group == Some(name.as_str()),
        })
        .collect();

    items.push(GroupMenuItem::AddGroup);
    items.push(GroupMenuItem::CreateGroup);
    items.push(GroupMenuItem::Back);
    items
}

fn create_group_for_user(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
    current_groups: &[String],
) -> Result<bool> {
    let group_name = prompt_group_name()?;
    if group_name.is_empty() {
        ctx.emit_info("settings.users.groups", "Group creation cancelled.");
        return Ok(false);
    }

    if let Err(err) = validate_group_name(&group_name) {
        ctx.emit_info(
            "settings.users.groups",
            &format!("Invalid group name: {}", err),
        );
        return Ok(false);
    }

    if group_exists(&group_name)? {
        if current_groups.iter().any(|group| group == &group_name) {
            ctx.emit_info(
                "settings.users.groups",
                &format!("{} is already in group {}.", username, group_name),
            );
            return Ok(false);
        }

        let message = format!(
            "Group '{}' already exists. Add '{}' to it?",
            group_name, username
        );
        let result = FzfWrapper::builder()
            .confirm(message)
            .yes_text("Add user")
            .no_text("Skip")
            .show_confirmation()?;

        if matches!(result, crate::menu_utils::ConfirmResult::Yes) {
            add_user_to_group(ctx, username, &group_name)?;
            if !store.is_managed(username) {
                store.add(username);
            }
            return Ok(true);
        }

        return Ok(false);
    }

    create_group(ctx, &group_name)?;

    let message = format!("Add '{}' to the new '{}' group?", username, group_name);
    let result = FzfWrapper::builder()
        .confirm(message)
        .yes_text("Add user")
        .no_text("Skip")
        .show_confirmation()?;

    if matches!(result, crate::menu_utils::ConfirmResult::Yes) {
        add_user_to_group(ctx, username, &group_name)?;
        if !store.is_managed(username) {
            store.add(username);
        }
        return Ok(true);
    }

    Ok(false)
}

/// Add groups to a user
fn add_groups_to_user(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
    current_groups: &[String],
) -> Result<bool> {
    let all_groups = get_all_system_groups()?;
    if all_groups.is_empty() {
        ctx.emit_info("settings.users.groups", "No system groups found.");
        return Ok(false);
    }

    // Filter out groups already assigned
    let current_set: BTreeSet<_> = current_groups.iter().cloned().collect();
    let available_groups: Vec<GroupItem> = all_groups
        .into_iter()
        .filter(|g| !current_set.contains(g))
        .map(|name| GroupItem { name })
        .collect();

    if available_groups.is_empty() {
        ctx.emit_info(
            "settings.users.groups",
            "User is already in all available groups.",
        );
        return Ok(false);
    }

    // Show selection menu with multi-select
    let selected = FzfWrapper::builder()
        .prompt("Select groups to add")
        .header("Use Tab to select multiple, Enter to confirm")
        .args(["--multi"])
        .select(available_groups)?;

    let mut groups_to_add = Vec::new();
    match selected {
        FzfResult::Selected(item) => {
            groups_to_add.push(item.name);
        }
        FzfResult::MultiSelected(group_items) => {
            groups_to_add.extend(group_items.into_iter().map(|item| item.name));
        }
        _ => {}
    }

    if !groups_to_add.is_empty() {
        for group in &groups_to_add {
            add_user_to_group(ctx, username, group)?;
        }
        // Mark as managed if not already
        if !store.is_managed(username) {
            store.add(username);
        }
        return Ok(true);
    }

    Ok(false)
}

/// Manage a single group for a user
fn manage_single_group(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
    group_name: &str,
    user_info: &super::models::UserInfo,
) -> Result<bool> {
    let is_primary = Some(group_name) == user_info.primary_group.as_deref();
    let actions = vec![
        GroupActionItem::RemoveGroup {
            name: group_name.to_string(),
            is_primary,
        },
        GroupActionItem::Back,
    ];

    match select_one_with_style(actions)? {
        Some(GroupActionItem::RemoveGroup { is_primary, .. }) => {
            // Don't remove primary group
            if is_primary {
                ctx.emit_info(
                    "settings.users.groups",
                    &format!("Cannot remove primary group: {}", group_name),
                );
                return Ok(false);
            }

            remove_user_from_group(ctx, username, group_name)?;

            // Mark as managed if not already
            if !store.is_managed(username) {
                store.add(username);
            }

            ctx.emit_success(
                "settings.users.groups",
                &format!("Removed {} from {}", group_name, username),
            );

            Ok(true)
        }
        _ => Ok(false),
    }
}
