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
    pub overlay: Option<OverlayPlan>,
    pub broll: Option<BrollPlan>,
    pub source_id: String,
}

#[derive(Debug, Clone)]
pub struct OverlayPlan {
    pub markdown: String,
}

#[derive(Debug, Clone)]
pub enum StandalonePlan {
    Pause {
        markdown: String,
        duration_seconds: f64,
    },
}

#[derive(Debug, Clone)]
pub struct MusicPlan {
    pub directive: MusicDirective,
}

#[derive(Debug, Clone)]
pub struct BrollClip {
    pub start: f64,
    pub end: f64,
    pub source_id: String,
}

#[derive(Debug, Clone)]
pub struct BrollPlan {
    pub clips: Vec<BrollClip>,
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
    /// Current B-roll state to apply to upcoming segments (persists like overlays).
    broll_state: Option<BrollPlan>,
    /// Index of the last clip added (for retroactive overlay application).
    last_clip_idx: Option<usize>,
    /// True if we're after a separator and before any segment (pause region).
    in_separator_region: bool,
    /// Accumulator for merging consecutive unhandled blocks.
    pending_content: Vec<String>,
    /// Pending B-roll clips being accumulated before applying to state.
    pending_broll: Vec<BrollClip>,
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
            broll_state: None,
            last_clip_idx: None,
            in_separator_region: false,
            pending_content: Vec::new(),
            pending_broll: Vec::new(),
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
                DocumentBlock::Broll(broll) => self.handle_broll(broll),
            }
        }
        self.final_flush();
    }

    fn handle_segment(&mut self, segment: &crate::video::document::SegmentBlock) {
        // Flush pending content as overlay before processing segment
        self.flush_pending_as_overlay();
        // Flush pending B-roll to state (applies to upcoming segments like overlays)
        self.flush_pending_broll_to_state();

        self.items.push(TimelinePlanItem::Clip(ClipPlan {
            start: segment.range.start_seconds(),
            end: segment.range.end_seconds(),
            kind: segment.kind,
            text: segment.text.clone(),
            overlay: self.overlay_state.clone(),
            broll: self.broll_state.clone(),
            source_id: segment.source_id.clone(),
        }));
        self.last_clip_idx = Some(self.items.len() - 1);
        self.stats.segment_count += 1;
        self.in_separator_region = false;
    }

    fn handle_heading(&mut self, heading: &crate::video::document::HeadingBlock) {
        // Headings are treated like regular content - they become overlays or pause slides
        // depending on context. The slide renderer detects heading-only content for hero styling.
        let hashes = "#".repeat(heading.level.max(1) as usize);
        let markdown = format!("{} {}", hashes, heading.text.trim());
        self.pending_content.push(markdown);
        self.stats.heading_count += 1;
    }

    fn handle_separator(&mut self) {
        // Flush pending B-roll to state before separator
        self.flush_pending_broll_to_state();
        if !self.pending_content.is_empty() {
            if self.in_separator_region {
                self.flush_pending_as_pause();
            } else {
                self.flush_pending_as_overlay();
            }
        }
        self.overlay_state = None;
        self.broll_state = None;  // Clear B-roll state on separator (like overlays)
        self.in_separator_region = true;
    }

    fn handle_music(&mut self, music: &crate::video::document::MusicBlock) {
        self.items.push(TimelinePlanItem::Music(MusicPlan {
            directive: music.directive.clone(),
        }));
        // Music blocks don't exit separator region
    }

    fn handle_unhandled(&mut self, unhandled: &crate::video::document::UnhandledBlock) {
        // Flush pending B-roll to state (B-roll persists across segments like overlays)
        self.flush_pending_broll_to_state();
        let trimmed = unhandled.description.trim();
        if trimmed.is_empty() {
            self.stats.ignored_count += 1;
            return;
        }
        self.pending_content.push(unhandled.description.clone());
    }

    fn handle_broll(&mut self, broll: &crate::video::document::BrollBlock) {
        self.pending_broll.push(BrollClip {
            start: broll.range.start_seconds(),
            end: broll.range.end_seconds(),
            source_id: broll.source_id.clone(),
        });
    }

    fn final_flush(&mut self) {
        // Flush pending B-roll to state
        self.flush_pending_broll_to_state();
        // Any remaining pending content becomes an overlay for the last clip
        self.flush_pending_as_overlay();
    }

    /// Flush pending B-roll clips to the B-roll state (persists across segments like overlays).
    fn flush_pending_broll_to_state(&mut self) {
        if self.pending_broll.is_empty() {
            return;
        }
        let clips = std::mem::take(&mut self.pending_broll);
        self.broll_state = Some(BrollPlan { clips });
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
        self.items
            .push(TimelinePlanItem::Standalone(StandalonePlan::Pause {
                markdown: merged.clone(),
                duration_seconds: pause_duration_seconds(trimmed),
            }));
        self.stats.standalone_count += 1;
        self.pending_content.clear();
    }

    /// Merge and clear pending content, returning an OverlayPlan if non-empty.
    fn merge_pending(&mut self) -> Option<OverlayPlan> {
        if self.pending_content.is_empty() {
            return None;
        }
        let markdown = self.merge_pending_text();
        self.pending_content.clear();
        Some(OverlayPlan { markdown })
    }

    /// Join pending content with paragraph breaks.
    fn merge_pending_text(&self) -> String {
        self.pending_content
            .iter()
            .map(|s| s.as_str())
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
