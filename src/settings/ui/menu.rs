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

    // Handle direct navigation
    if let Some(nav) = navigation {
        match nav {
            SettingsNavigation::Setting(setting_id) => {
                return handle_direct_setting(&mut ctx, &setting_id);
            }
            SettingsNavigation::Category(category_id) => {
                return handle_direct_category(&mut ctx, &category_id);
            }
            SettingsNavigation::Search => {
                return handle_search_all_persistent(&mut ctx);
            }
        }
    }

    let mut cursor: Option<usize> = None;

    loop {
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
                    char::from(crate::ui::prelude::Fa::ExclamationCircle)
                ),
                None,
            );
            break;
        }

        match select_one_with_style_at(menu_items.clone(), cursor)? {
            Some(selected) => {
                if let Some(index) = menu_item_index(&menu_items, selected) {
                    cursor = Some(index);
                }

                match selected {
                    CategoryMenuItem::SearchAll => {
                        if !handle_search_all(&mut ctx)? {
                            break;
                        }
                    }
                    CategoryMenuItem::Category(item) => {
                        if !handle_category(&mut ctx, item.category)? {
                            break;
                        }
                    }
                }
            }
            None => break,
        }
    }

    ctx.persist()?;
    Ok(())
}

/// Handle navigation directly to a specific setting
fn handle_direct_setting(ctx: &mut SettingsContext, setting_id: &str) -> Result<()> {
    // Find the setting by ID
    let setting = registry::SETTINGS
        .iter()
        .find(|s| s.id == setting_id)
        .ok_or_else(|| anyhow::anyhow!("Setting '{}' not found", setting_id))?;

    // Compute the state
    let state = super::state::compute_setting_state(ctx, setting);

    // Show the setting and allow interaction
    super::handlers::handle_setting(ctx, setting, state)?;
    ctx.persist()?;

    Ok(())
}

/// Handle navigation directly to a specific category
fn handle_direct_category(ctx: &mut SettingsContext, category_id: &str) -> Result<()> {
    let category = registry::category_by_id(category_id)
        .ok_or_else(|| anyhow::anyhow!("Category '{}' not found", category_id))?;

    // Enter the category and stay in it
    handle_category_persistent(ctx, category)?;
    ctx.persist()?;

    Ok(())
}

/// Handle search all in a persistent way (doesn't return to main menu)
fn handle_search_all_persistent(ctx: &mut SettingsContext) -> Result<()> {
    handle_search_all(ctx)?;
    ctx.persist()?;
    Ok(())
}

/// Handle category in a persistent way (for direct navigation)
fn handle_category_persistent(
    ctx: &mut SettingsContext,
    category: &'static registry::SettingCategory,
) -> Result<()> {
    let setting_defs = registry::settings_for_category(category.id);
    if setting_defs.is_empty() {
        ctx.emit_info(
            "settings.category.empty",
            &format!("No settings available for {} yet.", category.title),
        );
        return Ok(());
    }

    let mut cursor: Option<usize> = None;

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
            Some(CategoryPageItem::Back) | None => return Ok(()),
        }
    }
}

pub fn handle_category(
    ctx: &mut SettingsContext,
    category: &'static registry::SettingCategory,
) -> Result<bool> {
    let setting_defs = registry::settings_for_category(category.id);
    if setting_defs.is_empty() {
        ctx.emit_info(
            "settings.category.empty",
            &format!("No settings available for {} yet.", category.title),
        );
        return Ok(true);
    }

    let mut cursor: Option<usize> = None;

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

pub fn handle_search_all(ctx: &mut SettingsContext) -> Result<bool> {
    let mut cursor: Option<usize> = None;

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
