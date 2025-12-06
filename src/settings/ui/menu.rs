//! Settings menu UI
//!
//! Main entry point for the interactive settings menu system.

use anyhow::{Context, Result};

use crate::settings::setting::{self, Category, Setting};

use super::super::commands::SettingsNavigation;
use super::super::context::{SettingsContext, select_one_with_style_at};
use super::items::{CategoryItem, CategoryMenuItem, CategoryPageItem, SearchItem, SettingItem};

pub fn run_settings_ui(
    debug: bool,
    privileged_flag: bool,
    navigation: Option<SettingsNavigation>,
) -> Result<()> {
    let store = super::super::store::SettingsStore::load().context("loading settings file")?;
    let mut ctx = SettingsContext::new(store, debug, privileged_flag);

    // Determine initial state based on navigation
    let mut initial_view = match navigation {
        Some(SettingsNavigation::Setting(setting_id)) => {
            // Find the setting and jump to search view with it selected
            let mut target_index = None;
            let all = settings_by_category();
            let mut idx = 0;
            for (_cat, settings) in &all {
                for s in settings {
                    if s.metadata().id == setting_id {
                        target_index = Some(idx);
                        break;
                    }
                    idx += 1;
                }
                if target_index.is_some() {
                    break;
                }
            }

            if target_index.is_none() {
                anyhow::bail!("Setting '{}' not found", setting_id);
            }

            InitialView::SearchAll(target_index)
        }
        Some(SettingsNavigation::Category(category_id)) => {
            let category = Category::from_id(&category_id)
                .ok_or_else(|| anyhow::anyhow!("Category '{}' not found", category_id))?;
            InitialView::Category(category, Some(0))
        }
        Some(SettingsNavigation::Search) => InitialView::SearchAll(None),
        None => InitialView::MainMenu(None),
    };

    loop {
        match initial_view {
            InitialView::MainMenu(cursor) => match run_main_menu(&mut ctx, cursor)? {
                MenuAction::EnterCategory {
                    category,
                    main_menu_cursor,
                    category_cursor,
                } => {
                    let settings = setting::settings_in_category(category);
                    if settings.len() == 1 {
                        super::handlers::handle_trait_setting(&mut ctx, settings[0])?;
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                    } else {
                        let tree = build_tree(settings);
                        if navigate_node(&mut ctx, category.title(), &tree, category_cursor)? {
                            initial_view = InitialView::MainMenu(main_menu_cursor);
                        } else {
                            break;
                        }
                    }
                }
                MenuAction::EnterSearch(main_menu_cursor) => {
                    if handle_search_all(&mut ctx, None)? {
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                    } else {
                        break;
                    }
                }
                MenuAction::Exit => break,
            },
            InitialView::Category(category, cursor) => {
                let settings = setting::settings_in_category(category);
                let tree = build_tree(settings);
                if navigate_node(&mut ctx, category.title(), &tree, cursor)? {
                    initial_view = InitialView::MainMenu(None);
                } else {
                    break;
                }
            }
            InitialView::SearchAll(cursor) => {
                if handle_search_all(&mut ctx, cursor)? {
                    initial_view = InitialView::MainMenu(Some(0));
                } else {
                    break;
                }
            }
        }
    }

    ctx.persist()?;
    Ok(())
}

enum InitialView {
    MainMenu(Option<usize>),
    Category(Category, Option<usize>),
    SearchAll(Option<usize>),
}

enum MenuAction {
    EnterCategory {
        category: Category,
        main_menu_cursor: Option<usize>,
        category_cursor: Option<usize>,
    },
    EnterSearch(Option<usize>),
    Exit,
}

