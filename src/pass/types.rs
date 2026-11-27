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
use crate::ui::catppuccin::{colors, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(super) static PASS_OTP_DEP: Dependency = Dependency {
    name: "pass-otp",
    packages: &[
        PackageDefinition::new("pass-otp", PackageManager::Apk),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryKind {
    Password,
    Otp,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PassEntry {
    pub display_name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl PassEntry {
    pub(super) fn new(
        display_name: impl Into<String>,
        path: impl Into<PathBuf>,
        kind: EntryKind,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            path: path.into(),
            kind,
        }
    }

    #[cfg(test)]
    pub(super) fn test_entry(display_name: &str, kind: EntryKind) -> Self {
        Self::new(display_name, PathBuf::new(), kind)
    }

    pub fn is_file_entry(&self) -> bool {
        self.kind == EntryKind::File
    }

    /// Icon representing the entry's primary kind in menu rows.
    pub fn icon(&self) -> NerdFont {
        match self.kind {
            EntryKind::File => NerdFont::File,
            EntryKind::Otp => NerdFont::Clock,
            EntryKind::Password => NerdFont::Key,
        }
    }

    /// Badge color matching [`Self::icon`].
    pub fn icon_color(&self) -> &'static str {
        match self.kind {
            EntryKind::File => colors::LAVENDER,
            EntryKind::Otp => colors::TEAL,
            EntryKind::Password => colors::GREEN,
        }
    }

    /// Rendered row for flat, fuzzy-selectable entry lists: kind icon, then
    /// every folder segment in its own location color, then the entry name.
    pub fn display_row(&self) -> String {
        let icon = format_icon_colored(self.icon(), self.icon_color());
        let segments: Vec<&str> = self.display_name.split('/').collect();
        if segments.len() <= 1 {
            return format!("{icon}{}", self.display_name);
        }

        let mut location = String::new();
        let mut path = String::new();
        for segment in &segments[..segments.len() - 1] {
            if !location.is_empty() {
                location.push('/');
            }
            location.push_str(segment);
            path.push_str(&format_with_color(
                &format!("{segment}/"),
                location_color(&location),
            ));
        }

        let name = segments.last().copied().unwrap_or_default();
        format!("{icon}{path}{name}")
    }

    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            EntryKind::Password => "password",
            EntryKind::Otp => "otp",
            EntryKind::File => "file",
        }
    }

    pub fn primary_action_label(&self) -> &'static str {
        match self.kind {
            EntryKind::File => "Export file",
            EntryKind::Password => "Copy password",
            EntryKind::Otp => "Copy OTP code",
        }
    }

    pub fn has_secret(&self) -> bool {
        matches!(self.kind, EntryKind::Password | EntryKind::File)
    }

    pub fn has_otp(&self) -> bool {
        self.kind == EntryKind::Otp
    }

    pub fn primary_key(&self) -> &str {
        &self.display_name
    }

    pub fn otp_key(&self) -> Result<&str> {
        self.has_otp()
            .then_some(self.display_name.as_str())
            .ok_or_else(|| anyhow!("Entry '{}' has no OTP data", self.display_name))
    }

    pub fn primary_file_path(&self) -> &Path {
        &self.path
    }

    pub fn preview(&self) -> FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(self.icon(), &self.display_name)
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
        self.is_file_entry()
            || fs::metadata(self.primary_file_path())
                .map(|metadata| metadata.len() as usize > EXPORT_THRESHOLD_BYTES)
                .unwrap_or(false)
    }
}

/// Give every folder location a stable accent color. The same path keeps its
/// color across the quick-access, tree, and entry-selection menus.
pub(super) fn location_color(path: &str) -> &'static str {
    const PALETTE: [&str; 10] = [
        colors::BLUE,
        colors::MAUVE,
        colors::TEAL,
        colors::PEACH,
        colors::GREEN,
        colors::PINK,
        colors::SAPPHIRE,
        colors::YELLOW,
        colors::LAVENDER,
        colors::MAROON,
    ];

    // FNV-1a keeps the mapping stable across runs and platforms.
    let hash = path.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });

    PALETTE[(hash % PALETTE.len() as u64) as usize]
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
    AddOtpFromClipboard,
    AddFile,
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
    Move,
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
    Entry { key: String, kind: EntryKind },
    Menu,
    Add,
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
        self.display_row()
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
