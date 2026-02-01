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
    /// Description for groups (ignored if setting is present)
    pub description: Option<&'static str>,
    /// Child nodes (for grouping)
    pub children: Vec<CategoryNode>,
}

impl CategoryNode {
    /// Create a leaf node with a setting
    pub fn setting(setting: &'static dyn Setting) -> Self {
        CategoryNode {
            setting: Some(setting),
            name: None,
            description: None,
            children: Vec::new(),
        }
    }

    /// Create a group node with children (builder pattern)
    pub fn group(name: &'static str) -> Self {
        CategoryNode {
            setting: None,
            name: Some(name),
            description: None,
            children: Vec::new(),
        }
    }

    /// Set the description for a group (builder pattern)
    pub fn description(mut self, desc: &'static str) -> Self {
        self.description = Some(desc);
        self
    }

    /// Add a child node (builder pattern)
    pub fn child(mut self, node: CategoryNode) -> Self {
        self.children.push(node);
        self
    }
}

/// Get the tree structure for a category
///
/// Each category defines its own tree structure, specifying which settings
/// are top-level and which are grouped into subcategories.
pub fn category_tree(category: Category) -> Vec<CategoryNode> {
    use crate::settings::definitions::{
        appearance, appimages, apps, brightness, desktop, display, flatpak, installed_flatpaks,
        installed_packages, installed_snaps, keyboard, language, mouse, network, packages,
        printers, snap, storage, swap_escape, system, toggles, users, wiremix,
    };
    match category {
        Category::Appearance => vec![
            CategoryNode::setting(&brightness::Brightness),
            CategoryNode::setting(&appearance::Animations),
            CategoryNode::setting(&appearance::DarkMode),
            CategoryNode::setting(&appearance::CursorTheme),
            CategoryNode::group("Wallpaper")
                .description("Desktop background images and colored wallpapers.")
                .child(CategoryNode::setting(&appearance::SetWallpaper))
                .child(CategoryNode::setting(&appearance::RandomWallpaper))
                .child(CategoryNode::setting(&appearance::WallpaperLogo))
                .child(CategoryNode::setting(&appearance::WallpaperBgColor))
                .child(CategoryNode::setting(&appearance::WallpaperFgColor))
                .child(CategoryNode::setting(&appearance::ApplyColoredWallpaper)),
            CategoryNode::group("GTK")
                .description("GTK application theming and icons.")
                .child(CategoryNode::setting(&appearance::GtkTheme))
                .child(CategoryNode::setting(&appearance::GtkIconTheme))
                .child(CategoryNode::setting(&appearance::GtkMenuIcons))
                .child(CategoryNode::setting(&appearance::ResetGtk)),
            CategoryNode::group("Qt")
                .description("Qt application theming.")
                .child(CategoryNode::setting(&appearance::ResetQt)),
        ],
        Category::Network => vec![
            CategoryNode::setting(&network::IpInfo),
            CategoryNode::setting(&network::SpeedTest),
            CategoryNode::setting(&network::EditConnectionsTui),
            CategoryNode::setting(&network::EditConnections),
        ],
        Category::Bluetooth => vec![
            CategoryNode::setting(&toggles::BluetoothService),
            CategoryNode::setting(&toggles::BluetoothCapabilityCheck),
            CategoryNode::setting(&desktop::BluetoothManager),
        ],
        Category::InputDevices => vec![
            CategoryNode::setting(&keyboard::KeyboardLayout),
            CategoryNode::setting(&keyboard::TtyKeymap),
            CategoryNode::setting(&keyboard::LoginScreenLayout),
            CategoryNode::setting(&swap_escape::SwapEscape),
            CategoryNode::setting(&mouse::NaturalScroll),
            CategoryNode::setting(&mouse::SwapButtons),
            CategoryNode::setting(&mouse::MouseSensitivity),
            CategoryNode::setting(&desktop::GamingMouse),
        ],
        Category::Desktop => vec![
            CategoryNode::setting(&desktop::WindowLayout),
            CategoryNode::setting(&toggles::ClipboardManager),
            CategoryNode::setting(&desktop::ScreenRecordFramerate),
            CategoryNode::setting(&desktop::ScreenRecordAudioSources),
        ],
        Category::Display => vec![CategoryNode::setting(&display::ConfigureDisplay)],
        Category::Audio => vec![CategoryNode::setting(&wiremix::LaunchWiremix)],
        Category::Apps => vec![
            CategoryNode::setting(&apps::ManageAllApps),
            CategoryNode::setting(&apps::DefaultBrowser),
            CategoryNode::setting(&apps::DefaultTextEditor),
            CategoryNode::setting(&apps::DefaultFileManager),
            CategoryNode::setting(&apps::DefaultImageViewer),
            CategoryNode::setting(&apps::DefaultVideoPlayer),
            CategoryNode::setting(&apps::DefaultAudioPlayer),
            CategoryNode::setting(&apps::DefaultPdfViewer),
            CategoryNode::setting(&apps::DefaultArchiveManager),
            CategoryNode::setting(&apps::DefaultEmail),
        ],
        Category::Storage => vec![
            CategoryNode::setting(&toggles::AutomountDisks),
            CategoryNode::setting(&storage::DiskManagement),
            CategoryNode::setting(&storage::PartitionEditor),
        ],
        Category::Printers => vec![
            CategoryNode::setting(&printers::PrinterServices),
            CategoryNode::setting(&printers::PrinterManager),
        ],
        Category::Users => vec![CategoryNode::setting(&users::ManageUsers)],
        Category::Language => vec![
            CategoryNode::setting(&language::SystemLanguage),
            CategoryNode::setting(&language::Timezone),
        ],
        Category::System => vec![
            CategoryNode::setting(&system::AboutSystem),
            CategoryNode::setting(&system::SystemDoctor),
            CategoryNode::setting(&system::DotfileManager),
            CategoryNode::setting(&system::CockpitManager),
            CategoryNode::setting(&system::FirmwareManager),
            CategoryNode::setting(&system::SystemUpgrade),
            CategoryNode::setting(&system::PacmanAutoclean),
            CategoryNode::setting(&system::WelcomeAutostart),
        ],

        Category::Install => vec![
            CategoryNode::setting(&packages::InstallPackages),
            CategoryNode::setting(&installed_packages::ManageInstalledPackages),
            CategoryNode::setting(&flatpak::InstallFlatpakApps),
            CategoryNode::setting(&installed_flatpaks::ManageInstalledFlatpaks),
            CategoryNode::setting(&snap::InstallSnapApps),
            CategoryNode::setting(&installed_snaps::ManageInstalledSnaps),
            CategoryNode::setting(&appimages::ManageAppImages),
        ],
    }
}

