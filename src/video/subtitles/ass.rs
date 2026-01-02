//! ASS (Advanced SubStation Alpha) file format generation.
//!
//! Generates ASS subtitle files for burning into video with FFmpeg.

use std::fmt::Write;
use std::time::Duration;

use super::remap::RemappedSubtitle;

/// Style configuration for ASS subtitles.
#[derive(Debug, Clone)]
pub struct AssStyle {
    /// Style name
    pub name: String,
    /// Font name
    pub font_name: String,
    /// Font size in pixels
    pub font_size: u32,
    /// Primary color in ABGR format (e.g., &H00FFFFFF for white)
    pub primary_color: String,
    /// Secondary color in ABGR format (used for karaoke unsung part)
    pub secondary_color: String,
    /// Outline color in ABGR format
    pub outline_color: String,
    /// Background/shadow color in ABGR format
    pub back_color: String,
    /// Bold (-1 = true, 0 = false)
    pub bold: bool,
    /// Outline width in pixels
    pub outline: u32,
    /// Shadow depth in pixels
    pub shadow: u32,
    /// Alignment (numpad layout: 1-3=bottom, 4-6=mid, 7-9=top)
    pub alignment: u8,
    /// Left margin in pixels
    pub margin_l: u32,
    /// Right margin in pixels
    pub margin_r: u32,
    /// Vertical margin in pixels (distance from bottom for bottom-aligned)
    pub margin_v: u32,
}

impl Default for AssStyle {
    fn default() -> Self {
        Self::catppuccin_mocha()
    }
}

impl AssStyle {
    /// Catppuccin Mocha theme - modern, minimal, and fancy.
    ///
    /// Uses Catppuccin's signature soft colors:
    /// - Text: Catppuccin "Text" (CDD6F4) - soft white with slight blue tint
    /// - Outline: Catppuccin "Crust" (11111B) - deep dark for contrast
    /// - Shadow: Catppuccin "Mantle" with transparency - subtle depth
    ///
    /// Font: Inter (modern geometric sans-serif) with fallbacks
    pub fn catppuccin_mocha() -> Self {
        Self {
            name: "Default".to_string(),
            // Modern sans-serif stack: Inter preferred, with system fallbacks
            font_name: "Inter".to_string(),
            font_size: 52, // Slightly smaller for cleaner look
            // Catppuccin Mocha "Mauve" (#CBA6F7) - Primary (Sung/Active)
            primary_color: "&H00F7A6CB".to_string(),
            // Catppuccin Mocha "Text" (#CDD6F4) - Secondary (Unsung/Inactive)
            secondary_color: "&H00F4D6CD".to_string(),
            // Catppuccin Mocha "Crust" (#11111B) - deep dark outline
            outline_color: "&H001B1111".to_string(),
            // Catppuccin Mocha "Mantle" (#181825) with 60% opacity - subtle shadow
            back_color: "&H99251818".to_string(),
            bold: false,  // Clean, not bold for modern look
            outline: 2,   // Thinner outline for minimal aesthetic
            shadow: 1,    // Subtle shadow
            alignment: 2, // Bottom-center
            margin_l: 60,
            margin_r: 60,
            margin_v: 120, // Refined vertical margin
        }
    }

    /// Catppuccin Mocha with accent color (Mauve) for emphasis.
    #[allow(dead_code)]
    pub fn catppuccin_mocha_accent() -> Self {
        Self {
            name: "Accent".to_string(),
            font_name: "Inter".to_string(),
            font_size: 52,
            // Catppuccin Mocha "Mauve" (#CBA6F7)
            primary_color: "&H00F7A6CB".to_string(),
            // Catppuccin Mocha "Text" (#CDD6F4)
            secondary_color: "&H00F4D6CD".to_string(),
            outline_color: "&H001B1111".to_string(),
            back_color: "&H99251818".to_string(),
            bold: false,
            outline: 2,
            shadow: 1,
            alignment: 2,
            margin_l: 60,
            margin_r: 60,
            margin_v: 120,
        }
    }

