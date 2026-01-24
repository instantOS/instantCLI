//! UI items for settings menu
//!
//! Display types for the FZF-based settings menu system.

use crate::menu_utils::FzfSelectable;
use crate::settings::setting::{Category, Setting, SettingState};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, format_search_icon};
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;

// ============================================================================
// Unified Tree Node
// ============================================================================

/// Metadata for a folder node (category or subcategory)
#[derive(Clone)]
pub struct FolderMeta {
    pub icon: NerdFont,
    pub color: &'static str,
    pub title: String,
    pub description: Option<String>,
}

impl FolderMeta {
    /// Create folder metadata from a top-level category
    pub fn from_category(category: Category) -> Self {
        let meta = category.meta();
        Self {
            icon: meta.icon,
            color: meta.color,
            title: meta.title.to_string(),
            description: Some(meta.description.to_string()),
        }
    }

    /// Create folder metadata for a named subcategory
    pub fn from_name(name: &str, description: Option<&str>) -> Self {
        let (icon, color) = match name {
            "GTK" => (NerdFont::Palette, colors::TEAL),
            "Wallpaper" => (NerdFont::Image, colors::LAVENDER),
            "Qt" => (NerdFont::Palette, colors::PINK),
            _ => (NerdFont::Folder, colors::BLUE),
        };
        Self {
            icon,
            color,
            title: name.to_string(),
            description: description.map(|s| s.to_string()),
        }
    }
}

/// A node in the settings tree - either a folder or a setting
#[derive(Clone)]
pub enum TreeNode {
    /// A folder containing child nodes
    Folder {
        meta: FolderMeta,
        children: Vec<TreeNode>,
    },
    /// A leaf setting
    Setting(&'static dyn Setting),
}

impl TreeNode {
    /// Create a folder node from a category
    pub fn from_category(category: Category, children: Vec<TreeNode>) -> Self {
        TreeNode::Folder {
            meta: FolderMeta::from_category(category),
            children,
        }
    }

    /// Create a folder node from a name
    pub fn from_name(name: &str, description: Option<&str>, children: Vec<TreeNode>) -> Self {
        TreeNode::Folder {
            meta: FolderMeta::from_name(name, description),
            children,
        }
    }

    /// Get the display name
    pub fn name(&self) -> &str {
        match self {
            TreeNode::Folder { meta, .. } => &meta.title,
            TreeNode::Setting(s) => s.metadata().title,
        }
    }

    /// Count direct children
    pub fn child_count(&self) -> usize {
        match self {
            TreeNode::Folder { children, .. } => children.len(),
            TreeNode::Setting(_) => 0,
        }
    }

    /// Collect settings from this node (flattened), up to a limit
    pub fn collect_settings(&self, limit: usize) -> Vec<&'static dyn Setting> {
        let mut result = Vec::new();
        if limit != usize::MAX {
            result.reserve(limit);
        }
        self.collect_settings_inner(&mut result, limit);
        result
    }

    fn collect_settings_inner(&self, out: &mut Vec<&'static dyn Setting>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        match self {
            TreeNode::Setting(s) => out.push(*s),
            TreeNode::Folder { children, .. } => {
                for child in children {
                    child.collect_settings_inner(out, limit);
                    if out.len() >= limit {
                        break;
                    }
                }
            }
        }
    }
}

/// Convert CategoryNode tree to UI TreeNode tree
pub fn convert_category_tree(
    category: Category,
    nodes: Vec<crate::settings::category_tree::CategoryNode>,
) -> TreeNode {
    let children = nodes.into_iter().map(convert_category_node).collect();
    TreeNode::from_category(category, children)
}

fn convert_category_node(node: crate::settings::category_tree::CategoryNode) -> TreeNode {
    if let Some(setting) = node.setting {
        TreeNode::Setting(setting)
    } else {
        let children = node
            .children
            .into_iter()
            .map(convert_category_node)
            .collect();
        TreeNode::from_name(node.name.unwrap_or("Unknown"), node.description, children)
    }
}

// ============================================================================
// Menu Items
// ============================================================================

