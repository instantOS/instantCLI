//! User management settings

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Setting, SettingMetadata, SettingType};
use crate::settings::users;
use crate::ui::prelude::*;

// ============================================================================
// Manage Users
// ============================================================================

pub struct ManageUsers;

impl Setting for ManageUsers {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "users.manage",
            title: "Manage Users",
            category: Category::Users,
            icon: NerdFont::Users,
            breadcrumbs: &["Manage Users"],
            summary: "Create, modify, and delete user accounts.\n\nManage user groups, shells, and permissions.",
            requires_reapply: false,
            requirements: &[],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        users::manage_users(ctx)
    }
}

inventory::submit! { &ManageUsers as &'static dyn Setting }
