use anyhow::Result;

use crate::menu_utils::{
    FzfWrapper, HeaderBuilder, MenuCursor, MenuKey, MenuKeybind, MenuSelection, MenuSort,
    choose_sort, sort_keybind,
};
use crate::ui::nerd_font::NerdFont;

use super::super::browser::build_quick_access_items;
use super::super::dependencies::ensure_core_dependencies;
use super::super::sort::{PassSort, sort_entries};
use super::super::store::{ensure_password_store_dir, load_entries};
use super::super::types::{BrowserItemKind, BrowserMenuItem};
use super::tree_view::{add_prefix_for_selection, run_tree_view};
use super::{copy_entry, show_entry_actions};

/// Actions bound to global keys on the quick access menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickAccessBind {
    /// Open the hierarchical tree view.
    TreeView,
    /// Open the action menu for the focused entry.
    EntryActions,
    /// Add a new entry next to the focused row.
    Add,
    /// Change the order of the entry list.
    Sort,
}

enum QuickAccessIntent {
    Primary(BrowserMenuItem),
    TreeView(Option<BrowserMenuItem>),
    EntryActions(Option<BrowserMenuItem>),
    Add(Option<BrowserMenuItem>),
    Sort(Option<BrowserMenuItem>),
}

fn keybinds() -> Result<Vec<MenuKeybind<QuickAccessBind>>> {
    Ok(vec![
        MenuKeybind::new(
            MenuKey::new("ctrl-t")?,
            "tree view",
            QuickAccessBind::TreeView,
        ),
        MenuKeybind::new(
            MenuKey::new("ctrl-o")?,
            "entry actions",
            QuickAccessBind::EntryActions,
        ),
        MenuKeybind::new(MenuKey::new("ctrl-a")?, "add entry", QuickAccessBind::Add),
        sort_keybind()?.map_action(QuickAccessBind::Sort),
    ])
}

fn interpret(
    selection: MenuSelection<BrowserMenuItem, QuickAccessBind>,
) -> Option<QuickAccessIntent> {
    match selection.action {
        Some(QuickAccessBind::TreeView) => Some(QuickAccessIntent::TreeView(
            selection.items.first().cloned(),
        )),
        Some(QuickAccessBind::EntryActions) => Some(QuickAccessIntent::EntryActions(
            selection.items.first().cloned(),
        )),
        Some(QuickAccessBind::Add) => {
            Some(QuickAccessIntent::Add(selection.items.first().cloned()))
        }
        Some(QuickAccessBind::Sort) => {
            Some(QuickAccessIntent::Sort(selection.items.first().cloned()))
        }
        None => selection.into_single().ok().map(QuickAccessIntent::Primary),
    }
}

