use anyhow::Result;
use std::path::PathBuf;

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

fn discover_ssh_public_keys() -> Vec<(String, String)> {
    let home = std::env::var("HOME").map(PathBuf::from).ok();
    let Some(home_path) = home else {
        return Vec::new();
    };
    let ssh_dir = home_path.join(".ssh");
    if !ssh_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(ssh_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || !path.extension().is_some_and(|ext| ext == "pub") {
                return None;
            }
            let content = std::fs::read_to_string(&path).ok()?;
            let trimmed = content.trim();
            if trimmed.starts_with("ssh-") || trimmed.starts_with("ecdsa-") {
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some((name, trimmed.to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn build_identities_preview() -> String {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Key, "Local Encryption Identities")
        .blank();

    let pubkeys = crate::dot::operations::key::get_local_public_keys().unwrap_or_default();
    let ssh_keys = discover_ssh_public_keys();
    let has_any = !pubkeys.is_empty() || !ssh_keys.is_empty();

    if !pubkeys.is_empty() {
        builder = builder.line(
            colors::GREEN,
            Some(NerdFont::CheckCircle),
            "Age Public Key:",
        );
        for pk in &pubkeys {
            builder = builder.text(&format!("  {}", pk));
        }
        builder = builder.blank();
    }

    if !ssh_keys.is_empty() {
        builder = builder.line(
            colors::BLUE,
            Some(NerdFont::Terminal),
            "SSH Public Keys (age-compatible):",
        );
        for (name, key) in &ssh_keys {
            let short = if key.len() > 60 {
                format!("{}...", &key[..60])
            } else {
                key.clone()
            };
            builder = builder.text(&format!("  {}  ~/.ssh/{}", short, name));
        }
        builder = builder.blank();
    }

    if has_any {
        builder = builder.subtext("Share these public keys to authorize others.");
    } else {
        builder = builder
            .line(
                colors::RED,
                Some(NerdFont::Warning),
                "No local age identity found!",
            )
            .blank()
            .text("Run 'Init Keypair' to generate a secure identity for this machine.");
    }

    builder.build_string()
}

fn build_identities_message() -> String {
    let pubkeys = crate::dot::operations::key::get_local_public_keys().unwrap_or_default();
    let ssh_keys = discover_ssh_public_keys();
    let has_any = !pubkeys.is_empty() || !ssh_keys.is_empty();

    if !has_any {
        return "No local age identity or SSH keys found.\nRun 'Init Keypair' to generate a secure identity for this machine.".to_string();
    }

    let mut parts = Vec::new();

    if !pubkeys.is_empty() {
        parts.push("Age Public Key:".to_string());
        for pk in &pubkeys {
            parts.push(format!("  {}", pk));
        }
    }

    if !ssh_keys.is_empty() {
        if !pubkeys.is_empty() {
            parts.push(String::new());
        }
        parts.push("SSH Public Keys (age-compatible):".to_string());
        for (name, key) in &ssh_keys {
            parts.push(format!("  {}  (from ~/.ssh/{})", key, name));
        }
    }

    parts.push(String::new());
    parts.push("Share these public keys to authorize others.".to_string());

    parts.join("\n")
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
                        FzfWrapper::message(&build_identities_message())?;
                    }
                    EncryptionMenuAction::InitKeypair => {
                        crate::dot::operations::key::handle_init(false)?;
                        let pubkeys = crate::dot::operations::key::get_local_public_keys()
                            .unwrap_or_default();
                        if let Some(pk) = pubkeys.first() {
                            FzfWrapper::message(&format!(
                                "Age identity is ready.\n\nPublic key:\n{}",
                                pk
                            ))?;
                        } else {
                            FzfWrapper::message("Age identity is ready.")?;
                        }
                    }
                    EncryptionMenuAction::Back => return Ok(()),
                }
            }
            FzfResult::Cancelled => return Ok(()),
            FzfResult::Error(e) => return Err(anyhow::anyhow!("FZF Error: {}", e)),
            _ => return Ok(()),
        }
    }
}
