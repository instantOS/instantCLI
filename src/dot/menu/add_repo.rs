//! Repository addition flow for the dot menu

use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::cli::RepoCommands;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;

/// Classify user input for repository addition
#[derive(Debug, Clone, PartialEq)]
enum InputType {
    /// Valid URL (contains :// or ends with .git)
    Url(String),
    /// Shorthand like user/repo
    Shorthand(String),
    /// Plain name without slashes
    PlainName(String),
}

fn classify_repo_input(input: &str) -> InputType {
    let input = input.trim();

    // Check for URL patterns
    if input.contains("://") || input.ends_with(".git") || input.starts_with("git@") {
        return InputType::Url(input.to_string());
    }

    // Check for shorthand (contains / but not a URL)
    if input.contains('/') {
        return InputType::Shorthand(input.to_string());
    }

    // Plain name
    InputType::PlainName(input.to_string())
}

// --- Menu choice enums ---

/// Choice when user enters a shorthand like "user/repo"
#[derive(Clone)]
enum ShorthandChoice {
    GitHub,
    GitLab,
    Codeberg,
    EnterAnother,
    Cancel,
}

impl FzfSelectable for ShorthandChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            ShorthandChoice::GitHub => {
                format!(
                    "{} GitHub",
                    format_icon_colored(NerdFont::Git, colors::TEXT)
                )
            }
            ShorthandChoice::GitLab => format!(
                "{} GitLab",
                format_icon_colored(NerdFont::Git, colors::PEACH)
            ),
            ShorthandChoice::Codeberg => format!(
                "{} Codeberg",
                format_icon_colored(NerdFont::Git, colors::BLUE)
            ),
            ShorthandChoice::EnterAnother => format!(
                "{} Enter another URL",
                format_icon_colored(NerdFont::Edit, colors::LAVENDER)
            ),
            ShorthandChoice::Cancel => format!("{} Cancel", format_back_icon()),
        }
    }
    fn fzf_key(&self) -> String {
        match self {
            ShorthandChoice::GitHub => "github".to_string(),
            ShorthandChoice::GitLab => "gitlab".to_string(),
            ShorthandChoice::Codeberg => "codeberg".to_string(),
            ShorthandChoice::EnterAnother => "another".to_string(),
            ShorthandChoice::Cancel => "cancel".to_string(),
        }
    }
    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::None
    }
}

/// Choice when user enters a plain name (not a URL)
#[derive(Clone)]
enum PlainNameChoice {
    CreateLocal,
    EnterAnother,
    Cancel,
}

impl FzfSelectable for PlainNameChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            PlainNameChoice::CreateLocal => format!(
                "{} Create local repository",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            PlainNameChoice::EnterAnother => format!(
                "{} Enter a URL instead",
                format_icon_colored(NerdFont::Edit, colors::LAVENDER)
            ),
            PlainNameChoice::Cancel => format!("{} Cancel", format_back_icon()),
        }
    }
    fn fzf_key(&self) -> String {
        match self {
            PlainNameChoice::CreateLocal => "create".to_string(),
            PlainNameChoice::EnterAnother => "another".to_string(),
            PlainNameChoice::Cancel => "cancel".to_string(),
        }
    }
    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::None
    }
}

/// Choice when user enters empty input
#[derive(Clone)]
enum EmptyInputChoice {
    UseDefault,
    GoBack,
    EnterAnother,
}

impl FzfSelectable for EmptyInputChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            EmptyInputChoice::UseDefault => format!(
                "{} Use default (instantOS/dotfiles)",
                format_icon_colored(NerdFont::Check, colors::GREEN)
            ),
            EmptyInputChoice::GoBack => format!("{} Go back", format_back_icon()),
            EmptyInputChoice::EnterAnother => format!(
                "{} Enter another URL",
                format_icon_colored(NerdFont::Edit, colors::BLUE)
            ),
        }
    }
    fn fzf_key(&self) -> String {
        match self {
            EmptyInputChoice::UseDefault => "default".to_string(),
            EmptyInputChoice::GoBack => "back".to_string(),
            EmptyInputChoice::EnterAnother => "another".to_string(),
        }
    }
    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::None
    }
}

// --- Helper types and functions ---

/// Result of handling user input for repo addition
enum AddRepoInputResult {
    /// User provided a URL to clone
    Url(String),
    /// User wants to create a local repo (already handled)
    LocalCreated,
    /// User wants to enter different input
    TryAgain,
    /// User cancelled
    Cancelled,
}

/// Handle shorthand input like "user/repo" - show host selection menu
fn handle_shorthand_input(shorthand: &str) -> Result<AddRepoInputResult> {
    let choices = vec![
        ShorthandChoice::GitHub,
        ShorthandChoice::GitLab,
        ShorthandChoice::Codeberg,
        ShorthandChoice::EnterAnother,
        ShorthandChoice::Cancel,
    ];

    match FzfWrapper::builder()
        .header(Header::fancy(&format!("Clone '{}' from:", shorthand)))
        .prompt("Select host")
        .args(fzf_mocha_args())
        .select(choices)?
    {
        FzfResult::Selected(ShorthandChoice::GitHub) => Ok(AddRepoInputResult::Url(format!(
            "https://github.com/{}.git",
            shorthand
        ))),
        FzfResult::Selected(ShorthandChoice::GitLab) => Ok(AddRepoInputResult::Url(format!(
            "https://gitlab.com/{}.git",
            shorthand
        ))),
        FzfResult::Selected(ShorthandChoice::Codeberg) => Ok(AddRepoInputResult::Url(format!(
            "https://codeberg.org/{}.git",
            shorthand
        ))),
        FzfResult::Selected(ShorthandChoice::EnterAnother) => Ok(AddRepoInputResult::TryAgain),
        FzfResult::Selected(ShorthandChoice::Cancel) | FzfResult::Cancelled => {
            Ok(AddRepoInputResult::Cancelled)
        }
        _ => Ok(AddRepoInputResult::Cancelled),
    }
}

