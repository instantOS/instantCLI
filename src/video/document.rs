use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
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
    Unhandled(UnhandledBlock),
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
                    let line = base_line_offset + line_map.line_number(state.start_byte);
                    if let Some(block) = state.into_document_block(line)? {
                        blocks.push(block);
                    }
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
            Event::Text(text) => {
                let text_string = text.into_string();
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::Text(text_string.clone()));
                }
                if let Some(state) = heading.as_mut() {
                    state.push_text(text_string);
                }
            }
            Event::Code(code) => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::Code(code.into_string()));
                }
            }
            Event::SoftBreak => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::SoftBreak);
                }
                if let Some(state) = heading.as_mut() {
                    state.push_text(" ".to_string());
                }
            }
            Event::HardBreak => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::HardBreak);
                }
                if let Some(state) = heading.as_mut() {
                    state.push_text(" ".to_string());
                }
            }
            Event::Html(text) => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::Html(text.into_string()));
                }
            }
            Event::FootnoteReference(reference) => {
                if let Some(state) = paragraph.as_mut() {
                    state.push_fragment(InlineFragment::Text(reference.into_string()));
                }
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

    fn into_document_block(self, line: usize) -> Result<Option<DocumentBlock>> {
        let mut fragments = self.fragments.into_iter();

        let first = match fragments.next() {
            Some(fragment) => fragment,
            None => return Ok(None),
        };

        let code = match first {
            InlineFragment::Code(code) => code,
            InlineFragment::Text(text) if text.trim().is_empty() => match fragments.next() {
                Some(InlineFragment::Code(code)) => code,
                other => {
                    let mut remaining = Vec::new();
                    if let Some(fragment) = other {
                        remaining.push(fragment);
                    }
                    remaining.extend(fragments);
                    let summary = InlineFragment::render_many(remaining);
                    if summary.trim().is_empty() {
                        return Ok(None);
                    }
                    return Ok(Some(DocumentBlock::Unhandled(UnhandledBlock {
                        description: summary.trim().to_string(),
                        line,
                    })));
                }
            },
            other => {
                let mut all_fragments = vec![other];
                all_fragments.extend(fragments);
                let summary = InlineFragment::render_many(all_fragments);
                if summary.trim().is_empty() {
                    return Ok(None);
                }
                return Ok(Some(DocumentBlock::Unhandled(UnhandledBlock {
                    description: summary.trim().to_string(),
                    line,
                })));
            }
        };

        let text_fragments = fragments.collect::<Vec<_>>();
        let text = InlineFragment::render_many(text_fragments)
            .trim()
            .to_string();

        let range = parse_time_range(&code)
            .with_context(|| format!("Invalid timestamp range `{}` at line {}", code, line))?;

        let kind = if text.eq_ignore_ascii_case("silence") {
            SegmentKind::Silence
        } else {
            SegmentKind::Dialogue
        };

        Ok(Some(DocumentBlock::Segment(SegmentBlock {
            range,
            text,
            kind,
            line,
        })))
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

enum InlineFragment {
    Text(String),
    Code(String),
    SoftBreak,
    HardBreak,
    Html(String),
}

impl InlineFragment {
    fn render_many(fragments: Vec<InlineFragment>) -> String {
        let mut output = String::new();
        for fragment in fragments {
            match fragment {
                InlineFragment::Text(text) => output.push_str(&text),
                InlineFragment::Code(code) => output.push_str(&code),
                InlineFragment::SoftBreak => output.push(' '),
                InlineFragment::HardBreak => output.push('\n'),
                InlineFragment::Html(html) => output.push_str(&html),
            }
        }
        output
    }
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
        .ok_or_else(|| anyhow!("Timestamp must include milliseconds"))?;
    let parts = main.split(':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(anyhow!("Timestamp must be in HH:MM:SS.mmm format"));
    }
    let hours: u64 = parts[0]
        .parse()
        .with_context(|| format!("Invalid hour component in timestamp `{}`", value))?;
    let minutes: u64 = parts[1]
        .parse()
        .with_context(|| format!("Invalid minute component in timestamp `{}`", value))?;
    let seconds: u64 = parts[2]
        .parse()
        .with_context(|| format!("Invalid second component in timestamp `{}`", value))?;
    let raw_millis: u64 = frac
        .parse()
        .with_context(|| format!("Invalid millisecond component in timestamp `{}`", value))?;

    let milliseconds = match frac.len() {
        0 => 0,
        1 => raw_millis * 100,
        2 => raw_millis * 10,
        3 => raw_millis,
        len => {
            let scaled = raw_millis / 10_u64.pow((len - 3) as u32);
            scaled
        }
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
