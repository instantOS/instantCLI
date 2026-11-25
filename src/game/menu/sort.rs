//! Ordering of the games listed in the game menu.

use std::cmp::Ordering;

use crate::game::games::selection::GameMenuEntry;
use crate::menu_utils::{MenuSort, compare_names};
use crate::ui::nerd_font::NerdFont;

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

impl MenuSort for GameSort {
    const ALL: &'static [Self] = &[Self::Recent, Self::Installed, Self::Alphabetical];

    const CHOOSER_TITLE: &'static str = "Sort Games";

    const CHOOSER_SUBTITLE: &'static str = "Choose the order of the game list.";

    fn label(self) -> &'static str {
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
        let by_name = || compare_names(left_name, right_name);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