    /// Catppuccin Latte theme (light mode variant).
    #[allow(dead_code)]
    pub fn catppuccin_latte() -> Self {
        Self {
            name: "Default".to_string(),
            font_name: "Inter".to_string(),
            font_size: 52,
            // Catppuccin Latte "Mauve" (#8839EF) - darker purple for light mode
            primary_color: "&H00EF3988".to_string(),
            // Catppuccin Latte "Text" (#4C4F69) - dark text
            secondary_color: "&H00694F4C".to_string(),
            // Catppuccin Latte "Crust" (#DCE0E8) - light outline
            outline_color: "&H00E8E0DC".to_string(),
            // Catppuccin Latte "Mantle" (#E6E9EF) with opacity
            back_color: "&H99EFE9E6".to_string(),
            bold: false,
            outline: 2,
            shadow: 1,
            alignment: 2,
            margin_l: 60,
            margin_r: 60,
            margin_v: 120,
        }
    }
}

impl AssStyle {
    /// Create a style optimized for reels mode (9:16 vertical video).
    /// Uses Catppuccin Mocha theme for modern, minimal aesthetics.
    pub fn for_reels() -> Self {
        let mut style = Self::catppuccin_mocha();
        // Optimize for Reels/TikTok vertical layout (1080x1920)
        // The video content (assuming 16:9 source) is scaled to width 1080.
        // Height = 1080 * (9/16) ≈ 608px.
        // The video is positioned with a 10% top offset (from ffmpeg_compiler).
        // Top padding ≈ 1920 * (1 - 608/1920) * 0.1 ≈ 131px.
        // Video bottom ≈ 131 + 608 = 739px.
        // Empty space below ≈ 1920 - 739 = 1181px.
        // Center of empty space from bottom ≈ 1181 / 2 = 590px.
        // We position slightly lower than exact center to be safe from UI elements,
        // but high enough to be clearly in the "content" area.
        style.font_size = 70; // Larger for mobile visibility
        style.margin_v = 560; // Centered in empty space below video
        style
    }

    /// Format the style line for the ASS file.
    fn to_style_line(&self) -> String {
        let bold_val = if self.bold { -1 } else { 0 };
        format!(
            "Style: {name},{font},{size},{primary},{secondary},{outline},{back},{bold},0,0,0,100,100,0,0,1,{outline_w},{shadow},{align},{ml},{mr},{mv},1",
            name = self.name,
            font = self.font_name,
            size = self.font_size,
            primary = self.primary_color,
            secondary = self.secondary_color,
            outline = self.outline_color,
            back = self.back_color,
            bold = bold_val,
            outline_w = self.outline,
            shadow = self.shadow,
            align = self.alignment,
            ml = self.margin_l,
            mr = self.margin_r,
            mv = self.margin_v,
        )
    }

    /// Format the highlight style line (for karaoke current word).
    /// Uses Catppuccin Mauve for the highlighted word.
    fn to_highlight_style_line(&self) -> String {
        let bold_val = if self.bold { -1 } else { 0 };
        // Catppuccin Mocha "Mauve" (#CBA6F7) in ABGR format
        let highlight_color = "&H00F7A6CB";
        format!(
            "Style: Highlight,{font},{size},{primary},{secondary},{outline},{back},{bold},0,0,0,100,100,0,0,1,{outline_w},{shadow},{align},{ml},{mr},{mv},1",
            font = self.font_name,
            size = self.font_size,
            primary = highlight_color,
            secondary = self.secondary_color,
            outline = self.outline_color,
            back = self.back_color,
            bold = bold_val,
            outline_w = self.outline,
            shadow = self.shadow,
            align = self.alignment,
            ml = self.margin_l,
            mr = self.margin_r,
            mv = self.margin_v,
        )
    }
}

/// ASS subtitle animation constants
mod ass_constants {
    /// Color codes (ABGR format: Blue-Green-Red)
    /// Catppuccin Mocha "Text" (#CDD6F4) - normal word color
    pub const COLOR_NORMAL: &str = "&H00F4D6CD&";

    /// Highlighted word colors
    /// Catppuccin Mocha "Crust" (#11111B) - dark text color for current word
    pub const COLOR_HIGHLIGHT_TEXT: &str = "&H001B1111&";
    /// Catppuccin Mocha "Mauve" (#CBA6F7) - outline color for current word
    pub const COLOR_HIGHLIGHT_OUTLINE: &str = "&H00F7A6CB&";

