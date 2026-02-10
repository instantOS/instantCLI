use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::menu::repo_actions::build_repo_preview;
use crate::dot::repo::DotfileRepositoryManager;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::fzf_mocha_args;
use anyhow::{Context, Result};

#[derive(Clone)]
struct RepoSelectionItem {
    name: String,
    preview: String,
}

impl FzfSelectable for RepoSelectionItem {
    fn fzf_display_text(&self) -> String {
        self.name.clone()
    }

    fn fzf_key(&self) -> String {
        self.name.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

fn select_repo_interactive(config: &DotfileConfig, db: &Database, prompt: &str) -> Result<Option<String>> {
    let items: Vec<RepoSelectionItem> = config
        .repos
        .iter()
        .map(|r| {
            let preview = build_repo_preview(&r.name, config, db);
            RepoSelectionItem {
                name: r.name.clone(),
                preview,
            }
        })
        .collect();

    if items.is_empty() {
        println!("No repositories configured.");
        return Ok(None);
    }

    let result = FzfWrapper::builder()
        .header(Header::fancy("Select Repository"))
        .prompt(prompt)
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(items)?;

    match result {
        FzfResult::Selected(item) => Ok(Some(item.name)),
        _ => Ok(None),
    }
}

pub(super) fn open_repo_lazygit(config: &DotfileConfig, db: &Database, name: Option<&str>) -> Result<()> {
    let repo_name = match name {
        Some(n) => n.to_string(),
        None => {
            match select_repo_interactive(config, db, "Select repository to open in Lazygit")? {
                Some(n) => n,
                None => return Ok(()),
            }
        }
    };

    let repo_manager = DotfileRepositoryManager::new(config, db);
    let local_repo = repo_manager.get_repository_info(&repo_name)?;
    let repo_path = local_repo.local_path(config)?;

    std::process::Command::new("lazygit")
        .current_dir(&repo_path)
        .status()
        .context("Failed to launch lazygit")?;

    Ok(())
}

pub(super) fn open_repo_shell(config: &DotfileConfig, db: &Database, name: Option<&str>) -> Result<()> {
    let repo_name = match name {
        Some(n) => n.to_string(),
        None => match select_repo_interactive(config, db, "Select repository to open shell in")? {
            Some(n) => n,
            None => return Ok(()),
        },
    };

    let repo_manager = DotfileRepositoryManager::new(config, db);
    let local_repo = repo_manager.get_repository_info(&repo_name)?;
    let repo_path = local_repo.local_path(config)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());

    println!("Opening shell in {}...", repo_path.display());
    std::process::Command::new(shell)
        .current_dir(&repo_path)
        .status()
        .context("Failed to launch shell")?;

    Ok(())
}
