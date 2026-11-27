//! Ordering of the rows a menu lists.
//!
//! A menu declares its orders with [`MenuSort`], registers [`sort_keybind`]
//! and hands the pressed key to [`choose_sort`]. The menu's fixed actions
//! stay where they are; only its rows move, and the focused row rides along.

use std::cmp::Ordering;

use anyhow::Result;

use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::{
    DialogOutcome, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder, MenuKey, MenuKeybind,
};

/// Key that opens a menu's sort chooser.
const SORT_KEY: &str = "ctrl-s";

/// The orders a menu can list its rows in.
pub trait MenuSort: Copy + PartialEq + 'static {
    /// Every order, listed in this order by the chooser.
    const ALL: &'static [Self];

    /// Title of the menu's sort chooser.
    const CHOOSER_TITLE: &'static str;

    /// Guidance shown under the chooser title.
    const CHOOSER_SUBTITLE: &'static str;

    /// Short name shown in the menu header and the sort chooser.
    fn label(self) -> &'static str;

    fn icon(self) -> NerdFont;

    /// What this order does, shown in the chooser's preview.
    fn detail(self) -> &'static str;

    /// How this order breaks ties, shown under the detail.
    fn note(self) -> &'static str;
}

/// The sort key was pressed in a menu; the focused row rides along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortRequested;

/// The binding that opens a menu's sort chooser.
///
/// Menus with more than one keybind map it to their own action with
/// [`MenuKeybind::map_action`].
pub fn sort_keybind() -> Result<MenuKeybind<SortRequested>> {
    Ok(MenuKeybind::new(
        MenuKey::new(SORT_KEY)?,
        "sort",
        SortRequested,
    ))
}

/// Let the user choose the order of a menu's rows.
///
/// The chooser opens on `current`. Choosing Back, or dismissing the chooser,
/// returns `None`, which keeps `current`.
pub fn choose_sort<S: MenuSort>(current: S) -> Result<Option<S>> {
    let choices = choices(current);
    let initial = choices
        .iter()
        .position(|choice| matches!(choice, SortChoice::Order(option) if option.current))
        .unwrap_or_default();
    let selection = FzfWrapper::builder()
        .header(
            HeaderBuilder::new(NerdFont::Sliders, S::CHOOSER_TITLE)
                .subtitle(S::CHOOSER_SUBTITLE)
                .build(),
        )
        .prompt("Sort by")
        .responsive_layout()
        .initial_index(initial)
        .items(choices)
        .padded()
        .select_one()?;
    Ok(match selection {
        DialogOutcome::Submitted(SortChoice::Order(option)) => Some(option.sort),
        DialogOutcome::Submitted(SortChoice::Back) | DialogOutcome::Cancelled => None,
    })
}

/// The chooser's rows: the way back out, then every order.
fn choices<S: MenuSort>(current: S) -> Vec<SortChoice<S>> {
    let mut choices = vec![SortChoice::Back];
    choices.extend(S::ALL.iter().map(|&sort| {
        SortChoice::Order(SortOption {
            sort,
            current: sort == current,
        })
    }));
    choices
}

/// Compare two names letter by letter, ignoring case, without allocating.
///
/// Names that differ only in case are ordered by their exact spelling.
pub fn compare_names(left: &str, right: &str) -> Ordering {
    ignore_case(left)
        .cmp(ignore_case(right))
        .then_with(|| left.cmp(right))
}

fn ignore_case(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(char::to_lowercase)
}

/// One row of the sort chooser: the way back out, or an order to switch to.
#[derive(Clone)]
enum SortChoice<S> {
    /// Leave the chooser without changing the order.
    Back,
    /// Switch to this order.
    Order(SortOption<S>),
}

impl<S: MenuSort> FzfSelectable for SortChoice<S> {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Back => format!("{} Back", format_back_icon()),
            Self::Order(option) => option.fzf_display_text(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Back => "__back__".to_string(),
            Self::Order(option) => option.fzf_key(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Go Back")
                .text("Keep the current order.")
                .build(),
            Self::Order(option) => option.fzf_preview(),
        }
    }
}

