use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

/// Default length for generated passwords.
pub(super) const DEFAULT_PASSWORD_LENGTH: usize = 20;

/// Entries larger than this threshold (in bytes) will be offered for file export instead of clipboard.
pub(super) const EXPORT_THRESHOLD_BYTES: usize = 100 * 1024;

use crate::common::package::{Dependency, PackageDefinition, PackageManager};
use crate::common::requirements::InstallTest;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(super) static PASS_OTP_DEP: Dependency = Dependency {
    name: "pass-otp",
    packages: &[
        PackageDefinition::new("pass-otp", PackageManager::Pacman),
        PackageDefinition::new("pass-extension-otp", PackageManager::Apt),
        PackageDefinition::new("pass-otp", PackageManager::Dnf),
        PackageDefinition::new("pass-otp", PackageManager::Aur),
    ],
    tests: &[InstallTest::CommandSucceeds {
        program: "pass",
        args: &["otp", "help"],
    }],
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PassEntry {
    pub display_name: String,
    pub secret_key: Option<String>,
    pub otp_key: Option<String>,
    pub secret_path: Option<PathBuf>,
    pub otp_path: Option<PathBuf>,
}

impl PassEntry {
    pub fn kind_label(&self) -> &'static str {
        match (self.secret_key.is_some(), self.otp_key.is_some()) {
            (true, true) => "password + otp",
            (true, false) => "password",
            (false, true) => "otp",
            (false, false) => "empty",
        }
    }

    pub fn primary_action_label(&self) -> &'static str {
        if self.secret_key.is_some() {
            "Copy password"
        } else {
            "Copy OTP code"
        }
    }

    pub fn has_secret(&self) -> bool {
        self.secret_key.is_some()
    }

    pub fn has_otp(&self) -> bool {
        self.otp_key.is_some()
    }

    pub fn primary_key(&self) -> Result<&str> {
        self.secret_key
            .as_deref()
            .or(self.otp_key.as_deref())
            .ok_or_else(|| anyhow!("Entry '{}' has no secret or OTP data", self.display_name))
    }

    pub fn primary_file_path(&self) -> Option<&Path> {
        self.secret_path.as_deref().or(self.otp_path.as_deref())
    }

    pub fn preview(&self) -> FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Key, &self.display_name)
            .field("Type", self.kind_label())
            .field("Primary action", self.primary_action_label());

        if self.has_secret() {
            builder = builder.line(
                colors::GREEN,
                Some(NerdFont::Check),
                "Password data available",
            );
        }

        if self.has_otp() {
            builder = builder.line(colors::TEAL, Some(NerdFont::Clock), "OTP support available");
        }

        if self.has_otp() && self.has_secret() {
            builder = builder
                .blank()
                .subtext("Use `ins pass otp <entry>` to copy the OTP code directly.");
        }

        if self.should_offer_export() {
            builder = builder.blank().line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "Large or file-like entry: export will be offered instead of clipboard.",
            );
        }

        builder.build()
    }

    pub fn should_offer_export(&self) -> bool {
        self.display_name.ends_with(".file")
            || self
                .primary_file_path()
                .and_then(|path| fs::metadata(path).ok())
                .map(|metadata| metadata.len() as usize > EXPORT_THRESHOLD_BYTES)
                .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeleteMode {
    Secret,
    Otp,
    Both,
}

#[derive(Default, Clone)]
pub(super) struct EntryTreeNode {
    pub folders: BTreeMap<String, EntryTreeNode>,
    pub entries: Vec<PassEntry>,
}

#[derive(Debug, Clone)]
pub(super) enum AddMenuAction {
    AddPassword,
    GeneratePassword,
    AddOtp,
    Back,
}

#[derive(Debug, Clone)]
pub(super) struct AddMenuItem {
    pub key: &'static str,
    pub display: String,
    pub preview: FzfPreview,
    pub action: AddMenuAction,
}

#[derive(Debug, Clone)]
pub(super) enum EditAction {
    CopyPassword,
    CopyOtp,
    Export,
    Rename,
    EditPassword,
    GeneratePassword,
    EditOtp,
    Delete,
    Back,
}

#[derive(Debug, Clone)]
pub(super) struct EditActionItem {
    pub key: &'static str,
    pub display: String,
    pub preview: FzfPreview,
    pub action: EditAction,
}

#[derive(Debug, Clone)]
pub(super) enum BrowserItemKind {
    Folder(String),
    Entry(String),
    Add,
    Edit,
    Back,
    Close,
}

#[derive(Debug, Clone)]
pub(super) struct BrowserMenuItem {
    pub key: String,
    pub display: String,
    pub preview: FzfPreview,
    pub kind: BrowserItemKind,
}

impl FzfSelectable for PassEntry {
    fn fzf_display_text(&self) -> String {
        self.display_name.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }

    fn fzf_key(&self) -> String {
        self.display_name.clone()
    }
}

impl FzfSelectable for AddMenuItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

impl FzfSelectable for EditActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

impl FzfSelectable for BrowserMenuItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}
