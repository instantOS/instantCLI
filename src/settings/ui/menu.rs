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
                    if handle_category(&mut ctx, category, category_cursor)? {
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
                if handle_category(&mut ctx, category, cursor)? {
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

    let mut menu_items = Vec::with_capacity(categories_with_settings.len() + 1);
    menu_items.push(CategoryMenuItem::SearchAll);

    for (category, settings) in categories_with_settings {
        menu_items.push(CategoryMenuItem::Category(CategoryItem::new(
            category, settings,
        )));
    }

    let selection = select_one_with_style_at(menu_items.clone(), initial_cursor)?;
    let selected_index = selection.as_ref().and_then(|item| {
        menu_items.iter().position(|i| match (i, item) {
            (CategoryMenuItem::SearchAll, CategoryMenuItem::SearchAll) => true,
            (CategoryMenuItem::Category(a), CategoryMenuItem::Category(b)) => {
                a.category == b.category
            }
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
        None => MenuAction::Exit,
    };

    Ok(action)
}

pub fn handle_category(
    ctx: &mut SettingsContext,
    category: Category,
    initial_cursor: Option<usize>,
) -> Result<bool> {
    let settings = setting::settings_in_category(category);

    if settings.is_empty() {
        ctx.emit_info(
            "settings.category.empty",
            &format!("No settings available for {} yet.", category.title()),
        );
        return Ok(true);
    }

    if settings.len() == 1 {
        let setting = settings[0];
        super::handlers::handle_trait_setting(ctx, setting)?;
        return Ok(true);
    }

    let mut cursor = initial_cursor;

    loop {
        let settings = setting::settings_in_category(category);
        let mut entries: Vec<CategoryPageItem> = settings
            .iter()
            .map(|&s| {
                let state = super::state::compute_setting_state(ctx, s);
                CategoryPageItem::Setting(SettingItem { setting: s, state })
            })
            .collect();

        entries.push(CategoryPageItem::Back);

        match select_one_with_style_at(entries.clone(), cursor)? {
            Some(CategoryPageItem::Setting(item)) => {
                cursor = entries.iter().position(|e| match e {
                    CategoryPageItem::Setting(i) => {
                        i.setting.metadata().id == item.setting.metadata().id
                    }
                    _ => false,
                });
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
