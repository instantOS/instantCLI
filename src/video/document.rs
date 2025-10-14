use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::Deserialize;

#[derive(Debug)]
pub struct VideoDocument {
    pub metadata: VideoMetadata,
    pub blocks: Vec<DocumentBlock>,
}

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub video: Option<VideoMetadataVideo>,
    pub transcript: Option<VideoMetadataTranscript>,
    pub generated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VideoMetadataVideo {
    pub hash: Option<String>,
    pub name: Option<String>,
    pub source: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct VideoMetadataTranscript {
    pub source: Option<PathBuf>,
}

#[derive(Debug)]
pub enum DocumentBlock {
    Segment(SegmentBlock),
    Heading(HeadingBlock),
    Separator(SeparatorBlock),
    Unhandled(UnhandledBlock),
    Music(MusicBlock),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Dialogue,
    Silence,
}

#[derive(Debug)]
pub struct SegmentBlock {
    pub range: TimeRange,
    pub text: String,
    pub kind: SegmentKind,
    pub line: usize,
}

#[derive(Debug)]
pub struct HeadingBlock {
    pub level: u32,
    pub text: String,
    pub line: usize,
}

#[derive(Debug)]
pub struct UnhandledBlock {
    pub description: String,
    pub line: usize,
}

#[derive(Debug)]
pub struct SeparatorBlock {
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum MusicDirective {
    None,
    Source(String),
}

#[derive(Debug)]
pub struct MusicBlock {
    pub directive: MusicDirective,
    pub line: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeRange {
    pub start: Duration,
    pub end: Duration,
}

impl TimeRange {
    pub fn start_seconds(&self) -> f64 {
        self.start.as_secs_f64()
    }

    pub fn end_seconds(&self) -> f64 {
        self.end.as_secs_f64()
    }
}

pub fn parse_video_document(content: &str, source_path: &Path) -> Result<VideoDocument> {
    let (front_matter, body, body_offset) = split_front_matter(content)?;

    let metadata = parse_metadata(front_matter, source_path)?;

    let line_offset = count_newlines(&content[..body_offset]);
    let blocks = parse_body_blocks(body, line_offset)?;

    Ok(VideoDocument { metadata, blocks })
}

fn parse_metadata(front_matter: Option<&str>, source_path: &Path) -> Result<VideoMetadata> {
    if let Some(fm) = front_matter {
        if fm.trim().is_empty() {
            return Ok(VideoMetadata {
                video: None,
                transcript: None,
                generated_at: None,
            });
        }
        let parsed: FrontMatter = serde_yaml::from_str(fm).with_context(|| {
            format!(
                "Failed to parse YAML front matter in {}",
                source_path.display()
            )
        })?;

        Ok(VideoMetadata {
            video: parsed.video.map(|video| VideoMetadataVideo {
                hash: video.hash,
                name: video.name,
                source: video.source.map(PathBuf::from),
            }),
            transcript: parsed.transcript.map(|transcript| VideoMetadataTranscript {
                source: transcript.source.map(PathBuf::from),
            }),
            generated_at: parsed.generated_at,
        })
    } else {
        Ok(VideoMetadata {
            video: None,
            transcript: None,
            generated_at: None,
        })
    }
}

fn parse_body_blocks(body: &str, base_line_offset: usize) -> Result<Vec<DocumentBlock>> {
    let mut blocks = Vec::new();
    let mut paragraph: Option<ParagraphState> = None;
    let mut heading: Option<HeadingState> = None;
    let mut code_block: Option<CodeBlockState> = None;

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;

    let line_map = LineMap::new(body);

    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Paragraph) => {
                paragraph = Some(ParagraphState::new(range.start));
            }
            Event::End(TagEnd::Paragraph) => {
                if let Some(state) = paragraph.take() {
                    let mut paragraph_blocks =
                        state.into_document_blocks(base_line_offset, &line_map)?;
                    blocks.append(&mut paragraph_blocks);
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let numeric_level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                heading = Some(HeadingState::new(numeric_level, range.start));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(state) = heading.take() {
                    let line = base_line_offset + line_map.line_number(state.start_byte);
                    blocks.push(DocumentBlock::Heading(state.into_block(line)));
                }
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                if info
                    .split(|c: char| c.is_whitespace())
                    .next()
                    .map(|lang| lang.eq_ignore_ascii_case("music"))
                    .unwrap_or(false)
                {
                    code_block = Some(CodeBlockState::music(range.start));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(state) = code_block.take() {
                    let line = base_line_offset + line_map.line_number(state.start_byte);
                    let directive = state.into_music_directive(line)?;
                    blocks.push(DocumentBlock::Music(MusicBlock { directive, line }));
                }
            }
            Event::Text(text) => {
                let text_string = text.into_string();
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::text(range.start, text_string.clone()));
                }
                if let Some(state) = heading.as_mut() {
                    state.push_text(text_string.clone());
                }
                if let Some(state) = code_block.as_mut() {
                    state.push_text(text_string);
                }
            }
            Event::Code(code) => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::code(range.start, code.into_string()));
                }
            }
            Event::SoftBreak => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::soft_break(range.start));
                }
                if let Some(state) = heading.as_mut() {
                    state.push_text(" ".to_string());
                }
                if let Some(state) = code_block.as_mut() {
                    state.push_newline();
                }
            }
            Event::HardBreak => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::hard_break(range.start));
                }
                if let Some(state) = heading.as_mut() {
                    state.push_text(" ".to_string());
                }
                if let Some(state) = code_block.as_mut() {
                    state.push_newline();
                }
            }
            Event::Html(text) => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::html(range.start, text.into_string()));
                }
            }
            Event::FootnoteReference(reference) => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::text(range.start, reference.into_string()));
                }
            }
            Event::Rule => {
                // Flush any in-progress paragraph before recording the separator
                if let Some(state) = paragraph.take() {
                    let mut paragraph_blocks =
                        state.into_document_blocks(base_line_offset, &line_map)?;
                    blocks.append(&mut paragraph_blocks);
                }
                // Record heading if we somehow encountered a rule mid-heading
                if let Some(state) = heading.take() {
                    let line = base_line_offset + line_map.line_number(state.start_byte);
                    blocks.push(DocumentBlock::Heading(state.into_block(line)));
                }

                let line = base_line_offset + line_map.line_number(range.start);
                blocks.push(DocumentBlock::Separator(SeparatorBlock { line }));
            }
            _ => {}
        }
    }

    Ok(blocks)
}