#[derive(Clone)]
enum TreeNode {
    Folder {
        name: String,
        children: Vec<TreeNode>,
    },
    Leaf(&'static dyn Setting),
}

impl TreeNode {
    fn name(&self) -> &str {
        match self {
            TreeNode::Folder { name, .. } => name,
            TreeNode::Leaf(s) => s.metadata().title,
        }
    }
}

fn build_tree(settings: Vec<&'static dyn Setting>) -> Vec<TreeNode> {
    let mut nodes = Vec::new();
    for setting in settings {
        insert_into_tree(&mut nodes, &setting.metadata().breadcrumbs, setting);
    }

    // Optimize: Flatten single-child folders
    let nodes = optimize_tree(nodes);

    // Sort nodes: Folders first, then Leaves. Alphabetical within groups.
    // Note: We need to sort *after* optimization because flattening might change types
    let mut nodes = nodes;
    sort_tree(&mut nodes);

    nodes
}

fn optimize_tree(nodes: Vec<TreeNode>) -> Vec<TreeNode> {
    nodes
        .into_iter()
        .map(|node| {
            match node {
                TreeNode::Folder { name, children } => {
                    let optimized_children = optimize_tree(children);
                    if optimized_children.len() == 1 {
                        // Hoist the single child
                        optimized_children.into_iter().next().unwrap()
                    } else {
                        TreeNode::Folder {
                            name,
                            children: optimized_children,
                        }
                    }
                }
                leaf @ TreeNode::Leaf(_) => leaf,
            }
        })
        .collect()
}

fn insert_into_tree(nodes: &mut Vec<TreeNode>, path: &[&str], setting: &'static dyn Setting) {
    if path.is_empty() {
        nodes.push(TreeNode::Leaf(setting));
        return;
    }

    let current_part = path[0];
    let remaining_path = &path[1..];

    // Find existing folder
    let mut found_idx = None;
    for (idx, node) in nodes.iter().enumerate() {
        if let TreeNode::Folder { name, .. } = node {
            if name == current_part {
                found_idx = Some(idx);
                break;
            }
        }
    }

    match found_idx {
        Some(idx) => {
            if let TreeNode::Folder { children, .. } = &mut nodes[idx] {
                insert_into_tree(children, remaining_path, setting);
            }
        }
        None => {
            let mut children = Vec::new();
            insert_into_tree(&mut children, remaining_path, setting);
            nodes.push(TreeNode::Folder {
                name: current_part.to_string(),
                children,
            });
        }
    }
}

fn sort_tree(nodes: &mut Vec<TreeNode>) {
    nodes.sort_by(|a, b| match (a, b) {
        (TreeNode::Folder { name: a_name, .. }, TreeNode::Folder { name: b_name, .. }) => {
            a_name.cmp(b_name)
        }
        (TreeNode::Folder { .. }, TreeNode::Leaf(_)) => std::cmp::Ordering::Less,
        (TreeNode::Leaf(_), TreeNode::Folder { .. }) => std::cmp::Ordering::Greater,
        (TreeNode::Leaf(a_s), TreeNode::Leaf(b_s)) => {
            a_s.metadata().title.cmp(b_s.metadata().title)
        }
    });

    for node in nodes {
        if let TreeNode::Folder { children, .. } = node {
            sort_tree(children);
        }
    }
}

fn settings_by_category() -> Vec<(Category, Vec<&'static dyn Setting>)> {
    Category::all()
        .iter()
        .map(|&cat| {
            let settings = setting::settings_in_category(cat);
            (cat, settings)
        })
        .filter(|(_, settings)| !settings.is_empty())
        .collect()
}

fn run_main_menu(_ctx: &mut SettingsContext, initial_cursor: Option<usize>) -> Result<MenuAction> {
    let categories_with_settings = settings_by_category();

    if categories_with_settings.is_empty() {
        crate::ui::prelude::emit(
            crate::ui::prelude::Level::Warn,
            "settings.empty",
            &format!(
                "{} No settings registered yet.",
                char::from(crate::ui::prelude::NerdFont::Warning)
            ),
            None,
        );
        return Ok(MenuAction::Exit);
    }

    let mut menu_items = Vec::with_capacity(categories_with_settings.len() + 2);
    menu_items.push(CategoryMenuItem::SearchAll);

    for (category, settings) in categories_with_settings {
        menu_items.push(CategoryMenuItem::Category(CategoryItem::new(
            category, settings,
        )));
    }
    menu_items.push(CategoryMenuItem::Close);

    let selection = select_one_with_style_at(menu_items.clone(), initial_cursor)?;
    let selected_index = selection.as_ref().and_then(|item| {
        menu_items.iter().position(|i| match (i, item) {
            (CategoryMenuItem::SearchAll, CategoryMenuItem::SearchAll) => true,
            (CategoryMenuItem::Category(a), CategoryMenuItem::Category(b)) => {
                a.category == b.category
            }
            (CategoryMenuItem::Close, CategoryMenuItem::Close) => true,
            _ => false,
        })
    });

    let action = match selection {
        Some(CategoryMenuItem::SearchAll) => MenuAction::EnterSearch(selected_index),
        Some(CategoryMenuItem::Category(item)) => MenuAction::EnterCategory {
            category: item.category,
            main_menu_cursor: selected_index,
            category_cursor: None,
        },
        Some(CategoryMenuItem::Close) | None => MenuAction::Exit,
    };

    Ok(action)
}

fn navigate_node(
    ctx: &mut SettingsContext,
    title: &str,
    nodes: &[TreeNode],
    initial_cursor: Option<usize>,
) -> Result<bool> {
    if nodes.is_empty() {
        ctx.emit_info(
            "settings.category.empty",
            &format!("No settings available in {} yet.", title),
        );
        return Ok(true);
    }

    let mut cursor = initial_cursor;

    loop {
        use super::items::SubCategoryItem;

        // Re-compute display items on every loop iteration to reflect state changes
        let mut entries: Vec<CategoryPageItem> = nodes
            .iter()
            .map(|node| match node {
                TreeNode::Folder { name, children } => {
                    CategoryPageItem::SubCategory(SubCategoryItem {
                        name: name.clone(),
                        count: children.len(),
                    })
                }
                TreeNode::Leaf(s) => {
                    let state = super::state::compute_setting_state(ctx, *s);
                    CategoryPageItem::Setting(SettingItem { setting: *s, state })
                }
            })
            .collect();

        entries.push(CategoryPageItem::Back);

        // Display selection menu
        match select_one_with_style_at(entries.clone(), cursor)? {
            Some(CategoryPageItem::SubCategory(sub)) => {
                // Find the node corresponding to the selection
                if let Some(idx) = nodes.iter().position(|n| n.name() == sub.name) {
                    cursor = Some(idx); // Keep cursor on the folder when returning
                    if let TreeNode::Folder { name, children } = &nodes[idx] {
                        // Recurse into subfolder
                        if !navigate_node(ctx, name, children, None)? {
                            return Ok(false); // Propagate exit
                        }
                    }
                }
            }
            Some(CategoryPageItem::Setting(item)) => {
                // Find index to preserve cursor position
                if let Some(idx) = nodes
                    .iter()
                    .position(|n| n.name() == item.setting.metadata().title)
                {
                    cursor = Some(idx);
                }
                super::handlers::handle_trait_setting(ctx, item.setting)?;
            }
            Some(CategoryPageItem::Back) | None => return Ok(true),
        }
    }
}

pub fn handle_search_all(ctx: &mut SettingsContext, initial_cursor: Option<usize>) -> Result<bool> {
    let mut cursor = initial_cursor;

    loop {
        let mut items = Vec::new();

        for (_, settings) in settings_by_category() {
            for setting in settings {
                let state = super::state::compute_setting_state(ctx, setting);
                items.push(SearchItem { setting, state });
            }
        }

        if items.is_empty() {
            ctx.emit_info("settings.search.empty", "No settings found to search.");
            return Ok(true);
        }

        match select_one_with_style_at(items.clone(), cursor)? {
            Some(selection) => {
                cursor = items
                    .iter()
                    .position(|i| i.setting.metadata().id == selection.setting.metadata().id);
                super::handlers::handle_trait_setting(ctx, selection.setting)?;
            }
            None => return Ok(true),
        }
    }
}
