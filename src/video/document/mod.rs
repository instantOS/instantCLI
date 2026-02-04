pub mod frontmatter;
pub mod markdown;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::Deserialize;

use self::frontmatter::split_frontmatter;

const DEFAULT_SOURCE_ID: &str = "a";

#[derive(Debug)]
pub struct VideoDocument {
    pub metadata: VideoMetadata,
    pub blocks: Vec<DocumentBlock>,
}

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub sources: Vec<VideoSource>,
    pub default_source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VideoSource {
    pub id: String,
    pub name: Option<String>,
    pub source: PathBuf,
    pub transcript: PathBuf,
    pub audio: PathBuf,
    pub hash: Option<String>,
}

#[derive(Debug)]
pub enum DocumentBlock {
    Segment(SegmentBlock),
    Heading(HeadingBlock),
    Separator,
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
    pub source_id: String,
}

#[derive(Debug)]
pub struct HeadingBlock {
    pub level: u32,
    pub text: String,
}

#[derive(Debug)]
pub struct UnhandledBlock {
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum MusicDirective {
    None,
    Source(String),
}

#[derive(Debug)]
pub struct MusicBlock {
    pub directive: MusicDirective,
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
    let (front_matter, body, body_offset) = split_frontmatter(content)?;

    let metadata = parse_metadata(front_matter, source_path)?;

    let line_offset = count_newlines(&content[..body_offset]);
    let body = strip_html_comments(body);
    let source_config = SegmentSourceConfig::from_metadata(&metadata)?;
    let blocks = parse_body_blocks(&body, line_offset, &source_config)?;

    Ok(VideoDocument { metadata, blocks })
}

fn parse_metadata(front_matter: Option<&str>, source_path: &Path) -> Result<VideoMetadata> {
    let Some(fm) = front_matter else {
        return Ok(VideoMetadata {
            sources: Vec::new(),
            default_source: None,
        });
    };

    if fm.trim().is_empty() {
        return Ok(VideoMetadata {
            sources: Vec::new(),
            default_source: None,
        });
    }

    let parsed: FrontMatter = serde_yaml::from_str(fm).with_context(|| {
        format!(
            "Failed to parse YAML front matter in {}",
            source_path.display()
        )
    })?;

    let mut sources = Vec::new();
    if let Some(entries) = parsed.sources {
        for entry in entries {
            let id = entry
                .id
                .ok_or_else(|| anyhow!("Each source must include an id"))?;
            let id = id.trim().to_string();
            if id.is_empty() {
                bail!("Source id must not be empty");
            }
            if id.contains(':') || id.contains(char::is_whitespace) {
                bail!("Source id `{}` must not include ':' or whitespace", id);
            }
            let source = entry
                .source
                .ok_or_else(|| anyhow!("Source `{}` is missing `source`", id))?;
            let transcript = entry
                .transcript
                .ok_or_else(|| anyhow!("Source `{}` is missing `transcript`", id))?;
            sources.push(VideoSource {
                id,
                name: entry.name,
                source: PathBuf::from(source),
                transcript: PathBuf::from(transcript),
                audio: PathBuf::new(),
                hash: entry.hash,
            });
        }
    }

    if sources.is_empty() {
        if let Some(video) = parsed.video {
            let legacy_id = parsed
                .default_source
                .clone()
                .unwrap_or_else(|| DEFAULT_SOURCE_ID.to_string());
            let source = video
                .source
                .ok_or_else(|| anyhow!("Legacy front matter is missing `video.source`"))?;
            let transcript = parsed
                .transcript
                .and_then(|value| value.source)
                .ok_or_else(|| anyhow!("Legacy front matter is missing `transcript.source`"))?;
            sources.push(VideoSource {
                id: legacy_id.clone(),
                name: video.name,
                source: PathBuf::from(source),
                transcript: PathBuf::from(transcript),
                audio: PathBuf::new(),
                hash: video.hash,
            });
            return Ok(VideoMetadata {
                sources,
                default_source: Some(legacy_id),
            });
        }
        return Ok(VideoMetadata {
            sources: Vec::new(),
            default_source: None,
        });
    }

    let mut default_source = parsed.default_source;
    if default_source.is_none() && sources.len() == 1 {
        default_source = Some(sources[0].id.clone());
    }

    if let Some(default_id) = default_source.as_ref() {
        if !sources.iter().any(|source| &source.id == default_id) {
            bail!(
                "default_source `{}` does not match any declared source id",
                default_id
            );
        }
    }

    let mut seen = HashSet::new();
    for source in &sources {
        if !seen.insert(source.id.clone()) {
            bail!("Duplicate source id `{}` in front matter", source.id);
        }
    }

    Ok(VideoMetadata {
        sources,
        default_source,
    })
}

fn parse_body_blocks(
    body: &str,
    base_line_offset: usize,
    source_config: &SegmentSourceConfig,
) -> Result<Vec<DocumentBlock>> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;

