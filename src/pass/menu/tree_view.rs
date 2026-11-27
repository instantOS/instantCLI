use anyhow::Result;

use crate::menu_utils::{
    FzfWrapper, HeaderBuilder, MenuCursor, MenuKey, MenuKeybind, MenuSort, choose_sort,
    sort_keybind,
};
use crate::ui::nerd_font::NerdFont;

use super::super::browser::{build_browser_menu_items, path_prefix, path_segments};
use super::super::operations::open_store_lazygit;
use super::super::sort::{PassSort, sort_entries};
use super::super::store::{ensure_password_store_dir, load_entries};
use super::super::types::{BrowserItemKind, BrowserMenuItem};
use super::{run_add_menu, show_entry_actions};

/// Actions bound to global keys in the tree view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeViewBind {
    /// Add a new entry next to the focused row.
    Add,
    /// Open the password store in LazyGit.
    Lazygit,
    /// Change the order of the entry list.
    Sort,
}

fn keybinds() -> Result<Vec<MenuKeybind<TreeViewBind>>> {
    Ok(vec![
        MenuKeybind::new(MenuKey::new("ctrl-a")?, "add entry", TreeViewBind::Add),
        MenuKeybind::new(
            MenuKey::overriding("ctrl-g")?,
            "lazygit",
            TreeViewBind::Lazygit,
        ),
        sort_keybind()?.map_action(TreeViewBind::Sort),
    ])
}

/// The folder a new entry should be created in when the add key is pressed.
///
/// A focused folder receives the new entry directly. A focused entry adds it
/// to the entry's parent folder. Anything else (the Add/Back/Close rows, or no
/// focused row because the filter matched nothing) falls back to the folder
/// currently being browsed.
pub(super) fn add_prefix_for_selection(
    item: Option<&BrowserMenuItem>,
    current_path: &[String],
) -> Option<String> {
    match item.map(|item| &item.kind) {
        Some(BrowserItemKind::Folder(folder)) => Some(folder.clone()),
        Some(BrowserItemKind::Entry { key, .. }) => {
            let mut segments = path_segments(key);
            segments.pop();
            (!segments.is_empty()).then(|| segments.join("/"))
        }
        _ => path_prefix(current_path),
    }
}

/// Initial folder and selected row for the tree view. Focusing an entry
/// opens its parent folder and selects that entry there.
fn tree_view_start(
    initial_prefix: Option<&str>,
    focus_entry: Option<&str>,
) -> (Vec<String>, Option<String>) {
    if let Some(entry) = focus_entry {
        let mut path = path_segments(entry);
        path.pop();
        return (path, Some(format!("entry:{entry}")));
    }

    (initial_prefix.map(path_segments).unwrap_or_default(), None)
}

pub(crate) fn interactive_pass_tree_menu() -> Result<i32> {
    run_tree_view(None, None)?;
    Ok(0)
}

