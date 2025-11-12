use crate::common::requirements::RequiredPackage;
use crate::ui::prelude::NerdFont;

use super::actions;
use super::packages::*;

#[derive(Debug, Clone)]
pub enum AssistEntry {
    Action(AssistAction),
    Group(AssistGroup),
}

#[derive(Debug, Clone)]
pub struct AssistAction {
    pub key: char,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: NerdFont,
    pub requirements: &'static [RequiredPackage],
    pub execute: fn() -> anyhow::Result<()>,
}

#[derive(Debug, Clone)]
pub struct AssistGroup {
    pub key: char,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: NerdFont,
    pub children: &'static [AssistEntry],
}

pub const ASSISTS: &[AssistEntry] = &[
    AssistEntry::Action(AssistAction {
        key: 'c',
        title: "Caffeine",
        description: "Keep system awake",
        icon: NerdFont::Lightbulb,
        requirements: &[],
        execute: actions::system::caffeine,
    }),
    AssistEntry::Action(AssistAction {
        key: 'a',
        title: "Volume",
        description: "Adjust audio volume",
        icon: NerdFont::VolumeUp,
        requirements: &[],
        execute: actions::system::volume,
    }),
    AssistEntry::Action(AssistAction {
        key: 'm',
        title: "Music",
        description: "Play/pause music",
        icon: NerdFont::Music,
        requirements: &[PLAYERCTL_PACKAGE],
        execute: actions::media::music,
    }),
    AssistEntry::Action(AssistAction {
        key: 'e',
        title: "QR Encode Clipboard",
        description: "Generate QR code from clipboard",
        icon: NerdFont::Square,
        requirements: &[QRENCODE_PACKAGE],
        execute: actions::qr::qr_encode_clipboard,
    }),
    AssistEntry::Group(AssistGroup {
        key: 's',
        title: "Screenshot",
        description: "Screenshot and annotation tools",
        icon: NerdFont::Image,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'a',
                title: "Screenshot & Annotate",
                description: "Take screenshot with flameshot",
                icon: NerdFont::Edit,
                requirements: &[FLAMESHOT_PACKAGE],
                execute: actions::screenshot::screenshot_annotate,
            }),
            AssistEntry::Action(AssistAction {
                key: 'c',
                title: "Screenshot to Clipboard",
                description: "Capture area to clipboard",
                icon: NerdFont::Clipboard,
                requirements: SCREENSHOT_CLIPBOARD_PACKAGES,
                execute: actions::screenshot::screenshot_to_clipboard,
            }),
            AssistEntry::Action(AssistAction {
                key: 'i',
                title: "Screenshot to Imgur",
                description: "Capture area and upload to Imgur",
                icon: NerdFont::Upload,
                requirements: SCREENSHOT_IMGUR_PACKAGES,
                execute: actions::screenshot::screenshot_to_imgur,
            }),
        ],
    }),
    AssistEntry::Group(AssistGroup {
        key: 'v',
        title: "Media Navigation",
        description: "Control media playback tracks",
        icon: NerdFont::Music,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'n',
                title: "Previous Track",
                description: "Go to previous track",
                icon: NerdFont::ChevronLeft,
                requirements: &[PLAYERCTL_PACKAGE],
                execute: actions::media::previous_track,
            }),
            AssistEntry::Action(AssistAction {
                key: 'p',
                title: "Next Track",
                description: "Go to next track",
                icon: NerdFont::ChevronRight,
                requirements: &[PLAYERCTL_PACKAGE],
                execute: actions::media::next_track,
            }),
        ],
    }),
];

impl AssistEntry {
    pub fn key(&self) -> char {
        match self {
            AssistEntry::Action(action) => action.key,
            AssistEntry::Group(group) => group.key,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            AssistEntry::Action(action) => action.title,
            AssistEntry::Group(group) => group.title,
        }
    }

    pub fn icon(&self) -> NerdFont {
        match self {
            AssistEntry::Action(action) => action.icon,
            AssistEntry::Group(group) => group.icon,
        }
    }
}

pub fn find_action(key_sequence: &str) -> Option<&'static AssistAction> {
    if key_sequence.is_empty() {
        return None;
    }

    let mut chars = key_sequence.chars();
    let first_key = chars.next()?;

    let entry = ASSISTS.iter().find(|entry| entry.key() == first_key)?;

    match entry {
        AssistEntry::Action(action) => {
            if chars.next().is_none() {
                Some(action)
            } else {
                None
            }
        }
        AssistEntry::Group(group) => {
            let second_key = chars.next()?;
            if chars.next().is_some() {
                return None;
            }

            group.children.iter().find_map(|child| match child {
                AssistEntry::Action(action) if action.key == second_key => Some(action),
                _ => None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_single_key_action() {
        let action = find_action("c");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Caffeine");
    }

    #[test]
    fn test_find_grouped_action() {
        let action = find_action("vn");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Previous Track");

        let action = find_action("vp");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Next Track");
    }

    #[test]
    fn test_find_nonexistent_action() {
        assert!(find_action("z").is_none());
        assert!(find_action("vz").is_none());
        assert!(find_action("").is_none());
    }

    #[test]
    fn test_group_key_not_an_action() {
        assert!(find_action("v").is_none());
    }

    #[test]
    fn test_find_screenshot_action() {
        let action = find_action("sa");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Screenshot & Annotate");
    }

    #[test]
    fn test_screenshot_group_key_not_an_action() {
        assert!(find_action("s").is_none());
    }

    #[test]
    fn test_find_screenshot_to_clipboard_action() {
        let action = find_action("sc");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Screenshot to Clipboard");
    }
}
