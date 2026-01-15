//! Settings menu UI
//!
//! Main entry point for the interactive settings menu system.

use anyhow::{Context, Result};

use crate::settings::setting::{Category, Setting};

use super::super::commands::SettingsNavigation;
use super::super::context::SettingsContext;
use super::items::{CategoryItem, CategoryMenuItem, CategoryPageItem, SearchItem, SettingItem};
use crate::ui::catppuccin::select_one_with_style_at;

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
                    let tree = build_tree(category);
                    if tree.len() == 1
                        && let TreeNode::Leaf(setting) = &tree[0]
                    {
                        super::handlers::handle_trait_setting(&mut ctx, *setting)?;
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                        continue;
                    }
                    if navigate_node(&mut ctx, category.meta().title, &tree, category_cursor)? {
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                    } else {
                        break;
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
                let tree = build_tree(category);
                if navigate_node(&mut ctx, category.meta().title, &tree, cursor)? {
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

fn build_tree(category: Category) -> Vec<TreeNode> {
    use crate::settings::category_tree::category_tree;

    let category_nodes = category_tree(category);
    let mut nodes = Vec::new();

    for node in category_nodes {
        nodes.push(convert_category_node(node));
    }

    nodes
}

fn convert_category_node(node: crate::settings::category_tree::CategoryNode) -> TreeNode {
    if let Some(setting) = node.setting {
        TreeNode::Leaf(setting)
    } else {
        let converted_children = node
            .children
            .into_iter()
            .map(convert_category_node)
            .collect();
        TreeNode::Folder {
            name: node.name.unwrap_or("Unknown").to_string(),
            children: converted_children,
        }
    }
}

fn settings_by_category() -> Vec<(Category, Vec<&'static dyn Setting>)> {
    use crate::settings::category_tree::category_tree;

    Category::all()
        .iter()
        .map(|&cat| {
            let tree = category_tree(cat);
            let settings = collect_settings_from_tree(&tree);
            (cat, settings)
        })
        .filter(|(_, settings)| !settings.is_empty())
        .collect()
}

fn collect_settings_from_tree(
    nodes: &[crate::settings::category_tree::CategoryNode],
) -> Vec<&'static dyn Setting> {
    let mut settings = Vec::new();
    for node in nodes {
        if let Some(setting) = node.setting {
            settings.push(setting);
        }
        settings.extend(collect_settings_from_tree(&node.children));
    }
    settings
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
                    let state = s.get_display_state(ctx);
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
                let state = setting.get_display_state(ctx);
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