/// Items displayed in any tree level
#[derive(Clone)]
pub enum MenuItem {
    /// A folder node (can be entered)
    Folder(TreeNode),
    /// A setting leaf (can be activated)
    Setting {
        setting: &'static dyn Setting,
        state: SettingState,
    },
    /// Back navigation
    Back {
        /// Where we're going back to (None = main menu)
        parent_name: Option<String>,
    },
}

/// Main menu items (top level with search)
#[derive(Clone)]
pub enum MainMenuItem {
    SearchAll,
    Category(TreeNode),
    Close,
}

/// Search result item
#[derive(Clone)]
pub struct SearchItem {
    pub setting: &'static dyn Setting,
    pub state: SettingState,
}

// ============================================================================
// Shared Preview Builder
// ============================================================================

/// Build a folder-style preview with icon, title, optional description, and setting list.
fn build_folder_preview(
    icon: NerdFont,
    color: &str,
    title: &str,
    description: Option<&str>,
    settings: &[&'static dyn Setting],
) -> crate::menu_utils::FzfPreview {
    let mut builder = PreviewBuilder::new()
        .line(color, Some(icon), title)
        .separator()
        .blank();

    if let Some(desc) = description {
        builder = builder.text(desc).blank();
    }

    let preview_count = 6.min(settings.len());
    if preview_count > 0 {
        builder = builder.separator().blank();

        for (i, setting) in settings.iter().take(preview_count).enumerate() {
            let meta = setting.metadata();
            builder = builder.line(color, Some(meta.icon), meta.title);
            builder = builder.subtext(first_line(meta.summary));

            if i < preview_count - 1 {
                builder = builder.blank();
            }
        }

        if settings.len() > preview_count {
            builder = builder
                .blank()
                .subtext(&format!("… and {} more", settings.len() - preview_count));
        }
    }

    builder.build()
}

// ============================================================================
// FzfSelectable Implementations
// ============================================================================

impl FzfSelectable for MenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            MenuItem::Folder(node) => {
                if let TreeNode::Folder { meta, children, .. } = node {
                    format!(
                        "{} {} ({})",
                        format_icon_colored(meta.icon, meta.color),
                        meta.title,
                        children.len()
                    )
                } else {
                    "Invalid".to_string()
                }
            }
            MenuItem::Setting { setting, state } => format_setting_line(*setting, state),
            MenuItem::Back { .. } => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            MenuItem::Folder(node) => {
                if let TreeNode::Folder { meta, .. } = node {
                    let settings = node.collect_settings(6);
                    build_folder_preview(
                        meta.icon,
                        meta.color,
                        &meta.title,
                        meta.description.as_deref(),
                        &settings,
                    )
                } else {
                    crate::menu_utils::FzfPreview::None
                }
            }
            MenuItem::Setting { setting, state } => build_setting_preview(*setting, state),
            MenuItem::Back { parent_name } => {
                let destination = parent_name
                    .as_ref()
                    .map(|n| n.as_str())
                    .unwrap_or("Main Menu");
                PreviewBuilder::new()
                    .header(NerdFont::ArrowLeft, "Go Back")
                    .text(&format!("Return to {}", destination))
                    .build()
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            MenuItem::Folder(node) => node.name().to_string(),
            MenuItem::Setting { setting, .. } => setting.metadata().id.to_string(),
            MenuItem::Back { .. } => "__back__".to_string(),
        }
    }
}

impl FzfSelectable for MainMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            MainMenuItem::SearchAll => {
                format!("{} Search all settings", format_search_icon())
            }
            MainMenuItem::Category(node) => {
                if let TreeNode::Folder { meta, .. } = node {
                    format!(
                        "{} {} ({} settings)",
                        format_icon_colored(meta.icon, meta.color),
                        meta.title,
                        count_all_settings(node)
                    )
                } else {
                    "Invalid".to_string()
                }
            }
            MainMenuItem::Close => format!("{} Close", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            MainMenuItem::SearchAll => PreviewBuilder::new()
                .header(NerdFont::Search, "Search All Settings")
                .text("Find any setting by name or keyword")
                .build(),
            MainMenuItem::Category(node) => {
                if let TreeNode::Folder { meta, .. } = node {
                    let settings = node.collect_settings(6);
                    build_folder_preview(
                        meta.icon,
                        meta.color,
                        &meta.title,
                        meta.description.as_deref(),
                        &settings,
                    )
                } else {
                    crate::menu_utils::FzfPreview::None
                }
            }
            MainMenuItem::Close => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close")
                .text("Exit the settings menu")
                .build(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            MainMenuItem::SearchAll => "__search__".to_string(),
            MainMenuItem::Category(node) => node.name().to_string(),
            MainMenuItem::Close => "__close__".to_string(),
        }
    }
}

