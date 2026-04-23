use std::collections::HashMap;

use anyhow::{Result, anyhow};

use crate::menu::protocol::SerializableMenuItem;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::types::{BrowserItemKind, BrowserMenuItem, EntryTreeNode, PassEntry};

fn make_add_item(preview_text: &str) -> BrowserMenuItem {
    BrowserMenuItem {
        key: "add".to_string(),
        display: format!("{} Add", format_icon_colored(NerdFont::Plus, colors::GREEN)),
        preview: PreviewBuilder::new()
            .header(NerdFont::Plus, "Add Entry")
            .text(preview_text)
            .build(),
        kind: BrowserItemKind::Add,
    }
}

fn make_menu_item() -> BrowserMenuItem {
    BrowserMenuItem {
        key: "menu".to_string(),
        display: format!(
            "{} Edit Passwords",
            format_icon_colored(NerdFont::Edit, colors::BLUE)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Edit, "Edit Passwords")
            .text("Open `ins pass menu` for hierarchical password and OTP editing.")
            .build(),
        kind: BrowserItemKind::Menu,
    }
}

pub(super) fn build_quick_access_items(entries: &[PassEntry]) -> Vec<BrowserMenuItem> {
    let mut items: Vec<_> = entries
        .iter()
        .cloned()
        .map(|entry| BrowserMenuItem {
            key: format!("entry:{}", entry.display_name),
            display: entry.display_name.clone(),
            preview: entry.preview(),
            kind: BrowserItemKind::Entry(entry.display_name),
        })
        .collect();

    items.push(make_menu_item());
    items
}

pub(super) fn build_quick_access_menu_items(entries: &[PassEntry]) -> Vec<SerializableMenuItem> {
    build_quick_access_items(entries)
        .into_iter()
        .map(|item| {
            let mut metadata = HashMap::new();
            match &item.kind {
                BrowserItemKind::Entry(key) => {
                    metadata.insert("kind".to_string(), "entry".to_string());
                    metadata.insert("key".to_string(), key.clone());
                }
                BrowserItemKind::Menu => {
                    metadata.insert("kind".to_string(), "menu".to_string());
                }
                BrowserItemKind::Folder(folder) => {
                    metadata.insert("kind".to_string(), "folder".to_string());
                    metadata.insert("path".to_string(), folder.clone());
                }
                BrowserItemKind::Add => {
                    metadata.insert("kind".to_string(), "add".to_string());
                }
                BrowserItemKind::Back => {
                    metadata.insert("kind".to_string(), "back".to_string());
                }
                BrowserItemKind::Close => {
                    metadata.insert("kind".to_string(), "close".to_string());
                }
            }

            SerializableMenuItem {
                key: Some(item.key),
                display_text: item.display,
                preview: item.preview,
                metadata: Some(metadata),
            }
        })
        .collect()
}

pub(super) fn build_browser_menu_items(
    entries: &[PassEntry],
    path: &[String],
) -> Result<Vec<BrowserMenuItem>> {
    let tree = build_entry_tree(entries);
    let node = tree_node_for_path(&tree, path).ok_or_else(|| anyhow!("Invalid pass tree path"))?;
    let mut items = Vec::new();
    append_tree_browser_items(
        &mut items,
        node,
        path_prefix(path).as_deref().unwrap_or(""),
        "",
    );

    if path.is_empty() {
        items.push(make_add_item(
            "Open the add menu for new passwords and OTP entries.",
        ));
        items.push(BrowserMenuItem {
            key: "close".to_string(),
            display: format!("{} Close", format_back_icon()),
            preview: PreviewBuilder::new()
                .header(NerdFont::Cross, "Close")
                .text("Close the pass menu and return to the shell.")
                .build(),
            kind: BrowserItemKind::Close,
        });
    } else {
        items.push(make_add_item(
            "Open the add menu inside the current folder.",
        ));
        items.push(BrowserMenuItem {
            key: "back".to_string(),
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Go to the parent folder in the pass tree.")
                .build(),
            kind: BrowserItemKind::Back,
        });
    }

    items.reverse();
    Ok(items)
}

