use anyhow::{Context, Result};

use crate::settings::registry::{self, CATEGORIES, SettingKind};

use super::super::commands::SettingsNavigation;
use super::super::context::{SettingsContext, select_one_with_style_at};
use super::items::{CategoryItem, CategoryMenuItem, CategoryPageItem, SearchItem, SettingItem};

fn menu_item_index(items: &[CategoryMenuItem], selected: CategoryMenuItem) -> Option<usize> {
    items
        .iter()
        .enumerate()
        .find_map(|(idx, item)| match (item, selected) {
            (CategoryMenuItem::SearchAll, CategoryMenuItem::SearchAll) => Some(idx),
            (CategoryMenuItem::Category(lhs), CategoryMenuItem::Category(rhs))
                if lhs.category.id == rhs.category.id =>
            {
                Some(idx)
            }
            _ => None,
        })
}

fn category_page_index(items: &[CategoryPageItem], selected: CategoryPageItem) -> Option<usize> {
    items
        .iter()
        .enumerate()
        .find_map(|(idx, item)| match (item, selected) {
            (CategoryPageItem::Back, CategoryPageItem::Back) => Some(idx),
            (CategoryPageItem::Setting(lhs), CategoryPageItem::Setting(rhs))
                if lhs.definition.id == rhs.definition.id =>
            {
                Some(idx)
            }
            _ => None,
        })
}