pub(crate) fn run() -> Result<i32> {
    ensure_core_dependencies()?;

    let store_dir = ensure_password_store_dir()?;
    let mut cursor = MenuCursor::new();
    let mut sort = PassSort::quick_access_default();

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries(&mut entries, sort)?;
        let quick_access_items = build_quick_access_items(&entries);

        let mut builder = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::Key, "Pass")
                    .field("Sort", sort.label())
                    .build(),
            )
            .prompt("Copy")
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&quick_access_items) {
            builder = builder.initial_index(index);
        }

        let keybinds = keybinds()?;

        match builder
            .items(quick_access_items.clone())
            .keybinds(&keybinds)
            .select()?
        {
            crate::menu_utils::DialogOutcome::Submitted(sel) => {
                // Ctrl-T, Ctrl-O, Ctrl-A, and Ctrl-S are mode switches even
                // when the filter matches nothing; a plain submit still needs
                // a focused row.
                let Some(intent) = interpret(sel) else {
                    continue;
                };

                let item = match intent {
                    QuickAccessIntent::TreeView(item) => {
                        let focus = item.as_ref().and_then(|item| match &item.kind {
                            BrowserItemKind::Entry { key, .. } => Some(key.as_str()),
                            _ => None,
                        });
                        run_tree_view(None, focus)?;
                        continue;
                    }
                    QuickAccessIntent::EntryActions(item) => {
                        let Some(item) = item else {
                            continue;
                        };
                        let BrowserItemKind::Entry { key, .. } = &item.kind else {
                            continue;
                        };
                        cursor.update(&item, &quick_access_items);
                        if let Some(moved_key) = show_entry_actions(&entries, key)? {
                            cursor.update_from_key(&format!("entry:{moved_key}"));
                        }
                        continue;
                    }
                    QuickAccessIntent::Add(item) => {
                        if let Some(item) = &item {
                            cursor.update(item, &quick_access_items);
                        }
                        let prefix = add_prefix_for_selection(item.as_ref(), &[]);
                        super::run_add_menu(prefix.as_deref())?;
                        continue;
                    }
                    QuickAccessIntent::Sort(item) => {
                        if let Some(item) = item {
                            cursor.update(&item, &quick_access_items);
                        }
                        if let Some(chosen) = choose_sort(sort)? {
                            sort = chosen;
                        }
                        continue;
                    }
                    QuickAccessIntent::Primary(item) => item,
                };

                cursor.update(&item, &quick_access_items);
                match item.kind {
                    BrowserItemKind::Entry { key, .. } => {
                        copy_entry(&entries, &key)?;
                        // Exit after copying so the user can paste immediately
                        // (OTP codes expire; reopening is one keybinding press).
                        return Ok(0);
                    }
                    BrowserItemKind::Menu => {
                        run_tree_view(None, None)?;
                    }
                    BrowserItemKind::Close => return Ok(0),
                    BrowserItemKind::Folder(_) | BrowserItemKind::Add | BrowserItemKind::Back => {}
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::types::EntryKind;

    fn entry_item(name: &str) -> BrowserMenuItem {
        BrowserMenuItem {
            key: format!("entry:{name}"),
            display: name.to_string(),
            preview: crate::ui::preview::FzfPreview::None,
            kind: BrowserItemKind::Entry {
                key: name.to_string(),
                kind: EntryKind::Password,
            },
        }
    }

    #[test]
    fn enter_keeps_the_primary_copy_action() {
        let item = entry_item("mail/work");
        let interpreted = interpret(MenuSelection {
            items: vec![item.clone()],
            action: None,
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::Primary(selected)) if selected.key == item.key
        ));
    }

    #[test]
    fn tree_view_keybind_carries_the_focused_entry() {
        let item = entry_item("mail/work");
        let interpreted = interpret(MenuSelection {
            items: vec![item.clone()],
            action: Some(QuickAccessBind::TreeView),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::TreeView(Some(selected))) if selected.key == item.key
        ));
    }

    #[test]
    fn tree_view_keybind_opens_without_a_match() {
        let interpreted = interpret(MenuSelection::<BrowserMenuItem, _> {
            items: Vec::new(),
            action: Some(QuickAccessBind::TreeView),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::TreeView(None))
        ));
    }

    #[test]
    fn entry_actions_keybind_carries_the_focused_entry() {
        let item = entry_item("mail/work");
        let interpreted = interpret(MenuSelection {
            items: vec![item.clone()],
            action: Some(QuickAccessBind::EntryActions),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::EntryActions(Some(selected))) if selected.key == item.key
        ));
    }

    #[test]
    fn add_keybind_carries_the_focused_entry() {
        let item = entry_item("mail/work");
        let interpreted = interpret(MenuSelection {
            items: vec![item.clone()],
            action: Some(QuickAccessBind::Add),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::Add(Some(selected))) if selected.key == item.key
        ));
    }

    #[test]
    fn sort_keybind_carries_the_focused_entry() {
        let item = entry_item("mail/work");
        let interpreted = interpret(MenuSelection {
            items: vec![item.clone()],
            action: Some(QuickAccessBind::Sort),
        });

        assert!(matches!(
            interpreted,
            Some(QuickAccessIntent::Sort(Some(selected))) if selected.key == item.key
        ));
    }

    #[test]
    fn sort_keybind_without_a_match_still_opens_the_chooser() {
        let interpreted = interpret(MenuSelection::<BrowserMenuItem, _> {
            items: Vec::new(),
            action: Some(QuickAccessBind::Sort),
        });

        assert!(matches!(interpreted, Some(QuickAccessIntent::Sort(None))));
    }

    #[test]
    fn tree_view_is_bound_to_ctrl_t() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-t")
            .expect("quick access should bind ctrl-t");

        assert_eq!(bind.label, "tree view");
        assert_eq!(bind.action, QuickAccessBind::TreeView);
    }

    #[test]
    fn entry_actions_are_bound_to_ctrl_o() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-o")
            .expect("quick access should bind ctrl-o");

        assert_eq!(bind.label, "entry actions");
        assert_eq!(bind.action, QuickAccessBind::EntryActions);
    }

    #[test]
    fn add_is_bound_to_ctrl_a_in_quick_access() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-a")
            .expect("quick access should bind ctrl-a");

        assert_eq!(bind.label, "add entry");
        assert_eq!(bind.action, QuickAccessBind::Add);
    }

    #[test]
    fn sort_is_bound_to_ctrl_s_in_quick_access() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-s")
            .expect("quick access should bind ctrl-s");

        assert_eq!(bind.label, "sort");
        assert_eq!(bind.action, QuickAccessBind::Sort);
    }
}