impl FzfSelectable for SearchItem {
    fn fzf_display_text(&self) -> String {
        let meta = self.setting.metadata();
        let path = format_setting_path(self.setting);
        let category = crate::settings::category_tree::get_category_for_setting(meta.id)
            .unwrap_or(Category::System);
        let icon_color = meta.icon_color.unwrap_or_else(|| category.meta().color);

        format!(
            "{} {} {}",
            format_icon_colored(meta.icon, icon_color),
            meta.title,
            path
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        build_setting_preview(self.setting, &self.state)
    }

    fn fzf_key(&self) -> String {
        self.setting.metadata().id.to_string()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Count all settings in a tree node (recursive)
fn count_all_settings(node: &TreeNode) -> usize {
    match node {
        TreeNode::Setting(_) => 1,
        TreeNode::Folder { children, .. } => children.iter().map(count_all_settings).sum(),
    }
}

/// Format a setting line for display
fn format_setting_line(setting: &dyn Setting, state: &SettingState) -> String {
    let meta = setting.metadata();
    let category = crate::settings::category_tree::get_category_for_setting(meta.id)
        .unwrap_or(Category::System);
    let icon_color = meta.icon_color.unwrap_or_else(|| category.meta().color);

    let state_suffix = match state {
        SettingState::Toggle { enabled: true } => {
            format!(" {}", format_icon_colored(NerdFont::Check, colors::GREEN))
        }
        SettingState::Toggle { enabled: false } => {
            format!(" {}", format_icon_colored(NerdFont::Cross, colors::RED))
        }
        SettingState::Choice { current_label } => {
            let subtext = crate::ui::catppuccin::hex_to_ansi_fg(colors::SUBTEXT0);
            let reset = "\x1b[0m";
            format!(" {subtext}({current_label}){reset}")
        }
        SettingState::Action | SettingState::Command => String::new(),
    };

    format!(
        "{} {}{}",
        format_icon_colored(meta.icon, icon_color),
        meta.title,
        state_suffix
    )
}

/// Format the breadcrumb path for a setting
fn format_setting_path(setting: &dyn Setting) -> String {
    let meta = setting.metadata();
    let subtext = crate::ui::catppuccin::hex_to_ansi_fg(colors::SUBTEXT0);
    let reset = "\x1b[0m";

    if let Some(category) = crate::settings::category_tree::get_category_for_setting(meta.id) {
        let cat_meta = category.meta();
        format!("{subtext}({}){reset}", cat_meta.title)
    } else {
        String::new()
    }
}

/// Build preview for a setting
fn build_setting_preview(
    setting: &dyn Setting,
    state: &SettingState,
) -> crate::menu_utils::FzfPreview {
    // Check if setting has a custom preview command
    if let Some(cmd) = setting.preview_command() {
        return crate::menu_utils::FzfPreview::Command(cmd);
    }

    let meta = setting.metadata();
    let category = crate::settings::category_tree::get_category_for_setting(meta.id)
        .unwrap_or(Category::System);
    let icon_color = meta.icon_color.unwrap_or_else(|| category.meta().color);

    let mut builder = PreviewBuilder::new()
        .line(icon_color, Some(meta.icon), meta.title)
        .separator()
        .blank();

    // Show current state if available
    match state {
        SettingState::Toggle { enabled: true } => {
            builder = builder
                .line(colors::GREEN, Some(NerdFont::Check), "Enabled")
                .blank();
        }
        SettingState::Toggle { enabled: false } => {
            builder = builder
                .line(colors::RED, Some(NerdFont::Cross), "Disabled")
                .blank();
        }
        SettingState::Choice { current_label } => {
            builder = builder.field("Current", current_label).blank();
        }
        SettingState::Action | SettingState::Command => {}
    }

    // Summary text
    for line in meta.summary.lines() {
        builder = builder.text(line);
    }

    builder.build()
}

/// Get first line of a string
fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}
