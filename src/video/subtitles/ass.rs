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
        let start = format_ass_timestamp(subtitle.start);
        let end = format_ass_timestamp(subtitle.end);

        // Generate karaoke text with word-level timing
        let text = if subtitle.words.is_empty() {
            // No word timing, just use plain text
            escape_ass_text(&subtitle.text)
        } else {
            format_karaoke_text(subtitle)
        };

        writeln!(
            output,
            "Dialogue: 0,{start},{end},{style},,0,0,0,,{text}",
            start = start,
            end = end,
            style = style.name,
            text = text
        )
        .unwrap();
    }

    output
}

/// Format karaoke text with word-level timing tags.
fn format_karaoke_text(subtitle: &RemappedSubtitle) -> String {
    let mut output = String::new();
    let mut cursor = subtitle.start;

    for (i, word) in subtitle.words.iter().enumerate() {
        // Handle gap/space before word
        if i > 0 {
            let gap = if word.start > cursor {
                word.start - cursor
            } else {
                Duration::ZERO
            };
            // Separator space gets the duration of the gap
            write!(output, "{{\\k{}}} ", duration_to_cs(gap)).unwrap();
        } else {
            // First word: check for initial gap relative to subtitle start
            if word.start > cursor {
                let gap = word.start - cursor;
                write!(output, "{{\\k{}}}", duration_to_cs(gap)).unwrap();
            }
        }

        let duration = if word.end > word.start {
            word.end - word.start
        } else {
            Duration::ZERO
        };
        write!(output, "{{\\k{}}}{}", duration_to_cs(duration), escape_ass_text(&word.word)).unwrap();

        cursor = word.end;
    }

    output
}

fn duration_to_cs(duration: Duration) -> u32 {
    (duration.as_secs_f64() * 100.0).round() as u32
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

        let karaoke = format_karaoke_text(&subtitle);

        // Expected:
        // Hello: 500ms = 50cs -> {\k50}Hello
        // Gap: 100ms = 10cs -> {\k10} (space)
        // world: 500ms = 50cs -> {\k50}world
        // No gap
        // test: 500ms = 50cs -> {\k50}test

        assert_eq!(karaoke, r"{\k50}Hello{\k10} {\k50}world{\k0} {\k50}test");
    }
}
