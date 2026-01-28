//! Settings menu UI
//!
//! Main entry point for the interactive settings menu system.

use anyhow::{Context, Result};

use crate::menu_utils::MenuCursor;
use crate::settings::category_tree::category_tree;
use crate::settings::setting::Category;

use super::super::commands::SettingsNavigation;
use super::super::context::SettingsContext;
use super::items::{
    MainMenuItem, MenuItem, TreeNode, TreeSearchItem, build_tree_search_items,
    convert_category_tree,
};
use crate::menu_utils::select_one_with_style_at;

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
            // Find the setting in the tree view and jump to it
            let tree_items = build_tree_search_items(&ctx);
            let target_index = tree_items.iter().position(|item| {
                matches!(item, TreeSearchItem::Setting { setting, .. } if setting.metadata().id == setting_id)
            });

            if target_index.is_none() {
                anyhow::bail!("Setting '{}' not found", setting_id);
            }

            InitialView::SearchAll(MenuCursor::with_index(target_index))
        }
        Some(SettingsNavigation::Category(category_id)) => {
            let category = Category::from_id(&category_id)
                .ok_or_else(|| anyhow::anyhow!("Category '{}' not found", category_id))?;
            InitialView::Category(category, MenuCursor::with_index(Some(0)))
        }
        Some(SettingsNavigation::Search) => InitialView::SearchAll(MenuCursor::new()),
        None => InitialView::MainMenu(MenuCursor::new()),
    };

    loop {
        match initial_view {
            InitialView::MainMenu(cursor) => match run_main_menu(&mut ctx, cursor)? {
                MenuAction::EnterCategory {
                    tree,
                    main_menu_cursor,
                    category_cursor,
                } => {
                    // If category has only one setting, activate it directly
                    if let TreeNode::Folder { children, .. } = &tree
                        && children.len() == 1
                        && let TreeNode::Setting(setting) = &children[0]
                    {
                        super::handlers::handle_trait_setting(&mut ctx, *setting)?;
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                        continue;
                    }
                    if navigate_tree(&mut ctx, &tree, None, category_cursor)? {
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                    } else {
                        break;
                    }
                }
                MenuAction::EnterSearch(main_menu_cursor) => {
                    if handle_search_all(&mut ctx, MenuCursor::new())? {
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                    } else {
                        break;
                    }
                }
                MenuAction::Exit => break,
            },
            InitialView::Category(category, cursor) => {
                let tree = convert_category_tree(category, category_tree(category));
                if navigate_tree(&mut ctx, &tree, None, cursor)? {
                    initial_view = InitialView::MainMenu(MenuCursor::new());
                } else {
                    break;
                }
            }
            InitialView::SearchAll(cursor) => {
                if handle_search_all(&mut ctx, cursor)? {
                    initial_view = InitialView::MainMenu(MenuCursor::with_index(Some(0)));
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
    MainMenu(MenuCursor),
    Category(Category, MenuCursor),
    SearchAll(MenuCursor),
}

enum MenuAction {
    EnterCategory {
        tree: TreeNode,
        main_menu_cursor: MenuCursor,
        category_cursor: MenuCursor,
    },
    EnterSearch(MenuCursor),
    Exit,
}

/// Get all category trees
fn build_category_trees() -> Vec<TreeNode> {
    Category::all()
        .iter()
        .filter_map(|&cat| {
            let nodes = category_tree(cat);
            if nodes.is_empty() {
                None
            } else {
                Some(convert_category_tree(cat, nodes))
            }
        })
        .collect()
}

fn run_main_menu(_ctx: &mut SettingsContext, mut cursor: MenuCursor) -> Result<MenuAction> {
    let category_trees = build_category_trees();

    if category_trees.is_empty() {
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

    let mut menu_items = Vec::with_capacity(category_trees.len() + 2);
    menu_items.push(MainMenuItem::SearchAll);

    for tree in category_trees {
        menu_items.push(MainMenuItem::Category(tree));
    }
    menu_items.push(MainMenuItem::Close);

    let initial_cursor = cursor.initial_index(&menu_items);
    let selection = select_one_with_style_at(menu_items.clone(), initial_cursor)?;

    let action = match selection {
        Some(MainMenuItem::SearchAll) => {
            cursor.update(&MainMenuItem::SearchAll, &menu_items);
            MenuAction::EnterSearch(cursor)
        }
        Some(MainMenuItem::Category(tree)) => {
            cursor.update(&MainMenuItem::Category(tree.clone()), &menu_items);
            MenuAction::EnterCategory {
                tree,
                main_menu_cursor: cursor,
                category_cursor: MenuCursor::new(),
            }
        }
        Some(MainMenuItem::Close) | None => MenuAction::Exit,
    };

    Ok(action)
}

/// Navigate a tree node (folder)
fn navigate_tree(
    ctx: &mut SettingsContext,
    node: &TreeNode,
    parent_name: Option<&str>,
    mut cursor: MenuCursor,
) -> Result<bool> {
    let (title, children) = match node {
        TreeNode::Folder { meta, children } => (&meta.title, children),
        TreeNode::Setting(_) => return Ok(true), // Settings are handled directly
    };

    if children.is_empty() {
        ctx.emit_info(
            "settings.category.empty",
            &format!("No settings available in {} yet.", title),
        );
        return Ok(true);
    }

    loop {
        // Build menu items from children
        let mut entries: Vec<MenuItem> = children
            .iter()
            .map(|child| match child {
                TreeNode::Folder { .. } => MenuItem::Folder(child.clone()),
                TreeNode::Setting(s) => {
                    let state = s.get_display_state(ctx);
                    MenuItem::Setting { setting: *s, state }
                }
            })
            .collect();

        entries.push(MenuItem::Back {
            parent_name: parent_name.map(|s| s.to_string()),
        });

        let initial_cursor = cursor.initial_index(&entries);
        match select_one_with_style_at(entries.clone(), initial_cursor)? {
            Some(MenuItem::Folder(folder)) => {
                cursor.update(&MenuItem::Folder(folder.clone()), &entries);
                // Recurse into folder, current title becomes parent
                if !navigate_tree(ctx, &folder, Some(title), MenuCursor::new())? {
                    return Ok(false); // Propagate exit
                }
            }
            Some(MenuItem::Setting { setting, .. }) => {
                cursor.update(
                    &MenuItem::Setting {
                        setting,
                        state: setting.get_display_state(ctx),
                    },
                    &entries,
                );
                super::handlers::handle_trait_setting(ctx, setting)?;
            }
            Some(MenuItem::Back { .. }) => {
                cursor.update(
                    &MenuItem::Back {
                        parent_name: parent_name.map(|s| s.to_string()),
                    },
                    &entries,
                );
                return Ok(true);
            }
            None => return Ok(true),
        }
    }
}

pub fn handle_search_all(ctx: &mut SettingsContext, mut cursor: MenuCursor) -> Result<bool> {
    use crate::menu_utils::{FzfResult, FzfWrapper, Header};
    use crate::ui::catppuccin::{colors, fzf_mocha_args, hex_to_ansi_fg};

    loop {
        let items = build_tree_search_items(ctx);

        if items.is_empty() {
            ctx.emit_info("settings.search.empty", "No settings found to search.");
            return Ok(true);
        }

        let title_color = hex_to_ansi_fg(colors::MAUVE);
        let tip_color = hex_to_ansi_fg(colors::SUBTEXT0);
        let reset = "\x1b[0m";
        let header = format!(
            "{title_color}All Settings{reset}\n{tip_color}Browse all settings organized by category{reset}"
        );

        let prompt = format!(
            "{} Search",
            char::from(crate::ui::nerd_font::NerdFont::Search)
        );

        let initial_cursor = cursor.initial_index(&items);
        let result = FzfWrapper::builder()
            .prompt(prompt)
            .header(Header::fancy(&header))
            .args(fzf_mocha_args())
            .args(["--no-sort"])
            .initial_index(initial_cursor.unwrap_or(0))
            .responsive_layout()
            .select(items.clone())?;

        match result {
            FzfResult::Selected(selection) => {
                cursor.update(&selection, &items);
                match &selection {
                    TreeSearchItem::Setting { setting, .. } => {
                        super::handlers::handle_trait_setting(ctx, *setting)?;
                    }
                    TreeSearchItem::Category { category, .. } => {
                        let tree = convert_category_tree(*category, category_tree(*category));
                        navigate_tree(ctx, &tree, None, MenuCursor::new())?;
                    }
                    TreeSearchItem::Folder { .. } => {
                        // Folders are visual only in the tree view
                    }
                }
            }
            FzfResult::Cancelled => return Ok(true),
            FzfResult::Error(err) => {
                ctx.emit_info("settings.search.error", &format!("Search error: {err}"));
                return Ok(true);
            }
            _ => return Ok(true),
        }
    }
}
