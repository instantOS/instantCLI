use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::prelude::*;

use crate::ui::catppuccin::{colors, format_icon, format_icon_colored};

/// Menu item for the main user management screen
#[derive(Clone)]
pub(super) enum ManageMenuItem {
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
        use crate::ui::catppuccin::{colors, format_icon_colored};

        match self {
            ManageMenuItem::User { username, .. } => {
                format!("{} {}", format_icon(NerdFont::User), username)
            }
            ManageMenuItem::Add => format!(
                "{} Add user",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            ManageMenuItem::Back => format!(
                "{} Back",
                format_icon_colored(NerdFont::ArrowLeft, colors::OVERLAY0)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        use crate::ui::catppuccin::colors;
        use crate::ui::preview::PreviewBuilder;

        match self {
            ManageMenuItem::User {
                username,
                shell,
                groups,
                in_toml,
            } => {
                let status = if *in_toml { "Managed" } else { "System" };

                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::User, username)
                    .line(
                        colors::TEAL,
                        Some(NerdFont::Tag),
                        &format!("Status: {}", status),
                    )
                    .line(
                        colors::TEAL,
                        Some(NerdFont::Terminal),
                        &format!("Shell: {}", shell),
                    )
                    .blank()
                    .separator()
                    .blank();

                if groups.is_empty() {
                    builder = builder.subtext("No groups assigned");
                } else {
                    builder = builder
                        .line(colors::TEAL, Some(NerdFont::List), "Groups:")
                        .blank()
                        .bullets(groups);
                }

                builder.build()
            }
            ManageMenuItem::Add => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add User")
                .text("Create a new managed user entry")
                .blank()
                .text("The user will be added to the")
                .text("TOML configuration for tracking")
                .text("and management.")
                .build(),
            ManageMenuItem::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to settings.")
                .build(),
        }
    }
}

/// Actions available for a specific user
#[derive(Clone)]
pub(super) enum UserActionItem {
    ChangeShell {
        current_shell: String,
    },
    ChangePassword,
    ManageGroups {
        groups: Vec<String>,
        primary_group: Option<String>,
    },
    ToggleSudo {
        enabled: bool,
        wheel_warning: bool,
    },
    Remove {
        is_managed: bool,
    },
    DeleteUser {
        username: String,
    },
    Back,
}

impl FzfSelectable for UserActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            UserActionItem::ChangeShell { .. } => {
                format!("{} Change shell", format_icon(NerdFont::Terminal))
            }
            UserActionItem::ChangePassword => {
                format!("{} Change password", format_icon(NerdFont::Key))
            }
            UserActionItem::ManageGroups { .. } => {
                format!("{} Manage groups", format_icon(NerdFont::List))
            }
            UserActionItem::ToggleSudo { enabled, .. } => {
                let (icon, color, status) = if *enabled {
                    (NerdFont::ToggleOn, colors::GREEN, "enabled")
                } else {
                    (NerdFont::ToggleOff, colors::RED, "disabled")
                };
                format!(
                    "{} Can use sudo ({})",
                    format_icon_colored(icon, color),
                    status
                )
            }
            UserActionItem::Remove { .. } => {
                format!("{} Stop managing", format_icon(NerdFont::Trash))
            }
            UserActionItem::DeleteUser { .. } => {
                format!(
                    "{} Delete user",
                    format_icon_colored(NerdFont::Trash, colors::RED)
                )
            }
            UserActionItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            UserActionItem::ChangeShell { current_shell } => PreviewBuilder::new()
                .header(NerdFont::Terminal, "Change Shell")
                .text("Update the user's default login shell.")
                .blank()
                .field("Current shell", current_shell)
                .build(),
            UserActionItem::ChangePassword => PreviewBuilder::new()
                .header(NerdFont::Key, "Change Password")
                .text("Set a new password for this user.")
                .blank()
                .subtext("You will be prompted to confirm the password.")
                .build(),
            UserActionItem::ManageGroups {
                groups,
                primary_group,
            } => {
                let primary = primary_group.as_deref();
                let group_labels: Vec<String> = if groups.is_empty() {
                    primary
                        .map(|group| vec![format!("{group} (primary)")])
                        .unwrap_or_default()
                } else {
                    groups
                        .iter()
                        .map(|group| {
                            if Some(group.as_str()) == primary {
                                format!("{group} (primary)")
                            } else {
                                group.clone()
                            }
                        })
                        .collect()
                };

                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::List, "Manage Groups")
                    .text("Add or remove supplementary groups for this user.")
                    .blank();

                if group_labels.is_empty() {
                    builder = builder.subtext("No groups assigned.");
                } else {
                    builder = builder
                        .line(colors::TEAL, Some(NerdFont::List), "Current groups:")
                        .blank()
                        .bullets(group_labels);
                }

