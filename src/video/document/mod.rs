//! Parsing and representation of video markdown documents.
//!
//! Sub-modules:
//! - `frontmatter` — splitting front matter from body
//! - `markdown`    — building markdown from transcript cues (reverse direction)
//! - `types`       — core document types (`VideoDocument`, `DocumentBlock`, etc.)
//! - `metadata`    — YAML front matter parsing into `VideoMetadata`
//! - `body`        — markdown body parsing (state machines for paragraphs, headings, etc.)
//! - `time`        — timestamp/time-range parsing
//! - `reference`   — segment reference parsing (`source@time-range`)
//! - `util`        — internal helpers (html comment stripping, line map, etc.)

pub mod frontmatter;
pub mod markdown;

pub(crate) mod body;
pub(crate) mod metadata;
pub(crate) mod reference;
pub(crate) mod time;
pub(crate) mod types;
pub(crate) mod util;

// Re-export public types for external callers
pub use types::{
    BrollBlock, DocumentBlock, HeadingBlock, MusicDirective, SegmentBlock, SegmentKind,
    UnhandledBlock, VideoDocument, VideoMetadata, VideoSource,
};

use std::path::Path;

use anyhow::Result;

use self::frontmatter::split_frontmatter;
use self::metadata::parse_metadata;
use self::reference::SegmentSourceConfig;
use self::util::{count_newlines, strip_html_comments};

