use std::collections::BTreeSet;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfWrapper};

use super::super::context::{select_one_with_style, SettingsContext};
use super::menu_items::{
    GroupActionItem, GroupItem, GroupMenuItem, ManageMenuItem, UserActionItem,
};
use super::models::UserSpec;
use super::store::UserStore;
use super::system::{get_all_system_groups, get_system_users_with_home, get_user_info};
use super::utils::{
    apply_user_spec, get_or_init_user_spec, prompt_password_with_confirmation, select_groups,
    select_shell, set_user_password,
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

    Ok(items)
}

/// Add a new user
fn add_user(ctx: &mut SettingsContext, store: &mut UserStore) -> Result<bool> {
    let username = prompt_username()?;
    if username.is_empty() {
        ctx.emit_info("settings.users.add.cancelled", "Creation cancelled.");
        return Ok(false);
    }

    if store.get(&username).is_some() {
        ctx.emit_info(
            "settings.users.add.exists",
            &format!("{} already managed.", username),
        );
        return Ok(false);
    }

    let shell = select_shell(ctx, "Select shell")?.unwrap_or_else(super::models::default_shell);
    let selected_groups = select_groups("Use Tab to select multiple, Enter to confirm, Esc to skip")?;

    let spec = UserSpec {
        shell,
        groups: selected_groups,
    }
    .sanitized();

    store.insert(&username, spec.clone());
    apply_user_spec(ctx, &username, &spec)?;

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
    let mut current_spec = get_or_init_user_spec(store, username);

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
                store.insert(username, current_spec.clone());
                changed = true;
            }
            Some(UserActionItem::ChangeShell) => {
                if let Some(new_shell) = select_shell(ctx, "Select shell")? {
                    current_spec.shell = new_shell;
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

/// Manage groups for a user
fn manage_user_groups(
    ctx: &mut SettingsContext,
    store: &mut UserStore,
    username: &str,
    spec: &mut UserSpec,
) -> Result<bool> {
    let mut changed = false;

    loop {
        let items = build_group_menu_items(spec);

        match select_one_with_style(items)? {
            Some(GroupMenuItem::ExistingGroup(group_name)) => {
                if manage_single_group(ctx, store, username, spec, &group_name)? {
                    changed = true;
                }
            }
            Some(GroupMenuItem::AddGroup) => {
                if add_groups_to_user(ctx, store, username, spec)? {
                    changed = true;
                }
            }
            _ => break,
        }
    }

    Ok(changed)
}

/// Build menu items for group management
fn build_group_menu_items(spec: &UserSpec) -> Vec<GroupMenuItem> {
    let mut items: Vec<GroupMenuItem> = spec
        .groups
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
    spec: &mut UserSpec,
) -> Result<bool> {
    let all_groups = get_all_system_groups()?;
    if all_groups.is_empty() {
        ctx.emit_info("settings.users.groups", "No system groups found.");
        return Ok(false);
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
        return Ok(false);
    }

    // Show selection menu with multi-select
    let selected = FzfWrapper::builder()
        .prompt("Select groups to add")
        .header("Use Tab to select multiple, Enter to confirm")
        .args(["--multi"])
        .select(available_groups)?;

    let mut added = false;
    match selected {
        FzfResult::Selected(item) => {
            if !spec.groups.contains(&item.name) {
                spec.groups.push(item.name);
                added = true;
            }
        }
        FzfResult::MultiSelected(group_items) => {
            for item in group_items {
                if !spec.groups.contains(&item.name) {
                    spec.groups.push(item.name);
                    added = true;
                }
            }
        }
        _ => {}
    }

    if added {
        *spec = spec.sanitized();
        store.insert(username, spec.clone());
        apply_user_spec(ctx, username, spec)?;
    }

    Ok(added)
}

/// Manage a single group for a user
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