/// Get all settings from all category trees
pub fn all_settings_from_tree() -> Vec<&'static dyn Setting> {
    let mut settings = Vec::new();
    for &category in Category::all() {
        let tree = category_tree(category);
        collect_settings_from_tree(&tree, &mut settings);
    }
    settings
}

/// Recursively collect all settings from a tree
fn collect_settings_from_tree(nodes: &[CategoryNode], settings: &mut Vec<&'static dyn Setting>) {
    for node in nodes {
        if let Some(setting) = node.setting {
            settings.push(setting);
        }
        if !node.children.is_empty() {
            collect_settings_from_tree(&node.children, settings);
        }
    }
}

/// Get the category that a setting belongs to by searching all category trees
pub fn get_category_for_setting(setting_id: &str) -> Option<Category> {
    for &category in Category::all() {
        let tree = category_tree(category);
        if find_setting_in_tree(&tree, setting_id) {
            return Some(category);
        }
    }
    None
}

/// Check if a setting exists in a tree
fn find_setting_in_tree(nodes: &[CategoryNode], setting_id: &str) -> bool {
    for node in nodes {
        if let Some(setting) = node.setting {
            if setting.metadata().id == setting_id {
                return true;
            }
        } else if !node.children.is_empty() && find_setting_in_tree(&node.children, setting_id) {
            return true;
        }
    }
    false
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
        } else if !node.children.is_empty()
            && let Some(name) = node.name
        {
            path.push(name.to_string());
            if find_setting_path(&node.children, setting_id, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}
