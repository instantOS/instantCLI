use std::collections::BTreeSet;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfWrapper};

use super::super::context::{select_one_with_style, SettingsContext};
use super::menu_items::{
    GroupActionItem, GroupItem, GroupMenuItem, ManageMenuItem, UserActionItem,
};
use super::store::UserStore;
use super::system::{get_all_system_groups, get_system_users_with_home, get_user_info};
use super::utils::{
    add_user_to_group, change_user_shell, create_user, prompt_password_with_confirmation,
    remove_user_from_group, select_groups, select_shell, set_user_password,
};

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

    if store.is_managed(&username) {
        ctx.emit_info(
            "settings.users.add.exists",
            &format!("{} already managed.", username),
        );
        return Ok(false);
    }

    let shell = select_shell(ctx, "Select shell")?.unwrap_or_else(super::models::default_shell);
    let selected_groups = select_groups("Use Tab to select multiple, Enter to confirm, Esc to skip")?;

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
        let actions = vec![
            UserActionItem::ChangeShell,
            UserActionItem::ChangePassword,
            UserActionItem::ManageGroups,
            UserActionItem::Remove,
            UserActionItem::Back,
        ];

        match select_one_with_style(actions)? {
            Some(UserActionItem::ChangeShell) => {
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
            Some(UserActionItem::ManageGroups) => {
                if manage_user_groups(ctx, store, username)? {
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

        let items = build_group_menu_items(&user_info.groups);

        match select_one_with_style(items)? {
            Some(GroupMenuItem::ExistingGroup(group_name)) => {
                if manage_single_group(ctx, store, username, &group_name, &user_info)? {
                    changed = true;
                }
            }
            Some(GroupMenuItem::AddGroup) => {
                if add_groups_to_user(ctx, store, username, &user_info.groups)? {
                    changed = true;
                }
            }
            _ => break,
        }
    }

    Ok(changed)
}

/// Build menu items for group management
fn build_group_menu_items(current_groups: &[String]) -> Vec<GroupMenuItem> {
    let mut items: Vec<GroupMenuItem> = current_groups
        .iter()
        .map(|name| GroupMenuItem::ExistingGroup(name.clone()))
        .collect();

    items.push(GroupMenuItem::AddGroup);
    items.push(GroupMenuItem::Back);
    items
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
    let actions = vec![GroupActionItem::RemoveGroup, GroupActionItem::Back];

    match select_one_with_style(actions)? {
        Some(GroupActionItem::RemoveGroup) => {
            // Don't remove primary group
            if Some(group_name) == user_info.primary_group.as_deref() {
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

