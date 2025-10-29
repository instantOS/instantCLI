use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Default shell for new users
pub(super) fn default_shell() -> String {
    "/bin/bash".to_string()
}

/// Root structure for the users.toml file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct UsersFile {
    #[serde(default)]
    pub users: BTreeMap<String, UserSpec>,
}

/// Specification for a managed user
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct UserSpec {
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default)]
    pub groups: Vec<String>,
}

impl UserSpec {
    /// Returns a sanitized copy of this spec with normalized values
    pub fn sanitized(&self) -> UserSpec {
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

/// Information about a system user
#[derive(Debug)]
pub(super) struct UserInfo {
    pub shell: String,
    pub primary_group: Option<String>,
    pub groups: Vec<String>,
}

