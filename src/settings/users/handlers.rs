use std::collections::BTreeSet;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfWrapper};

use super::super::context::SettingsContext;
use super::menu_items::{
    GroupActionItem, GroupItem, GroupMenuItem, ManageMenuItem, UserActionItem,
};
use super::system::{
    WheelSudoStatus, get_all_system_groups, get_system_users_with_home, get_user_info,
    group_exists, wheel_sudo_status,
};
use super::utils::{
    add_user_to_group, change_user_shell, create_group, create_user, delete_user,
    prompt_group_name, prompt_password_with_confirmation, remove_user_from_group, select_groups,
    select_shell, set_user_password, validate_group_name, validate_username,
};
use crate::menu_utils::select_one_with_style;

const WHEEL_GROUP: &str = "wheel";

pub fn manage_users(ctx: &mut SettingsContext) -> Result<()> {
    loop {
        let items = build_user_menu_items()?;

        match select_one_with_style(items)? {
            Some(ManageMenuItem::Add) => {
                add_user(ctx)?;
            }
            Some(ManageMenuItem::User { username, .. }) => {
                handle_user(ctx, &username)?;
            }
            _ => break,
        }
    }

    ctx.emit_info("settings.users.noop", "No changes made to users.");

    Ok(())
}

fn build_user_menu_items() -> Result<Vec<ManageMenuItem>> {
    let system_users = get_system_users_with_home()?;
    let mut all_usernames = BTreeSet::new();

    for username in system_users {
        all_usernames.insert(username);
    }

    let mut items: Vec<ManageMenuItem> = all_usernames
        .into_iter()
        .filter_map(|username| {
            let info = get_user_info(&username).ok()??;

            Some(ManageMenuItem::User {
                username,
                shell: info.shell,
                groups: info.groups,
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

fn add_user(ctx: &mut SettingsContext) -> Result<()> {
    let username = prompt_username()?;
    if username.is_empty() {
        ctx.emit_info("settings.users.add.cancelled", "Creation cancelled.");
        return Ok(());
    }

    if let Err(err) = validate_username(&username) {
        ctx.emit_info(
            "settings.users.add.invalid",
            &format!("Invalid username: {}", err),
        );
        return Ok(());
    }

    let shell = select_shell(ctx, "Select shell")?.unwrap_or_else(super::models::default_shell);
    let selected_groups =
        select_groups("Use Tab to select multiple, Enter to confirm, Esc to skip")?;

    create_user(ctx, &username, &shell, &selected_groups)?;

    if let Some(password) = prompt_password_with_confirmation(ctx, "Set password for user")? {
        set_user_password(ctx, &username, &password)?;
    }

    ctx.emit_success(
        "settings.users.added",
        &format!("User {} created.", username),
    );

    Ok(())
}

fn prompt_username() -> Result<String> {
    let username = FzfWrapper::builder()
        .prompt("New username")
        .input()
        .input_dialog()?;
    Ok(username.trim().to_string())
}

fn handle_user(ctx: &mut SettingsContext, username: &str) -> Result<()> {
    loop {
        let user_info = match get_user_info(username)? {
            Some(info) => info,
            None => {
                ctx.emit_info("settings.users.user", "User not found on system.");
                return Ok(());
            }
        };

        let has_wheel = user_info.groups.iter().any(|group| group == WHEEL_GROUP);
        let wheel_warning = matches!(wheel_sudo_status(), WheelSudoStatus::Denied);

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
            UserActionItem::DeleteUser {
                username: username.to_string(),
            },
            UserActionItem::Back,
        ];

        match select_one_with_style(actions)? {
            Some(UserActionItem::ChangeShell { .. }) => {
                if let Some(new_shell) = select_shell(ctx, "Select shell")? {
                    change_user_shell(ctx, username, &new_shell)?;
                }
            }
            Some(UserActionItem::ChangePassword) => {
                if let Some(password) = prompt_password_with_confirmation(ctx, "New password")? {
                    set_user_password(ctx, username, &password)?;
                }
            }
            Some(UserActionItem::ManageGroups { .. }) => {
                manage_user_groups(ctx, username)?;
            }
            Some(UserActionItem::ToggleSudo { enabled, .. }) => {
                toggle_user_sudo(ctx, username, enabled)?;
            }
            Some(UserActionItem::DeleteUser { .. }) => {
                if confirm_delete_user(ctx, username)? {
                    delete_user(ctx, username)?;
                    ctx.emit_success(
                        "settings.users.deleted",
                        &format!("User {} has been deleted.", username),
                    );
                    break;
                }
            }
            _ => break,
        }
    }

    Ok(())
}

fn toggle_user_sudo(
    ctx: &mut SettingsContext,
    username: &str,
    currently_enabled: bool,
) -> Result<()> {
    if !group_exists(WHEEL_GROUP)? {
        ctx.emit_info(
            "settings.users.sudo",
            "Wheel group not found on this system.",
        );
        return Ok(());
    }

    if currently_enabled {
        if is_current_user(username) {
            let result = FzfWrapper::builder()
                .confirm(
                    "You are removing sudo access from the current user. This cannot be undone without another admin account.",
                )
                .yes_text("Remove sudo")
                .no_text("Cancel")
                .confirm_dialog()?;

            if result != crate::menu_utils::ConfirmResult::Yes {
                return Ok(());
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

    if matches!(wheel_sudo_status(), WheelSudoStatus::Denied) {
        ctx.emit_info(
            "settings.users.sudo",
            "Warning: wheel group is not allowed to use sudo on this system.",
        );
    }

    Ok(())
}

fn is_current_user(username: &str) -> bool {
    std::env::var("SUDO_USER")
        .ok()
        .or_else(|| std::env::var("USER").ok())
        .as_deref()
        == Some(username)
}

fn confirm_delete_user(ctx: &mut SettingsContext, username: &str) -> Result<bool> {
    if is_current_user(username) {
        ctx.emit_failure(
            "settings.users.delete",
            "Cannot delete the current user account.",
        );
        return Ok(false);
    }

    let expected = username.to_uppercase();
    let prompt = format!("Type '{}' to confirm deletion", expected);

    let confirmation = FzfWrapper::builder()
        .prompt(&prompt)
        .input()
        .input_dialog()?;

    if confirmation.trim() == expected {
        Ok(true)
    } else {
        ctx.emit_info("settings.users.delete", "Deletion cancelled.");
        Ok(false)
    }
}

fn manage_user_groups(ctx: &mut SettingsContext, username: &str) -> Result<()> {
    loop {
        let user_info = match get_user_info(username)? {
            Some(info) => info,
            None => {
                ctx.emit_info("settings.users.groups", "User not found on system.");
                return Ok(());
            }
        };

        let items = build_group_menu_items(&user_info.groups, user_info.primary_group.as_deref());

        match select_one_with_style(items)? {
            Some(GroupMenuItem::ExistingGroup {
                name: group_name, ..
            }) => {
                manage_single_group(ctx, username, &group_name, &user_info)?;
            }
            Some(GroupMenuItem::AddGroup) => {
                add_groups_to_user(ctx, username, &user_info.groups)?;
            }
            Some(GroupMenuItem::CreateGroup) => {
                create_group_for_user(ctx, username, &user_info.groups)?;
            }
            _ => break,
        }
    }

    Ok(())
}

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
    username: &str,
    current_groups: &[String],
) -> Result<()> {
    let group_name = prompt_group_name()?;
    if group_name.is_empty() {
        ctx.emit_info("settings.users.groups", "Group creation cancelled.");
        return Ok(());
    }

    if let Err(err) = validate_group_name(&group_name) {
        ctx.emit_info(
            "settings.users.groups",
            &format!("Invalid group name: {}", err),
        );
        return Ok(());
    }

    if group_exists(&group_name)? {
        if current_groups.iter().any(|group| group == &group_name) {
            ctx.emit_info(
                "settings.users.groups",
                &format!("{} is already in group {}.", username, group_name),
            );
            return Ok(());
        }

        let message = format!(
            "Group '{}' already exists. Add '{}' to it?",
            group_name, username
        );
        let result = FzfWrapper::builder()
            .confirm(message)
            .yes_text("Add user")
            .no_text("Skip")
            .confirm_dialog()?;

        if matches!(result, crate::menu_utils::ConfirmResult::Yes) {
            add_user_to_group(ctx, username, &group_name)?;
        }

        return Ok(());
    }

    create_group(ctx, &group_name)?;

    let message = format!("Add '{}' to the new '{}' group?", username, group_name);
    let result = FzfWrapper::builder()
        .confirm(message)
        .yes_text("Add user")
        .no_text("Skip")
        .confirm_dialog()?;

    if matches!(result, crate::menu_utils::ConfirmResult::Yes) {
        add_user_to_group(ctx, username, &group_name)?;
    }

    Ok(())
}

fn add_groups_to_user(
    ctx: &mut SettingsContext,
    username: &str,
    current_groups: &[String],
) -> Result<()> {
    let all_groups = get_all_system_groups()?;
    if all_groups.is_empty() {
        ctx.emit_info("settings.users.groups", "No system groups found.");
        return Ok(());
    }

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
        return Ok(());
    }

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
    }

    Ok(())
}

fn manage_single_group(
    ctx: &mut SettingsContext,
    username: &str,
    group_name: &str,
    user_info: &super::models::UserInfo,
) -> Result<()> {
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
            if is_primary {
                ctx.emit_info(
                    "settings.users.groups",
                    &format!("Cannot remove primary group: {}", group_name),
                );
                return Ok(());
            }

            remove_user_from_group(ctx, username, group_name)?;

            ctx.emit_success(
                "settings.users.groups",
                &format!("Removed {} from {}", group_name, username),
            );

            Ok(())
        }
        _ => Ok(()),
    }
}
