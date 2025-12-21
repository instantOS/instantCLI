//! Video Planning Module
//!
//! This module parses markdown documents and creates abstract video segment plans.
//! It handles the transformation from markdown format with timestamps into a structured
//! timeline plan that can be used for video rendering.
//!
//! The planning phase includes:
//! - Parsing markdown blocks into video segments
//! - Aligning segments with subtitle cues for precise timing
//! - Managing overlays, pauses, and music directives
//! - Creating a high-level plan before actual video rendering

use anyhow::{Result, anyhow};

use crate::video::srt::SrtCue;

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

pub fn align_plan_with_subtitles(plan: &mut TimelinePlan, cues: &[SrtCue]) -> Result<()> {
    let dialogue_indices = align_dialogue_clips_to_cues(plan, cues)?;
    if dialogue_indices.is_empty() {
        return Ok(());
    }

    let silence_updates = collect_silence_time_updates(&plan.items, &dialogue_indices);
    apply_clip_time_updates(plan, silence_updates);

    Ok(())
}

const MAX_SILENCE_STRETCH_RATIO: f64 = 2.5;

#[derive(Debug, Clone, Copy)]
struct ClipTimeUpdate {
    idx: usize,
    start: f64,
    end: f64,
}

const DEFAULT_DIALOGUE_PADDING_SECONDS: f64 = 0.08;
const DEFAULT_PADDING_GUARD_SECONDS: f64 = 0.01;

fn align_dialogue_clips_to_cues(plan: &mut TimelinePlan, cues: &[SrtCue]) -> Result<Vec<usize>> {
    let mut dialogue_indices: Vec<usize> = Vec::new();

    for (idx, item) in plan.items.iter_mut().enumerate() {
        let TimelinePlanItem::Clip(clip) = item else {
            continue;
        };

        if clip.kind == SegmentKind::Silence {
            continue;
        }

        let match_idx = find_best_overlapping_cue(cues, clip.start, clip.end).ok_or_else(|| {
            anyhow!(
                "Unable to locate subtitle entry for segment `{}` at line {}",
                clip.text,
                clip.line
            )
        })?;

        let (start, end) = padded_cue_bounds(
            cues,
            match_idx,
            DEFAULT_DIALOGUE_PADDING_SECONDS,
            DEFAULT_PADDING_GUARD_SECONDS,
        );

        clip.start = start;
        clip.end = end;
        dialogue_indices.push(idx);
    }

    Ok(dialogue_indices)
}

fn collect_silence_time_updates(
    items: &[TimelinePlanItem],
    dialogue_indices: &[usize],
) -> Vec<ClipTimeUpdate> {
    let mut updates = Vec::new();

    let mut idx = 0usize;
    while idx < items.len() {
        let Some(silence_run) = SilenceRun::from_items(items, idx) else {
            idx += 1;
            continue;
        };

        idx = silence_run.end_idx;

        let Some((previous_dialogue_idx, next_dialogue_idx)) =
            silence_run.surrounding_dialogue(dialogue_indices)
        else {
            continue;
        };

        let Some(prev_end) = clip_end(items, previous_dialogue_idx) else {
            continue;
        };
        let Some(next_start) = clip_start(items, next_dialogue_idx) else {
            continue;
        };

        if let Some(run_updates) = silence_run.redistribute(items, prev_end, next_start) {
            updates.extend(run_updates);
        }
    }

    updates
}

fn apply_clip_time_updates(plan: &mut TimelinePlan, updates: Vec<ClipTimeUpdate>) {
    for update in updates {
        if let Some(TimelinePlanItem::Clip(clip)) = plan.items.get_mut(update.idx) {
            clip.start = update.start;
            clip.end = update.end.max(update.start);
        }
    }
}

fn clip_start(items: &[TimelinePlanItem], idx: usize) -> Option<f64> {
    match items.get(idx) {
        Some(TimelinePlanItem::Clip(clip)) => Some(clip.start),
        _ => None,
    }
}

