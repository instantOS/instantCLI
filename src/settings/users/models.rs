pub(super) fn default_shell() -> String {
    "/bin/bash".to_string()
}

#[derive(Debug, Clone)]
pub(super) struct UserInfo {
    pub shell: String,
    pub primary_group: Option<String>,
    pub groups: Vec<String>,
}