pub fn parse_video_document(content: &str, source_path: &Path) -> Result<VideoDocument> {
    let (front_matter, body, body_offset) = split_frontmatter(content)?;

    let metadata = parse_metadata(front_matter, source_path)?;

    let line_offset = count_newlines(&content[..body_offset]);
    let body = strip_html_comments(body);
    let source_config = SegmentSourceConfig::from_metadata(&metadata)?;
    let blocks = body::parse_body_blocks(&body, line_offset, &source_config)?;

    Ok(VideoDocument { metadata, blocks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_multiple_segments_within_single_paragraph() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "`a@00:00.0-00:01.0` first line\n",
            "`a@00:01.5-00:02.0` second line\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 2);

        match &document.blocks[0] {
            DocumentBlock::Segment(segment) => {
                assert!((segment.range.start_seconds() - 0.0).abs() < f64::EPSILON);
                assert!((segment.range.end_seconds() - 1.0).abs() < f64::EPSILON);
                assert_eq!(segment.text, "first line");
                assert_eq!(segment.source_id, "a");
            }
            other => panic!("Expected first block to be Segment, got {:?}", other),
        }

        match &document.blocks[1] {
            DocumentBlock::Segment(segment) => {
                assert!((segment.range.start_seconds() - 1.5).abs() < f64::EPSILON);
                assert!((segment.range.end_seconds() - 2.0).abs() < f64::EPSILON);
                assert_eq!(segment.text, "second line");
                assert_eq!(segment.source_id, "a");
            }
            other => panic!("Expected second block to be Segment, got {:?}", other),
        }
    }

    #[test]
    fn preserves_unhandled_text_when_no_segments() {
        let markdown = "This is an intro paragraph without timestamps.";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 1);
        match &document.blocks[0] {
            DocumentBlock::Unhandled(unhandled) => {
                assert_eq!(
                    unhandled.description,
                    "This is an intro paragraph without timestamps."
                );
            }
            other => panic!("Expected Unhandled block, got {:?}", other),
        }
    }

    #[test]
    fn parses_music_blocks() {
        let markdown = "```music\nbackground.mp3\n```";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 1);
        match &document.blocks[0] {
            DocumentBlock::Music(directive) => match directive {
                MusicDirective::Source(value) => assert_eq!(value, "background.mp3"),
                other => panic!("Expected music source directive, got {:?}", other),
            },
            other => panic!("Expected music block, got {:?}", other),
        }
    }

    #[test]
    fn skips_html_comments() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "`a@00:00.0-00:01.0` This should appear\n\n",
            "<!-- This is a comment that should be skipped -->\n\n",
            "`a@00:01.0-00:02.0` This should also appear\n\n",
            "<!-- `a@00:02.0-00:03.0` This timestamp should be skipped -->\n\n",
            "`a@00:03.0-00:04.0` Final segment",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 3);

        match &document.blocks[0] {
            DocumentBlock::Segment(segment) => {
                assert_eq!(segment.text, "This should appear");
            }
            other => panic!("Expected first block to be Segment, got {:?}", other),
        }

        match &document.blocks[1] {
            DocumentBlock::Segment(segment) => {
                assert_eq!(segment.text, "This should also appear");
            }
            other => panic!("Expected second block to be Segment, got {:?}", other),
        }

        match &document.blocks[2] {
            DocumentBlock::Segment(segment) => {
                assert_eq!(segment.text, "Final segment");
            }
            other => panic!("Expected third block to be Segment, got {:?}", other),
        }
    }

    #[test]
    fn skips_multiline_html_comments() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "`a@00:00.0-00:01.0` First segment\n\n",
            "<!-- \n",
            "Multi-line comment\n",
            "that spans multiple lines\n",
            "with `a@00:01.0-00:02.0` embedded timestamp\n",
            "-->\n\n",
            "`a@00:02.0-00:03.0` Second segment",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 2);

        match &document.blocks[0] {
            DocumentBlock::Segment(segment) => {
                assert_eq!(segment.text, "First segment");
            }
            other => panic!("Expected first block to be Segment, got {:?}", other),
        }

        match &document.blocks[1] {
            DocumentBlock::Segment(segment) => {
                assert_eq!(segment.text, "Second segment");
            }
            other => panic!("Expected second block to be Segment, got {:?}", other),
        }
    }

    #[test]
    fn strips_comment_wrapped_timestamp_lines_before_parsing() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "`a@00:00.0-00:04.0` Keep this\n\n",
            "<!--\n",
            "`a@00:04.0-00:08.0` Drop this whole clip\n",
            "`a@00:08.0-00:12.0` Drop this too\n",
            "-->\n\n",
            "`a@00:12.0-00:16.0` Keep this too",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 2);
        match &document.blocks[0] {
            DocumentBlock::Segment(segment) => assert_eq!(segment.text, "Keep this"),
            other => panic!("Expected Segment, got {:?}", other),
        }
        match &document.blocks[1] {
            DocumentBlock::Segment(segment) => assert_eq!(segment.text, "Keep this too"),
            other => panic!("Expected Segment, got {:?}", other),
        }
    }

    #[test]
    fn parses_explicit_source_prefix() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "- id: b\n  source: video_b.mp4\n  transcript: b.json\n",
            "default_source: a\n",
            "---\n",
            "`a@00:00.0-00:01.0` hello\n",
            "`b@00:01.0-00:02.0` world\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        let segments: Vec<_> = document
            .blocks
            .iter()
            .filter_map(|block| match block {
                DocumentBlock::Segment(segment) => Some(segment),
                _ => None,
            })
            .collect();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source_id, "a");
        assert_eq!(segments[1].source_id, "b");
    }

    #[test]
    fn rejects_missing_source_prefix_when_multiple_sources() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "- id: b\n  source: video_b.mp4\n  transcript: b.json\n",
            "default_source: a\n",
            "---\n",
            "`00:00.0-00:01.0` missing prefix\n",
        );

        let err = parse_video_document(markdown, Path::new("test.md")).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Missing source id for timestamp"));
    }

    #[test]
    fn allows_silence_without_prefix_using_previous_source() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "- id: b\n  source: video_b.mp4\n  transcript: b.json\n",
            "default_source: a\n",
            "---\n",
            "`00:00.0-00:01.0` SILENCE\n",
            "`a@00:01.0-00:02.0` hello\n",
        );

        let err = parse_video_document(markdown, Path::new("test.md")).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Missing source id for timestamp"));
    }

    #[test]
    fn parses_broll_from_blockquote_with_segment() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "> `a@00:05.0-00:10.0` some optional text\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 1);
        match &document.blocks[0] {
            DocumentBlock::Broll(broll) => {
                assert!((broll.range.start_seconds() - 5.0).abs() < f64::EPSILON);
                assert!((broll.range.end_seconds() - 10.0).abs() < f64::EPSILON);
                assert_eq!(broll.text, "some optional text");
                assert_eq!(broll.source_id, "a");
            }
            other => panic!("Expected Broll block, got {:?}", other),
        }
    }

    #[test]
    fn blockquote_without_segment_becomes_unhandled() {
        let markdown = "> This is just a quote without timestamps.\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 1);
        match &document.blocks[0] {
            DocumentBlock::Unhandled(unhandled) => {
                assert!(unhandled.description.starts_with("> "));
            }
            other => panic!("Expected Unhandled block, got {:?}", other),
        }
    }

    #[test]
    fn preserves_non_timestamp_inline_code_as_text() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "testing stuff `test` stuff\n",
            "`a@00:01.0-00:02.0` actual segment\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(
            document.blocks.len(),
            2,
            "Expected 2 blocks: one for segment, one for text with inline code"
        );

        match &document.blocks[0] {
            DocumentBlock::Segment(segment) => {
                assert_eq!(segment.text, "actual segment");
                assert_eq!(segment.source_id, "a");
            }
            other => panic!("Expected Segment block, got {:?}", other),
        }

        match &document.blocks[1] {
            DocumentBlock::Unhandled(unhandled) => {
                assert!(
                    unhandled.description.contains("`test`"),
                    "Inline code should be preserved as text, got: {}",
                    unhandled.description
                );
                assert!(unhandled.description.contains("testing stuff"));
            }
            other => panic!(
                "Expected Unhandled block for non-timestamp code, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn looks_like_timestamp_reference_works() {
        assert!(super::reference::looks_like_timestamp_reference(
            "00:01.0-00:02.0"
        ));
        assert!(super::reference::looks_like_timestamp_reference(
            "a@00:01.0-00:02.0"
        ));
        assert!(super::reference::looks_like_timestamp_reference("00:01.0"));
        assert!(super::reference::looks_like_timestamp_reference(
            "01:23:45.678"
        ));
        assert!(super::reference::looks_like_timestamp_reference(
            "source_id@01:23:45.678-02:34:56.789"
        ));

        assert!(!super::reference::looks_like_timestamp_reference("test"));
        assert!(!super::reference::looks_like_timestamp_reference("ins"));
        assert!(!super::reference::looks_like_timestamp_reference(
            "`ins video`"
        ));
        assert!(!super::reference::looks_like_timestamp_reference(
            "some text"
        ));
        assert!(!super::reference::looks_like_timestamp_reference("00:01"));
        assert!(!super::reference::looks_like_timestamp_reference("01.0"));
    }

    #[test]
    fn preserves_code_blocks_as_unhandled() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "Some text before\n",
            "\n",
            "```bash\n",
            "curl -fsSL https://example.com/install.sh | sh\n",
            "```\n",
            "\n",
            "Some text after\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(
            document.blocks.len(),
            3,
            "Expected 3 blocks: text, code block, text"
        );

        match &document.blocks[0] {
            DocumentBlock::Unhandled(unhandled) => {
                assert!(unhandled.description.contains("Some text before"));
            }
            other => panic!("Expected Unhandled block for text, got {:?}", other),
        }

        match &document.blocks[1] {
            DocumentBlock::Unhandled(unhandled) => {
                assert!(
                    unhandled.description.contains("```bash"),
                    "Code block should have bash language"
                );
                assert!(
                    unhandled.description.contains("curl -fsSL"),
                    "Code block should contain the curl command"
                );
                assert!(
                    unhandled.description.contains("```"),
                    "Code block should end with ```"
                );
            }
            other => panic!("Expected Unhandled block for code block, got {:?}", other),
        }

        match &document.blocks[2] {
            DocumentBlock::Unhandled(unhandled) => {
                assert!(unhandled.description.contains("Some text after"));
            }
            other => panic!("Expected Unhandled block for text, got {:?}", other),
        }
    }

    #[test]
    fn preserves_code_blocks_without_language() {
        let markdown = concat!(
            "---\n",
            "sources:\n",
            "- id: a\n  source: video_a.mp4\n  transcript: a.json\n",
            "default_source: a\n",
            "---\n",
            "```\n",
            "plain code\n",
            "```\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 1);
        match &document.blocks[0] {
            DocumentBlock::Unhandled(unhandled) => {
                assert!(unhandled.description.contains("```"));
                assert!(unhandled.description.contains("plain code"));
            }
            other => panic!("Expected Unhandled block, got {:?}", other),
        }
    }
}
