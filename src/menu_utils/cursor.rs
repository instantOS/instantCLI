use super::FzfSelectable;

/// Tracks the last selection to restore cursor position across nested menus.
///
/// Uses the item's `fzf_key()` when possible so cursor stays stable even if the
/// menu reorders its entries between refreshes. Indexes are used as a fallback
/// when no key is available or a key no longer exists in the refreshed list.
#[derive(Debug, Default, Clone)]
pub struct MenuCursor {
    last_key: Option<String>,
    last_index: Option<usize>,
}

impl MenuCursor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_index(index: Option<usize>) -> Self {
        Self {
            last_key: None,
            last_index: index,
        }
    }

    pub fn initial_index<T: FzfSelectable>(&self, items: &[T]) -> Option<usize> {
        if items.is_empty() {
            return None;
        }

        if let Some(index) = self.last_index
            && index < items.len()
        {
            if let Some(ref key) = self.last_key {
                if items[index].fzf_key() == *key {
                    return Some(index);
                }
            } else {
                return Some(index);
            }
        }

        if let Some(ref key) = self.last_key
            && let Some(index) = items.iter().position(|item| item.fzf_key() == *key)
        {
            return Some(index);
        }

        self.last_index.map(|index| index.min(items.len() - 1))
    }

    pub fn update<T: FzfSelectable>(&mut self, selected: &T, items: &[T]) {
        let key = selected.fzf_key();

        self.last_index = items.iter().position(|item| item.fzf_key() == key);
        self.last_key = Some(key);
    }

    pub fn update_from_key(&mut self, key: &str) {
        self.last_key = Some(key.to_string());
        self.last_index = None;
    }
}
