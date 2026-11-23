//! Ordering of the games listed in the game menu.

use std::cmp::Ordering;

use anyhow::Result;

use crate::game::games::selection::GameMenuEntry;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{
    DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder, MenuKey, MenuKeybind,
};
use crate::ui::catppuccin::{colors, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Key that opens the sort chooser from the game menu.
const SORT_KEY: &str = "ctrl-s";

/// How the games in the game menu are ordered.
///
/// The menu's fixed actions (Add Game, Sync All, ...) always stay above the
/// games; only the games move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameSort {
    /// Games launched recently or often first. Games that were never
    /// launched follow alphabetically.
    Recent,
    /// Games with save backups configured on this device first.
    Installed,
    /// Games from A to Z.
    Alphabetical,
}

impl Default for GameSort {
    /// The game menu opens sorted by recent play.
    fn default() -> Self {
        Self::Recent
    }
}

impl GameSort {
    const ALL: [Self; 3] = [Self::Recent, Self::Installed, Self::Alphabetical];

    /// Short name shown in the game menu header and the sort chooser.
    pub fn label(self) -> &'static str {
        match self {
            Self::Recent => "Recently played",
            Self::Installed => "Installed first",
            Self::Alphabetical => "Alphabetical",
        }
    }

    fn icon(self) -> NerdFont {
        match self {
            Self::Recent => NerdFont::Clock,
            Self::Installed => NerdFont::CheckCircle,
            Self::Alphabetical => NerdFont::List,
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Self::Recent => "Games you launched recently or often are listed first.",
            Self::Installed => {
                "Games with save backups configured on this device are listed first."
            }
            Self::Alphabetical => "Games are listed from A to Z.",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Recent => "Games that were never launched follow alphabetically.",
            Self::Installed => "Each group is alphabetical.",
            Self::Alphabetical => "Recent launches are ignored.",
        }
    }
}

/// The sort key was pressed in the game menu; the focused row rides along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortRequested;

/// The game menu binding that opens the sort chooser.
pub fn sort_keybind() -> Result<MenuKeybind<SortRequested>> {
    Ok(MenuKeybind::new(
        MenuKey::new(SORT_KEY)?,
        "sort",
        SortRequested,
    ))
}

/// Order the menu's game rows.
///
/// `score` reports how recently and often a game was launched; it is only
/// consulted for [`GameSort::Recent`]. The sort is stable, so equal games keep
/// their incoming order.
pub fn sort_games(
    games: Vec<GameMenuEntry>,
    sort: GameSort,
    score: impl Fn(&str) -> f64,
) -> Vec<GameMenuEntry> {
    let mut games = games;
    games.sort_by(|left, right| {
        let (
            GameMenuEntry::Game(left_name, left_installed),
            GameMenuEntry::Game(right_name, right_installed),
        ) = (left, right)
        else {
            return Ordering::Equal;
        };
        let by_name = || {
            ignore_case(left_name)
                .cmp(ignore_case(right_name))
                .then_with(|| left_name.cmp(right_name))
        };
        match sort {
            GameSort::Recent => score(right_name)
                .total_cmp(&score(left_name))
                .then_with(by_name),
            GameSort::Installed => right_installed.cmp(left_installed).then_with(by_name),
            GameSort::Alphabetical => by_name(),
        }
    });
    games
}

/// Compare `text` letter by letter, ignoring case, without allocating.
fn ignore_case(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(char::to_lowercase)
}

/// One selectable order in the sort chooser.
#[derive(Clone)]
struct SortOption {
    sort: GameSort,
    current: bool,
}

impl FzfSelectable for SortOption {
    fn fzf_display_text(&self) -> String {
        let color = if self.current {
            colors::GREEN
        } else {
            colors::MAUVE
        };
        let current = if self.current {
            format!("  {}", format_with_color("current", colors::GREEN))
        } else {
            String::new()
        };
        format!(
            "{} {}{current}",
            format_icon_colored(self.sort.icon(), color),
            self.sort.label()
        )
    }

    fn fzf_key(&self) -> String {
        self.sort.label().to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let status = if self.current {
            "This is the current order."
        } else {
            "Press Enter to switch to this order."
        };
        PreviewBuilder::new()
            .header(self.sort.icon(), self.sort.label())
            .text(self.sort.detail())
            .text(self.sort.note())
            .blank()
            .subtext(status)
            .build()
    }
}

