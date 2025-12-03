use crate::common::dependencies::Dependency;
use crate::ui::prelude::NerdFont;

use super::actions;
use super::packages;

use packages::*;

#[derive(Debug, Clone)]
pub enum AssistEntry {
    Action(AssistAction),
    Group(AssistGroup),
}

#[derive(Debug, Clone)]
pub struct AssistAction {
    pub key: char,
    pub description: &'static str,
    pub icon: NerdFont,
    pub dependencies: &'static [Dependency],
    pub execute: fn() -> anyhow::Result<()>,
}

#[derive(Debug, Clone)]
pub struct AssistGroup {
    pub key: char,
    pub description: &'static str,
    pub icon: NerdFont,
    pub children: &'static [AssistEntry],
}

pub const ASSISTS: &[AssistEntry] = &[
    AssistEntry::Action(AssistAction {
        key: 'h',
        description: "Help: Show all available assists",
        icon: NerdFont::Question,
        dependencies: &[],
        execute: actions::help::show_help,
    }),
    AssistEntry::Group(AssistGroup {
        key: 'i',
        description: "Info: AI assistants and man pages",
        icon: NerdFont::Info,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'a',
                description: "Ask Claude: Ask Claude AI about clipboard content",
                icon: NerdFont::Lightbulb,
                dependencies: &[],
                execute: actions::ai::ask_claude,
            }),
            AssistEntry::Action(AssistAction {
                key: 'c',
                description: "Ask ChatGPT: Ask ChatGPT about clipboard content",
                icon: NerdFont::Lightbulb,
                dependencies: &[],
                execute: actions::ai::ask_chatgpt,
            }),
            AssistEntry::Action(AssistAction {
                key: 'g',
                description: "Ask Gemini: Ask Google Gemini about clipboard content",
                icon: NerdFont::Lightbulb,
                dependencies: &[],
                execute: actions::ai::ask_gemini,
            }),
            AssistEntry::Action(AssistAction {
                key: 'm',
                description: "Man Pages: Search and view man pages",
                icon: NerdFont::FileText,
                dependencies: &[],
                execute: actions::man::search_man_pages,
            }),
        ],
    }),
    AssistEntry::Action(AssistAction {
        key: 'b',
        description: "Brightness: Adjust display brightness",
        icon: NerdFont::Monitor,
        dependencies: &[Dependency::os(&BRIGHTNESSCTL)],
        execute: actions::system::brightness,
    }),
    AssistEntry::Group(AssistGroup {
        key: 'j',
        description: "Jokes: Quick fun assists",
        icon: NerdFont::Star,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'b',
                description: "Bruh: Display a bruh moment",
                icon: NerdFont::Frown,
                dependencies: &[Dependency::os(&MPV)],
                execute: actions::joke::bruh,
            }),
            AssistEntry::Action(AssistAction {
                key: 'c',
                description: "CMatrix: Run the Matrix rain",
                icon: NerdFont::Waves,
                dependencies: &[Dependency::os(&CMATRIX)],
                execute: actions::joke::cmatrix,
            }),
            AssistEntry::Action(AssistAction {
                key: 'q',
                description: "Asciiquarium: Launch the terminal aquarium",
                icon: NerdFont::FishBowl,
                dependencies: &[Dependency::os(&ASCIIQUARIUM)],
                execute: actions::joke::asciiquarium,
            }),
        ],
    }),
    AssistEntry::Action(AssistAction {
        key: 'c',
        description: "Caffeine: Keep system awake",
        icon: NerdFont::Coffee,
        dependencies: &[],
        execute: actions::system::caffeine,
    }),
    AssistEntry::Action(AssistAction {
        key: 'a',
        description: "Volume: Adjust audio volume",
        icon: NerdFont::VolumeUp,
        dependencies: &[],
        execute: actions::system::volume,
    }),
    AssistEntry::Action(AssistAction {
        key: 'm',
        description: "Music: Play/pause music",
        icon: NerdFont::Music,
        dependencies: &[Dependency::os(&PLAYERCTL)],
        execute: actions::media::music,
    }),
    AssistEntry::Action(AssistAction {
        key: 'p',
        description: "Password Manager: Open password manager",
        icon: NerdFont::ClosedLockWithKey,
        dependencies: &[],
        execute: actions::password::open_password_manager,
    }),
    AssistEntry::Action(AssistAction {
        key: 'q',
        description: "QR Encode Clipboard: Generate QR code from clipboard",
        icon: NerdFont::QrCode,
        dependencies: &[Dependency::os(&QRENCODE)],
        execute: actions::qr::qr_encode_clipboard,
    }),
    AssistEntry::Action(AssistAction {
        key: 'e',
        description: "Emoji Picker: Open emoji picker",
        icon: NerdFont::Smile,
        dependencies: &[Dependency::flatpak(&EMOTE)],
        execute: actions::emoji::emoji_picker,
    }),
    AssistEntry::Action(AssistAction {
        key: 'k',
        description: "Color Picker: Pick color from screen",
        icon: NerdFont::Palette,
        dependencies: &[
            Dependency::os(&HYPRPICKER),
            Dependency::os(&XCOLOR),
            Dependency::os(&IMAGEMAGICK),
            Dependency::os(&WL_CLIPBOARD),
            Dependency::os(&XCLIP),
            Dependency::os(&LIBNOTIFY),
        ],
        execute: actions::color_picker::pick_color,
    }),
    AssistEntry::Group(AssistGroup {
        key: 's',
        description: "Screenshot: Screenshot and annotation tools",
        icon: NerdFont::Image,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'f',
                description: "Fullscreen to Pictures: Fullscreen screenshot to Pictures folder",
                icon: NerdFont::Desktop,
                dependencies: &[Dependency::os(&GRIM), Dependency::os(&IMAGEMAGICK)],
                execute: actions::screenshot::fullscreen_screenshot,
            }),
            AssistEntry::Action(AssistAction {
                key: 'a',
                description: "Screenshot & Annotate: Take screenshot with flameshot",
                icon: NerdFont::Edit,
                dependencies: &[Dependency::os(&FLAMESHOT)],
                execute: actions::screenshot::screenshot_annotate,
            }),
            AssistEntry::Action(AssistAction {
                key: 'c',
                description: "Screenshot to Clipboard: Capture area to clipboard",
                icon: NerdFont::Clipboard,
                dependencies: &[
                    Dependency::os(&SLURP),
                    Dependency::os(&SLOP),
                    Dependency::os(&GRIM),
                    Dependency::os(&IMAGEMAGICK),
                    Dependency::os(&WL_CLIPBOARD),
                    Dependency::os(&XCLIP),
                ],
                execute: actions::screenshot::screenshot_to_clipboard,
            }),
            AssistEntry::Action(AssistAction {
                key: 'i',
                description: "Screenshot to Imgur: Capture area and upload to Imgur",
                icon: NerdFont::Upload,
                dependencies: &[
                    Dependency::os(&SLURP),
                    Dependency::os(&SLOP),
                    Dependency::os(&GRIM),
                    Dependency::os(&IMAGEMAGICK),
                    Dependency::os(&WL_CLIPBOARD),
                    Dependency::os(&XCLIP),
                    Dependency::os(&LIBNOTIFY),
                ],
                execute: actions::screenshot::screenshot_to_imgur,
            }),
            AssistEntry::Action(AssistAction {
                key: 'm',
                description: "Fullscreen to Clipboard: Fullscreen screenshot directly to clipboard",
                icon: NerdFont::Desktop,
                dependencies: &[
                    Dependency::os(&SLURP),
                    Dependency::os(&SLOP),
                    Dependency::os(&GRIM),
                    Dependency::os(&IMAGEMAGICK),
                    Dependency::os(&WL_CLIPBOARD),
                    Dependency::os(&XCLIP),
                ],
                execute: actions::screenshot::screenshot_to_clipboard_fullscreen,
            }),
            AssistEntry::Action(AssistAction {
                key: 'p',
                description: "Area to Pictures: Selected area screenshot to Pictures folder",
                icon: NerdFont::FolderOpen,
                dependencies: &[
                    Dependency::os(&SLURP),
                    Dependency::os(&SLOP),
                    Dependency::os(&GRIM),
                    Dependency::os(&IMAGEMAGICK),
                    Dependency::os(&WL_CLIPBOARD),
                    Dependency::os(&XCLIP),
                ],
                execute: actions::screenshot::screenshot_area_to_pictures,
            }),
            AssistEntry::Action(AssistAction {
                key: 'q',
                description: "QR Code Scanner: Scan QR code from selected area",
                icon: NerdFont::QrCode,
                dependencies: &[
                    Dependency::os(&SLURP),
                    Dependency::os(&SLOP),
                    Dependency::os(&GRIM),
                    Dependency::os(&IMAGEMAGICK),
                    Dependency::os(&ZBAR),
                    Dependency::os(&LIBNOTIFY),
                ],
                execute: actions::qr::qr_scan,
            }),
            AssistEntry::Action(AssistAction {
                key: 'r',
                description: "OCR Text Recognition: Extract text from selected area",
                icon: NerdFont::FileText,
                dependencies: &[
                    Dependency::os(&SLURP),
                    Dependency::os(&SLOP),
                    Dependency::os(&GRIM),
                    Dependency::os(&IMAGEMAGICK),
                    Dependency::os(&WL_CLIPBOARD),
                    Dependency::os(&XCLIP),
                    Dependency::os(&TESSERACT),
                    Dependency::os(&LIBNOTIFY),
                ],
                execute: actions::screenshot::screenshot_ocr,
            }),
        ],
    }),
    AssistEntry::Group(AssistGroup {
        key: 't',
        description: "Tools: System tools and tweaks",
        icon: NerdFont::Gear,
        children: &[AssistEntry::Action(AssistAction {
            key: 'm',
            description: "Mouse Speed: Adjust mouse sensitivity",
            icon: NerdFont::MousePointer,
            dependencies: &[],
            execute: actions::mouse::mouse_speed_slider,
        })],
    }),
    AssistEntry::Group(AssistGroup {
        key: 'v',
        description: "Media Navigation: Control media playback tracks",
        icon: NerdFont::Music,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'c',
                description: "Control Media: Select player to play/pause",
                icon: NerdFont::Music,
                dependencies: &[Dependency::os(&PLAYERCTL)],
                execute: actions::media::control_media,
            }),
            AssistEntry::Action(AssistAction {
                key: 'n',
                description: "Next Track: Go to next track",
                icon: NerdFont::ChevronRight,
                dependencies: &[Dependency::os(&PLAYERCTL)],
                execute: actions::media::next_track,
            }),
            AssistEntry::Action(AssistAction {
                key: 'p',
                description: "Previous Track: Go to previous track",
                icon: NerdFont::ChevronLeft,
                dependencies: &[Dependency::os(&PLAYERCTL)],
                execute: actions::media::previous_track,
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

    #[allow(dead_code)]
    pub fn description(&self) -> &'static str {
        match self {
            AssistEntry::Action(action) => action.description,
            AssistEntry::Group(group) => group.description,
        }
    }

    #[allow(dead_code)]
    pub fn icon(&self) -> NerdFont {
        match self {
            AssistEntry::Action(action) => action.icon,
            AssistEntry::Group(group) => group.icon,
        }
    }
}