    /// Outline animation constants
    /// Border thickness values for scaling effect
    pub const BORD_THICKNESS_NONE: u32 = 0;      // No outline
    pub const BORD_THICKNESS_FULL: u32 = 8;      // Full outline thickness
    pub const BORD_THICKNESS_MIN: u32 = 2;       // Minimum thickness for "stay" phase

    /// Rounded corners: blur value (higher = more rounded)
    pub const BLUR_ROUNDED: u32 = 2;             // Slightly rounded corners

    /// Animation timing for outline phases
    pub const BOX_SCALE_IN_CS: u32 = 10;         // 100ms to scale up
    pub const BOX_SCALE_OUT_CS: u32 = 10;        // 100ms to scale down

    /// Maximum gap between words (in milliseconds) to pad
    /// Gaps smaller than this are considered minor pauses and will be extended
    /// Gaps larger are considered sentence/slide breaks and left as-is
    pub const MAX_GAP_MS: u64 = 600;
}

/// Generate an ASS subtitle file content with karaoke-style word highlighting.
///
/// # Arguments
/// * `subtitles` - List of remapped subtitles with final timeline timing
/// * `style` - Style configuration for the subtitles
/// * `play_res` - Resolution tuple (width, height) matching output video
///
/// # Returns
/// The complete ASS file content as a string.
pub fn generate_ass_file(
    subtitles: &[RemappedSubtitle],
    style: &AssStyle,
    play_res: (u32, u32),
) -> String {
    let mut output = String::new();

    // Script Info section
    writeln!(output, "[Script Info]").unwrap();
    writeln!(output, "; Generated by instantCLI").unwrap();
    writeln!(output, "ScriptType: v4.00+").unwrap();
    writeln!(output, "PlayResX: {}", play_res.0).unwrap();
    writeln!(output, "PlayResY: {}", play_res.1).unwrap();
    writeln!(output, "WrapStyle: 0").unwrap();
    writeln!(output, "ScaledBorderAndShadow: yes").unwrap();
    writeln!(output).unwrap();

    // V4+ Styles section
    writeln!(output, "[V4+ Styles]").unwrap();
    writeln!(
        output,
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding"
    )
    .unwrap();
    // Default style (for text before it's highlighted)
    writeln!(output, "{}", style.to_style_line()).unwrap();
    // Highlight style (Catppuccin Mauve for current word)
    writeln!(output, "{}", style.to_highlight_style_line()).unwrap();
    writeln!(output).unwrap();

    // Events section
    writeln!(output, "[Events]").unwrap();
    writeln!(
        output,
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"
    )
    .unwrap();

    for subtitle in subtitles {
        if subtitle.words.is_empty() {
            // No word timing, just use plain text
            let start = format_ass_timestamp(subtitle.start);
            let end = format_ass_timestamp(subtitle.end);

            writeln!(
                output,
                "Dialogue: 0,{start},{end},{style},,0,0,0,,{text}",
                start = start,
                end = end,
                style = style.name,
                text = escape_ass_text(&subtitle.text)
            )
            .unwrap();
        } else {
            // Generate karaoke with per-word dialogue lines
            // Each line shows the full text, but only highlights one word
            let word_lines = format_karaoke_text(subtitle);

            for word_line in &word_lines {
                let start = format_ass_timestamp(word_line.start);
                let end = format_ass_timestamp(word_line.end);

                writeln!(
                    output,
                    "Dialogue: 0,{start},{end},{style},,0,0,0,,{text}",
                    start = start,
                    end = end,
                    style = style.name,
                    text = word_line.text
                )
                .unwrap();
            }
        }
    }

    output
}

