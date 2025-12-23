use crate::video::document::{DocumentBlock, MusicDirective, SegmentKind, VideoDocument};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct TimelinePlan {
    pub items: Vec<TimelinePlanItem>,
    pub standalone_count: usize,
    pub overlay_count: usize,
    pub ignored_count: usize,
    pub heading_count: usize,
    pub segment_count: usize,
}

#[derive(Debug, Clone)]
pub enum TimelinePlanItem {
    Clip(ClipPlan),
    Standalone(StandalonePlan),
    Music(MusicPlan),
}

#[derive(Debug, Clone)]
pub struct ClipPlan {
    pub start: f64,
    pub end: f64,
    pub kind: SegmentKind,
    pub text: String,
    pub line: usize,
    pub overlay: Option<OverlayPlan>,
}

#[derive(Debug, Clone)]
pub struct OverlayPlan {
    pub markdown: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum StandalonePlan {
    Heading {
        level: u32,
        text: String,
        line: usize,
    },
    Pause {
        markdown: String,
        display_text: String,
        duration_seconds: f64,
        line: usize,
    },
}

#[derive(Debug, Clone)]
pub struct MusicPlan {
    pub directive: MusicDirective,
    pub line: usize,
}

pub fn plan_timeline(document: &VideoDocument) -> Result<TimelinePlan> {
    let mut planner = TimelinePlanner::new();
    planner.process_document(document);
    Ok(planner.into_plan())
}

/// State machine for building a timeline plan from document blocks.
struct TimelinePlanner {
    items: Vec<TimelinePlanItem>,
    stats: PlanStats,
    /// Current overlay to apply to upcoming segments.
    overlay_state: Option<OverlayPlan>,
    /// Index of the last clip added (for retroactive overlay application).
    last_clip_idx: Option<usize>,
    /// True if we're after a separator and before any segment (pause region).
    in_separator_region: bool,
    /// Accumulator for merging consecutive unhandled blocks.
    pending_content: Vec<(String, usize)>,
}

#[derive(Default)]
struct PlanStats {
    standalone_count: usize,
    overlay_count: usize,
    ignored_count: usize,
    heading_count: usize,
    segment_count: usize,
}

impl TimelinePlanner {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            stats: PlanStats::default(),
            overlay_state: None,
            last_clip_idx: None,
            in_separator_region: false,
            pending_content: Vec::new(),
        }
    }

    fn process_document(&mut self, document: &VideoDocument) {
        for block in &document.blocks {
            match block {
                DocumentBlock::Segment(segment) => self.handle_segment(segment),
                DocumentBlock::Heading(heading) => self.handle_heading(heading),
                DocumentBlock::Separator => self.handle_separator(),
                DocumentBlock::Music(music) => self.handle_music(music),
                DocumentBlock::Unhandled(unhandled) => self.handle_unhandled(unhandled),
            }
        }
        self.final_flush();
    }

    fn handle_segment(&mut self, segment: &crate::video::document::SegmentBlock) {
        // Flush pending content as overlay before processing segment
        self.flush_pending_as_overlay();

        self.items.push(TimelinePlanItem::Clip(ClipPlan {
            start: segment.range.start_seconds(),
            end: segment.range.end_seconds(),
            kind: segment.kind,
            text: segment.text.clone(),
            line: segment.line,
            overlay: self.overlay_state.clone(),
        }));
        self.last_clip_idx = Some(self.items.len() - 1);
        self.stats.segment_count += 1;
        self.in_separator_region = false;
    }

    fn handle_heading(&mut self, heading: &crate::video::document::HeadingBlock) {
        self.items
            .push(TimelinePlanItem::Standalone(StandalonePlan::Heading {
                level: heading.level,
                text: heading.text.clone(),
                line: heading.line,
            }));
        self.stats.standalone_count += 1;
        self.stats.heading_count += 1;
        // Headings don't exit separator region
    }

    fn handle_separator(&mut self) {
        if !self.pending_content.is_empty() {
            if self.in_separator_region {
                self.flush_pending_as_pause();
            } else {
                self.flush_pending_as_overlay();
            }
        }
        self.overlay_state = None;
        self.in_separator_region = true;
    }

    fn handle_music(&mut self, music: &crate::video::document::MusicBlock) {
        self.items.push(TimelinePlanItem::Music(MusicPlan {
            directive: music.directive.clone(),
            line: music.line,
        }));
        // Music blocks don't exit separator region
    }

    fn handle_unhandled(&mut self, unhandled: &crate::video::document::UnhandledBlock) {
        let trimmed = unhandled.description.trim();
        if trimmed.is_empty() {
            self.stats.ignored_count += 1;
            return;
        }
        self.pending_content
            .push((unhandled.description.clone(), unhandled.line));
    }

    fn final_flush(&mut self) {
        // Any remaining pending content becomes an overlay for the last clip
        self.flush_pending_as_overlay();
    }

    /// Merge pending content into an overlay and apply to the last clip.
    fn flush_pending_as_overlay(&mut self) {
        if let Some(overlay) = self.merge_pending() {
            if let Some(last_idx) = self.last_clip_idx
                && let Some(TimelinePlanItem::Clip(clip)) = self.items.get_mut(last_idx)
            {
                clip.overlay = Some(overlay.clone());
            }
            self.overlay_state = Some(overlay);
            self.stats.overlay_count += 1;
        }
    }

    /// Merge pending content into a standalone pause.
    fn flush_pending_as_pause(&mut self) {
        let merged = self.merge_pending_text();
        if merged.is_empty() {
            return;
        }
        let trimmed = merged.trim();
        let line = self.pending_content.first().map(|(_, l)| *l).unwrap_or(0);
        self.items
            .push(TimelinePlanItem::Standalone(StandalonePlan::Pause {
                markdown: merged.clone(),
                display_text: trimmed.to_string(),
                duration_seconds: pause_duration_seconds(trimmed),
                line,
            }));
        self.stats.standalone_count += 1;
        self.pending_content.clear();
    }

    /// Merge and clear pending content, returning an OverlayPlan if non-empty.
    fn merge_pending(&mut self) -> Option<OverlayPlan> {
        if self.pending_content.is_empty() {
            return None;
        }
        let line = self.pending_content[0].1;
        let markdown = self.merge_pending_text();
        self.pending_content.clear();
        Some(OverlayPlan { markdown, line })
    }

    /// Join pending content with paragraph breaks.
    fn merge_pending_text(&self) -> String {
        self.pending_content
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn into_plan(self) -> TimelinePlan {
        TimelinePlan {
            items: self.items,
            standalone_count: self.stats.standalone_count,
            overlay_count: self.stats.overlay_count,
            ignored_count: self.stats.ignored_count,
            heading_count: self.stats.heading_count,
            segment_count: self.stats.segment_count,
        }
    }
}

pub const DEFAULT_PAUSE_MIN_SECONDS: f64 = 5.0;
pub const DEFAULT_PAUSE_MAX_SECONDS: f64 = 20.0;
pub const DEFAULT_PAUSE_READING_WPM: f64 = 180.0;

pub fn pause_duration_seconds(display_text: &str) -> f64 {
    let words = display_text.split_whitespace().count() as f64;
    if words <= 0.0 {
        return DEFAULT_PAUSE_MIN_SECONDS;
    }

    let words_per_second = DEFAULT_PAUSE_READING_WPM / 60.0;
    let seconds = words / words_per_second;
    seconds.clamp(DEFAULT_PAUSE_MIN_SECONDS, DEFAULT_PAUSE_MAX_SECONDS)
}
