use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{
    colors, format_back_icon, format_icon_colored, format_with_color, fzf_mocha_args,
};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone)]
pub enum EncryptionKeyKind {
    AgeIdentity {
        path: PathBuf,
        public_key: String,
    },
    SshKey {
        path: PathBuf,
        public_key: String,
        filename: String,
    },
}

impl EncryptionKeyKind {
    pub fn public_key(&self) -> &str {
        match self {
            Self::AgeIdentity { public_key, .. } => public_key,
            Self::SshKey { public_key, .. } => public_key,
        }
    }

    pub fn path(&self) -> &PathBuf {
        match self {
            Self::AgeIdentity { path, .. } => path,
            Self::SshKey { path, .. } => path,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::AgeIdentity { path, .. } => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string()),
            Self::SshKey { filename, .. } => filename.clone(),
        }
    }

    pub fn short_key(&self) -> String {
        let key = self.public_key();
        const MAX_DISPLAY: usize = 40;
        const ELLIPSIS: &str = "...";
        if key.len() > MAX_DISPLAY {
            format!("{}{}", &key[..MAX_DISPLAY - ELLIPSIS.len()], ELLIPSIS)
        } else {
            key.to_string()
        }
    }

    pub fn key_type_label(&self) -> &'static str {
        match self {
            Self::AgeIdentity { .. } => "key",
            Self::SshKey { .. } => "ssh",
        }
    }
}

#[derive(Debug, Clone)]
pub enum EncryptionMenuAction {
    SelectKey(EncryptionKeyKind),
    GenerateNewKey,
    Back,
}