    let line_map = LineMap::new(body);
    let mut state = BodyParserState::new(base_line_offset, &line_map, source_config);

    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        state.process_event(event, range)?;
    }

    Ok(state.into_blocks())
}

struct BodyParserState<'a> {
    blocks: Vec<DocumentBlock>,
    paragraph: Option<ParagraphState>,
    heading: Option<HeadingState>,
    code_block: Option<CodeBlockState>,
    blockquote: Option<BlockquoteState>,
    base_line_offset: usize,
    line_map: &'a LineMap,
    source_config: &'a SegmentSourceConfig,
}

impl<'a> BodyParserState<'a> {
    fn new(
        base_line_offset: usize,
        line_map: &'a LineMap,
        source_config: &'a SegmentSourceConfig,
    ) -> Self {
        Self {
            blocks: Vec::new(),
            paragraph: None,
            heading: None,
            code_block: None,
            blockquote: None,
            base_line_offset,
            line_map,
            source_config,
        }
    }

    fn byte_to_line(&self, byte_offset: usize) -> usize {
        self.line_map.line_number(byte_offset)
    }

    fn process_event(&mut self, event: Event, range: std::ops::Range<usize>) -> Result<()> {
        match event {
            Event::Start(Tag::Paragraph) => {
                self.paragraph = Some(ParagraphState::new());
            }
            Event::End(TagEnd::Paragraph) => {
                self.flush_paragraph()?;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                self.heading = Some(HeadingState::new(heading_level_to_u32(level)));
            }
            Event::End(TagEnd::Heading(_)) => {
                self.flush_heading();
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                if is_music_code_block(&info) {
                    let line = self.byte_to_line(range.start);
                    self.code_block = Some(CodeBlockState::music(line));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                self.flush_code_block()?;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.blockquote = Some(BlockquoteState::new());
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush_blockquote();
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                // Capture the image markdown syntax to preserve it for Pandoc
                if let Some(state) = self.paragraph.as_mut() {
                    state.set_pending_image(dest_url.into_string(), title.into_string());
                }
            }
            Event::End(TagEnd::Image) => {
                // Flush the pending image with its alt text
                let line = self.byte_to_line(range.start);
                if let Some(state) = self.paragraph.as_mut() {
                    state.flush_pending_image(line);
                }
            }
            Event::Text(text) => {
                self.handle_text(text.into_string(), range.start);
            }
            Event::Code(code) => {
                let line = self.byte_to_line(range.start);
                if let Some(state) = self.paragraph.as_mut() {
                    state.push_fragment(InlineFragment::code(line, code.into_string()));
                }
            }
            Event::SoftBreak => {
                self.handle_break(range.start, false);
            }
            Event::HardBreak => {
                self.handle_break(range.start, true);
            }
            Event::Html(text) => {
                let line = self.byte_to_line(range.start);
                if let Some(state) = self.paragraph.as_mut() {
                    state.push_fragment(InlineFragment::html(line, text.into_string()));
                }
            }
            Event::FootnoteReference(reference) => {
                let line = self.byte_to_line(range.start);
                if let Some(state) = self.paragraph.as_mut() {
                    state.push_fragment(InlineFragment::text(line, reference.into_string()));
                }
            }
            Event::Rule => {
                self.flush_paragraph()?;
                self.flush_heading();
                self.blocks.push(DocumentBlock::Separator);
            }
            _ => {}
        }
        Ok(())
    }

    fn flush_paragraph(&mut self) -> Result<()> {
        if let Some(state) = self.paragraph.take() {
            let mut paragraph_blocks =
                state.into_document_blocks(self.base_line_offset, self.source_config)?;
            self.blocks.append(&mut paragraph_blocks);
        }
        Ok(())
    }

    fn flush_heading(&mut self) {
        if let Some(state) = self.heading.take() {
            self.blocks.push(DocumentBlock::Heading(state.into_block()));
        }
    }

    fn flush_code_block(&mut self) -> Result<()> {
        if let Some(state) = self.code_block.take() {
            let line = self.base_line_offset + state.start_line;
            let directive = state.into_music_directive(line)?;
            self.blocks
                .push(DocumentBlock::Music(MusicBlock { directive }));
        }
        Ok(())
    }

    fn flush_blockquote(&mut self) {
        if let Some(state) = self.blockquote.take() {
            // Re-wrap content with `> ` prefix so it renders as <blockquote> in Pandoc
            let markdown = state
                .content
                .lines()
                .map(|line| format!("> {}", line))
                .collect::<Vec<_>>()
                .join("\n");
            if !markdown.trim().is_empty() {
                self.blocks.push(DocumentBlock::Unhandled(UnhandledBlock {
                    description: markdown,
                }));
            }
        }
    }

    fn handle_text(&mut self, text: String, start: usize) {
        if let Some(state) = self.blockquote.as_mut() {
            state.push_text(text.clone());
        } else if self.paragraph.is_some() {
            // If inside an image tag, accumulate as alt text; otherwise add as text fragment
            let line = self.byte_to_line(start);
            let state = self.paragraph.as_mut().unwrap();
            if state.is_inside_image() {
                state.push_image_alt(&text);
            } else {
                state.push_fragment(InlineFragment::text(line, text.clone()));
            }
        }
        if let Some(state) = self.heading.as_mut() {
            state.push_text(text.clone());
        }
        if let Some(state) = self.code_block.as_mut() {
            state.push_text(text);
        }
    }

    fn handle_break(&mut self, start: usize, hard: bool) {
        if let Some(state) = self.blockquote.as_mut() {
            state.push_newline();
        } else if self.paragraph.is_some() {
            let line = self.byte_to_line(start);
            let state = self.paragraph.as_mut().unwrap();
            if hard {
                state.push_fragment(InlineFragment::hard_break(line));
            } else {
                state.push_fragment(InlineFragment::soft_break(line));
            }
        }
        if let Some(state) = self.heading.as_mut() {
            state.push_text(" ".to_string());
        }
        if let Some(state) = self.code_block.as_mut() {
            state.push_newline();
        }
    }

    fn into_blocks(self) -> Vec<DocumentBlock> {
        self.blocks
    }
}

fn heading_level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn is_music_code_block(info: &str) -> bool {
    info.split(|c: char| c.is_whitespace())
        .next()
        .map(|lang| lang.eq_ignore_ascii_case("music"))
        .unwrap_or(false)
}

fn count_newlines(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count()
}

fn strip_html_comments(input: &str) -> String {
    // pulldown-cmark emits HTML comments as `Event::Html`, which we currently ignore.
    // That means comment-contained timestamps can still be parsed from nested events.
    // Strip them up-front so commented sections behave like deleted lines.
    let mut output = String::with_capacity(input.len());

    let mut cursor = 0usize;
    while let Some(start_rel) = input[cursor..].find("<!--") {
        let start = cursor + start_rel;
        output.push_str(&input[cursor..start]);

        let after_start = start + "<!--".len();
        if let Some(end_rel) = input[after_start..].find("-->") {
            let end = after_start + end_rel + "-->".len();
            cursor = end;
        } else {
            // Unclosed comment: drop the remainder.
            return output;
        }
    }

    output.push_str(&input[cursor..]);
    output
}

struct ParagraphState {
    fragments: Vec<InlineFragment>,
    /// Pending image to be flushed when we encounter the end tag
    pending_image: Option<(String, String)>, // (url, title)
    /// Alt text accumulated while inside an image tag
    pending_image_alt: String,
}

impl ParagraphState {
    fn new() -> Self {
        Self {
            fragments: Vec::new(),
            pending_image: None,
            pending_image_alt: String::new(),
        }
    }