pub(crate) fn run_tree_view(initial_prefix: Option<&str>, focus_entry: Option<&str>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let (mut path, initial_key) = tree_view_start(initial_prefix, focus_entry);
    let mut cursor = MenuCursor::new();
    if let Some(key) = initial_key {
        cursor.update_from_key(&key);
    }
    let mut sort = PassSort::tree_view_default();

    loop {
        let mut entries = load_entries(&store_dir)?;
        sort_entries(&mut entries, sort)?;
        let browser_items = build_browser_menu_items(&entries, &path)?;

        let title = if path.is_empty() {
            "Tree View".to_string()
        } else {
            format!("Tree View / {}", path.join("/"))
        };

        let mut builder = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(NerdFont::Folder, &title)
                    .field("Sort", sort.label())
                    .build(),
            )
            .prompt("Select")
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&browser_items) {
            builder = builder.initial_index(index);
        }

        let keybinds = keybinds()?;

        match builder
            .items(browser_items.clone())
            .keybinds(&keybinds)
            .select()?
        {
            crate::menu_utils::DialogOutcome::Submitted(selection) => {
                let action = selection.action;
                let item = selection.items.into_iter().next();

                if let Some(item) = &item {
                    cursor.update(item, &browser_items);
                }

                match action {
                    Some(TreeViewBind::Add) => {
                        let prefix = add_prefix_for_selection(item.as_ref(), &path);
                        run_add_menu(prefix.as_deref())?;
                        continue;
                    }
                    Some(TreeViewBind::Lazygit) => {
                        open_store_lazygit(&store_dir)?;
                        continue;
                    }
                    Some(TreeViewBind::Sort) => {
                        if let Some(chosen) = choose_sort(sort)? {
                            sort = chosen;
                        }
                        continue;
                    }
                    None => {}
                }

                let Some(item) = item else {
                    continue;
                };

                match item.kind {
                    BrowserItemKind::Folder(folder) => path = path_segments(&folder),
                    BrowserItemKind::Entry { key, .. } => {
                        if let Some(moved_key) = show_entry_actions(&entries, &key)? {
                            let (destination, focus) = tree_view_start(None, Some(&moved_key));
                            path = destination;
                            if let Some(focus) = focus {
                                cursor.update_from_key(&focus);
                            }
                        }
                    }
                    BrowserItemKind::Back => {
                        if path.is_empty() {
                            return Ok(());
                        }
                        path.pop();
                    }
                    BrowserItemKind::Close => return Ok(()),
                    BrowserItemKind::Add => run_add_menu(path_prefix(&path).as_deref())?,
                    BrowserItemKind::Menu => {}
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => {
                if path.is_empty() {
                    return Ok(());
                }
                path.pop();
            }
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

    fn folder_item(path: &str) -> BrowserMenuItem {
        BrowserMenuItem {
            key: format!("folder:{path}"),
            display: path.to_string(),
            preview: crate::ui::preview::FzfPreview::None,
            kind: BrowserItemKind::Folder(path.to_string()),
        }
    }

    #[test]
    fn tree_view_focus_starts_in_the_entries_parent_folder() {
        let (path, key) = tree_view_start(None, Some("mail/work/github"));

        assert_eq!(path.join("/"), "mail/work");
        assert_eq!(key.as_deref(), Some("entry:mail/work/github"));
    }

    #[test]
    fn tree_view_without_focus_uses_the_initial_prefix() {
        let (path, key) = tree_view_start(Some("mail/work"), None);

        assert_eq!(path.join("/"), "mail/work");
        assert!(key.is_none());
    }

    #[test]
    fn add_is_bound_to_ctrl_a_in_the_tree_view() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-a")
            .expect("tree view should bind ctrl-a");

        assert_eq!(bind.label, "add entry");
        assert_eq!(bind.action, TreeViewBind::Add);
    }

    #[test]
    fn lazygit_is_bound_to_ctrl_g() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-g")
            .expect("tree view should bind ctrl-g");

        assert_eq!(bind.label, "lazygit");
        assert_eq!(bind.action, TreeViewBind::Lazygit);
    }

    #[test]
    fn sort_is_bound_to_ctrl_s_in_the_tree_view() {
        let binds = keybinds().unwrap();
        let bind = binds
            .iter()
            .find(|bind| bind.key.as_str() == "ctrl-s")
            .expect("tree view should bind ctrl-s");

        assert_eq!(bind.label, "sort");
        assert_eq!(bind.action, TreeViewBind::Sort);
    }

    #[test]
    fn add_on_a_folder_uses_that_folder() {
        let item = folder_item("mail/work");
        let prefix = add_prefix_for_selection(Some(&item), &[]);

        assert_eq!(prefix.as_deref(), Some("mail/work"));
    }

    #[test]
    fn add_on_an_entry_uses_the_entrys_parent_folder() {
        let item = entry_item("mail/work/github");
        let prefix = add_prefix_for_selection(Some(&item), &[]);

        assert_eq!(prefix.as_deref(), Some("mail/work"));
    }

    #[test]
    fn add_on_a_root_entry_uses_the_store_root() {
        let item = entry_item("github");
        let prefix = add_prefix_for_selection(Some(&item), &[]);

        assert!(prefix.is_none());
    }

    #[test]
    fn add_on_a_non_entry_row_uses_the_current_folder() {
        let item = BrowserMenuItem {
            key: "back".to_string(),
            display: "Back".to_string(),
            preview: crate::ui::preview::FzfPreview::None,
            kind: BrowserItemKind::Back,
        };
        let current = ["mail".to_string()];
        let prefix = add_prefix_for_selection(Some(&item), &current);

        assert_eq!(prefix.as_deref(), Some("mail"));
    }

    #[test]
    fn add_without_a_focused_row_uses_the_current_folder() {
        let prefix = add_prefix_for_selection(None, &["mail".to_string()]);

        assert_eq!(prefix.as_deref(), Some("mail"));
    }
}