/// Handle plain name input - offer to create local repo
fn handle_plain_name_input(name: &str) -> Result<AddRepoInputResult> {
    let choices = vec![
        PlainNameChoice::CreateLocal,
        PlainNameChoice::EnterAnother,
        PlainNameChoice::Cancel,
    ];

    match FzfWrapper::builder()
        .header(Header::fancy(&format!("'{}' is not a URL", name)))
        .prompt("Select action")
        .args(fzf_mocha_args())
        .select(choices)?
    {
        FzfResult::Selected(PlainNameChoice::CreateLocal) => {
            let mut config = Config::load(None)?;
            match crate::dot::meta::create_local_repo(&mut config, Some(name), false, true, true) {
                Ok(outcome) => {
                    if let crate::dot::meta::InitOutcome::CreatedDefault { info } = outcome {
                        FzfWrapper::message(&format!(
                            "Created local repository '{}'\n\nLocation: {}",
                            info.name,
                            info.path.display()
                        ))?;
                    }
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error creating repository: {}", e))?;
                }
            }
            Ok(AddRepoInputResult::LocalCreated)
        }
        FzfResult::Selected(PlainNameChoice::EnterAnother) => Ok(AddRepoInputResult::TryAgain),
        FzfResult::Selected(PlainNameChoice::Cancel) | FzfResult::Cancelled => {
            Ok(AddRepoInputResult::Cancelled)
        }
        _ => Ok(AddRepoInputResult::Cancelled),
    }
}

/// Handle empty input - offer default repo or retry
fn handle_empty_input(default_repo: &str) -> Result<AddRepoInputResult> {
    let choices = vec![
        EmptyInputChoice::UseDefault,
        EmptyInputChoice::GoBack,
        EmptyInputChoice::EnterAnother,
    ];

    match FzfWrapper::builder()
        .header(Header::fancy("No URL entered"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .select(choices)?
    {
        FzfResult::Selected(EmptyInputChoice::UseDefault) => {
            Ok(AddRepoInputResult::Url(default_repo.to_string()))
        }
        FzfResult::Selected(EmptyInputChoice::GoBack) | FzfResult::Cancelled => {
            Ok(AddRepoInputResult::Cancelled)
        }
        FzfResult::Selected(EmptyInputChoice::EnterAnother) => Ok(AddRepoInputResult::TryAgain),
        _ => Ok(AddRepoInputResult::Cancelled),
    }
}

/// Prompt for optional repository name
fn prompt_optional_name() -> Result<Option<String>> {
    match FzfWrapper::builder()
        .input()
        .prompt("Repository name (optional)")
        .input_result()?
    {
        FzfResult::Selected(s) if !s.is_empty() => Ok(Some(s)),
        FzfResult::Selected(_) => Ok(None),
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

/// Prompt for optional branch name
fn prompt_optional_branch() -> Result<Option<String>> {
    match FzfWrapper::builder()
        .input()
        .prompt("Branch (optional)")
        .input_result()?
    {
        FzfResult::Selected(s) if !s.is_empty() => Ok(Some(s)),
        FzfResult::Selected(_) => Ok(None),
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

// --- Main function ---

/// Handle adding a new repository
pub fn handle_add_repo(_config: &Config, _db: &Database, debug: bool) -> Result<()> {
    const DEFAULT_REPO: &str = "https://github.com/instantOS/dotfiles";

    // Loop for URL input (allows retrying)
    let url = loop {
        match FzfWrapper::builder()
            .input()
            .prompt("Repository URL or name")
            .ghost(DEFAULT_REPO)
            .input_result()?
        {
            FzfResult::Selected(s) if !s.is_empty() => {
                let result = match classify_repo_input(&s) {
                    InputType::Url(url) => AddRepoInputResult::Url(url),
                    InputType::Shorthand(shorthand) => handle_shorthand_input(&shorthand)?,
                    InputType::PlainName(name) => handle_plain_name_input(&name)?,
                };

                match result {
                    AddRepoInputResult::Url(url) => break url,
                    AddRepoInputResult::LocalCreated => return Ok(()),
                    AddRepoInputResult::TryAgain => continue,
                    AddRepoInputResult::Cancelled => return Ok(()),
                }
            }
            FzfResult::Cancelled => return Ok(()),
            FzfResult::Selected(_) => {
                // Empty input
                match handle_empty_input(DEFAULT_REPO)? {
                    AddRepoInputResult::Url(url) => break url,
                    AddRepoInputResult::TryAgain => continue,
                    _ => return Ok(()),
                }
            }
            _ => return Ok(()),
        }
    };

    let name = prompt_optional_name()?;
    let branch = prompt_optional_branch()?;

    // Clone the repository
    let clone_args = RepoCommands::Clone(crate::dot::repo::cli::CloneArgs {
        url,
        name,
        branch,
        read_only: false,
        force_write: false,
    });

    let mut config = Config::load(None)?;
    let db = Database::new(config.database_path().to_path_buf())?;

    crate::dot::repo::commands::handle_repo_command(&mut config, &db, &clone_args, debug)?;

    Ok(())
}
