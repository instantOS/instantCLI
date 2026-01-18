//! CLI action selection for alternatives.

/// CLI action for the alternative command.
pub(crate) enum Action {
    /// Interactive source selection menu
    Select,
    /// Interactive destination picker for creating alternatives
    Create,
    /// Non-interactive: list alternatives
    List,
    /// Non-interactive: reset/remove override
    Reset,
    /// Non-interactive: set source to specific repo[/subdir]
    SetDirect {
        repo: String,
        subdir: Option<String>,
    },
    /// Non-interactive: create at specific repo/subdir
    CreateDirect { repo: String, subdir: String },
}

impl Action {
    pub(crate) fn from_flags(
        reset: bool,
        create: bool,
        list: bool,
        set: Option<&str>,
        repo: Option<&str>,
        subdir: Option<&str>,
    ) -> Self {
        if reset {
            Self::Reset
        } else if let Some(set_value) = set {
            // Parse "repo" or "repo/subdir" format
            let (repo, subdir) = if let Some(idx) = set_value.find('/') {
                let (r, s) = set_value.split_at(idx);
                (r.to_string(), Some(s[1..].to_string()))
            } else {
                (set_value.to_string(), None)
            };
            Self::SetDirect { repo, subdir }
        } else if create {
            if let Some(repo_name) = repo {
                // Non-interactive create with explicit destination
                let subdir_name = subdir.unwrap_or("dots").to_string();
                Self::CreateDirect {
                    repo: repo_name.to_string(),
                    subdir: subdir_name,
                }
            } else {
                // Interactive create
                Self::Create
            }
        } else if list {
            Self::List
        } else {
            Self::Select
        }
    }
}
