use anyhow::{Context, Result};

use crate::settings::registry::{self, CATEGORIES, SettingKind};

use super::super::context::{select_one_with_style, SettingsContext};
use super::items::{
    CategoryItem, CategoryMenuItem, CategoryPageItem, SearchItem, SettingItem,
};

pub fn run_settings_ui(debug: bool, privileged_flag: bool) -> Result<()> {
    let store = super::super::store::SettingsStore::load().context("loading settings file")?;
    let mut ctx = SettingsContext::new(store, debug, privileged_flag);

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

        match select_one_with_style(menu_items)? {
            Some(CategoryMenuItem::SearchAll) => {
                if !handle_search_all(&mut ctx)? {
                    break;
                }
            }
            Some(CategoryMenuItem::Category(item)) => {
                if !handle_category(&mut ctx, item.category)? {
                    break;
                }
            }
            None => break,
        }
    }

    ctx.persist()?;
    Ok(())
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

    loop {
        let mut entries: Vec<CategoryPageItem> = Vec::with_capacity(setting_defs.len() + 1);
        for &definition in &setting_defs {
            let state = super::state::compute_setting_state(ctx, definition);
            entries.push(CategoryPageItem::Setting(SettingItem { definition, state }));
        }

        entries.push(CategoryPageItem::Back);

        match select_one_with_style(entries)? {
            Some(CategoryPageItem::Setting(item)) => {
                super::handlers::handle_setting(ctx, item.definition, item.state)?;
                ctx.persist()?;
            }
            Some(CategoryPageItem::Back) | None => return Ok(true),
        }
    }
}

pub fn handle_search_all(ctx: &mut SettingsContext) -> Result<bool> {
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

        match select_one_with_style(items)? {
            Some(selection) => {
                super::handlers::handle_setting(ctx, selection.definition, selection.state)?;
                ctx.persist()?;
            }
            None => return Ok(true),
        }
    }
}
