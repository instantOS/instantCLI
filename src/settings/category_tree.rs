//! Category tree structure for organizing settings
//!
//! This module defines a tree-based structure for organizing settings into
//! categories and subcategories, eliminating the need for manual breadcrumb management.

use super::setting::{Category, Setting};

/// A node in the category tree - can contain a setting, or be a group with children
pub struct CategoryNode {
    /// Optional setting (None means this is a folder/group)
    pub setting: Option<&'static dyn Setting>,
    /// Display name (used for groups, ignored if setting is present)
    pub name: Option<&'static str>,
    /// Child nodes (for grouping)
    pub children: Vec<CategoryNode>,
}

impl CategoryNode {
    /// Create a leaf node with a setting
    pub fn setting(setting: &'static dyn Setting) -> Self {
        CategoryNode {
            setting: Some(setting),
            name: None,
            children: Vec::new(),
        }
    }

    /// Create a group node with children (builder pattern)
    pub fn group(name: &'static str) -> Self {
        CategoryNode {
            setting: None,
            name: Some(name),
            children: Vec::new(),
        }
    }

    /// Add a child node (builder pattern)
    pub fn child(mut self, node: CategoryNode) -> Self {
        self.children.push(node);
        self
    }

    /// Add multiple children (builder pattern)
    pub fn children(mut self, nodes: Vec<CategoryNode>) -> Self {
        self.children.extend(nodes);
        self
    }

    /// Check if this is a leaf (setting) node
    pub fn is_leaf(&self) -> bool {
        self.setting.is_some()
    }

    /// Get the display name of this node
    pub fn display_name(&self) -> &str {
        if let Some(setting) = self.setting {
            setting.metadata().title
        } else {
            self.name.unwrap_or("Unknown")
        }
    }
}

/// Get the tree structure for a category
///
/// Each category defines its own tree structure, specifying which settings
/// are top-level and which are grouped into subcategories.
pub fn category_tree(category: Category) -> Vec<CategoryNode> {
    use crate::settings::definitions::{appearance, brightness, flatpak, network};

    match category {
        Category::Appearance => vec![
            CategoryNode::setting(&brightness::Brightness),
            CategoryNode::setting(&appearance::Animations),
            CategoryNode::group("Wallpaper")
                .child(CategoryNode::setting(&appearance::SetWallpaper))
                .child(CategoryNode::setting(&appearance::RandomWallpaper))
                .child(CategoryNode::setting(&appearance::WallpaperLogo))
                .child(CategoryNode::setting(&appearance::WallpaperBgColor))
                .child(CategoryNode::setting(&appearance::WallpaperFgColor))
                .child(CategoryNode::setting(&appearance::ApplyColoredWallpaper)),
            CategoryNode::group("GTK")
                .child(CategoryNode::setting(&appearance::GtkTheme))
                .child(CategoryNode::setting(&appearance::GtkIconTheme))
                .child(CategoryNode::setting(&appearance::ResetGtk)),
        ],
        Category::Network => vec![
            CategoryNode::setting(&network::IpInfo),
            CategoryNode::setting(&network::SpeedTest),
            CategoryNode::setting(&network::EditConnections),
        ],
        Category::Bluetooth => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Mouse => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Desktop => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Audio => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Apps => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Storage => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Printers => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Users => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Language => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::System => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
        Category::Install => super::setting::settings_in_category(category)
            .into_iter()
            .map(CategoryNode::setting)
            .collect(),
    }
}

/// Get breadcrumbs for a setting by searching the category tree
///
/// Returns the path from the category root to the setting (excluding the setting itself)
pub fn get_breadcrumbs_for_setting(category: Category, setting_id: &str) -> Vec<String> {
    let tree = category_tree(category);
    let mut path = Vec::new();
    if find_setting_path(&tree, setting_id, &mut path) {
        path
    } else {
        Vec::new()
    }
}

fn find_setting_path(nodes: &[CategoryNode], setting_id: &str, path: &mut Vec<String>) -> bool {
    for node in nodes {
        if let Some(setting) = node.setting {
            if setting.metadata().id == setting_id {
                return true;
            }
        } else if !node.children.is_empty() {
            if let Some(name) = node.name {
                path.push(name.to_string());
                if find_setting_path(&node.children, setting_id, path) {
                    return true;
                }
                path.pop();
            }
        }
    }
    false
}
