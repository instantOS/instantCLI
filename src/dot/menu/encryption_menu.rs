use anyhow::{Context, Result};
use std::path::PathBuf;
use std::str::FromStr;

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
    fn public_key(&self) -> &str {
        match self {
            Self::AgeIdentity { public_key, .. } => public_key,
            Self::SshKey { public_key, .. } => public_key,
        }
    }

    fn path(&self) -> &PathBuf {
        match self {
            Self::AgeIdentity { path, .. } => path,
            Self::SshKey { path, .. } => path,
        }
    }

    fn display_name(&self) -> String {
        match self {
            Self::AgeIdentity { path, .. } => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string()),
            Self::SshKey { filename, .. } => filename.clone(),
        }
    }

    fn short_key(&self) -> String {
        let key = self.public_key();
        const MAX_DISPLAY: usize = 40;
        const ELLIPSIS: &str = "...";
        if key.len() > MAX_DISPLAY {
            format!("{}{}", &key[..MAX_DISPLAY - ELLIPSIS.len()], ELLIPSIS)
        } else {
            key.to_string()
        }
    }

    fn key_type_label(&self) -> &'static str {
        match self {
            Self::AgeIdentity { .. } => "age",
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
            KeyAction::DeleteKey => "delete_key".to_string(),
            KeyAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

fn discover_all_keys() -> Vec<EncryptionKeyKind> {
    let mut keys = Vec::new();

    for path in crate::dot::encryption::discover_identity_files() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("AGE-SECRET-KEY-1")
                    && let Ok(identity) = age::x25519::Identity::from_str(trimmed)
                {
                    keys.push(EncryptionKeyKind::AgeIdentity {
                        path: path.clone(),
                        public_key: identity.to_public().to_string(),
                    });
                }
            }
        }
    }

    let home = std::env::var("HOME").map(PathBuf::from).ok();
    if let Some(home_path) = home {
        let ssh_dir = home_path.join(".ssh");
        if ssh_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(ssh_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && path.extension().is_some_and(|ext| ext == "pub")
                    && let Ok(content) = std::fs::read_to_string(&path)
                {
                    let content_trimmed = content.trim();
                    if content_trimmed.starts_with("ssh-") || content_trimmed.starts_with("ecdsa-")
                    {
                        let Some(filename) = path.file_name() else {
                            continue;
                        };
                        let filename = filename.to_string_lossy().into_owned();
                        keys.push(EncryptionKeyKind::SshKey {
                            path: path.clone(),
                            public_key: content_trimmed.to_string(),
                            filename,
                        });
                    }
                }
            }
        }
    }

    keys
}

fn build_key_preview(key: &EncryptionKeyKind) -> String {
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

    let repos_using_key: Vec<String> = config
        .get_writable_repos()
        .into_iter()
        .filter_map(|r| {
            let dotfile_repo =
                crate::dot::dotfilerepo::DotfileRepo::new(config, r.name.clone()).ok()?;
            let repo_path = dotfile_repo.local_path(config).ok()?;
            let meta = crate::dot::meta::read_meta(&repo_path).ok()?;
            if meta.age_recipients.iter().any(|r| r == public_key) {
                Some(r.name.clone())
            } else {
                None
            }
        })
        .collect();

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
                .text("Create a new age x25519 identity keypair.")
                .blank()
                .text("The private key is saved to:")
                .text("  ~/.config/instant/age/identity")
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
