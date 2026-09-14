use std::path::PathBuf;

pub(super) fn default_shell() -> String {
    "/bin/bash".to_string()
}

#[derive(Debug, Clone)]
pub(in crate::settings) struct UserInfo {
    /// Account name from the passwd database, used to run commands as this
    /// user via `sudo -u`.
    pub username: String,
    pub shell: String,
    pub primary_group: Option<String>,
    pub groups: Vec<String>,
    /// Home directory from the passwd database.
    pub home: PathBuf,
    /// Numeric UID, used to tell the process's own file from a foreign one.
    pub uid: u32,
}