                builder.build()
            }
            UserActionItem::ToggleSudo {
                enabled,
                wheel_warning,
            } => {
                let (status_color, status_icon, action_color, action_icon, action_label) =
                    if *enabled {
                        (
                            colors::GREEN,
                            NerdFont::ToggleOn,
                            colors::RED,
                            NerdFont::ToggleOff,
                            "Select to disable",
                        )
                    } else {
                        (
                            colors::RED,
                            NerdFont::ToggleOff,
                            colors::GREEN,
                            NerdFont::ToggleOn,
                            "Select to enable",
                        )
                    };

                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::Shield, "Sudo Access")
                    .line(
                        status_color,
                        Some(status_icon),
                        &format!("Status: {}", if *enabled { "Enabled" } else { "Disabled" }),
                    )
                    .blank()
                    .line(action_color, Some(action_icon), action_label)
                    .blank()
                    .subtext("Adds or removes the user from the wheel group.")
                    .field("Group", "wheel");

                if *wheel_warning {
                    builder = builder.blank().line(
                        colors::YELLOW,
                        Some(NerdFont::Warning),
                        "Wheel group is not allowed to use sudo on this system.",
                    );
                }

                builder.build()
            }
            UserActionItem::Remove { is_managed } => {
                let status = if *is_managed {
                    "Managed"
                } else {
                    "Not managed"
                };
                PreviewBuilder::new()
                    .header(NerdFont::Trash, "Stop Managing")
                    .text("Stop tracking this user in the configuration.")
                    .blank()
                    .line(
                        colors::TEAL,
                        Some(NerdFont::Tag),
                        &format!("Status: {}", status),
                    )
                    .blank()
                    .subtext("This does not delete the system account.")
                    .build()
            }
            UserActionItem::DeleteUser { username } => PreviewBuilder::new()
                .header(NerdFont::Trash, "Delete User")
                .line(colors::RED, Some(NerdFont::Warning), "DESTRUCTIVE ACTION")
                .blank()
                .text(&format!("Permanently delete user '{}'.", username))
                .blank()
                .line(colors::YELLOW, Some(NerdFont::Info), "This will:")
                .text("  - Delete the user account")
                .text("  - Remove the home directory")
                .text("  - Delete all user data")
                .blank()
                .subtext("This action cannot be undone.")
                .build(),
            UserActionItem::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to the previous menu.")
                .build(),
        }
    }
}

/// Menu item for group management
#[derive(Clone)]
pub(super) enum GroupMenuItem {
    ExistingGroup { name: String, is_primary: bool },
    AddGroup,
    CreateGroup,
    Back,
}

impl FzfSelectable for GroupMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            GroupMenuItem::ExistingGroup { name, .. } => {
                format!("{} {}", char::from(NerdFont::List), name)
            }
            GroupMenuItem::AddGroup => {
                format!("{} Add existing group", format_icon(NerdFont::Plus))
            }
            GroupMenuItem::CreateGroup => format!(
                "{} Create group",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            GroupMenuItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            GroupMenuItem::ExistingGroup { name, is_primary } => {
                let group_type = if *is_primary {
                    "Primary"
                } else {
                    "Supplementary"
                };
                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::List, "Group")
                    .field("Name", name)
                    .field("Type", group_type)
                    .blank()
                    .text("Select to manage this group membership.");

                if *is_primary {
                    builder = builder.blank().line(
                        colors::YELLOW,
                        Some(NerdFont::Warning),
                        "Primary groups cannot be removed.",
                    );
                }

                builder.build()
            }
            GroupMenuItem::AddGroup => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Group")
                .text("Add an existing supplementary group to this user.")
                .blank()
                .subtext("Use Tab to select multiple groups.")
                .build(),
            GroupMenuItem::CreateGroup => PreviewBuilder::new()
                .header(NerdFont::Plus, "Create Group")
                .text("Create a new system group.")
                .blank()
                .subtext("You can optionally add this user after creation.")
                .build(),
            GroupMenuItem::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to user management.")
                .build(),
        }
    }
}

/// Actions for a specific group
#[derive(Clone)]
pub(super) enum GroupActionItem {
    RemoveGroup { name: String, is_primary: bool },
    Back,
}

impl FzfSelectable for GroupActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            GroupActionItem::RemoveGroup { .. } => {
                format!("{} Remove group", format_icon(NerdFont::Minus))
            }
            GroupActionItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            GroupActionItem::RemoveGroup { name, is_primary } => {
                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::Minus, "Remove Group")
                    .text("Remove this group from the user.")
                    .blank()
                    .field("Group", name);

                if *is_primary {
                    builder = builder.blank().line(
                        colors::YELLOW,
                        Some(NerdFont::Warning),
                        "Primary groups cannot be removed.",
                    );
                }

                builder.build()
            }
            GroupActionItem::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to group list.")
                .build(),
        }
    }
}

/// Selectable group item
#[derive(Clone)]
pub(super) struct GroupItem {
    pub name: String,
}

impl FzfSelectable for GroupItem {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", char::from(NerdFont::List), self.name)
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::List, "Group")
            .field("Name", &self.name)
            .build()
    }
}

/// Selectable shell item
#[derive(Clone)]
pub(super) struct ShellItem {
    pub path: String,
}

impl FzfSelectable for ShellItem {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", char::from(NerdFont::Terminal), self.path)
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Terminal, "Shell")
            .field("Path", &self.path)
            .build()
    }
}
