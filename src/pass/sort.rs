//! Ordering of the entries listed in the pass menus.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::time::SystemTime;

use anyhow::Result;

use crate::menu_utils::{MenuSort, compare_names};
use crate::ui::nerd_font::NerdFont;

use super::types::PassEntry;
use super::utils::frecency_scores;

/// How pass entries are ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PassSort {
    /// Entries used recently or often first.
    Frequency,
    /// Entries from A to Z.
    Alphabetical,
    /// Entries from Z to A.
    ReverseAlphabetical,
    /// Entries with the newest file modification time first.
    Modified,
}

impl Default for PassSort {
    /// The pass menus open with the most frequently used entries first.
    fn default() -> Self {
        Self::Frequency
    }
}

impl MenuSort for PassSort {
    const ALL: &'static [Self] = &[
        Self::Frequency,
        Self::Alphabetical,
        Self::ReverseAlphabetical,
        Self::Modified,
    ];

    const CHOOSER_TITLE: &'static str = "Sort Passwords";

    const CHOOSER_SUBTITLE: &'static str = "Choose the order of the password list.";

    fn label(self) -> &'static str {
        match self {
            Self::Frequency => "Frequency",
            Self::Alphabetical => "Alphabetical",
            Self::ReverseAlphabetical => "Reverse alphabetical",
            Self::Modified => "Modification date",
        }
    }

    fn icon(self) -> NerdFont {
        match self {
            Self::Frequency => NerdFont::Chart,
            Self::Alphabetical => NerdFont::List,
            Self::ReverseAlphabetical => NerdFont::ArrowDown,
            Self::Modified => NerdFont::Clock,
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Self::Frequency => "Passwords you copy or export recently and often are listed first.",
            Self::Alphabetical => "Passwords are listed from A to Z.",
            Self::ReverseAlphabetical => "Passwords are listed from Z to A.",
            Self::Modified => "Passwords with the newest file modification time are listed first.",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Frequency => "Entries that were never used follow alphabetically.",
            Self::Alphabetical | Self::ReverseAlphabetical => "The order ignores usage history.",
            Self::Modified => "Entries without a readable timestamp follow alphabetically.",
        }
    }
}

/// Order a pass entry list in place.
pub(super) fn sort_entries(entries: &mut [PassEntry], sort: PassSort) -> Result<()> {
    match sort {
        PassSort::Frequency => sort_by_frequency(entries)?,
        PassSort::Alphabetical => {
            entries.sort_by(|left, right| compare_names(&left.display_name, &right.display_name));
        }
        PassSort::ReverseAlphabetical => {
            entries.sort_by(|left, right| compare_names(&right.display_name, &left.display_name));
        }
        PassSort::Modified => sort_by_modified(entries),
    }
    Ok(())
}

fn sort_by_frequency(entries: &mut [PassEntry]) -> Result<()> {
    let scores = frecency_scores(entries)?;

    entries.sort_by(|left, right| {
        let left_score = scores.get(&left.display_name).copied().unwrap_or_default();
        let right_score = scores.get(&right.display_name).copied().unwrap_or_default();

        right_score
            .total_cmp(&left_score)
            .then_with(|| compare_names(&left.display_name, &right.display_name))
    });

    Ok(())
}

fn sort_by_modified(entries: &mut [PassEntry]) {
    let modified: HashMap<String, SystemTime> = entries
        .iter()
        .filter_map(|entry| {
            let time = fs::metadata(entry.primary_file_path()?)
                .and_then(|metadata| metadata.modified())
                .ok()?;
            Some((entry.display_name.clone(), time))
        })
        .collect();

    entries.sort_by(|left, right| {
        let left_time = modified.get(&left.display_name);
        let right_time = modified.get(&right.display_name);

        match (left_time, right_time) {
            (Some(left_time), Some(right_time)) => right_time
                .cmp(left_time)
                .then_with(|| compare_names(&left.display_name, &right.display_name)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => compare_names(&left.display_name, &right.display_name),
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use tempfile::tempdir;

    use super::*;

    fn entry(name: &str) -> PassEntry {
        PassEntry {
            display_name: name.to_string(),
            secret_key: Some(name.to_string()),
            otp_key: None,
            secret_path: None,
            otp_path: None,
        }
    }

    fn names(entries: &[PassEntry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.display_name.as_str())
            .collect()
    }

    #[test]
    fn defaults_to_frequency_order() {
        assert_eq!(PassSort::default(), PassSort::Frequency);
    }

    #[test]
    fn alphabetical_orders_ignore_case() {
        let mut entries = vec![entry("zebra"), entry("Alpha"), entry("beta")];

        sort_entries(&mut entries, PassSort::Alphabetical).unwrap();
        assert_eq!(names(&entries), ["Alpha", "beta", "zebra"]);

        sort_entries(&mut entries, PassSort::ReverseAlphabetical).unwrap();
        assert_eq!(names(&entries), ["zebra", "beta", "Alpha"]);
    }

    #[test]
    fn modification_date_puts_newest_first_and_missing_last() -> Result<()> {
        let temp = tempdir()?;
        let old_path = temp.path().join("old.gpg");
        let new_path = temp.path().join("new.gpg");
        fs::write(&old_path, b"old")?;
        fs::write(&new_path, b"new")?;

        set_modified(&old_path, 100)?;
        set_modified(&new_path, 200)?;

        let mut old = entry("old");
        old.secret_path = Some(old_path);
        let mut new = entry("new");
        new.secret_path = Some(new_path);

        let mut entries = vec![old, entry("missing"), new];
        sort_entries(&mut entries, PassSort::Modified)?;

        assert_eq!(names(&entries), ["new", "old", "missing"]);
        Ok(())
    }

    fn set_modified(path: &std::path::Path, seconds: u64) -> Result<()> {
        let modified = UNIX_EPOCH + Duration::from_secs(seconds);
        fs::File::open(path)?.set_times(fs::FileTimes::new().set_modified(modified))?;
        Ok(())
    }
}