fn build_entry_tree(entries: &[PassEntry]) -> EntryTreeNode {
    let mut root = EntryTreeNode::default();
    for entry in entries {
        insert_entry_into_tree(
            &mut root,
            entry.clone(),
            &path_segments(&entry.display_name),
        );
    }
    root
}

fn insert_entry_into_tree(node: &mut EntryTreeNode, entry: PassEntry, segments: &[String]) {
    if segments.len() <= 1 {
        node.entries.push(entry);
        return;
    }

    let head = &segments[0];
    let child = node.folders.entry(head.clone()).or_default();
    insert_entry_into_tree(child, entry, &segments[1..]);
}

fn tree_node_for_path<'a>(root: &'a EntryTreeNode, path: &[String]) -> Option<&'a EntryTreeNode> {
    let mut current = root;
    for segment in path {
        current = current.folders.get(segment)?;
    }
    Some(current)
}

fn append_tree_browser_items(
    items: &mut Vec<BrowserMenuItem>,
    node: &EntryTreeNode,
    base_path: &str,
    prefix: &str,
) {
    let folder_count = node.folders.len();
    let entry_count = node.entries.len();
    let total = folder_count + entry_count;
    let mut index = 0usize;

    for (name, child) in &node.folders {
        index += 1;
        let is_last = index == total;
        let connector = if is_last { "└─" } else { "├─" };
        let child_prefix = if is_last { "   " } else { "│  " };
        let folder_path = if base_path.is_empty() {
            name.clone()
        } else {
            format!("{base_path}/{name}")
        };

        items.push(BrowserMenuItem {
            key: format!("folder:{folder_path}"),
            display: format_tree_line(prefix, connector, &folder_path, name, true),
            preview: PreviewBuilder::new()
                .header(NerdFont::Folder, name)
                .text("Open this folder in the pass tree.")
                .blank()
                .field("Path", &folder_path)
                .field("Entries below", &count_entries(child).to_string())
                .build(),
            kind: BrowserItemKind::Folder(folder_path.clone()),
        });

        append_tree_browser_items(
            items,
            child,
            &folder_path,
            &format!("{prefix}{child_prefix}"),
        );
    }

    for entry in &node.entries {
        index += 1;
        let is_last = index == total;
        let connector = if is_last { "└─" } else { "├─" };
        let leaf = path_segments(&entry.display_name)
            .last()
            .cloned()
            .unwrap_or_else(|| entry.display_name.clone());
        items.push(BrowserMenuItem {
            key: format!("entry:{}", entry.display_name),
            display: format_tree_line(prefix, connector, &entry.display_name, &leaf, false),
            preview: entry.preview(),
            kind: BrowserItemKind::Entry(entry.display_name.clone()),
        });
    }
}

fn count_entries(node: &EntryTreeNode) -> usize {
    node.entries.len() + node.folders.values().map(count_entries).sum::<usize>()
}

fn format_tree_line(
    prefix: &str,
    connector: &str,
    full_path: &str,
    label: &str,
    folder: bool,
) -> String {
    let tree_color = crate::ui::catppuccin::hex_to_ansi_fg(colors::SURFACE1);
    let key_color = crate::ui::catppuccin::hex_to_ansi_fg(if folder {
        colors::SAPPHIRE
    } else {
        colors::GREEN
    });
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";
    let tree = format!("{tree_color}{prefix}{connector}{reset}");
    let icon = if folder {
        format_icon_colored(NerdFont::Folder, colors::MAUVE)
    } else if full_path.ends_with(".otp") {
        format_icon_colored(NerdFont::Clock, colors::TEAL)
    } else {
        format_icon_colored(NerdFont::Key, colors::GREEN)
    };
    let label = if folder {
        format!("{bold}{key_color}{label}{reset}")
    } else {
        label.to_string()
    };
    format!("{tree} {icon}{label}")
}

pub(super) fn path_prefix(path: &[String]) -> Option<String> {
    if path.is_empty() {
        None
    } else {
        Some(path.join("/"))
    }
}

pub(super) fn path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}