fn search_item_index(items: &[SearchItem], selected: SearchItem) -> Option<usize> {
    items.iter().enumerate().find_map(|(idx, item)| {
        if item.definition.id == selected.definition.id && item.category.id == selected.category.id
        {
            Some(idx)
        } else {
            None
        }
    })
}

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
            // Find the setting and determine its index in the search view
            let mut target_index = None;
            let mut found = false;

            let mut idx = 0;
            for category in CATEGORIES {
                let definitions = registry::settings_for_category(category.id);
                for definition in definitions {
                    if definition.id == setting_id {
                        target_index = Some(idx);
                        found = true;
                        break;
                    }
                    idx += 1;
                }
                if found {
                    break;
                }
            }

            if !found {
                anyhow::bail!("Setting '{}' not found", setting_id);
            }

            InitialView::SearchAll(target_index)
        }
        Some(SettingsNavigation::Category(category_id)) => {
            let category = registry::category_by_id(&category_id)
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
                    // Enter category and handle navigation
                    if handle_category(&mut ctx, category, category_cursor)? {
                        // User selected Back or Esc, return to main menu at previous position
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                    } else {
                        // User exited (shouldn't happen with Back option, but handle it)
                        break;
                    }
                }
                MenuAction::EnterSearch(main_menu_cursor) => {
                    // Enter search and handle navigation
                    if handle_search_all(&mut ctx, None)? {
                        // User exited search, return to main menu at previous position
                        initial_view = InitialView::MainMenu(main_menu_cursor);
                    } else {
                        break;
                    }
                }
                MenuAction::Exit => break,
            },
            InitialView::Category(category, cursor) => {
                // Handle direct navigation to category (e.g., via URL/command)
                if handle_category(&mut ctx, category, cursor)? {
                    // User selected Back or Esc, return to main menu
                    initial_view = InitialView::MainMenu(None);
                } else {
                    break;
                }
            }
            InitialView::SearchAll(cursor) => {
                // Handle direct navigation to search (e.g., via URL/command)
                if handle_search_all(&mut ctx, cursor)? {
                    // User exited search, return to main menu at "Search All"
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

/// Initial view state for the settings UI
enum InitialView {
    MainMenu(Option<usize>),
    Category(&'static registry::SettingCategory, Option<usize>),
    SearchAll(Option<usize>),
}

/// Action to take after a menu interaction
enum MenuAction {
    /// Enter a category
    /// First Option<usize> is the main menu cursor position (for back navigation)
    /// Second Option<usize> is the initial cursor in the category page
    EnterCategory {
        category: &'static registry::SettingCategory,
        main_menu_cursor: Option<usize>,
        category_cursor: Option<usize>,
    },
    /// Enter search view
    /// Option<usize> is the main menu cursor position (for back navigation)
    EnterSearch(Option<usize>),
    Exit,
}

/// Run the main category selection menu
fn run_main_menu(ctx: &mut SettingsContext, initial_cursor: Option<usize>) -> Result<MenuAction> {
    let mut menu_items = Vec::with_capacity(CATEGORIES.len() + 1);
    menu_items.push(CategoryMenuItem::SearchAll);

    let mut total_settings = 0usize;
    for category in CATEGORIES {
        let definitions = registry::settings_for_category(category.id);
        total_settings += definitions.len();

        let mut toggles = 0usize;
        let mut choices = 0usize;
        let mut actions = 0usize;
        let mut commands = 0usize;
        let mut highlights = [None, None, None];

        for (idx, definition) in definitions.iter().enumerate() {
            match definition.kind {
                SettingKind::Toggle { .. } => toggles += 1,
                SettingKind::Choice { .. } => choices += 1,
                SettingKind::Action { .. } => actions += 1,
                SettingKind::Command { .. } => commands += 1,
            }

            if idx < highlights.len() {
                highlights[idx] = Some(*definition);
            }
        }

        menu_items.push(CategoryMenuItem::Category(CategoryItem {
            category,
            total: definitions.len(),
            toggles,
            choices,
            actions,
            commands,
            highlights,
        }));
    }

    if total_settings == 0 {
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

    let selection = select_one_with_style_at(menu_items.clone(), initial_cursor)?;
    let selected_index = selection.and_then(|item| menu_item_index(&menu_items, item));

    let action = match selection {
        Some(CategoryMenuItem::SearchAll) => MenuAction::EnterSearch(selected_index),
        Some(CategoryMenuItem::Category(item)) => {
            // Save main menu position for back navigation, start category at first item
            MenuAction::EnterCategory {
                category: item.category,
                main_menu_cursor: selected_index,
                category_cursor: None,
            }
        }
        None => MenuAction::Exit,
    };

    Ok(action)
}

pub fn handle_category(
    ctx: &mut SettingsContext,
    category: &'static registry::SettingCategory,
    initial_cursor: Option<usize>,
) -> Result<bool> {
    let setting_defs = registry::settings_for_category(category.id);
    if setting_defs.is_empty() {
        ctx.emit_info(
            "settings.category.empty",
            &format!("No settings available for {} yet.", category.title),
        );
        return Ok(true);
    }

    let mut cursor = initial_cursor;

    loop {
        let mut entries: Vec<CategoryPageItem> = Vec::with_capacity(setting_defs.len() + 1);
        for &definition in &setting_defs {
            let state = super::state::compute_setting_state(ctx, definition);
            entries.push(CategoryPageItem::Setting(SettingItem { definition, state }));
        }

        entries.push(CategoryPageItem::Back);

        match select_one_with_style_at(entries.clone(), cursor)? {
            Some(CategoryPageItem::Setting(item)) => {
                if let Some(index) = category_page_index(&entries, CategoryPageItem::Setting(item))
                {
                    cursor = Some(index);
                }
                super::handlers::handle_setting(ctx, item.definition, item.state)?;
                ctx.persist()?;
            }
            Some(CategoryPageItem::Back) | None => return Ok(true),
        }
    }
}

pub fn handle_search_all(ctx: &mut SettingsContext, initial_cursor: Option<usize>) -> Result<bool> {
    let mut cursor = initial_cursor;

    loop {
        let mut items = Vec::new();

        for category in CATEGORIES {
            let definitions = registry::settings_for_category(category.id);
            for definition in definitions {
                let state = super::state::compute_setting_state(ctx, definition);
                items.push(SearchItem {
                    category,
                    definition,
                    state,
                });
            }
        }

        if items.is_empty() {
            ctx.emit_info("settings.search.empty", "No settings found to search.");
            return Ok(true);
        }

        match select_one_with_style_at(items.clone(), cursor)? {
            Some(selection) => {
                if let Some(index) = search_item_index(&items, selection) {
                    cursor = Some(index);
                }
                super::handlers::handle_setting(ctx, selection.definition, selection.state)?;
                ctx.persist()?;
            }
            None => return Ok(true),
        }
    }
}
