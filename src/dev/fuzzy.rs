use crate::dev::github::GitHubRepo;
use crate::dev::package::Package;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::Result;

#[derive(thiserror::Error, Debug)]
pub enum FzfError {
    #[error("FZF error: {0}")]
    FzfError(String),

    #[error("User cancelled selection")]
    UserCancelled,

    #[error("No repositories available")]
    NoRepositories,

    #[error("No packages available")]
    NoPackages,
}

#[derive(Debug, Clone)]
pub struct GitHubRepoSelectItem {
    pub repo: GitHubRepo,
}

impl FzfSelectable for GitHubRepoSelectItem {
    fn fzf_display_text(&self) -> String {
        let desc = self.repo.description.as_deref().unwrap_or("No description");
        format!(
            "{} {} - {}",
            format_icon_colored(NerdFont::GitBranch, colors::MAUVE),
            self.repo.name,
            desc
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::GitBranch, &self.repo.name)
            .text(
                self.repo
                    .description
                    .as_deref()
                    .unwrap_or("No description"),
            )
            .blank()
            .field(
                "Language",
                self.repo.language.as_deref().unwrap_or("Not specified"),
            )
            .field(
                "Stars",
                &self.repo.stargazers_count.unwrap_or(0).to_string(),
            )
            .field(
                "Forks",
                &self.repo.forks_count.unwrap_or(0).to_string(),
            )
            .build()
    }
}

#[derive(Debug, Clone)]
pub struct PackageSelectItem {
    pub package: Package,
}

impl FzfSelectable for PackageSelectItem {
    fn fzf_display_text(&self) -> String {
        if let Some(desc) = &self.package.description {
            format!(
                "{} {} - {}",
                format_icon_colored(NerdFont::Package, colors::SAPPHIRE),
                self.package.name,
                desc
            )
        } else {
            format!(
                "{} {}",
                format_icon_colored(NerdFont::Package, colors::SAPPHIRE),
                self.package.name
            )
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Package, &self.package.name)
            .text(
                self.package
                    .description
                    .as_deref()
                    .unwrap_or("No description"),
            )
            .blank()
            .field("Path", &self.package.path.display().to_string())
            .build()
    }
}

pub fn select_repository(repos: Vec<GitHubRepo>) -> Result<GitHubRepo, FzfError> {
    if repos.is_empty() {
        return Err(FzfError::NoRepositories);
    }

    let items: Vec<GitHubRepoSelectItem> = repos
        .into_iter()
        .map(|repo| GitHubRepoSelectItem { repo })
        .collect();

    match FzfWrapper::builder()
        .header(Header::fancy("Clone Repository"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(items)
        .map_err(|e| FzfError::FzfError(format!("Selection error: {e}")))?
    {
        crate::menu_utils::FzfResult::Selected(item) => Ok(item.repo),
        crate::menu_utils::FzfResult::Cancelled => Err(FzfError::UserCancelled),
        crate::menu_utils::FzfResult::Error(e) => Err(FzfError::FzfError(e)),
        _ => Err(FzfError::FzfError(
            "Unexpected selection result".to_string(),
        )),
    }
}

pub fn select_package(packages: Vec<Package>) -> Result<Package, FzfError> {
    if packages.is_empty() {
        return Err(FzfError::NoPackages);
    }

    let items: Vec<PackageSelectItem> = packages
        .into_iter()
        .map(|package| PackageSelectItem { package })
        .collect();

    match FzfWrapper::builder()
        .header(Header::fancy("Install Package"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(items)
        .map_err(|e| FzfError::FzfError(format!("Selection error: {e}")))?
    {
        crate::menu_utils::FzfResult::Selected(item) => Ok(item.package),
        crate::menu_utils::FzfResult::Cancelled => Err(FzfError::UserCancelled),
        crate::menu_utils::FzfResult::Error(e) => Err(FzfError::FzfError(e)),
        _ => Err(FzfError::FzfError(
            "Unexpected selection result".to_string(),
        )),
    }
}