#[derive(Debug, Clone)]
pub enum KeyAction {
    CopyPublicKey,
    AuthorizeToRepo,
    RenameKey,
    DeleteKey,
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
        match &self.action {
            EncryptionMenuAction::SelectKey(key) => format!("key_{}", key.public_key()),
            EncryptionMenuAction::GenerateNewKey => "generate_new_key".to_string(),
            EncryptionMenuAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

#[derive(Clone)]
struct KeyActionItem {
    action: KeyAction,
    display: String,
    preview: String,
}

impl FzfSelectable for KeyActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        match self.action {
            KeyAction::CopyPublicKey => "copy_public_key".to_string(),
            KeyAction::AuthorizeToRepo => "authorize_to_repo".to_string(),
            KeyAction::RenameKey => "rename_key".to_string(),
            KeyAction::DeleteKey => "delete_key".to_string(),
            KeyAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

pub(crate) fn discover_all_keys() -> Vec<EncryptionKeyKind> {
    let mut keys = Vec::new();

    if let Ok(info_keys) = crate::dot::operations::key::discover_all_keys_info() {
        for info in info_keys {
            match info.key_type {
                crate::dot::operations::key::KeyType::Age => {
                    keys.push(EncryptionKeyKind::AgeIdentity {
                        path: info.path,
                        public_key: info.public_key,
                    });
                }
                crate::dot::operations::key::KeyType::Ssh => {
                    let filename = info
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| info.name.clone());
                    keys.push(EncryptionKeyKind::SshKey {
                        path: info.path,
                        public_key: info.public_key,
                        filename,
                    });
                }
            }
        }
    }

    keys
}

pub(crate) fn build_key_preview(key: &EncryptionKeyKind) -> String {
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Key, &format!("{} Key", key.key_type_label()))
        .blank()
        .field("Public key", key.public_key())
        .field("Path", &key.path().to_string_lossy())
        .blank()
        .subtext("Select to copy, authorize, or delete this key.");

    if let EncryptionKeyKind::SshKey { filename, .. } = key {
        builder = builder.field("SSH file", &format!("~/.ssh/{}", filename));
    }

    builder.build_string()
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    let display_server = crate::common::display_server::DisplayServer::detect();
    crate::assist::utils::copy_to_clipboard(text.as_bytes(), &display_server).context(
        "Failed to copy to clipboard. Ensure wl-copy (Wayland) or xclip (X11) is installed.",
    )
}

fn handle_key_action_menu(
    key: &EncryptionKeyKind,
    config: &DotfileConfig,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let mut actions = Vec::new();

        actions.push(KeyActionItem {
            action: KeyAction::CopyPublicKey,
            display: format!(
                "{} Copy Public Key",
                format_icon_colored(NerdFont::Clipboard, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clipboard, "Copy Public Key")
                .text("Copy this key to the system clipboard.")
                .blank()
                .field("Key", key.public_key())
                .build_string(),
        });

        actions.push(KeyActionItem {
            action: KeyAction::AuthorizeToRepo,
            display: format!(
                "{} Authorize to Repo",
                format_icon_colored(NerdFont::Users, colors::PEACH)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Users, "Authorize to Repository")
                .text("Authorize this key in a dotfile repository.")
                .blank()
                .text("This will add the key to the repository's")
                .text("authorized recipients and re-encrypt files.")
                .build_string(),
        });

        actions.push(KeyActionItem {
            action: KeyAction::RenameKey,
            display: format!(
                "{} Rename Key",
                format_icon_colored(NerdFont::Edit, colors::MAUVE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Edit, "Rename Key")
                .text("Rename this key file.")
                .blank()
                .field("Current name", &key.display_name())
                .build_string(),
        });

        actions.push(KeyActionItem {
            action: KeyAction::DeleteKey,
            display: format!(
                "{} Delete Key",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Trash, "Delete Key")
                .line(
                    colors::RED,
                    Some(NerdFont::Warning),
                    "Warning: This cannot be undone!",
                )
                .blank()
                .text("Permanently delete this key file from disk.")
                .blank()
                .field("Path", &key.path().to_string_lossy())
                .build_string(),
        });

        actions.push(KeyActionItem {
            action: KeyAction::Back,
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .subtext("Return to encryption keys menu")
                .build_string(),
        });

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("Key: {}", key.display_name())))
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
                    KeyAction::CopyPublicKey => {
                        copy_to_clipboard(key.public_key())?;
                        FzfWrapper::message(&format!(
                            "Public key copied to clipboard.\n\n{}",
                            key.public_key()
                        ))?;
                    }
                    KeyAction::AuthorizeToRepo => {
                        handle_authorize_key_to_repo(key.public_key(), config, db, debug)?;
                    }
                    KeyAction::RenameKey => {
                        let current_name = key.display_name();
                        let new_name = FzfWrapper::builder()
                            .header(Header::fancy("Rename Key"))
                            .prompt("New name")
                            .query(&current_name)
                            .input()
                            .input_dialog()?;
                        if new_name.is_empty() || new_name == current_name {
                            continue;
                        }
                        match crate::dot::operations::key::handle_rename(
                            &current_name,
                            &new_name,
                        ) {
                            Ok(()) => {
                                FzfWrapper::message(&format!(
                                    "Key renamed: {} → {}",
                                    current_name, new_name
                                ))?;
                                return Ok(());
                            }
                            Err(e) => {
                                FzfWrapper::message(&format!("Rename failed: {}", e))?;
                            }
                        }
                    }
                    KeyAction::DeleteKey => {
                        if handle_delete_key(key, config)? {
                            return Ok(());
                        }
                    }
                    KeyAction::Back => return Ok(()),
                }
            }
            FzfResult::Cancelled => return Ok(()),
            FzfResult::Error(e) => return Err(anyhow::anyhow!("FZF Error: {}", e)),
            _ => return Ok(()),
        }
    }
}

fn handle_authorize_key_to_repo(
    public_key: &str,
    config: &DotfileConfig,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let writable_repos = config.get_writable_repos();
    if writable_repos.is_empty() {
        FzfWrapper::message("No writable repositories found in config.")?;
        return Ok(());
    }

    #[derive(Clone)]
    struct RepoOption(String);
    impl FzfSelectable for RepoOption {
        fn fzf_display_text(&self) -> String {
            format!(
                "{} {}",
                format_icon_colored(NerdFont::Folder, colors::MAUVE),
                self.0
            )
        }
        fn fzf_key(&self) -> String {
            self.0.clone()
        }
        fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
            crate::menu::protocol::FzfPreview::Text(format!(
                "Authorize key in repository '{}'",
                self.0
            ))
        }
    }

    let repo_options: Vec<RepoOption> = writable_repos
        .iter()
        .map(|r| RepoOption(r.name.clone()))
        .collect();

    let builder = FzfWrapper::builder()
        .header(Header::fancy("Select Repository"))
        .prompt("Repository")
        .args(fzf_mocha_args())
        .responsive_layout();

    let result = builder.select(repo_options.clone())?;

    if let FzfResult::Selected(repo_option) = result {
        let dry_run = false;
        crate::dot::operations::key::handle_authorize(
            config,
            db,
            Some(public_key),
            Some(&repo_option.0),
            dry_run,
            debug,
        )?;
        FzfWrapper::message(&format!(
            "Key authorized for '{}'.\n\n{}",
            repo_option.0, public_key
        ))?;
    }

    Ok(())
}

