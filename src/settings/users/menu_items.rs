use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::prelude::*;

use super::super::context::format_icon;

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

/// Actions available for a specific user
#[derive(Clone)]
pub(super) enum UserActionItem {
    ChangeShell,
    ChangePassword,
    ManageGroups,
    Remove,
    Back,
}

impl FzfSelectable for UserActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
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
            UserActionItem::ChangeShell => "Update the user's default shell on the system",
            UserActionItem::ChangePassword => "Change the user's password",
            UserActionItem::ManageGroups => "Add or remove supplementary groups",
            UserActionItem::Remove => "Stop managing this user (does not delete the account)",
            UserActionItem::Back => "Return to the previous menu",
        };
        FzfPreview::Text(text.to_string())
    }
}

/// Menu item for group management
#[derive(Clone)]
pub(super) enum GroupMenuItem {
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

/// Actions for a specific group
#[derive(Clone)]
pub(super) enum GroupActionItem {
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
        FzfPreview::Text(format!("Group: {}", self.name))
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
        FzfPreview::Text(format!("Shell: {}", self.path))
    }
}