fn split_front_matter(content: &str) -> Result<(Option<&str>, &str, usize)> {
    if !(content.starts_with("---\n") || content.starts_with("---\r\n")) {
        return Ok((None, content, 0));
    }

    let first_newline = content
        .find('\n')
        .ok_or_else(|| anyhow!("Front matter start delimiter without newline"))?;
    let mut cursor = first_newline + 1;
    let front_start = cursor;

    while cursor <= content.len() {
        let next_newline = content[cursor..].find('\n');
        let line_end = match next_newline {
            Some(offset) => cursor + offset + 1,
            None => content.len(),
        };
        let line = &content[cursor..line_end];

        if line.trim_end_matches(['\r', '\n']) == "---" {
            let front = &content[front_start..cursor];
            let body_start = line_end;
            return Ok((Some(front), &content[body_start..], body_start));
        }

        cursor = line_end;
    }

    Err(anyhow!("Closing front matter delimiter '---' not found"))
}

fn count_newlines(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count()
}

struct ParagraphState {
    start_byte: usize,
    fragments: Vec<InlineFragment>,
}

impl ParagraphState {
    fn new(start: usize) -> Self {
        Self {
            start_byte: start,
            fragments: Vec::new(),
        }
    }

    fn push_fragment(&mut self, fragment: InlineFragment) {
        self.fragments.push(fragment);
    }

    fn into_document_blocks(
        self,
        base_line_offset: usize,
        line_map: &LineMap,
    ) -> Result<Vec<DocumentBlock>> {
        if self.fragments.is_empty() {
            return Ok(Vec::new());
        }

        let mut blocks = Vec::new();
        let mut fragments = self.fragments.into_iter().peekable();
        let mut leftover_text = Vec::new();

        while let Some(fragment) = fragments.next() {
            match fragment.kind {
                InlineFragmentKind::Code(code) => {
                    let code_line = line_map.line_number(fragment.start_byte);
                    let mut following = Vec::new();
                    while let Some(next) = fragments.peek() {
                        if matches!(next.kind, InlineFragmentKind::Code(_)) {
                            break;
                        }
                        let next_line = line_map.line_number(next.start_byte);
                        if next_line != code_line {
                            break;
                        }
                        following.push(fragments.next().unwrap());
                    }

                    let text = InlineFragment::render_many(&following).trim().to_string();
                    let line = base_line_offset + line_map.line_number(fragment.start_byte);
                    let range = parse_time_range(&code).with_context(|| {
                        format!("Invalid timestamp range `{}` at line {}", code, line)
                    })?;
                    let kind = if text.eq_ignore_ascii_case("silence") {
                        SegmentKind::Silence
                    } else {
                        SegmentKind::Dialogue
                    };
                    blocks.push(DocumentBlock::Segment(SegmentBlock {
                        range,
                        text,
                        kind,
                        line,
                    }));
                }
                _ => leftover_text.push(fragment),
            }
        }

        if blocks.is_empty() {
            let summary = InlineFragment::render_many(&leftover_text)
                .trim()
                .to_string();
            if !summary.is_empty() {
                let line = base_line_offset + line_map.line_number(self.start_byte);
                blocks.push(DocumentBlock::Unhandled(UnhandledBlock {
                    description: summary,
                    line,
                }));
            }
        } else {
            let trailing = InlineFragment::render_many(&leftover_text)
                .trim()
                .to_string();
            if !trailing.is_empty() {
                let line = base_line_offset + line_map.line_number(self.start_byte);
                blocks.push(DocumentBlock::Unhandled(UnhandledBlock {
                    description: trailing,
                    line,
                }));
            }
        }

        Ok(blocks)
    }
}

