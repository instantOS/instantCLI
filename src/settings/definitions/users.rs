//! User management settings

use anyhow::Result;

use crate::common::distro::OperatingSystem;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::users;
use crate::ui::prelude::*;

// ============================================================================
// Manage Users
// ============================================================================

pub struct ManageUsers;

impl Setting for ManageUsers {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("users.manage")
            .title("Manage Users")
            .icon(NerdFont::Users)
            .summary("Create, modify, and delete user accounts.\n\nManage user groups, shells, and permissions.")
            .unsupported_distros(&[OperatingSystem::Termux])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        users::manage_users(ctx)
    }
}

// ============================================================================
// SSH Keys
// ============================================================================

pub struct SshKeys;

impl Setting for SshKeys {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("users.ssh_keys")
            .title("SSH Keys")
            .icon(NerdFont::Key)
            .summary("Manage the SSH public keys authorized to access your account.\n\nAdd keys, edit their comments, or revoke access by removing them from ~/.ssh/authorized_keys.")
            .unsupported_distros(&[OperatingSystem::Termux])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        users::manage_ssh_keys(ctx)
    }
}
