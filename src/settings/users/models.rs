use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Default shell for new users
pub(super) fn default_shell() -> String {
    "/bin/bash".to_string()
}

/// Root structure for the users.toml file
/// This file only tracks which users are managed by ins.
/// The actual user state (shell, groups, etc.) is always read from the system.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct UsersFile {
    #[serde(default)]
    pub managed_users: BTreeSet<String>,
}

/// Information about a system user (read from the system)
#[derive(Debug, Clone)]
pub(super) struct UserInfo {
    pub shell: String,
    pub primary_group: Option<String>,
    pub groups: Vec<String>,
}
