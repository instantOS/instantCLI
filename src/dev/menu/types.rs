use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone)]
pub enum DevMenuEntry {
    Clone,
    Install,
    Setup,
    CloseMenu,
}

impl std::fmt::Display for DevMenuEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DevMenuEntry::Clone => write!(f, "!__clone__"),
            DevMenuEntry::Install => write!(f, "!__install__"),
            DevMenuEntry::Setup => write!(f, "!__setup__"),
            DevMenuEntry::CloseMenu => write!(f, "!__close_menu__"),
        }
    }
}

impl FzfSelectable for DevMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            DevMenuEntry::Clone => format!(
                "{} Clone Repository",
                format_icon_colored(NerdFont::GitBranch, colors::GREEN)
            ),
            DevMenuEntry::Install => format!(
                "{} Install Package",
                format_icon_colored(NerdFont::Package, colors::SAPPHIRE)
            ),
            DevMenuEntry::Setup => format!(
                "{} Dev Environment Setup",
                format_icon_colored(NerdFont::Wrench, colors::PEACH)
            ),
            DevMenuEntry::CloseMenu => format!("{} Close Menu", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        self.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            DevMenuEntry::Clone => PreviewBuilder::new()
                .header(NerdFont::GitBranch, "Clone Repository")
                .text("Clone an instantOS repository into ~/workspace.")
                .blank()
                .text("This will:")
                .bullet("Fetch available repos from GitHub")
                .bullet("Let you pick one with fuzzy search")
                .bullet("Clone it into ~/workspace/<name>")
                .build(),
            DevMenuEntry::Install => PreviewBuilder::new()
                .header(NerdFont::Package, "Install Package")
                .text("Build and install an instantOS package.")
                .blank()
                .text("This will:")
                .bullet("Update the packages repository")
                .bullet("Let you select a package")
                .bullet("Build with makepkg and install")
                .build(),
            DevMenuEntry::Setup => PreviewBuilder::new()
                .header(NerdFont::Wrench, "Dev Environment Setup")
                .text("Set up a development environment on a live ISO.")
                .blank()
                .text("Installs zsh, git, mise, neovim and")
                .text("applies instantOS dotfiles.")
                .blank()
                .subtext("Only works inside a live ISO session.")
                .build(),
            DevMenuEntry::CloseMenu => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close Menu")
                .text("Exit the dev menu.")
                .build(),
        }
    }
}
