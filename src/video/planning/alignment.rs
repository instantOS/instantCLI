use super::core::{TimelinePlan, TimelinePlanItem};
use super::graph::{McmfEdge, add_edge, min_cost_max_flow};
use crate::video::document::SegmentKind;
use crate::video::render::timeline::TimeWindow;
use crate::video::support::transcript::TranscriptCue;
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};

pub fn align_plan_with_subtitles(plan: &mut TimelinePlan, cues: &[TranscriptCue]) -> Result<()> {
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
    time_window: TimeWindow,
}

const DEFAULT_DIALOGUE_PADDING_SECONDS: f64 = 0.08;
const DEFAULT_PADDING_GUARD_SECONDS: f64 = 0.0;

fn align_dialogue_clips_to_cues(
    plan: &mut TimelinePlan,
    cues: &[TranscriptCue],
) -> Result<Vec<usize>> {
    let mut dialogue_indices: Vec<usize> = Vec::new();

    let mut cue_map: HashMap<String, Vec<TranscriptCue>> = HashMap::new();
    for cue in cues {
        let key = cue.source_id.clone();
        cue_map.entry(key).or_default().push(cue.clone());
    }

    for cues in cue_map.values_mut() {
        cues.sort_by(|a, b| {
            a.start
                .partial_cmp(&b.start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    let mut per_source_indices: HashMap<String, Vec<usize>> = HashMap::new();

    for (idx, item) in plan.items.iter().enumerate() {
        let TimelinePlanItem::Clip(clip) = item else {
            continue;
        };

        if clip.kind == SegmentKind::Silence {
            continue;
        }

        per_source_indices
            .entry(clip.source_id.clone())
            .or_default()
            .push(idx);
    }

    for (source_id, clip_indices) in per_source_indices {
        let Some(source_cues) = cue_map.get(&source_id) else {
            bail!("No transcript cues loaded for source `{}`", source_id);
        };

        let mut dialogue_clips: Vec<(usize, TimeWindow, String)> = Vec::new();
        for idx in clip_indices {
            let Some(TimelinePlanItem::Clip(clip)) = plan.items.get(idx) else {
                continue;
            };
            dialogue_clips.push((idx, clip.time_window, clip.text.clone()));
        }

        if dialogue_clips.is_empty() {
            continue;
        }

        let assignments = assign_cues_max_overlap(&dialogue_clips, source_cues)?;

        for (clip_idx, cue_idx) in assignments {
            let Some(TimelinePlanItem::Clip(clip)) = plan.items.get_mut(clip_idx) else {
                continue;
            };

            let bounds = padded_cue_bounds(
                source_cues,
                cue_idx,
                DEFAULT_DIALOGUE_PADDING_SECONDS,
                DEFAULT_PADDING_GUARD_SECONDS,
            );

            clip.time_window = bounds;
            dialogue_indices.push(clip_idx);
        }
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
            clip.time_window = TimeWindow::new(
                update.time_window.start,
                update.time_window.end.max(update.time_window.start),
            );
        }
    }
}

fn clip_start(items: &[TimelinePlanItem], idx: usize) -> Option<f64> {
    match items.get(idx) {
        Some(TimelinePlanItem::Clip(clip)) => Some(clip.time_window.start),
        _ => None,
    }
}

fn clip_end(items: &[TimelinePlanItem], idx: usize) -> Option<f64> {
    match items.get(idx) {
        Some(TimelinePlanItem::Clip(clip)) => Some(clip.time_window.end),
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
                    approximate_total += run_clip.time_window.duration();
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

            let approx_duration = run_clip.time_window.duration();
            if approx_duration <= 0.0 {
                continue;
            }

            let fraction = approx_duration / self.approximate_total;
            let actual_duration = actual_gap * fraction;
            updates.push(ClipTimeUpdate {
                idx,
                time_window: TimeWindow::new(current, current + actual_duration),
            });
            current += actual_duration;
        }

        if let Some(last) = updates.last_mut() {
            last.time_window.end = next_start;
        }

        Some(updates)
    }
}

fn padded_cue_bounds(
    cues: &[TranscriptCue],
    cue_idx: usize,
    padding_seconds: f64,
    guard_seconds: f64,
) -> TimeWindow {
    let cue = &cues[cue_idx];

    let cue_start = cue.start.as_secs_f64();
    let cue_end = cue.end.as_secs_f64();

    let mut padded_start = (cue_start - padding_seconds).max(0.0);
    let mut padded_end = cue_end + padding_seconds;

    if cue_idx > 0 {
        let prev_end = cues[cue_idx - 1].end.as_secs_f64();
        let gap_mid = (prev_end + cue_start) / 2.0;
        padded_start = padded_start.max(gap_mid + guard_seconds);
    }

    if cue_idx + 1 < cues.len() {
        let next_start = cues[cue_idx + 1].start.as_secs_f64();
        let gap_mid = (cue_end + next_start) / 2.0;
        padded_end = padded_end.min(gap_mid - guard_seconds);
    }

    if padded_end <= padded_start {
        TimeWindow::new(padded_start, padded_start)
    } else {
        TimeWindow::new(padded_start, padded_end)
    }
}

fn assign_cues_max_overlap(
    dialogue_clips: &[(usize, TimeWindow, String)],
    cues: &[TranscriptCue],
) -> Result<Vec<(usize, usize)>> {
    validate_alignment_inputs(dialogue_clips, cues)?;

    let clip_count = dialogue_clips.len();
    let cue_count = cues.len();

    let mut graph_builder = AssignmentGraphBuilder::new(clip_count, cue_count);
    graph_builder.build_base_edges();
    graph_builder.add_overlap_options(dialogue_clips, cues);

    let (flow, _cost) = min_cost_max_flow(
        &mut graph_builder.graph,
        graph_builder.source,
        graph_builder.sink,
        clip_count as i64,
    );

    if flow < clip_count as i64 {
        diagnose_alignment_failure(dialogue_clips, cues)?;
    }

    graph_builder.extract_assignments(dialogue_clips)
}

fn validate_alignment_inputs(
    dialogue_clips: &[(usize, TimeWindow, String)],
    cues: &[TranscriptCue],
) -> Result<()> {
    if cues.is_empty() {
        bail!("Unable to align subtitles: no subtitle cues available");
    }

    let mut source_ids = HashSet::new();
    for cue in cues {
        source_ids.insert(cue.source_id.as_str());
    }
    if source_ids.len() > 1 {
        bail!("Unable to align subtitles: mixed source ids supplied");
    }

    let clip_count = dialogue_clips.len();
    let cue_count = cues.len();

    if cue_count < clip_count {
        bail!(
            "Unable to align subtitles: only {} subtitle cues for {} dialogue segments",
            cue_count,
            clip_count
        );
    }
    Ok(())
}

struct AssignmentGraphBuilder {
    graph: Vec<Vec<McmfEdge>>,
    source: usize,
    sink: usize,
    clip_offset: usize,
    cue_offset: usize,
    cue_count: usize,
}

impl AssignmentGraphBuilder {
    fn new(clip_count: usize, cue_count: usize) -> Self {
        let source = 0usize;
        let clip_offset = 1usize;
        let cue_offset = clip_offset + clip_count;
        let sink = cue_offset + cue_count;
        let node_count = sink + 1;
        let graph = vec![Vec::new(); node_count];

        Self {
            graph,
            source,
            sink,
            clip_offset,
            cue_offset,
            cue_count,
        }
    }

    fn build_base_edges(&mut self) {
        // Edges from Source -> Clips
        let clip_count = self.cue_offset - self.clip_offset;
        for clip_idx in 0..clip_count {
            add_edge(
                &mut self.graph,
                self.source,
                self.clip_offset + clip_idx,
                1,
                0,
            );
        }

        // Edges from Cues -> Sink
        for cue_idx in 0..self.cue_count {
            add_edge(&mut self.graph, self.cue_offset + cue_idx, self.sink, 1, 0);
        }
    }

    fn add_overlap_options(
        &mut self,
        dialogue_clips: &[(usize, TimeWindow, String)],
        cues: &[TranscriptCue],
    ) {
        for (clip_idx, (_timeline_idx, clip_window, _text)) in dialogue_clips.iter().enumerate() {
            let clip_duration = clip_window.duration();
            if clip_duration <= 0.0 {
                continue;
            }

            for (cue_idx, cue) in cues.iter().enumerate() {
                let cue_start = cue.start.as_secs_f64();
                let cue_end = cue.end.as_secs_f64();
                let overlap = clip_window.overlap_seconds(TimeWindow::new(cue_start, cue_end));

                if overlap <= 0.0 {
                    continue;
                }

                if overlap / clip_duration < 0.01 {
                    continue;
                }

                // Convert to integer cost: maximize overlap, then prefer closer starts.
                // Costs are negated because the solver minimizes total cost.
                let overlap_cost = -(overlap * 1_000_000.0).round() as i64;
                let distance = (cue_start - clip_window.start).abs();
                let distance_cost = (distance * 1_000.0).round() as i64;

                let cost = overlap_cost + distance_cost;
                add_edge(
                    &mut self.graph,
                    self.clip_offset + clip_idx,
                    self.cue_offset + cue_idx,
                    1,
                    cost,
                );
            }
        }
    }

    fn extract_assignments(
        &self,
        dialogue_clips: &[(usize, TimeWindow, String)],
    ) -> Result<Vec<(usize, usize)>> {
        let clip_count = dialogue_clips.len();
        let mut result: Vec<(usize, usize)> = Vec::with_capacity(clip_count);

        for (clip_idx, dialogue_clip) in dialogue_clips.iter().enumerate() {
            let clip_node = self.clip_offset + clip_idx;
            let timeline_idx = dialogue_clip.0;

            let mut matched: Option<usize> = None;
            for edge in &self.graph[clip_node] {
                let is_to_cue =
                    edge.to >= self.cue_offset && edge.to < self.cue_offset + self.cue_count;
                if !is_to_cue {
                    continue;
                }

                if edge.cap == 0 {
                    matched = Some(edge.to - self.cue_offset);
                    break;
                }
            }

            let Some(cue_idx) = matched else {
                bail!(
                    "Unable to align subtitles: missing cue assignment for segment index {}",
                    timeline_idx
                );
            };

            result.push((timeline_idx, cue_idx));
        }

        Ok(result)
    }
}

fn diagnose_alignment_failure(
    dialogue_clips: &[(usize, TimeWindow, String)],
    cues: &[TranscriptCue],
) -> Result<()> {
    for (_timeline_idx, clip_window, text) in dialogue_clips {
        let clip_duration = clip_window.duration();
        if clip_duration <= 0.0 {
            bail!("Invalid segment duration for `{}`", text);
        }

        let mut has_candidate = false;
        for cue in cues {
            let cue_start = cue.start.as_secs_f64();
            let cue_end = cue.end.as_secs_f64();
            let overlap = clip_window.overlap_seconds(TimeWindow::new(cue_start, cue_end));
            if overlap <= 0.0 {
                continue;
            }
            if overlap / clip_duration < 0.01 {
                continue;
            }
            has_candidate = true;
            break;
        }

        if !has_candidate {
            bail!("Unable to locate subtitle entry for segment `{}`", text);
        }
    }

    bail!("Unable to align subtitles: could not assign unique cues to every segment");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::parse_video_document;
    use crate::video::planning::{TimelinePlanItem, plan_timeline};
    use crate::video::support::transcript::TranscriptCue;

    use std::path::Path;
    use std::time::Duration;

    use super::super::core::{
        DEFAULT_PAUSE_MAX_SECONDS, DEFAULT_PAUSE_MIN_SECONDS, pause_duration_seconds,
    };

    #[test]
    fn includes_music_blocks_in_plan() {
        let markdown = "```music\ntrack.mp3\n```\n`a@00:00:00.000-00:00:01.000` line";
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
    fn slide_applies_to_immediately_previous_clip_and_clears_on_separator() {
        let markdown = concat!(
            "`a@00:00:00.0-00:00:01.0` first\n",
            "slide 1\n\n",
            "---\n\n",
            "`a@00:00:01.0-00:00:02.0` second\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        let clips: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Clip(clip) => Some(clip),
                _ => None,
            })
            .collect();

        assert_eq!(clips.len(), 2);
        assert!(clips[0].overlay.is_some());
        assert!(clips[1].overlay.is_none());

        let overlay = clips[0].overlay.as_ref().unwrap();
        assert_eq!(overlay.markdown.trim(), "slide 1");
    }

    #[test]
    fn consecutive_slides_merge_into_single_overlay() {
        let markdown = concat!(
            "`a@00:00:00.0-00:00:01.0` first\n",
            "slide 1\n\n",
            "slide 2\n\n",
            "`a@00:00:01.0-00:00:02.0` second\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        let clips: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Clip(clip) => Some(clip),
                _ => None,
            })
            .collect();

        assert_eq!(clips.len(), 2);

        // Consecutive paragraphs merge into a single overlay with \n\n separator
        let overlay_first = clips[0].overlay.as_ref().unwrap();
        assert_eq!(overlay_first.markdown.trim(), "slide 1\n\nslide 2");

        // Overlay carries forward to the next segment
        let overlay_second = clips[1].overlay.as_ref().unwrap();
        assert_eq!(overlay_second.markdown.trim(), "slide 1\n\nslide 2");
    }

    #[test]
    fn pause_duration_scales_with_word_count() {
        let markdown = concat!(
            "`a@00:00:00.0-00:00:01.0` first\n\n",
            "---\n\n",
            "short\n\n",
            "---\n\n",
            "`a@00:00:01.0-00:00:02.0` second\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        let pauses: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Standalone(standalone) => Some(standalone.duration_seconds),
                _ => None,
            })
            .collect();

        assert_eq!(pauses.len(), 1);
        assert!((pauses[0] - DEFAULT_PAUSE_MIN_SECONDS).abs() < 1e-9);

        let long = "word ".repeat(500);
        assert!((pause_duration_seconds(&long) - DEFAULT_PAUSE_MAX_SECONDS).abs() < 1e-9);
    }

    #[test]
    fn aligns_dialogue_segments_with_subtitles() {
        let markdown = "`a@00:00:00.0-00:00:01.2` first\n`a@00:00:01.2-00:00:02.3` second\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            TranscriptCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(950),
                text: "first".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(1200),
                end: Duration::from_millis(2450),
                text: "second".to_string(),
                words: vec![],
                source_id: "a".to_string(),
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

        assert!((clip_segments[0].time_window.start - 0.0).abs() < 1e-6);
        // End is the padded cue end, clamped to the next cue start minus guard.
        assert!((clip_segments[0].time_window.end - 1.03).abs() < 1e-6);

        // Start is the padded cue start, clamped to the previous cue end plus guard.
        assert!((clip_segments[1].time_window.start - 1.12).abs() < 1e-6);
        assert!((clip_segments[1].time_window.end - 2.53).abs() < 1e-6);
    }

    #[test]
    fn aligns_using_time_overlap_not_text() {
        let markdown = "`a@00:00:00.0-00:00:01.0` hello\n`a@00:00:01.0-00:00:02.0` world\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            TranscriptCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1100),
                text: "completely different".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(1100),
                end: Duration::from_millis(2000),
                text: "also different".to_string(),
                words: vec![],
                source_id: "a".to_string(),
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
        assert!((clip_segments[0].time_window.start - 0.0).abs() < 1e-6);
        // Clip 0 is clamped to the next cue start minus guard.
        assert!((clip_segments[0].time_window.end - 1.1).abs() < 1e-6);

        // Clip 1 is clamped to the previous cue end plus guard.
        assert!((clip_segments[1].time_window.start - 1.1).abs() < 1e-6);
        // Clip 1 has no next cue to clamp against.
        assert!((clip_segments[1].time_window.end - 2.08).abs() < 1e-6);
    }

    #[test]
    fn padding_never_overlaps_neighbor_cues() {
        let markdown = "`a@00:00:01.0-00:00:02.0` mid\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        // Cues are tightly packed with a 20ms gap.
        let cues = vec![
            TranscriptCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1000),
                text: "first".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(1020),
                end: Duration::from_millis(2000),
                text: "mid".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(2020),
                end: Duration::from_millis(3000),
                text: "third".to_string(),
                words: vec![],
                source_id: "a".to_string(),
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

        assert_eq!(clip_segments.len(), 1);

        let prev_end = cues[0].end.as_secs_f64();
        let next_start = cues[2].start.as_secs_f64();

        // The padded start/end should still fit within the 20ms gaps (with guards).
        assert!(clip_segments[0].time_window.start >= prev_end + DEFAULT_PADDING_GUARD_SECONDS);
        assert!(clip_segments[0].time_window.end <= next_start - DEFAULT_PADDING_GUARD_SECONDS);
    }

    #[test]
    fn does_not_match_same_cue_twice() {
        // Two planned dialogue clips overlap the same single cue.
        // We should error rather than align both clips to identical cue bounds.
        let markdown = "`a@00:00:00.0-00:00:00.5` first\n`a@00:00:00.4-00:00:00.9` second\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![TranscriptCue {
            start: Duration::from_millis(0),
            end: Duration::from_millis(1000),
            text: "only".to_string(),
            words: vec![],
            source_id: "a".to_string(),
        }];

        let err = align_plan_with_subtitles(&mut plan, &cues).unwrap_err();
        assert!(
            err.to_string()
                .contains("only 1 subtitle cues for 2 dialogue segments"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn aligns_out_of_order_segments_without_reusing_cues() {
        // Model the `vidtest/pups.video.md` shape: segments appear out-of-order relative to time.
        // Two segments overlap the same last cue; cue uniqueness avoids rendering duplicates.
        let markdown = concat!(
            "`a@00:00:09.7-00:00:11.6` I do not want to eat the following.\n",
            "`a@00:00:00.9-00:00:09.7` Hello, I want to eat a big, big orange.\n",
            "`a@00:00:14.4-00:00:16.0` A big pile of dog poo.\n",
            "`a@00:00:24.8-00:00:26.9` No, you don't say that.\n",
            "`a@00:00:19.2-00:00:24.8` Goodbye, this has been it.\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            TranscriptCue {
                start: Duration::from_millis(866),
                end: Duration::from_millis(7274),
                text: "Hello".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(9677),
                end: Duration::from_millis(11559),
                text: "I do not want".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(14403),
                end: Duration::from_millis(16005),
                text: "A big pile".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(19189),
                end: Duration::from_millis(20730),
                text: "Goodbye".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(20791),
                end: Duration::from_millis(26898),
                text: "No, you don't say".to_string(),
                words: vec![],
                source_id: "a".to_string(),
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

        assert_eq!(clip_segments.len(), 5);

        // Ensure no two clips end up with identical (start, end) bounds.
        // (Cue uniqueness implies unique bounds because bounds are derived from cue index.)
        for i in 0..clip_segments.len() {
            for j in (i + 1)..clip_segments.len() {
                assert!(
                    (clip_segments[i].time_window.start - clip_segments[j].time_window.start).abs()
                        > 1e-9
                        || (clip_segments[i].time_window.end - clip_segments[j].time_window.end)
                            .abs()
                            > 1e-9,
                    "clips {} and {} aligned to identical bounds",
                    i,
                    j
                );
            }
        }

        // Specifically: ensure the "No..." segment (4th authored) aligns to the last cue,
        // and the "Goodbye..." segment (5th authored) aligns to the prior cue.
        // This is the case that used to duplicate the last cue.
        let no_bounds = padded_cue_bounds(
            &cues,
            4,
            DEFAULT_DIALOGUE_PADDING_SECONDS,
            DEFAULT_PADDING_GUARD_SECONDS,
        );
        let goodbye_bounds = padded_cue_bounds(
            &cues,
            3,
            DEFAULT_DIALOGUE_PADDING_SECONDS,
            DEFAULT_PADDING_GUARD_SECONDS,
        );

        assert!((clip_segments[3].time_window.start - no_bounds.start).abs() < 1e-6);
        assert!((clip_segments[3].time_window.end - no_bounds.end).abs() < 1e-6);
        assert!((clip_segments[4].time_window.start - goodbye_bounds.start).abs() < 1e-6);
        assert!((clip_segments[4].time_window.end - goodbye_bounds.end).abs() < 1e-6);
    }

    #[test]
    fn redistributes_silence_segments_across_actual_gap() {
        let markdown = "`a@00:00:00.0-00:00:01.2` intro\n`a@00:00:01.2-00:00:03.8` SILENCE\n`a@00:00:03.8-00:00:06.8` SILENCE\n`a@00:00:06.8-00:00:08.0` outro\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            TranscriptCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1234),
                text: "intro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(6789),
                end: Duration::from_millis(8000),
                text: "outro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
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

        assert!((first_clip.time_window.end - first_silence.time_window.start).abs() < 1e-6);
        assert!((second_clip.time_window.start - second_silence.time_window.end).abs() < 1e-6);
        assert!((second_silence.time_window.start - first_silence.time_window.end).abs() < 1e-6);

        // Expected gap accounts for per-cue padding + guard.
        // Intro ends at cue1 end + padding; outro starts at cue2 start - padding.
        let expected_gap =
            (6.789 - DEFAULT_DIALOGUE_PADDING_SECONDS) - (1.234 + DEFAULT_DIALOGUE_PADDING_SECONDS);
        let actual_gap = (second_clip.time_window.start - first_clip.time_window.end).max(0.0);
        assert!((expected_gap - actual_gap).abs() < 1e-6);
    }

    #[test]
    fn does_not_stretch_silence_when_gap_is_huge() {
        let markdown = "`a@00:00:00.0-00:00:01.0` intro\n`a@00:00:01.0-00:00:02.0` SILENCE\n`a@00:00:02.0-00:00:03.0` SILENCE\n`a@00:00:50.0-00:00:51.0` outro\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            TranscriptCue {
                start: Duration::from_millis(0),
                end: Duration::from_millis(1000),
                text: "intro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                start: Duration::from_millis(50_000),
                end: Duration::from_millis(51_000),
                text: "outro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
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
        assert!((intro.time_window.end - 1.08).abs() < 1e-6);
        assert!((outro.time_window.start - 49.92).abs() < 1e-6);

        // ...but silence remains based on authored timestamps (not stretched to fill 49s).
        assert!((silence1.time_window.start - 1.0).abs() < 1e-6);
        assert!((silence1.time_window.end - 2.0).abs() < 1e-6);
        assert!((silence2.time_window.start - 2.0).abs() < 1e-6);
        assert!((silence2.time_window.end - 3.0).abs() < 1e-6);
    }

    #[test]
    fn broll_persists_across_segments_until_separator() {
        // B-roll should persist across multiple segments (like overlays) until a separator clears it
        let markdown = concat!(
            "`a@00:00:00.0-00:00:01.0` first segment\n",
            "\n",
            "> `b@00:00:00.0-00:00:05.0` B-roll footage\n",
            "\n",
            "`a@00:00:01.0-00:00:02.0` second segment (should have B-roll)\n",
            "`a@00:00:02.0-00:00:03.0` third segment (should have B-roll)\n",
            "\n",
            "---\n",
            "\n",
            "`a@00:00:03.0-00:00:04.0` fourth segment (no B-roll after separator)\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        let clips: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Clip(clip) => Some(clip),
                _ => None,
            })
            .collect();

        assert_eq!(clips.len(), 4, "expected 4 clips");

        // First clip has no B-roll (B-roll defined after it)
        assert!(
            clips[0].broll.is_none(),
            "first clip should not have B-roll"
        );

        // Second and third clips have B-roll (persisted from first B-roll definition)
        assert!(clips[1].broll.is_some(), "second clip should have B-roll");
        assert!(clips[2].broll.is_some(), "third clip should have B-roll");

        // Fourth clip has no B-roll (cleared by separator)
        assert!(
            clips[3].broll.is_none(),
            "fourth clip should not have B-roll after separator"
        );
    }

    #[test]
    fn broll_can_be_stopped_with_separator() {
        // Test that a separator stops B-roll and new B-roll can be defined after
        let markdown = concat!(
            "`a@00:00:00.0-00:00:01.0` first\n",
            "\n",
            "> `b@00:00:00.0-00:00:03.0` first B-roll\n",
            "\n",
            "`a@00:00:01.0-00:00:02.0` second (with first B-roll)\n",
            "\n",
            "---\n",
            "\n",
            "`a@00:00:02.0-00:00:03.0` third (no B-roll)\n",
            "\n",
            "> `c@00:00:00.0-00:00:02.0` second B-roll\n",
            "\n",
            "`a@00:00:03.0-00:00:04.0` fourth (with second B-roll)\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        let clips: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Clip(clip) => Some(clip),
                _ => None,
            })
            .collect();

        assert_eq!(clips.len(), 4, "expected 4 clips");

        // First clip has no B-roll
        assert!(clips[0].broll.is_none());

        // Second clip has first B-roll (from source b)
        assert!(clips[1].broll.is_some());
        assert_eq!(clips[1].broll.as_ref().unwrap().clips[0].source_id, "b");

        // Third clip has no B-roll (after separator)
        assert!(clips[2].broll.is_none());

        // Fourth clip has second B-roll (from source c)
        assert!(clips[3].broll.is_some());
        assert_eq!(clips[3].broll.as_ref().unwrap().clips[0].source_id, "c");
    }
}
