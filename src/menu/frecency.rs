//! Optional, menu-owned frecency ranking.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::protocol::SerializableMenuItem;
use crate::frecency::FrecencyStore;
use crate::menu_utils::FzfSelectable;

/// Frecency state for one named menu.
pub struct MenuFrecency {
    path: PathBuf,
    store: FrecencyStore,
}

impl MenuFrecency {
    pub fn open(namespace: &str) -> Result<Self> {
        validate_namespace(namespace)?;
        let cache_root = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
        let path = cache_root
            .join(env!("CARGO_BIN_NAME"))
            .join("menu-frecency")
            .join(format!("{namespace}.json"));
        let store = FrecencyStore::load(&path);
        Ok(Self { path, store })
    }

    /// Remove duplicate stable keys (first occurrence wins), then globally
    /// rank the remaining corpus. Equal scores retain producer order.
    pub fn prepare(&self, items: Vec<SerializableMenuItem>) -> Vec<SerializableMenuItem> {
        let mut seen = HashSet::with_capacity(items.len());
        let mut scored = items
            .into_iter()
            .enumerate()
            .filter_map(|(position, item)| {
                let key = item.fzf_key();
                seen.insert(key.clone())
                    .then(|| (self.store.score(&key), position, item))
            })
            .collect::<Vec<_>>();

        scored.sort_by(|left, right| {
            right
                .0
                .total_cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        scored.into_iter().map(|(_, _, item)| item).collect()
    }

    pub fn record_all(&mut self, items: &[SerializableMenuItem]) -> Result<()> {
        for item in items {
            self.store.record(&item.fzf_key());
        }
        self.store.save(&self.path).with_context(|| {
            format!(
                "Failed to save menu frecency namespace at {}",
                self.path.display()
            )
        })
    }
}

pub(super) fn validate_namespace(namespace: &str) -> Result<()> {
    if namespace.is_empty()
        || !namespace
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("Frecency namespace must contain only ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_as_namespaces() {
        assert!(validate_namespace("launch/apps").is_err());
        assert!(validate_namespace("../launch").is_err());
        assert!(validate_namespace("launch").is_ok());
    }

    #[test]
    fn prepare_deduplicates_stably() {
        let state = MenuFrecency {
            path: PathBuf::new(),
            store: FrecencyStore::new(),
        };
        let mut first = SerializableMenuItem::plain("First");
        first.key = Some("same".to_string());
        let mut duplicate = SerializableMenuItem::plain("Duplicate");
        duplicate.key = Some("same".to_string());
        let other = SerializableMenuItem::plain("Other");

        let prepared = state.prepare(vec![first, duplicate, other]);
        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[0].display_text, "First");
        assert_eq!(prepared[1].display_text, "Other");
    }

    #[test]
    fn prepare_globally_ranks_items_from_late_batches() {
        let mut state = MenuFrecency {
            path: PathBuf::new(),
            store: FrecencyStore::new(),
        };
        let first = SerializableMenuItem {
            key: Some("first".to_string()),
            ..SerializableMenuItem::plain("First")
        };
        let late = SerializableMenuItem {
            key: Some("late".to_string()),
            ..SerializableMenuItem::plain("Late favorite")
        };
        state.store.record("late");
        state.store.record("late");

        let prepared = state.prepare(vec![first, late]);
        assert_eq!(prepared[0].key.as_deref(), Some("late"));
        assert_eq!(prepared[1].key.as_deref(), Some("first"));
    }
}