    fn push_fragment(&mut self, fragment: InlineFragment) {
        self.fragments.push(fragment);
    }

    fn set_pending_image(&mut self, url: String, title: String) {
        self.pending_image = Some((url, title));
        self.pending_image_alt.clear();
    }

    fn push_image_alt(&mut self, text: &str) {
        self.pending_image_alt.push_str(text);
    }

    fn flush_pending_image(&mut self, line: usize) {
        if let Some((url, title)) = self.pending_image.take() {
            let alt = std::mem::take(&mut self.pending_image_alt);
            self.fragments
                .push(InlineFragment::image(line, url, alt, title));
        }
    }

    fn is_inside_image(&self) -> bool {
        self.pending_image.is_some()
    }

    fn into_document_blocks(
        self,
        base_line_offset: usize,
        source_config: &SegmentSourceConfig,
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
                    let code_line = fragment.start_line;
                    let mut following = Vec::new();
                    while let Some(next) = fragments.peek() {
                        if matches!(next.kind, InlineFragmentKind::Code(_)) {
                            break;
                        }
                        let next_line = next.start_line;
                        if next_line != code_line {
                            break;
                        }
                        following.push(fragments.next().unwrap());
                    }

                    let text = InlineFragment::render_many(&following).trim().to_string();
                    let line = base_line_offset + code_line;
                    let (source_id, range) = parse_segment_reference(&code, source_config, line)
                        .with_context(|| {
                            format!("Invalid timestamp `{}` at line {}", code, line)
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
                        source_id,
                    }));
                }
                _ => leftover_text.push(fragment),
            }
        }

        let leftover_content = InlineFragment::render_many(&leftover_text)
            .trim()
            .to_string();
        if !leftover_content.is_empty() {
            blocks.push(DocumentBlock::Unhandled(UnhandledBlock {
                description: leftover_content,
            }));
        }

        Ok(blocks)
    }
}