/// Format karaoke text with word-level timing using separate dialogue events.
///
/// This approach creates multiple dialogue lines, one per word, where each line
/// shows the full text but only highlights the current word with an animated
/// outline and custom colors.
///
/// For example, for "Hi there" with two words (assuming each word is ~500ms):
/// - Line 1 (0-0.5s): "{\1c&H001B1111&\3c&H00F7A6CB&\bord0\blur2\t(0,10\bord8)\t(40,50\bord2)}Hi{\r} {\1c&H00F4D6CD&}there"
/// - Line 2 (0.5-1.0s): "{\1c&H00F4D6CD&}Hi {\1c&H001B1111&\3c&H00F7A6CB&\bord0\blur2\t(0,10\bord8)\t(40,50\bord2)}there{\r}"
///
/// The highlighting:
/// - Current word: Dark text color (Crust #11111B) with mauve animated outline (Mauve #CBA6F7)
/// - Other words: Normal text color (Text #CDD6F4) with default outline
///
/// The outline animation:
/// - Word starts with no outline (bord=0)
/// - Scales up to thickness 8 over first 100ms (10cs)
/// - Stays at thickness 2 for most of the word duration
/// - Scales back to thickness 2 at the very end of the word
///
/// Color codes:
/// - \1c (primary color) for text color
/// - \3c (outline/border color) for the outline around text
/// - \bord (border thickness) for the outline thickness
/// - \blur (blur) for rounded corners on the outline
/// - \t(start,end,commands) for animation transitions
/// - \r to reset to style defaults
///
/// To prevent flickering, small gaps between words (< MAX_GAP_MS) are padded
/// by extending word end times to the next word's start time.
fn format_karaoke_text(subtitle: &RemappedSubtitle) -> Vec<KaraokeWordLine> {
    use ass_constants::*;

    subtitle.words.iter().enumerate().map(|(idx, current_word)| {
        // Build the full text with only the current word highlighted
        let mut formatted_text = String::new();

        for (word_idx, word) in subtitle.words.iter().enumerate() {
            // Add space before word (except first)
            if word_idx > 0 {
                formatted_text.push(' ');
            }

            // Determine if this is the highlighted word
            let is_highlighted = word_idx == idx;

            if is_highlighted {
                // Current word: dark text color with mauve animated outline
                // Calculate word duration for proper timing
                let word_duration_cs = current_word.end
                    .saturating_sub(current_word.start)
                    .as_millis() as u32 / 10; // Convert to centiseconds

                // Calculate when to start scaling out (near end of word)
                let scale_out_start = word_duration_cs.saturating_sub(BOX_SCALE_OUT_CS);

                // Start with outline at 0 thickness, rounded corners
                // Scale in: 0 -> 8 thickness over 100ms (10cs)
                // Stay: at 2 thickness for most of word duration
                // Scale out: back to 2 at end of word
                write!(
                    formatted_text,
                    "{{\\1c{}\\3c{}\\bord{}\\blur{}\\t({},{}\\bord{})\\t({},{}\\bord{})}}",
                    COLOR_HIGHLIGHT_TEXT,       // Dark text (Crust)
                    COLOR_HIGHLIGHT_OUTLINE,    // Mauve outline
                    BORD_THICKNESS_NONE,        // Start at 0
                    BLUR_ROUNDED,               // Blur=2 for rounded corners
                    0,                          // Start scale-in at time 0
                    BOX_SCALE_IN_CS,            // End scale-in at 10cs (100ms)
                    BORD_THICKNESS_FULL,        // Scale to thickness 8
                    scale_out_start,            // Start scale-out near word end
                    word_duration_cs,           // End scale-out at word end
                    BORD_THICKNESS_MIN          // Scale to thickness 2 (persistent)
                ).unwrap();
            } else {
                // Other words: normal color, no outline override
                write!(
                    formatted_text,
                    "{{\\1c{}}}",
                    COLOR_NORMAL
                ).unwrap();
            }

            formatted_text.push_str(&escape_ass_text(&word.word));

            // Reset to style defaults after highlighted word
            if is_highlighted {
                write!(formatted_text, "{{\\r}}").unwrap();
            }
        }

        // Calculate word duration, padding small gaps to prevent flickering
        let mut end = current_word.end;

        // If there's a next word:
        // 1. Pad small gaps (> 0 and < MAX_GAP_MS) to extend to next word's start
        // 2. Clip overlaps to prevent displaying two lines simultaneously
        if idx + 1 < subtitle.words.len() {
            let next_word = &subtitle.words[idx + 1];

            // Prevent overlap: if current word extends past next word start, clip it
            if end > next_word.start {
                end = next_word.start;
            } else {
                // Pad small gaps to prevent flickering
                let gap_ms = next_word.start.saturating_sub(end).as_millis();
                if gap_ms > 0 && (gap_ms as u64) < MAX_GAP_MS {
                    end = next_word.start;
                }
            }
        }

        KaraokeWordLine {
            start: current_word.start,
            end,
            text: formatted_text,
        }
    }).collect()
}

