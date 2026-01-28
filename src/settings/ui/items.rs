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

/// Tree search item - displays settings and categories in a tree structure
#[derive(Clone)]
pub enum TreeSearchItem {
    /// A category folder in the tree
    Category {
        category: Category,
        tree_prefix: String,
    },
    /// A subcategory folder in the tree
    Folder {
        meta: FolderMeta,
        tree_prefix: String,
        path: String,
    },
    /// A setting leaf in the tree
    Setting {
        setting: &'static dyn Setting,
        state: SettingState,
        tree_prefix: String,
    },
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

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";

impl FzfSelectable for TreeSearchItem {
    fn fzf_display_text(&self) -> String {
        let tree_color = crate::ui::catppuccin::hex_to_ansi_fg(colors::SURFACE1);
        let text_color = crate::ui::catppuccin::hex_to_ansi_fg(colors::TEXT);

        match self {
            TreeSearchItem::Category {
                category,
                tree_prefix,
            } => {
                let meta = category.meta();
                let icon = format_icon_colored(meta.icon, meta.color);
                let title_color = crate::ui::catppuccin::hex_to_ansi_fg(meta.color);
                format!(
                    "{tree_color}{tree_prefix}{RESET} {icon} {BOLD}{title_color}{}{RESET}",
                    meta.title
                )
            }
            TreeSearchItem::Folder {
                meta, tree_prefix, ..
            } => {
                let icon = format_icon_colored(meta.icon, meta.color);
                let title_color = crate::ui::catppuccin::hex_to_ansi_fg(meta.color);
                format!(
                    "{tree_color}{tree_prefix}{RESET} {icon} {BOLD}{title_color}{}{RESET}",
                    meta.title
                )
            }
            TreeSearchItem::Setting {
                setting,
                state,
                tree_prefix,
            } => {
                let meta = setting.metadata();
                let category = crate::settings::category_tree::get_category_for_setting(meta.id)
                    .unwrap_or(Category::System);
                let icon_color = meta.icon_color.unwrap_or_else(|| category.meta().color);
                let icon = format_icon_colored(meta.icon, icon_color);

                let state_suffix = match state {
                    SettingState::Toggle { enabled: true } => {
                        format!(" {}", format_icon_colored(NerdFont::Check, colors::GREEN))
                    }
                    SettingState::Toggle { enabled: false } => {
                        format!(" {}", format_icon_colored(NerdFont::Cross, colors::RED))
                    }
                    SettingState::Choice { current_label } => {
                        let subtext = crate::ui::catppuccin::hex_to_ansi_fg(colors::SUBTEXT0);
                        format!(" {subtext}({current_label}){RESET}")
                    }
                    SettingState::Action | SettingState::Command => String::new(),
                };

                format!(
                    "{tree_color}{tree_prefix}{RESET} {icon} {text_color}{}{RESET}{state_suffix}",
                    meta.title
                )
            }
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            TreeSearchItem::Category { category, .. } => {
                let meta = category.meta();
                let tree = crate::settings::category_tree::category_tree(*category);
                let settings = collect_category_settings(&tree);
                build_folder_preview(
                    meta.icon,
                    meta.color,
                    meta.title,
                    Some(meta.description),
                    &settings,
                )
            }
            TreeSearchItem::Folder { meta, .. } => build_folder_preview(
                meta.icon,
                meta.color,
                &meta.title,
                meta.description.as_deref(),
                &[],
            ),
            TreeSearchItem::Setting { setting, state, .. } => {
                build_setting_preview(*setting, state)
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            TreeSearchItem::Category { category, .. } => format!("cat:{}", category.meta().title),
            TreeSearchItem::Folder { path, .. } => format!("folder:{path}"),
            TreeSearchItem::Setting { setting, .. } => format!("setting:{}", setting.metadata().id),
        }
    }
}

fn collect_category_settings(
    nodes: &[crate::settings::category_tree::CategoryNode],
) -> Vec<&'static dyn Setting> {
    let mut settings = Vec::new();
    for node in nodes {
        if let Some(s) = node.setting {
            settings.push(s);
        }
        settings.extend(collect_category_settings(&node.children));
    }
    settings
}

/// Build tree search items from all categories
pub fn build_tree_search_items(
    ctx: &crate::settings::context::SettingsContext,
) -> Vec<TreeSearchItem> {
    let mut items = Vec::new();
    let categories = Category::all();
    let total_categories = categories.len();

    for (cat_idx, category) in categories.iter().enumerate() {
        let is_last_category = cat_idx == total_categories - 1;
        let cat_connector = if is_last_category { "└─" } else { "├─" };
        let cat_child_prefix = if is_last_category { "   " } else { "│  " };

        items.push(TreeSearchItem::Category {
            category: *category,
            tree_prefix: cat_connector.to_string(),
        });

        let tree = crate::settings::category_tree::category_tree(*category);
        append_tree_items(
            &mut items,
            &tree,
            cat_child_prefix,
            category.meta().title,
            ctx,
        );
    }

    items.reverse();
    items
}

fn append_tree_items(
    items: &mut Vec<TreeSearchItem>,
    nodes: &[crate::settings::category_tree::CategoryNode],
    prefix: &str,
    path: &str,
    ctx: &crate::settings::context::SettingsContext,
) {
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == nodes.len() - 1;
        let connector = if is_last { "└─" } else { "├─" };
        let child_prefix = if is_last {
            format!("{prefix}   ")
        } else {
            format!("{prefix}│  ")
        };

        if let Some(setting) = node.setting {
            let state = setting.get_display_state(ctx);
            items.push(TreeSearchItem::Setting {
                setting,
                state,
                tree_prefix: format!("{prefix}{connector}"),
            });
        } else if let Some(name) = node.name {
            let folder_path = format!("{path}/{name}");
            items.push(TreeSearchItem::Folder {
                meta: FolderMeta::from_name(name, node.description),
                tree_prefix: format!("{prefix}{connector}"),
                path: folder_path.clone(),
            });

            append_tree_items(items, &node.children, &child_prefix, &folder_path, ctx);
        }
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
