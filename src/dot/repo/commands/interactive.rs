use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::repo::DotfileRepositoryManager;
use crate::menu_utils::{FzfSelectable, FzfWrapper, Header};
use crate::preview::{DotRepositoryPreviewPayload, PreviewId, preview_command};
use anyhow::{Context, Result};

#[derive(Clone)]
struct RepoSelectionItem {
    name: String,
    preview_key: String,
}

impl FzfSelectable for RepoSelectionItem {
    fn fzf_display_text(&self) -> String {
        self.name.clone()
    }

    fn fzf_key(&self) -> String {
        self.preview_key.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Command(preview_command(PreviewId::DotRepository))
    }
}

fn select_repo_interactive(config: &DotfileConfig, prompt: &str) -> Result<Option<String>> {
    let items: Vec<RepoSelectionItem> = config
        .repos
        .iter()
        .map(|repo| {
            let preview_key = DotRepositoryPreviewPayload::new(
                &repo.name,
                config.repos_path().join(&repo.name),
                &repo.url,
                repo.branch.clone(),
                repo.enabled,
                repo.read_only,
            )
            .to_key()?;
            Ok(RepoSelectionItem {
                name: repo.name.clone(),
                preview_key,
            })
        })
        .collect::<Result<_>>()?;

    if items.is_empty() {
        println!("No repositories configured.");
        return Ok(None);
    }

    let result = FzfWrapper::builder()
        .header(Header::fancy("Select Repository"))
        .prompt(prompt)
        .responsive_layout()
        .select_one(items)?;

    match result {
        crate::menu_utils::DialogOutcome::Submitted(item) => Ok(Some(item.name)),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
    }
}

pub fn open_repo_lazygit(config: &DotfileConfig, db: &Database, name: Option<&str>) -> Result<()> {
    let repo_name = match name {
        Some(n) => n.to_string(),
        None => match select_repo_interactive(config, "Select repository to open in Lazygit")? {
            Some(n) => n,
            None => return Ok(()),
        },
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

pub(super) fn open_repo_shell(
    config: &DotfileConfig,
    db: &Database,
    name: Option<&str>,
) -> Result<()> {
    let repo_name = match name {
        Some(n) => n.to_string(),
        None => match select_repo_interactive(config, "Select repository to open shell in")? {
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