/// Per-word karaoke dialogue line data
struct KaraokeWordLine {
    start: Duration,
    end: Duration,
    text: String,
}

/// Format a Duration as an ASS timestamp (H:MM:SS.cc).
fn format_ass_timestamp(duration: Duration) -> String {
    let total_secs = duration.as_secs_f64();
    let hours = (total_secs / 3600.0).floor() as u32;
    let minutes = ((total_secs % 3600.0) / 60.0).floor() as u32;
    let seconds = (total_secs % 60.0).floor() as u32;
    let centiseconds = ((total_secs % 1.0) * 100.0).round() as u32;

    format!(
        "{}:{:02}:{:02}.{:02}",
        hours, minutes, seconds, centiseconds
    )
}

/// Escape special characters in ASS text.
fn escape_ass_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('\n', "\\N")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_ass_timestamp() {
        assert_eq!(format_ass_timestamp(Duration::from_secs(0)), "0:00:00.00");
        assert_eq!(
            format_ass_timestamp(Duration::from_millis(1500)),
            "0:00:01.50"
        );
        assert_eq!(format_ass_timestamp(Duration::from_secs(61)), "0:01:01.00");
        assert_eq!(
            format_ass_timestamp(Duration::from_secs(3661)),
            "1:01:01.00"
        );
        assert_eq!(
            format_ass_timestamp(Duration::from_millis(125)),
            "0:00:00.13"
        ); // Rounds to nearest centisecond
    }

    #[test]
    fn test_escape_ass_text() {
        assert_eq!(escape_ass_text("Hello world"), "Hello world");
        assert_eq!(escape_ass_text("Line1\nLine2"), "Line1\\NLine2");
        assert_eq!(escape_ass_text("{override}"), "\\{override\\}");
        assert_eq!(escape_ass_text("path\\to\\file"), "path\\\\to\\\\file");
    }

    #[test]
    fn test_generate_ass_file() {
        let subtitles = vec![
            RemappedSubtitle {
                start: Duration::from_secs(0),
                end: Duration::from_millis(2500),
                text: "Hello world".to_string(),
                words: vec![],
            },
            RemappedSubtitle {
                start: Duration::from_millis(3000),
                end: Duration::from_millis(5000),
                text: "Second line".to_string(),
                words: vec![],
            },
        ];

        let style = AssStyle::for_reels();
        let output = generate_ass_file(&subtitles, &style, (1080, 1920));

        assert!(output.contains("[Script Info]"));
        assert!(output.contains("PlayResX: 1080"));
        assert!(output.contains("PlayResY: 1920"));
        assert!(output.contains("[V4+ Styles]"));
        assert!(output.contains("[Events]"));
        assert!(output.contains("Dialogue: 0,0:00:00.00,0:00:02.50,Default,,0,0,0,,Hello world"));
        assert!(output.contains("Dialogue: 0,0:00:03.00,0:00:05.00,Default,,0,0,0,,Second line"));
    }

    #[test]
    fn test_style_line() {
        let style = AssStyle::for_reels();
        let line = style.to_style_line();

        // Catppuccin Mocha theme with Inter font
        assert!(line.starts_with("Style: Default,Inter,70,")); // Size 70
        assert!(line.contains(",2,")); // Alignment = 2
        assert!(line.contains(",560,")); // MarginV = 560
        // Verify Catppuccin colors are present (ABGR format)
        assert!(line.contains("&H00F4D6CD")); // Catppuccin Text color
    }

    #[test]
    fn test_catppuccin_mocha_colors() {
        let style = AssStyle::catppuccin_mocha();

        // Verify Catppuccin Mocha palette
        assert_eq!(style.primary_color, "&H00F7A6CB"); // Mauve (Sung)
        assert_eq!(style.secondary_color, "&H00F4D6CD"); // Text (Unsung)
        assert_eq!(style.outline_color, "&H001B1111"); // Crust (#11111B)
        assert!(style.back_color.contains("251818")); // Mantle (#181825)
        assert!(!style.bold); // Modern clean look = not bold
        assert_eq!(style.font_name, "Inter");
    }

    #[test]
    fn test_format_karaoke_text() {
        use crate::video::subtitles::remap::RemappedWord;

        let start = Duration::from_secs(10);
        let subtitle = RemappedSubtitle {
            start,
            end: start + Duration::from_secs(2),
            text: "Hello world test".to_string(),
            words: vec![
                RemappedWord {
                    word: "Hello".to_string(),
                    start: start,
                    end: start + Duration::from_millis(500),
                },
                RemappedWord {
                    word: "world".to_string(),
                    start: start + Duration::from_millis(600), // 100ms gap
                    end: start + Duration::from_millis(1100),
                },
                RemappedWord {
                    word: "test".to_string(),
                    start: start + Duration::from_millis(1100), // No gap
                    end: start + Duration::from_millis(1600),
                },
            ],
        };

        let word_lines = format_karaoke_text(&subtitle);

        // Verify we have one line per word
        assert_eq!(word_lines.len(), 3);

        // Verify first line: "Hello" highlighted with dark text and mauve outline animation
        // End time padded to 600ms to cover 100ms gap to next word
        assert_eq!(word_lines[0].start, start);
        assert_eq!(word_lines[0].end, start + Duration::from_millis(600));
        // Check for highlighting: dark text color with mauve outline
        assert!(word_lines[0].text.contains(r"\1c&H001B1111&")); // Dark text (Crust)
        assert!(word_lines[0].text.contains(r"\3c&H00F7A6CB&")); // Mauve outline
        assert!(word_lines[0].text.contains(r"\blur2")); // Rounded corners
        assert!(word_lines[0].text.contains(r"\bord0")); // Start at 0 thickness
        assert!(word_lines[0].text.contains(r"\t(0,10\bord8)")); // Scale up to 8
        assert!(word_lines[0].text.contains(r"\t(40,50\bord2)")); // Scale down to 2 at end (500ms word = 50cs, scale out at 40cs)
        assert!(word_lines[0].text.contains(r"\r")); // Reset after highlighted word
        // Check for normal color on non-highlighted words
        assert!(word_lines[0].text.contains(r"\1c&H00F4D6CD&")); // Normal text color
        assert!(word_lines[0].text.contains(r"Hello"));
        assert!(word_lines[0].text.contains(r"world"));
        assert!(word_lines[0].text.contains(r"test"));

        // Verify second line: "world" highlighted with animation
        // Gap to next word is 0ms, so no padding - end time stays at 1100ms
        assert_eq!(word_lines[1].start, start + Duration::from_millis(600));
        assert_eq!(word_lines[1].end, start + Duration::from_millis(1100));
        // Word duration is 500ms, so scale out should be at 40cs
        assert!(word_lines[1].text.contains(r"\1c&H001B1111&")); // Dark text
        assert!(word_lines[1].text.contains(r"\3c&H00F7A6CB&")); // Mauve outline
        assert!(word_lines[1].text.contains(r"\blur2")); // Rounded corners
        assert!(word_lines[1].text.contains(r"\t(0,10\bord8)")); // Scale up
        assert!(word_lines[1].text.contains(r"\t(40,50\bord2)")); // Scale down
        assert!(word_lines[1].text.contains(r"\r")); // Reset
        assert!(word_lines[1].text.contains(r"Hello"));
        assert!(word_lines[1].text.contains(r"world"));
        assert!(word_lines[1].text.contains(r"test"));

        // Verify third line: "test" highlighted with animation
        // No next word, so end time is not padded (1600ms)
        assert_eq!(word_lines[2].start, start + Duration::from_millis(1100));
        assert_eq!(word_lines[2].end, start + Duration::from_millis(1600));
        assert!(word_lines[2].text.contains(r"\1c&H001B1111&")); // Dark text
        assert!(word_lines[2].text.contains(r"\3c&H00F7A6CB&")); // Mauve outline
        assert!(word_lines[2].text.contains(r"\blur2")); // Rounded corners
        assert!(word_lines[2].text.contains(r"\t(0,10\bord8)")); // Scale up
        assert!(word_lines[2].text.contains(r"\t(40,50\bord2)")); // Scale down
        assert!(word_lines[2].text.contains(r"\r")); // Reset
        assert!(word_lines[2].text.contains(r"Hello"));
        assert!(word_lines[2].text.contains(r"world"));
        assert!(word_lines[2].text.contains(r"test"));

        // Verify all lines contain all three words
        for line in &word_lines {
            assert!(line.text.contains("Hello"));
            assert!(line.text.contains("world"));
            assert!(line.text.contains("test"));
        }
    }
}