struct HeadingState {
    level: u32,
    text: String,
}

impl HeadingState {
    fn new(level: u32) -> Self {
        Self {
            level,
            text: String::new(),
        }
    }

    fn push_text(&mut self, text: String) {
        self.text.push_str(&text);
    }

    fn into_block(self) -> HeadingBlock {
        HeadingBlock {
            level: self.level,
            text: self.text.trim().to_string(),
        }
    }
}

struct BlockquoteState {
    content: String,
}

impl BlockquoteState {
    fn new() -> Self {
        Self {
            content: String::new(),
        }
    }

    fn push_text(&mut self, text: String) {
        self.content.push_str(&text);
    }

    fn push_newline(&mut self) {
        self.content.push('\n');
    }
}

struct InlineFragment {
    start_line: usize,
    kind: InlineFragmentKind,
}

struct CodeBlockState {
    kind: CodeBlockKindState,
    start_line: usize,
    content: String,
}

enum CodeBlockKindState {
    Music,
}

impl CodeBlockState {
    fn music(start_line: usize) -> Self {
        Self {
            kind: CodeBlockKindState::Music,
            start_line,
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
            start_line: start,
            kind: InlineFragmentKind::Text(text),
        }
    }

    fn code(start: usize, code: String) -> Self {
        Self {
            start_line: start,
            kind: InlineFragmentKind::Code(code),
        }
    }

    fn soft_break(start: usize) -> Self {
        Self {
            start_line: start,
            kind: InlineFragmentKind::SoftBreak,
        }
    }

    fn hard_break(start: usize) -> Self {
        Self {
            start_line: start,
            kind: InlineFragmentKind::HardBreak,
        }
    }

    fn html(start: usize, html: String) -> Self {
        Self {
            start_line: start,
            kind: InlineFragmentKind::Html(html),
        }
    }

