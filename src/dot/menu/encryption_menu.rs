use anyhow::Result;
use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone)]
pub enum EncryptionMenuAction {
    ShowIdentities,
    InitKeypair,
    Back,
}

#[derive(Clone)]
struct EncryptionMenuItem {
    action: EncryptionMenuAction,
    display: String,
    preview: String,
}

impl FzfSelectable for EncryptionMenuItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        match self.action {
            EncryptionMenuAction::ShowIdentities => "show_identities".to_string(),
            EncryptionMenuAction::InitKeypair => "init_keypair".to_string(),
            EncryptionMenuAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

fn build_identities_preview() -> String {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Key, "Local Encryption Identities")
        .blank();

    let pubkeys = crate::dot::operations::key::get_local_public_keys().unwrap_or_default();

    if pubkeys.is_empty() {
        builder = builder
            .line(
                colors::RED,
                Some(NerdFont::Warning),
                "No local age identity found!",
            )
            .blank()
            .text("Run 'Init Keypair' to generate a secure identity for this machine.");
    } else {
        builder = builder.line(
            colors::GREEN,
            Some(NerdFont::CheckCircle),
            "Primary Age Public Key:",
        );
        for pk in &pubkeys {
            builder = builder.text(&format!("  {}", pk));
        }
        builder = builder
            .blank()
            .subtext("Share this public key with others to allow them to authorize you.");
    }

    builder.build_string()
}

pub fn handle_encryption_keys_menu(
    _config: &mut DotfileConfig,
    _db: &Database,
    _debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let mut actions = Vec::new();

        actions.push(EncryptionMenuItem {
            action: EncryptionMenuAction::ShowIdentities,
            display: format!(
                "{} Show Identities",
                format_icon_colored(NerdFont::InfoCircle, colors::BLUE)
            ),
            preview: build_identities_preview(),
        });

        actions.push(EncryptionMenuItem {
            action: EncryptionMenuAction::InitKeypair,
            display: format!(
                "{} Init Keypair",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .line(colors::GREEN, Some(NerdFont::Plus), "Generate New Identity")
                .blank()
                .text("Creates a new secure age identity keypair for this machine.")
                .text("If an identity already exists, it will not be overwritten.")
                .build_string(),
        });

        actions.push(EncryptionMenuItem {
            action: EncryptionMenuAction::Back,
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .subtext("Return to main menu")
                .build_string(),
        });

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("Encryption Keys"))
            .prompt("Select action")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&actions) {
            builder = builder.initial_index(index);
        }

        let result = builder.select(actions.clone())?;

        match result {
            FzfResult::Selected(item) => {
                cursor.update(&item, &actions);
                match item.action {
                    EncryptionMenuAction::ShowIdentities => {
                        crate::dot::operations::key::handle_identity()?;
                        let _ = crate::menu_utils::prompt_text_edit(
                            crate::menu_utils::TextEditPrompt::new("Press Enter to continue", None),
                        )?;
                    }
                    EncryptionMenuAction::InitKeypair => {
                        crate::dot::operations::key::handle_init(false)?;
                        let _ = crate::menu_utils::prompt_text_edit(
                            crate::menu_utils::TextEditPrompt::new("Press Enter to continue", None),
                        )?;
                    }
                    EncryptionMenuAction::Back => return Ok(()),
                }
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        }
    }
}