/// One selectable order in the sort chooser.
#[derive(Clone)]
struct SortOption<S> {
    sort: S,
    current: bool,
}

impl<S: MenuSort> FzfSelectable for SortOption<S> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestSort {
        First,
        Second,
        Third,
    }

    impl MenuSort for TestSort {
        const ALL: &'static [Self] = &[Self::First, Self::Second, Self::Third];

        const CHOOSER_TITLE: &'static str = "Sort Tests";

        const CHOOSER_SUBTITLE: &'static str = "Choose the order of the test list.";

        fn label(self) -> &'static str {
            match self {
                Self::First => "First order",
                Self::Second => "Second order",
                Self::Third => "Third order",
            }
        }

        fn icon(self) -> NerdFont {
            NerdFont::List
        }

        fn detail(self) -> &'static str {
            "Rows are reordered."
        }

        fn note(self) -> &'static str {
            "Ties keep their incoming order."
        }
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
            sort: TestSort::First,
            current: true,
        };
        let other = SortOption {
            sort: TestSort::Third,
            current: false,
        };

        assert!(current.fzf_display_text().contains("First order"));
        assert!(current.fzf_display_text().contains("current"));
        assert!(other.fzf_display_text().contains("Third order"));
        assert!(!other.fzf_display_text().contains("current"));
        assert_ne!(current.fzf_key(), other.fzf_key());

        let FzfPreview::Text(preview) = other.fzf_preview() else {
            panic!("sort options should have a text preview");
        };
        assert!(preview.contains("Rows are reordered."));
        assert!(preview.contains("Ties keep their incoming order."));
        assert!(preview.contains("Press Enter to switch to this order."));

        let FzfPreview::Text(preview) = current.fzf_preview() else {
            panic!("sort options should have a text preview");
        };
        assert!(preview.contains("This is the current order."));
    }

    #[test]
    fn chooser_returns_the_picked_order_and_nothing_when_dismissed() {
        // Rows: Back, then the three orders.
        let _guard = MockQueue::new()
            .select_index(1)
            .select_index(0)
            .cancel_selection()
            .select_index(3)
            .guard();

        assert_eq!(
            choose_sort(TestSort::Second).unwrap(),
            Some(TestSort::First)
        );
        assert_eq!(choose_sort(TestSort::Second).unwrap(), None);
        assert_eq!(choose_sort(TestSort::Second).unwrap(), None);
        assert_eq!(
            choose_sort(TestSort::Second).unwrap(),
            Some(TestSort::Third)
        );
        assert_eq!(scripted_responses_remaining(), 0);
    }

    #[test]
    fn chooser_offers_back_before_every_order() {
        let rows = choices(TestSort::Second);

        assert_eq!(rows.len(), 4);
        assert!(matches!(rows[0], SortChoice::Back));
        assert!(rows[0].fzf_display_text().contains("Back"));
        assert_eq!(rows[0].fzf_key(), "__back__");
        let FzfPreview::Text(preview) = rows[0].fzf_preview() else {
            panic!("the back row should have a text preview");
        };
        assert!(preview.contains("Keep the current order."));
        assert!(matches!(
            &rows[1],
            SortChoice::Order(option) if option.sort == TestSort::First && !option.current
        ));
        assert!(matches!(
            &rows[2],
            SortChoice::Order(option) if option.sort == TestSort::Second && option.current
        ));
        assert!(matches!(
            &rows[3],
            SortChoice::Order(option) if option.sort == TestSort::Third && !option.current
        ));
    }

    #[test]
    fn names_are_compared_ignoring_case() {
        assert_eq!(compare_names("alpha", "Beta"), Ordering::Less);
        assert_eq!(compare_names("Beta", "alpha"), Ordering::Greater);
        assert_eq!(compare_names("same", "same"), Ordering::Equal);
        assert_eq!(compare_names("Zeta", "alpha"), Ordering::Greater);
    }

    #[test]
    fn names_differing_only_in_case_keep_a_stable_order() {
        assert_eq!(compare_names("Alpha", "alpha"), Ordering::Less);
        assert_eq!(compare_names("alpha", "Alpha"), Ordering::Greater);
    }
}