    fn image(start: usize, url: String, alt: String, title: String) -> Self {
        Self {
            start_line: start,
            kind: InlineFragmentKind::Image { url, alt, title },
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
                InlineFragmentKind::Image { url, alt, title } => {
                    // Reconstruct markdown image syntax
                    if title.is_empty() {
                        output.push_str(&format!("![{}]({})", alt, url));
                    } else {
                        output.push_str(&format!("![{}]({} \"{}\")", alt, url, title));
                    }
                }
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
    Image {
        url: String,
        alt: String,
        title: String,
    },
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

struct SegmentSourceConfig {
    default_source: String,
    known_sources: HashSet<String>,
    require_explicit: bool,
}

impl SegmentSourceConfig {
    fn from_metadata(metadata: &VideoMetadata) -> Result<Self> {
        if metadata.sources.is_empty() {
            return Ok(Self {
                default_source: DEFAULT_SOURCE_ID.to_string(),
                known_sources: HashSet::new(),
                require_explicit: false,
            });
        }

        let mut known_sources = HashSet::new();
        for source in &metadata.sources {
            known_sources.insert(source.id.clone());
        }

        let require_explicit = metadata.sources.len() > 1;
        let default_source = metadata
            .default_source
            .clone()
            .unwrap_or_else(|| metadata.sources[0].id.clone());

        if !known_sources.contains(&default_source) {
            bail!(
                "default_source `{}` is not a declared source",
                default_source
            );
        }

        Ok(Self {
            default_source,
            known_sources,
            require_explicit,
        })
    }
}

fn parse_segment_reference(
    input: &str,
    source_config: &SegmentSourceConfig,
    line: usize,
) -> Result<(String, TimeRange)> {
    let trimmed = input.trim();
    let mut explicit_source: Option<&str> = None;
    let mut range_str = trimmed;

    if let Some((prefix, rest)) = trimmed.split_once(':') {
        let prefix = prefix.trim();
        let rest = rest.trim();
        let prefix_valid = is_valid_source_id(prefix);
        if !source_config.known_sources.is_empty() {
            if source_config.known_sources.contains(prefix) {
                explicit_source = Some(prefix);
                range_str = rest;
            } else if prefix_valid {
                bail!("Unknown source id `{}` at line {}", prefix, line);
            }
        } else if prefix_valid {
            explicit_source = Some(prefix);
            range_str = rest;
        }
    }

    if explicit_source.is_none() && source_config.require_explicit {
        bail!("Missing source id for timestamp at line {}", line);
    }

    let source_id = explicit_source.unwrap_or(source_config.default_source.as_str());

    let source_id = source_id.trim();
    if source_id.is_empty() {
        bail!("Missing source id at line {}", line);
    }
    if source_id.contains(char::is_whitespace) {
        bail!(
            "Source id `{}` at line {} must not include whitespace",
            source_id,
            line
        );
    }

    if !source_config.known_sources.is_empty() && !source_config.known_sources.contains(source_id) {
        bail!("Unknown source id `{}` at line {}", source_id, line);
    }

    let range = parse_time_range(range_str).with_context(|| {
        format!(
            "Invalid timestamp range `{}` for source `{}`",
            range_str, source_id
        )
    })?;

    Ok((source_id.to_string(), range))
}

fn is_valid_source_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    if value.contains(char::is_whitespace) || value.contains(':') {
        return false;
    }
    for ch in chars {
        if !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
            return false;
        }
    }
    true
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

    #[test]
    fn strips_comment_wrapped_timestamp_lines_before_parsing() {
        let markdown = r#"`00:00.0-00:04.0` Keep this

<!--
`00:04.0-00:08.0` Drop this whole clip
`00:08.0-00:12.0` Drop this too
-->

`00:12.0-00:16.0` Keep this too"#;

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
            "`a:00:00.0-00:01.0` hello\n",
            "`b:00:01.0-00:02.0` world\n",
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
        assert!(err.to_string().contains("Missing source id for timestamp"));
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
    sources: Option<Vec<FrontMatterSource>>,
    default_source: Option<String>,
    video: Option<FrontMatterVideo>,
    transcript: Option<FrontMatterTranscript>,
}

#[derive(Debug, Deserialize)]
struct FrontMatterSource {
    id: Option<String>,
    name: Option<String>,
    source: Option<String>,
    transcript: Option<String>,
    hash: Option<String>,
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
