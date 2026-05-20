use anyhow::{Context, Result, bail};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::reference::{
    SegmentSourceConfig, looks_like_timestamp_reference, parse_segment_reference,
};
use super::types::{
    BrollBlock, DocumentBlock, HeadingBlock, MusicDirective, SegmentBlock, SegmentKind,
    UnhandledBlock,
};
use super::util::{LineMap, heading_level_to_u32, is_music_code_block};

pub fn parse_body_blocks(
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
                let line = self.byte_to_line(range.start);
                self.flush_paragraph()?;
                if is_music_code_block(&info) {
                    self.code_block = Some(CodeBlockState::music(line));
                } else {
                    let lang = info.to_string();
                    self.code_block = Some(CodeBlockState::generic(line, lang));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                self.flush_code_block()?;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.blockquote = Some(BlockquoteState::new());
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush_blockquote()?;
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                if let Some(state) = self.paragraph.as_mut() {
                    state.set_pending_image(dest_url.into_string(), title.into_string());
                }
            }
            Event::End(TagEnd::Image) => {
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
                if let Some(state) = self.blockquote.as_mut() {
                    state.push_code(code.into_string());
                } else if let Some(state) = self.paragraph.as_mut() {
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
            if state.is_music() {
                let line = self.base_line_offset + state.start_line;
                let directive = state.into_music_directive(line)?;
                self.blocks.push(DocumentBlock::Music(directive));
            } else {
                let markdown = state.to_markdown();
                if !markdown.trim().is_empty() {
                    self.blocks.push(DocumentBlock::Unhandled(UnhandledBlock {
                        description: markdown,
                    }));
                }
            }
        }
        Ok(())
    }

    fn flush_blockquote(&mut self) -> Result<()> {
        if let Some(state) = self.blockquote.take() {
            if let Some(code_span) = state.code_span {
                let (source_id, range) =
                    parse_segment_reference(&code_span, self.source_config, self.base_line_offset)?;
                let text = state.following_text.trim().to_string();
                self.blocks.push(DocumentBlock::Broll(BrollBlock {
                    range,
                    text,
                    source_id,
                }));
            } else {
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
        Ok(())
    }

    fn handle_text(&mut self, text: String, start: usize) {
        if let Some(state) = self.blockquote.as_mut() {
            state.push_text(text.clone());
        } else if self.paragraph.is_some() {
            let line = self.byte_to_line(start);
            if let Some(state) = &mut self.paragraph {
                if state.is_inside_image() {
                    state.push_image_alt(&text);
                } else {
                    state.push_fragment(InlineFragment::text(line, text.clone()));
                }
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
            if let Some(state) = &mut self.paragraph {
                if hard {
                    state.push_fragment(InlineFragment::hard_break(line));
                } else {
                    state.push_fragment(InlineFragment::soft_break(line));
                }
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

struct ParagraphState {
    fragments: Vec<InlineFragment>,
    pending_image: Option<(String, String)>,
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
            let is_timestamp_code = matches!(&fragment.kind, InlineFragmentKind::Code(code) if looks_like_timestamp_reference(code));

            match fragment.kind {
                InlineFragmentKind::Code(code) if is_timestamp_code => {
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
                    let kind = if text.eq_ignore_ascii_case("silence") {
                        SegmentKind::Silence
                    } else {
                        SegmentKind::Dialogue
                    };
                    let (source_id, range) = parse_segment_reference(&code, source_config, line)
                        .with_context(|| {
                            format!("Invalid timestamp `{}` at line {}", code, line)
                        })?;
                    blocks.push(DocumentBlock::Segment(SegmentBlock {
                        range,
                        text,
                        kind,
                        source_id,
                    }));
                }
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

                    leftover_text.push(InlineFragment::text(
                        fragment.start_line,
                        format!("`{}`", code),
                    ));
                    for f in following {
                        leftover_text.push(f);
                    }
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
    code_span: Option<String>,
    following_text: String,
}

impl BlockquoteState {
    fn new() -> Self {
        Self {
            content: String::new(),
            code_span: None,
            following_text: String::new(),
        }
    }

    fn push_text(&mut self, text: String) {
        if self.code_span.is_some() {
            self.following_text.push_str(&text);
        } else {
            self.content.push_str(&text);
        }
    }

    fn push_code(&mut self, code: String) {
        if self.code_span.is_none() {
            self.code_span = Some(code);
        } else {
            self.content.push('`');
            self.content.push_str(&code);
            self.content.push('`');
        }
    }

    fn push_newline(&mut self) {
        self.content.push('\n');
    }
}

struct InlineFragment {
    start_line: usize,
    kind: InlineFragmentKind,
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

struct CodeBlockState {
    kind: CodeBlockKindState,
    start_line: usize,
    content: String,
}

enum CodeBlockKindState {
    Music,
    Generic { lang: String },
}

impl CodeBlockState {
    fn music(start_line: usize) -> Self {
        Self {
            kind: CodeBlockKindState::Music,
            start_line,
            content: String::new(),
        }
    }

    fn generic(start_line: usize, lang: String) -> Self {
        Self {
            kind: CodeBlockKindState::Generic { lang },
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
            _ => bail!("Expected music block at line {}", line),
        }
    }

    fn to_markdown(&self) -> String {
        match &self.kind {
            CodeBlockKindState::Music => format!("```music\n{}\n```", self.content.trim()),
            CodeBlockKindState::Generic { lang } => {
                format!("```{lang}\n{}\n```", self.content.trim())
            }
        }
    }

    fn is_music(&self) -> bool {
        matches!(self.kind, CodeBlockKindState::Music)
    }
}
