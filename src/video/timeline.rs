use anyhow::{Result, anyhow};

use crate::video::srt::SrtCue;
use crate::video::utils::duration_to_tenths;

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
    let mut cue_index = 0usize;
    let mut dialogue_indices: Vec<usize> = Vec::new();

    for (idx, item) in plan.items.iter_mut().enumerate() {
        if let TimelinePlanItem::Clip(clip) = item {
            if clip.kind == SegmentKind::Silence {
                continue;
            }

            let match_idx =
                find_matching_cue(cues, &clip.text, clip.start, cue_index).ok_or_else(|| {
                    anyhow!(
                        "Unable to locate subtitle entry for segment `{}` at line {}",
                        clip.text,
                        clip.line
                    )
                })?;

            let cue = &cues[match_idx];
            clip.start = cue.start.as_secs_f64();
            clip.end = cue.end.as_secs_f64();
            cue_index = match_idx + 1;
            dialogue_indices.push(idx);
        }
    }

    if dialogue_indices.is_empty() {
        return Ok(());
    }

    let mut silence_updates: Vec<(usize, f64, f64)> = Vec::new();
    let mut idx = 0usize;
    while idx < plan.items.len() {
        let Some(TimelinePlanItem::Clip(clip)) = plan.items.get(idx) else {
            idx += 1;
            continue;
        };

        if clip.kind != SegmentKind::Silence {
            idx += 1;
            continue;
        }

        let run_start = idx;
        let mut run_end = idx;
        let mut approximate_total = 0.0f64;

        while run_end < plan.items.len() {
            match plan.items.get(run_end) {
                Some(TimelinePlanItem::Clip(run_clip)) if run_clip.kind == SegmentKind::Silence => {
                    approximate_total += run_clip.end - run_clip.start;
                    run_end += 1;
                }
                _ => break,
            }
        }

        let previous_dialogue_idx = dialogue_indices
            .iter()
            .rev()
            .find(|&&dialogue_idx| dialogue_idx < run_start)
            .copied();
        let next_dialogue_idx = dialogue_indices
            .iter()
            .find(|&&dialogue_idx| dialogue_idx >= run_end)
            .copied();

        if let (Some(prev_idx), Some(next_idx)) = (previous_dialogue_idx, next_dialogue_idx) {
            let prev_end = match plan.items.get(prev_idx) {
                Some(TimelinePlanItem::Clip(prev_clip)) => prev_clip.end,
                _ => 0.0,
            };
            let next_start = match plan.items.get(next_idx) {
                Some(TimelinePlanItem::Clip(next_clip)) => next_clip.start,
                _ => prev_end,
            };
            let actual_gap = (next_start - prev_end).max(0.0);

            if approximate_total > 0.0 && actual_gap > 0.0 {
                let mut current = prev_end;
                let mut run_updates: Vec<(usize, f64, f64)> = Vec::new();
                for inner_idx in run_start..run_end {
                    if let Some(TimelinePlanItem::Clip(run_clip)) = plan.items.get(inner_idx) {
                        let approx_duration = run_clip.end - run_clip.start;
                        if approx_duration <= 0.0 {
                            continue;
                        }
                        let fraction = approx_duration / approximate_total;
                        let actual_duration = actual_gap * fraction;
                        run_updates.push((inner_idx, current, current + actual_duration));
                        current += actual_duration;
                    }
                }
                if let Some(last) = run_updates.last_mut() {
                    last.2 = next_start;
                }
                silence_updates.extend(run_updates);
            }
        }

        idx = run_end;
    }

    for (segment_idx, start, end) in silence_updates {
        if let Some(TimelinePlanItem::Clip(clip)) = plan.items.get_mut(segment_idx) {
            clip.start = start;
            clip.end = end.max(start);
        }
    }

    Ok(())
}

fn find_matching_cue(
    cues: &[SrtCue],
    text: &str,
    approx_start: f64,
    start_index: usize,
) -> Option<usize> {
    let target_text = normalize_text(text);
    let target_tenths = seconds_to_tenths(approx_start);

    for (idx, cue) in cues.iter().enumerate().skip(start_index) {
        if normalize_text(&cue.text) != target_text {
            continue;
        }

        let cue_tenths = duration_to_tenths(cue.start) as i64;
        if (cue_tenths - target_tenths).abs() <= 1 {
            return Some(idx);
        }
    }

    None
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn seconds_to_tenths(value: f64) -> i64 {
    (value * 10.0).round() as i64
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
        assert!((clip_segments[0].end - 0.95).abs() < 1e-6);
        assert!((clip_segments[1].start - 1.2).abs() < 1e-6);
        assert!((clip_segments[1].end - 2.45).abs() < 1e-6);
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

        let expected_gap = 6.789 - 1.234;
        let actual_gap = (second_clip.start - first_clip.end).max(0.0);
        assert!((expected_gap - actual_gap).abs() < 1e-6);
    }
}