fn clip_end(items: &[TimelinePlanItem], idx: usize) -> Option<f64> {
    match items.get(idx) {
        Some(TimelinePlanItem::Clip(clip)) => Some(clip.end),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct SilenceRun {
    start_idx: usize,
    end_idx: usize,
    approximate_total: f64,
}

impl SilenceRun {
    fn from_items(items: &[TimelinePlanItem], start_idx: usize) -> Option<Self> {
        let Some(TimelinePlanItem::Clip(clip)) = items.get(start_idx) else {
            return None;
        };

        if clip.kind != SegmentKind::Silence {
            return None;
        }

        let mut end_idx = start_idx;
        let mut approximate_total = 0.0f64;

        while end_idx < items.len() {
            match items.get(end_idx) {
                Some(TimelinePlanItem::Clip(run_clip)) if run_clip.kind == SegmentKind::Silence => {
                    approximate_total += run_clip.end - run_clip.start;
                    end_idx += 1;
                }
                _ => break,
            }
        }

        Some(Self {
            start_idx,
            end_idx,
            approximate_total,
        })
    }

    fn surrounding_dialogue(self, dialogue_indices: &[usize]) -> Option<(usize, usize)> {
        let previous_dialogue_idx = dialogue_indices
            .iter()
            .rev()
            .find(|&&dialogue_idx| dialogue_idx < self.start_idx)
            .copied()?;

        let next_dialogue_idx = dialogue_indices
            .iter()
            .find(|&&dialogue_idx| dialogue_idx >= self.end_idx)
            .copied()?;

        Some((previous_dialogue_idx, next_dialogue_idx))
    }

    fn redistribute(
        self,
        items: &[TimelinePlanItem],
        prev_end: f64,
        next_start: f64,
    ) -> Option<Vec<ClipTimeUpdate>> {
        let actual_gap = (next_start - prev_end).max(0.0);
        if self.approximate_total <= 0.0 || actual_gap <= 0.0 {
            return None;
        }

        let stretch_ratio = actual_gap / self.approximate_total;
        if stretch_ratio > MAX_SILENCE_STRETCH_RATIO {
            return None;
        }

        let mut updates = Vec::new();
        let mut current = prev_end;

        for idx in self.start_idx..self.end_idx {
            let Some(TimelinePlanItem::Clip(run_clip)) = items.get(idx) else {
                continue;
            };

            let approx_duration = run_clip.end - run_clip.start;
            if approx_duration <= 0.0 {
                continue;
            }

            let fraction = approx_duration / self.approximate_total;
            let actual_duration = actual_gap * fraction;
            updates.push(ClipTimeUpdate {
                idx,
                start: current,
                end: current + actual_duration,
            });
            current += actual_duration;
        }

        if let Some(last) = updates.last_mut() {
            last.end = next_start;
        }

        Some(updates)
    }
}

fn padded_cue_bounds(
    cues: &[SrtCue],
    cue_idx: usize,
    padding_seconds: f64,
    guard_seconds: f64,
) -> (f64, f64) {
    let cue = &cues[cue_idx];

    let cue_start = cue.start.as_secs_f64();
    let cue_end = cue.end.as_secs_f64();

    let mut padded_start = (cue_start - padding_seconds).max(0.0);
    let mut padded_end = cue_end + padding_seconds;

    if cue_idx > 0 {
        let prev_end = cues[cue_idx - 1].end.as_secs_f64();
        padded_start = padded_start.max(prev_end + guard_seconds);
    }

    if cue_idx + 1 < cues.len() {
        let next_start = cues[cue_idx + 1].start.as_secs_f64();
        padded_end = padded_end.min(next_start - guard_seconds);
    }

    if padded_end <= padded_start {
        (cue_start, cue_end)
    } else {
        (padded_start, padded_end)
    }
}

fn find_best_overlapping_cue(cues: &[SrtCue], clip_start: f64, clip_end: f64) -> Option<usize> {
    let clip_duration = (clip_end - clip_start).max(0.0);
    if cues.is_empty() || clip_duration <= 0.0 {
        return None;
    }

    let mut best_idx: Option<usize> = None;
    let mut best_overlap = 0.0f64;
    let mut best_distance = f64::INFINITY;

    for (idx, cue) in cues.iter().enumerate() {
        let cue_start = cue.start.as_secs_f64();
        let cue_end = cue.end.as_secs_f64();

        let overlap = overlap_seconds(clip_start, clip_end, cue_start, cue_end);
        if overlap <= 0.0 {
            continue;
        }

        let distance = (cue_start - clip_start).abs();
        if overlap > best_overlap || (overlap == best_overlap && distance < best_distance) {
            best_overlap = overlap;
            best_distance = distance;
            best_idx = Some(idx);
        }
    }

    if best_overlap / clip_duration < 0.01 {
        return None;
    }

    best_idx
}

fn overlap_seconds(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> f64 {
    let start = a_start.max(b_start);
    let end = a_end.min(b_end);
    (end - start).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::parse_video_document;
    use crate::video::srt::SrtCue;

    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn includes_music_blocks_in_plan() {
        let markdown = "```music\ntrack.mp3\n```\n`00:00:00.000-00:00:01.000` line";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        assert!(matches!(
            plan.items.first(),
            Some(TimelinePlanItem::Music(_))
        ));
        assert!(
            plan.items
                .iter()
                .any(|item| matches!(item, TimelinePlanItem::Clip(_)))
        );
    }

    #[test]
    fn aligns_dialogue_segments_with_subtitles() {
        let markdown = "`00:00:00.0-00:00:01.2` first\n`00:00:01.2-00:00:02.3` second\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            SrtCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(950),
                text: "first".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(1200),
                end: Duration::from_millis(2450),
                text: "second".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues).unwrap();

        let clip_segments: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Clip(clip) if clip.kind != SegmentKind::Silence => Some(clip),
                _ => None,
            })
            .collect();

        assert_eq!(clip_segments.len(), 2);

        assert!((clip_segments[0].start - 0.0).abs() < 1e-6);
        // End is the padded cue end, clamped to the next cue start minus guard.
        assert!((clip_segments[0].end - 1.03).abs() < 1e-6);

        // Start is the padded cue start, clamped to the previous cue end plus guard.
        assert!((clip_segments[1].start - 1.12).abs() < 1e-6);
        assert!((clip_segments[1].end - 2.53).abs() < 1e-6);
    }

    #[test]
    fn aligns_using_time_overlap_not_text() {
        let markdown = "`00:00:00.0-00:00:01.0` hello\n`00:00:01.0-00:00:02.0` world\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            SrtCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1100),
                text: "completely different".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(1100),
                end: Duration::from_millis(2000),
                text: "also different".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues).unwrap();

        let clip_segments: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Clip(clip) if clip.kind != SegmentKind::Silence => Some(clip),
                _ => None,
            })
            .collect();

        assert_eq!(clip_segments.len(), 2);

        // Clip 0 has no previous cue to clamp against.
        assert!((clip_segments[0].start - 0.0).abs() < 1e-6);
        // Clip 0 is clamped to the next cue start minus guard.
        assert!((clip_segments[0].end - 1.09).abs() < 1e-6);

        // Clip 1 is clamped to the previous cue end plus guard.
        assert!((clip_segments[1].start - 1.11).abs() < 1e-6);
        // Clip 1 has no next cue to clamp against.
        assert!((clip_segments[1].end - 2.08).abs() < 1e-6);
    }

    #[test]
    fn padding_never_overlaps_neighbor_cues() {
        let markdown = "`00:00:01.0-00:00:02.0` mid\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        // Cues are tightly packed with a 20ms gap.
        let cues = vec![
            SrtCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1000),
                text: "first".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(1020),
                end: Duration::from_millis(2000),
                text: "mid".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(2020),
                end: Duration::from_millis(3000),
                text: "last".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues).unwrap();

        let clip = plan
            .items
            .iter()
            .find_map(|item| match item {
                TimelinePlanItem::Clip(clip) if clip.kind != SegmentKind::Silence => Some(clip),
                _ => None,
            })
            .unwrap();

        // Start is clamped to prev_end + guard.
        assert!((clip.start - 1.01).abs() < 1e-6);
        // End is clamped to next_start - guard.
        assert!((clip.end - 2.01).abs() < 1e-6);
    }

    #[test]
    fn redistributes_silence_segments_across_actual_gap() {
        let markdown = "`00:00:00.0-00:00:01.2` intro\n`00:00:01.2-00:00:03.8` SILENCE\n`00:00:03.8-00:00:06.8` SILENCE\n`00:00:06.8-00:00:08.0` outro\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            SrtCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1234),
                text: "intro".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(6789),
                end: Duration::from_millis(8000),
                text: "outro".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues).unwrap();

        let mut iter = plan.items.iter();
        let first_clip = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected first clip"),
        };
        let first_silence = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected first silence"),
        };
        let second_silence = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected second silence"),
        };
        let second_clip = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected second clip"),
        };

        assert!((first_clip.end - first_silence.start).abs() < 1e-6);
        assert!((second_clip.start - second_silence.end).abs() < 1e-6);
        assert!((second_silence.start - first_silence.end).abs() < 1e-6);

        // Expected gap accounts for per-cue padding + guard.
        // Intro ends at cue1 end + padding; outro starts at cue2 start - padding.
        let expected_gap =
            (6.789 - DEFAULT_DIALOGUE_PADDING_SECONDS) - (1.234 + DEFAULT_DIALOGUE_PADDING_SECONDS);
        let actual_gap = (second_clip.start - first_clip.end).max(0.0);
        assert!((expected_gap - actual_gap).abs() < 1e-6);
    }

    #[test]
    fn does_not_stretch_silence_when_gap_is_huge() {
        let markdown = "`00:00:00.0-00:00:01.0` intro\n`00:00:01.0-00:00:02.0` SILENCE\n`00:00:02.0-00:00:03.0` SILENCE\n`00:00:50.0-00:00:51.0` outro\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            SrtCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1000),
                text: "intro".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(50_000),
                end: Duration::from_millis(51_000),
                text: "outro".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues).unwrap();

        let mut iter = plan.items.iter();
        let intro = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected intro"),
        };
        let silence1 = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected silence1"),
        };
        let silence2 = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected silence2"),
        };
        let outro = match iter.next() {
            Some(TimelinePlanItem::Clip(clip)) => clip,
            _ => panic!("expected outro"),
        };

        // Intro/outro are aligned to cues with padding.
        assert!((intro.end - 1.08).abs() < 1e-6);
        assert!((outro.start - 49.92).abs() < 1e-6);

        // ...but silence remains based on authored timestamps (not stretched to fill 49s).
        assert!((silence1.start - 1.0).abs() < 1e-6);
        assert!((silence1.end - 2.0).abs() < 1e-6);
        assert!((silence2.start - 2.0).abs() < 1e-6);
        assert!((silence2.end - 3.0).abs() < 1e-6);
    }
}