/// Let the user choose the order of the game list.
///
/// Returns `None` when the chooser is dismissed, which keeps `current`.
pub fn choose_sort(current: GameSort) -> Result<Option<GameSort>> {
    let options = GameSort::ALL
        .iter()
        .map(|&sort| SortOption {
            sort,
            current: sort == current,
        })
        .collect::<Vec<_>>();
    let initial = options
        .iter()
        .position(|option| option.current)
        .unwrap_or_default();
    let selection = FzfWrapper::builder()
        .header(
            HeaderBuilder::new(NerdFont::Sliders, "Sort Games")
                .subtitle("Choose the order of the game list.")
                .build(),
        )
        .prompt("Sort by")
        .responsive_layout()
        .initial_index(initial)
        .items(options)
        .padded()
        .select_one()?;
    Ok(match selection {
        DialogOutcome::Submitted(option) => Some(option.sort),
        DialogOutcome::Cancelled => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};

    fn games(names: &[(&str, bool)]) -> Vec<GameMenuEntry> {
        names
            .iter()
            .map(|(name, installed)| GameMenuEntry::Game((*name).to_string(), *installed))
            .collect()
    }

    fn names(entries: &[GameMenuEntry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| match entry {
                GameMenuEntry::Game(name, _) => name.as_str(),
                other => panic!("expected only games, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn recent_ranks_launched_games_first_and_the_rest_alphabetically() {
        let scores = |name: &str| match name {
            "Often" => 3.0,
            "Once" => 1.0,
            _ => 0.0,
        };
        let ordered = sort_games(
            games(&[
                ("zeta", false),
                ("Once", false),
                ("Alpha", false),
                ("Often", false),
            ]),
            GameSort::Recent,
            scores,
        );

        assert_eq!(names(&ordered), ["Often", "Once", "Alpha", "zeta"]);
    }

    #[test]
    fn installed_lists_backed_up_games_first() {
        let ordered = sort_games(
            games(&[
                ("zeta", false),
                ("Delta", false),
                ("Beta", true),
                ("Alpha", true),
            ]),
            GameSort::Installed,
            |_| 0.0,
        );

        assert_eq!(names(&ordered), ["Alpha", "Beta", "Delta", "zeta"]);
    }

    #[test]
    fn alphabetical_ignores_case_and_launch_history() {
        let ordered = sort_games(
            games(&[("zeta", true), ("Beta", false), ("alpha", false)]),
            GameSort::Alphabetical,
            |name| if name == "zeta" { 9.0 } else { 0.0 },
        );

        assert_eq!(names(&ordered), ["alpha", "Beta", "zeta"]);
    }

    #[test]
    fn recent_is_the_default_order() {
        assert_eq!(GameSort::default(), GameSort::Recent);
    }

    #[test]
    fn sort_is_bound_to_ctrl_s() {
        let bind = sort_keybind().unwrap();

        assert_eq!(bind.key.as_str(), "ctrl-s");
        assert_eq!(bind.label, "sort");
        assert_eq!(bind.action, SortRequested);
    }

    #[test]
    fn options_mark_the_current_order() {
        let current = SortOption {
            sort: GameSort::Recent,
            current: true,
        };
        let other = SortOption {
            sort: GameSort::Alphabetical,
            current: false,
        };

        assert!(current.fzf_display_text().contains("Recently played"));
        assert!(current.fzf_display_text().contains("current"));
        assert!(other.fzf_display_text().contains("Alphabetical"));
        assert!(!other.fzf_display_text().contains("current"));
        assert_ne!(current.fzf_key(), other.fzf_key());
    }

    #[test]
    fn chooser_returns_the_picked_order_and_nothing_when_dismissed() {
        let _guard = MockQueue::new()
            .select_index(0)
            .cancel_selection()
            .select_index(2)
            .guard();

        assert_eq!(
            choose_sort(GameSort::Installed).unwrap(),
            Some(GameSort::Recent)
        );
        assert_eq!(choose_sort(GameSort::Recent).unwrap(), None);
        assert_eq!(
            choose_sort(GameSort::Recent).unwrap(),
            Some(GameSort::Alphabetical)
        );
        assert_eq!(scripted_responses_remaining(), 0);
    }
}
