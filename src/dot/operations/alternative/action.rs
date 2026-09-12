//! CLI action selection for alternatives.

/// CLI action for the alternative command.
///
/// Each variant maps 1:1 to an `AlternativeCommands` subcommand, so no
/// precedence resolution between flags is needed.
pub enum Action {
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
    /// Parse a REPO or REPO/SUBDIR specification into a `SetDirect` action.
    pub(crate) fn set_direct(spec: &str) -> Self {
        match spec.find('/') {
            Some(idx) => {
                let (repo, subdir) = spec.split_at(idx);
                Self::SetDirect {
                    repo: repo.to_string(),
                    subdir: Some(subdir[1..].to_string()),
                }
            }
            None => Self::SetDirect {
                repo: spec.to_string(),
                subdir: None,
            },
        }
    }
}