struct HeadingState {
    level: u32,
    start_byte: usize,
    text: String,
}

impl HeadingState {
    fn new(level: u32, start: usize) -> Self {
        Self {
            level,
            start_byte: start,
            text: String::new(),
        }
    }

    fn push_text(&mut self, text: String) {
        self.text.push_str(&text);
    }

    fn into_block(self, line: usize) -> HeadingBlock {
        HeadingBlock {
            level: self.level,
            text: self.text.trim().to_string(),
            line,
        }
    }
}

struct InlineFragment {
    start_byte: usize,
    kind: InlineFragmentKind,
}

struct CodeBlockState {
    kind: CodeBlockKindState,
    start_byte: usize,
    content: String,
}

enum CodeBlockKindState {
    Music,
}

impl CodeBlockState {
    fn music(start_byte: usize) -> Self {
        Self {
            kind: CodeBlockKindState::Music,
            start_byte,
            content: String::new(),
        }
    }

    fn push_text(&mut self, text: String) {
        self.content.push_str(&text);
    }

    fn push_newline(&mut self) {
        self.content.push('\n');
    }

    fn into_music_directive(self, line: usize) -> Result<MusicDirective> {
        match self.kind {
            CodeBlockKindState::Music => {
                let value = self.content.trim();
                if value.is_empty() {
                    bail!("Music block at line {} must not be empty", line);
                }
                if value.eq_ignore_ascii_case("none") {
                    Ok(MusicDirective::None)
                } else {
                    Ok(MusicDirective::Source(value.to_string()))
                }
            }
        }
    }
}

impl InlineFragment {
    fn text(start: usize, text: String) -> Self {
        Self {
            start_byte: start,
            kind: InlineFragmentKind::Text(text),
        }
    }

    fn code(start: usize, code: String) -> Self {
        Self {
            start_byte: start,
            kind: InlineFragmentKind::Code(code),
        }
    }

    fn soft_break(start: usize) -> Self {
        Self {
            start_byte: start,
            kind: InlineFragmentKind::SoftBreak,
        }
    }

    fn hard_break(start: usize) -> Self {
        Self {
            start_byte: start,
            kind: InlineFragmentKind::HardBreak,
        }
    }

    fn html(start: usize, html: String) -> Self {
        Self {
            start_byte: start,
            kind: InlineFragmentKind::Html(html),
        }
    }

    fn render_many(fragments: &[InlineFragment]) -> String {
        let mut output = String::new();
        for fragment in fragments {
            match &fragment.kind {
                InlineFragmentKind::Text(text) => output.push_str(text),
                InlineFragmentKind::Code(code) => output.push_str(code),
                InlineFragmentKind::SoftBreak => output.push(' '),
                InlineFragmentKind::HardBreak => output.push('\n'),
                InlineFragmentKind::Html(html) => output.push_str(html),
            }
        }
        output
    }
}

enum InlineFragmentKind {
    Text(String),
    Code(String),
    SoftBreak,
    HardBreak,
    Html(String),
}

fn parse_time_range(input: &str) -> Result<TimeRange> {
    let (start, end) = input
        .split_once('-')
        .ok_or_else(|| anyhow!("Missing `-` separator in timestamp range"))?;
    let start = parse_timestamp(start.trim())?;
    let end = parse_timestamp(end.trim())?;
    if end <= start {
        return Err(anyhow!("Timestamp range end must be greater than start"));
    }
    Ok(TimeRange { start, end })
}