pub fn find_group_entries(key_sequence: &str) -> Option<&'static [AssistEntry]> {
    if key_sequence.is_empty() {
        return Some(ASSISTS);
    }

    let mut chars = key_sequence.chars();
    let first_key = chars.next()?;

    let entry = ASSISTS.iter().find(|entry| entry.key() == first_key)?;

    match entry {
        AssistEntry::Action(_) => None,
        AssistEntry::Group(group) => {
            if chars.next().is_none() {
                Some(group.children)
            } else {
                None
            }
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
        assert_eq!(action.unwrap().description, "Caffeine: Keep system awake");
    }

    #[test]
    fn test_find_grouped_action() {
        let action = find_action("vn");
        assert!(action.is_some());
        assert_eq!(action.unwrap().description, "Next Track: Go to next track");

        let action = find_action("vp");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "Previous Track: Go to previous track"
        );
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
        assert_eq!(
            action.unwrap().description,
            "Screenshot & Annotate: Take screenshot with flameshot"
        );
    }

    #[test]
    fn test_screenshot_group_key_not_an_action() {
        assert!(find_action("s").is_none());
    }

    #[test]
    fn test_find_screenshot_to_clipboard_action() {
        let action = find_action("sc");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "Screenshot to Clipboard: Capture area to clipboard"
        );
    }

    #[test]
    fn test_find_fullscreen_screenshot_action() {
        let action = find_action("sf");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "Fullscreen to Pictures: Fullscreen screenshot to Pictures folder"
        );
    }

    #[test]
    fn test_find_emoji_picker_action() {
        let action = find_action("e");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "Emoji Picker: Open emoji picker"
        );
    }

    #[test]
    fn test_find_color_picker_action() {
        let action = find_action("k");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "Color Picker: Pick color from screen"
        );
    }

    #[test]
    fn test_find_qr_encode_action() {
        let action = find_action("q");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "QR Encode Clipboard: Generate QR code from clipboard"
        );
    }

    #[test]
    fn test_find_help_action() {
        let action = find_action("h");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "Help: Show all available assists"
        );
    }

    #[test]
    fn test_find_screenshot_ocr_action() {
        let action = find_action("sr");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "OCR Text Recognition: Extract text from selected area"
        );
    }

    #[test]
    fn test_find_qr_scan_action() {
        let action = find_action("sq");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "QR Code Scanner: Scan QR code from selected area"
        );
    }
    #[test]
    fn test_find_media_control_action() {
        let action = find_action("vc");
        assert!(action.is_some());
        assert_eq!(
            action.unwrap().description,
            "Control Media: Select player to play/pause"
        );
    }
}
