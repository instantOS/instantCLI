use std::path::PathBuf;
use std::time::Duration;

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
    /// Resolved at runtime during render; never serialized to frontmatter.
    pub audio: PathBuf,
    pub hash: Option<String>,
}

#[derive(Debug)]
pub enum DocumentBlock {
    Segment(SegmentBlock),
    Heading(HeadingBlock),
    Separator,
    Unhandled(UnhandledBlock),
    Music(MusicDirective),
    Broll(BrollBlock),
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

#[derive(Debug)]
pub struct BrollBlock {
    pub range: TimeRange,
    pub text: String,
    pub source_id: String,
}

#[derive(Debug, Clone)]
pub enum MusicDirective {
    None,
    Source(String),
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