fn handle_delete_key(key: &EncryptionKeyKind, config: &DotfileConfig) -> Result<bool> {
    let key_path = key.path();
    let public_key = key.public_key();

    let repos_using_key = crate::dot::operations::key::find_repos_using_key(config, public_key);

    let warning = if repos_using_key.is_empty() {
        String::new()
    } else {
        format!(
            "\nWARNING: This key is authorized in: {}\nDeleting it may prevent decryption of those repositories.",
            repos_using_key.join(", ")
        )
    };

    let result = FzfWrapper::builder()
        .responsive_layout()
        .confirm(format!(
            "Delete {}?\n\nPath: {}\n\nThis cannot be undone.{}",
            key.display_name(),
            key_path.to_string_lossy(),
            warning
        ))
        .yes_text("Delete")
        .no_text("Cancel")
        .confirm_dialog()?;

    if result != ConfirmResult::Yes {
        return Ok(false);
    }

    std::fs::remove_file(key_path).with_context(|| {
        format!(
            "Failed to delete key file at {}",
            key_path.to_string_lossy()
        )
    })?;
    FzfWrapper::message(&format!(
        "Key deleted.\n\nRemoved: {}",
        key_path.to_string_lossy()
    ))?;
    Ok(true)
}

pub fn handle_encryption_keys_menu(
    config: &DotfileConfig,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let keys = discover_all_keys();
        let mut items = Vec::new();

        for key in &keys {
            let icon = format_icon_colored(
                match key {
                    EncryptionKeyKind::AgeIdentity { .. } => NerdFont::Lock,
                    EncryptionKeyKind::SshKey { .. } => NerdFont::Terminal,
                },
                match key {
                    EncryptionKeyKind::AgeIdentity { .. } => colors::GREEN,
                    EncryptionKeyKind::SshKey { .. } => colors::BLUE,
                },
            );
            let short = format_with_color(&key.short_key(), colors::OVERLAY0);
            items.push(EncryptionMenuItem {
                action: EncryptionMenuAction::SelectKey(key.clone()),
                display: format!("{} {}  {}", icon, key.display_name(), short),
                preview: build_key_preview(key),
            });
        }

        items.push(EncryptionMenuItem {
            action: EncryptionMenuAction::GenerateNewKey,
            display: format!(
                "{} Generate New Key",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Plus, "Generate New Key")
                .text("Create a new x25519 encryption keypair.")
                .blank()
                .text("The private key is saved to:")
                .text("  ~/.config/instant/encryption/identities/<name>")
                .blank()
                .subtext("Existing keys are not overwritten.")
                .build_string(),
        });

        items.push(EncryptionMenuItem {
            action: EncryptionMenuAction::Back,
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .subtext("Return to main menu")
                .build_string(),
        });

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("Encryption Keys"))
            .prompt("Select key")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&items) {
            builder = builder.initial_index(index);
        }

        let result = builder.select(items.clone())?;

        match result {
            FzfResult::Selected(item) => {
                cursor.update(&item, &items);
                match item.action {
                    EncryptionMenuAction::SelectKey(key) => {
                        handle_key_action_menu(&key, config, db, debug)?;
                    }
                    EncryptionMenuAction::GenerateNewKey => {
                        let default_name = nix::unistd::gethostname()
                            .ok()
                            .and_then(|h| h.into_string().ok())
                            .unwrap_or_else(|| "default".to_string());
                        let name = FzfWrapper::builder()
                            .header(Header::fancy("Generate New Key"))
                            .prompt("Key name")
                            .query(&default_name)
                            .input()
                            .input_dialog()?;
                        if name.is_empty() {
                            continue;
                        }
                        crate::dot::operations::key::handle_init(Some(&name), false)?;
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
