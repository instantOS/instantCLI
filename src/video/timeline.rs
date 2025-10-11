use anyhow::Result;

use super::document::{DocumentBlock, MusicDirective, SegmentKind, VideoDocument};

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
        line: usize,
    },
}

#[derive(Debug, Clone)]
pub struct MusicPlan {
    pub directive: MusicDirective,
    pub line: usize,
}

pub fn plan_timeline(document: &VideoDocument) -> Result<TimelinePlan> {
    let mut items = Vec::new();
    let mut standalone_count = 0usize;
    let mut overlay_count = 0usize;
    let mut ignored_count = 0usize;
    let mut heading_count = 0usize;
    let mut segment_count = 0usize;
    let mut overlay_state: Option<OverlayPlan> = None;
    let mut last_was_separator = false;

    for (idx, block) in document.blocks.iter().enumerate() {
        match block {
            DocumentBlock::Segment(segment) => {
                items.push(TimelinePlanItem::Clip(ClipPlan {
                    start: segment.range.start_seconds(),
                    end: segment.range.end_seconds(),
                    kind: segment.kind,
                    text: segment.text.clone(),
                    line: segment.line,
                    overlay: overlay_state.clone(),
                }));
                segment_count += 1;
                last_was_separator = false;
            }
            DocumentBlock::Heading(heading) => {
                items.push(TimelinePlanItem::Standalone(StandalonePlan::Heading {
                    level: heading.level,
                    text: heading.text.clone(),
                    line: heading.line,
                }));
                standalone_count += 1;
                heading_count += 1;
                last_was_separator = false;
            }
            DocumentBlock::Separator(_) => {
                overlay_state = None;
                last_was_separator = true;
            }
            DocumentBlock::Music(music) => {
                items.push(TimelinePlanItem::Music(MusicPlan {
                    directive: music.directive.clone(),
                    line: music.line,
                }));
                last_was_separator = false;
            }
            DocumentBlock::Unhandled(unhandled) => {
                let raw_description = unhandled.description.as_str();
                let trimmed = raw_description.trim();
                if trimmed.is_empty() {
                    ignored_count += 1;
                    last_was_separator = false;
                    continue;
                }

                let next_is_separator = document
                    .blocks
                    .get(idx + 1)
                    .map(|next| matches!(next, DocumentBlock::Separator(_)))
                    .unwrap_or(false);

                if last_was_separator && next_is_separator {
                    items.push(TimelinePlanItem::Standalone(StandalonePlan::Pause {
                        markdown: raw_description.to_string(),
                        display_text: trimmed.to_string(),
                        line: unhandled.line,
                    }));
                    standalone_count += 1;
                    overlay_state = None;
                    last_was_separator = false;
                } else {
                    overlay_state = Some(OverlayPlan {
                        markdown: raw_description.to_string(),
                        line: unhandled.line,
                    });
                    overlay_count += 1;
                    last_was_separator = false;
                }
            }
        }
    }

    Ok(TimelinePlan {
        items,
        standalone_count,
        overlay_count,
        ignored_count,
        heading_count,
        segment_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::parse_video_document;
    use std::path::Path;

    #[test]
    fn includes_music_blocks_in_plan() {
        let markdown = "```music\ntrack.mp3\n```\n`00:00:00.000-00:00:01.000` line";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        assert!(matches!(plan.items.first(), Some(TimelinePlanItem::Music(_))));
        assert!(plan
            .items
            .iter()
            .any(|item| matches!(item, TimelinePlanItem::Clip(_))));
    }
}