fn parse_timestamp(value: &str) -> Result<Duration> {
    let (main, frac) = value
        .split_once('.')
        .ok_or_else(|| anyhow!("Timestamp must include fractional seconds"))?;
    let parts = main.split(':').collect::<Vec<_>>();

    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => {
            let minutes: u64 = minutes
                .parse()
                .with_context(|| format!("Invalid minute component in timestamp `{}`", value))?;
            let seconds: u64 = seconds
                .parse()
                .with_context(|| format!("Invalid second component in timestamp `{}`", value))?;
            (0, minutes, seconds)
        }
        [hours, minutes, seconds] => {
            let hours: u64 = hours
                .parse()
                .with_context(|| format!("Invalid hour component in timestamp `{}`", value))?;
            let minutes: u64 = minutes
                .parse()
                .with_context(|| format!("Invalid minute component in timestamp `{}`", value))?;
            let seconds: u64 = seconds
                .parse()
                .with_context(|| format!("Invalid second component in timestamp `{}`", value))?;
            (hours, minutes, seconds)
        }
        _ => {
            return Err(anyhow!(
                "Timestamp must be in HH:MM:SS.xxx or MM:SS.xxx format"
            ));
        }
    };
    let raw_millis: u64 = frac
        .parse()
        .with_context(|| format!("Invalid millisecond component in timestamp `{}`", value))?;

    let milliseconds = match frac.len() {
        0 => 0,
        1 => raw_millis * 100,
        2 => raw_millis * 10,
        3 => raw_millis,
        len => raw_millis / 10_u64.pow((len - 3) as u32),
    };

    if minutes >= 60 {
        return Err(anyhow!(
            "Minutes component must be less than 60 in `{}`",
            value
        ));
    }
    if seconds >= 60 {
        return Err(anyhow!(
            "Seconds component must be less than 60 in `{}`",
            value
        ));
    }

    let total_millis = hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + milliseconds;
    Ok(Duration::from_millis(total_millis))
}

struct LineMap {
    offsets: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_multiple_segments_within_single_paragraph() {
        let markdown = "`00:00.0-00:01.0` first line\n`00:01.5-00:02.0` second line\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        assert_eq!(document.blocks.len(), 2);

        match &document.blocks[0] {
            DocumentBlock::Segment(segment) => {
                assert!((segment.range.start_seconds() - 0.0).abs() < f64::EPSILON);
                assert!((segment.range.end_seconds() - 1.0).abs() < f64::EPSILON);
                assert_eq!(segment.text, "first line");
                assert_eq!(segment.line, 1);
            }
            other => panic!("Expected first block to be Segment, got {:?}", other),
        }

        match &document.blocks[1] {
            DocumentBlock::Segment(segment) => {
                assert!((segment.range.start_seconds() - 1.5).abs() < f64::EPSILON);
                assert!((segment.range.end_seconds() - 2.0).abs() < f64::EPSILON);
                assert_eq!(segment.text, "second line");
                assert_eq!(segment.line, 2);
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
            DocumentBlock::Music(block) => match &block.directive {
                MusicDirective::Source(value) => assert_eq!(value, "background.mp3"),
                other => panic!("Expected music source directive, got {:?}", other),
            },
            other => panic!("Expected music block, got {:?}", other),
        }
    }

    #[test]
    fn skips_html_comments() {
        let markdown = r#"`00:00.0-00:01.0` This should appear

<!-- This is a comment that should be skipped -->

`00:01.0-00:02.0` This should also appear

<!-- `00:02.0-00:03.0` This timestamp should be skipped -->

`00:03.0-00:04.0` Final segment"#;
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        // Should have exactly 3 segments, not 4 (the commented one should be skipped)
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
        let markdown = r#"`00:00.0-00:01.0` First segment

<!-- 
Multi-line comment
that spans multiple lines
with `00:01.0-00:02.0` embedded timestamp
-->

`00:02.0-00:03.0` Second segment"#;
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();

        // Should have exactly 2 segments, not 3 (the commented one should be skipped)
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
}

impl LineMap {
    fn new(text: &str) -> Self {
        let mut offsets = Vec::new();
        offsets.push(0);
        for (idx, _) in text.match_indices('\n') {
            offsets.push(idx + 1);
        }
        Self { offsets }
    }

    fn line_number(&self, byte_index: usize) -> usize {
        match self.offsets.binary_search(&byte_index) {
            Ok(pos) => pos + 1,
            Err(pos) => pos,
        }
    }
}

#[derive(Debug, Deserialize)]
struct FrontMatter {
    video: Option<FrontMatterVideo>,
    transcript: Option<FrontMatterTranscript>,
    #[serde(rename = "generated_at")]
    generated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FrontMatterVideo {
    hash: Option<String>,
    name: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FrontMatterTranscript {
    source: Option<String>,
}
