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

use anyhow::{Result, bail};

use std::cmp::Ordering;
use std::collections::BinaryHeap;

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
    let mut items = Vec::new();
    let mut standalone_count = 0usize;
    let mut overlay_count = 0usize;
    let mut ignored_count = 0usize;
    let mut heading_count = 0usize;
    let mut segment_count = 0usize;
    let mut overlay_state: Option<OverlayPlan> = None;
    let mut last_clip_item_idx: Option<usize> = None;
    // Track whether we're in a "separator region" - after a separator, before any segment.
    // Content in a separator region that ends with another separator becomes a pause.
    let mut in_separator_region = false;
    // Accumulator for merging consecutive unhandled blocks into a single slide/overlay
    let mut pending_content: Vec<(String, usize)> = Vec::new();

    /// Merge pending content into a single OverlayPlan, clearing the accumulator.
    fn flush_pending(pending: &mut Vec<(String, usize)>) -> Option<OverlayPlan> {
        if pending.is_empty() {
            return None;
        }
        let line = pending[0].1;
        let markdown = pending
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        pending.clear();
        Some(OverlayPlan { markdown, line })
    }

    for (_idx, block) in document.blocks.iter().enumerate() {
        match block {
            DocumentBlock::Segment(segment) => {
                // Flush any pending content before processing segment
                if !pending_content.is_empty() {
                    if let Some(overlay) = flush_pending(&mut pending_content) {
                        // Apply to previous clip retroactively
                        if let Some(last_idx) = last_clip_item_idx {
                            if let Some(TimelinePlanItem::Clip(clip)) = items.get_mut(last_idx) {
                                clip.overlay = Some(overlay.clone());
                            }
                        }
                        overlay_state = Some(overlay);
                        overlay_count += 1;
                    }
                }

                items.push(TimelinePlanItem::Clip(ClipPlan {
                    start: segment.range.start_seconds(),
                    end: segment.range.end_seconds(),
                    kind: segment.kind,
                    text: segment.text.clone(),
                    line: segment.line,
                    overlay: overlay_state.clone(),
                }));
                last_clip_item_idx = Some(items.len().saturating_sub(1));
                segment_count += 1;
                in_separator_region = false;
            }
            DocumentBlock::Heading(heading) => {
                items.push(TimelinePlanItem::Standalone(StandalonePlan::Heading {
                    level: heading.level,
                    text: heading.text.clone(),
                    line: heading.line,
                }));
                standalone_count += 1;
                heading_count += 1;
                // Headings don't exit separator region - they can appear between separators
            }
            DocumentBlock::Separator(_) => {
                // Flush pending content - if in separator region, it becomes a pause
                if !pending_content.is_empty() {
                    if in_separator_region {
                        // Between separators → standalone pause
                        let merged = pending_content
                            .iter()
                            .map(|(s, _)| s.as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        let trimmed = merged.trim();
                        if !trimmed.is_empty() {
                            items.push(TimelinePlanItem::Standalone(StandalonePlan::Pause {
                                markdown: merged.clone(),
                                display_text: trimmed.to_string(),
                                duration_seconds: pause_duration_seconds(trimmed),
                                line: pending_content[0].1,
                            }));
                            standalone_count += 1;
                        }
                    } else {
                        // Before separator but after segment → apply to previous clip as overlay
                        if let Some(overlay) = flush_pending(&mut pending_content) {
                            if let Some(last_idx) = last_clip_item_idx {
                                if let Some(TimelinePlanItem::Clip(clip)) = items.get_mut(last_idx)
                                {
                                    clip.overlay = Some(overlay.clone());
                                }
                            }
                            overlay_state = Some(overlay);
                            overlay_count += 1;
                        }
                    }
                    pending_content.clear();
                }
                overlay_state = None;
                in_separator_region = true;
            }
            DocumentBlock::Music(music) => {
                items.push(TimelinePlanItem::Music(MusicPlan {
                    directive: music.directive.clone(),
                    line: music.line,
                }));
                // Music blocks don't exit separator region
            }
            DocumentBlock::Unhandled(unhandled) => {
                let raw_description = unhandled.description.as_str();
                let trimmed = raw_description.trim();
                if trimmed.is_empty() {
                    ignored_count += 1;
                    continue;
                }

                // Accumulate for merging - the flush will determine if it's pause or overlay
                pending_content.push((raw_description.to_string(), unhandled.line));
            }
        }
    }

    // Final flush: any remaining pending content becomes an overlay for the last clip
    if let Some(overlay) = flush_pending(&mut pending_content) {
        if let Some(last_idx) = last_clip_item_idx {
            if let Some(TimelinePlanItem::Clip(clip)) = items.get_mut(last_idx) {
                clip.overlay = Some(overlay.clone());
            }
        }
        overlay_count += 1;
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

const DEFAULT_PAUSE_MIN_SECONDS: f64 = 5.0;
const DEFAULT_PAUSE_MAX_SECONDS: f64 = 20.0;
const DEFAULT_PAUSE_READING_WPM: f64 = 180.0;

fn pause_duration_seconds(display_text: &str) -> f64 {
    let words = display_text.split_whitespace().count() as f64;
    if words <= 0.0 {
        return DEFAULT_PAUSE_MIN_SECONDS;
    }

    let words_per_second = DEFAULT_PAUSE_READING_WPM / 60.0;
    let seconds = words / words_per_second;
    seconds.clamp(DEFAULT_PAUSE_MIN_SECONDS, DEFAULT_PAUSE_MAX_SECONDS)
}

fn align_dialogue_clips_to_cues(plan: &mut TimelinePlan, cues: &[SrtCue]) -> Result<Vec<usize>> {
    let mut dialogue_clips: Vec<(usize, f64, f64, usize, String)> = Vec::new();

    for (idx, item) in plan.items.iter().enumerate() {
        let TimelinePlanItem::Clip(clip) = item else {
            continue;
        };

        if clip.kind == SegmentKind::Silence {
            continue;
        }

        dialogue_clips.push((idx, clip.start, clip.end, clip.line, clip.text.clone()));
    }

    if dialogue_clips.is_empty() {
        return Ok(Vec::new());
    }

    let assignments = assign_cues_max_overlap(&dialogue_clips, cues)?;

    let mut dialogue_indices: Vec<usize> = Vec::new();
    for (clip_idx, cue_idx) in assignments {
        let Some(TimelinePlanItem::Clip(clip)) = plan.items.get_mut(clip_idx) else {
            continue;
        };

        let (start, end) = padded_cue_bounds(
            cues,
            cue_idx,
            DEFAULT_DIALOGUE_PADDING_SECONDS,
            DEFAULT_PADDING_GUARD_SECONDS,
        );

        clip.start = start;
        clip.end = end;
        dialogue_indices.push(clip_idx);
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

#[derive(Debug, Clone)]
struct McmfEdge {
    to: usize,
    rev: usize,
    cap: i64,
    cost: i64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct HeapState {
    cost: i64,
    node: usize,
}

impl Ord for HeapState {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap behavior via reversed ordering.
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| self.node.cmp(&other.node))
    }
}

impl PartialOrd for HeapState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn min_cost_max_flow(
    graph: &mut [Vec<McmfEdge>],
    source: usize,
    sink: usize,
    max_flow: i64,
) -> (i64, i64) {
    let node_count = graph.len();
    let mut potentials = vec![0i64; node_count];
    let mut total_flow = 0i64;
    let mut total_cost = 0i64;

    let mut dist = vec![0i64; node_count];
    let mut prev_node = vec![0usize; node_count];
    let mut prev_edge = vec![0usize; node_count];

    while total_flow < max_flow {
        dist.fill(i64::MAX / 4);
        dist[source] = 0;

        let mut heap = BinaryHeap::new();
        heap.push(HeapState {
            cost: 0,
            node: source,
        });

        while let Some(HeapState { cost, node }) = heap.pop() {
            if cost != dist[node] {
                continue;
            }

            for (edge_idx, edge) in graph[node].iter().enumerate() {
                if edge.cap <= 0 {
                    continue;
                }

                let next = edge.to;
                let next_cost = cost + edge.cost + potentials[node] - potentials[next];
                if next_cost < dist[next] {
                    dist[next] = next_cost;
                    prev_node[next] = node;
                    prev_edge[next] = edge_idx;
                    heap.push(HeapState {
                        cost: next_cost,
                        node: next,
                    });
                }
            }
        }

        if dist[sink] >= i64::MAX / 5 {
            break;
        }

        for node in 0..node_count {
            if dist[node] < i64::MAX / 5 {
                potentials[node] += dist[node];
            }
        }

        let mut add_flow = max_flow - total_flow;
        let mut v = sink;
        while v != source {
            let u = prev_node[v];
            let edge_idx = prev_edge[v];
            let cap = graph[u][edge_idx].cap;
            add_flow = add_flow.min(cap);
            v = u;
        }

        v = sink;
        while v != source {
            let u = prev_node[v];
            let edge_idx = prev_edge[v];
            let rev = graph[u][edge_idx].rev;

            graph[u][edge_idx].cap -= add_flow;
            graph[v][rev].cap += add_flow;

            total_cost += graph[u][edge_idx].cost * add_flow;
            v = u;
        }

        total_flow += add_flow;
    }

    (total_flow, total_cost)
}

fn add_edge(graph: &mut [Vec<McmfEdge>], from: usize, to: usize, cap: i64, cost: i64) {
    let from_rev = graph[to].len();
    let to_rev = graph[from].len();

    graph[from].push(McmfEdge {
        to,
        rev: from_rev,
        cap,
        cost,
    });
    graph[to].push(McmfEdge {
        to: from,
        rev: to_rev,
        cap: 0,
        cost: -cost,
    });
}

fn assign_cues_max_overlap(
    dialogue_clips: &[(usize, f64, f64, usize, String)],
    cues: &[SrtCue],
) -> Result<Vec<(usize, usize)>> {
    if cues.is_empty() {
        bail!("Unable to align subtitles: no subtitle cues available");
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

    let source = 0usize;
    let clip_offset = 1usize;
    let cue_offset = clip_offset + clip_count;
    let sink = cue_offset + cue_count;
    let node_count = sink + 1;
    let mut graph: Vec<Vec<McmfEdge>> = vec![Vec::new(); node_count];

    for clip_idx in 0..clip_count {
        add_edge(&mut graph, source, clip_offset + clip_idx, 1, 0);
    }

    for cue_idx in 0..cue_count {
        add_edge(&mut graph, cue_offset + cue_idx, sink, 1, 0);
    }

    for (clip_idx, (_timeline_idx, clip_start, clip_end, _line, _text)) in
        dialogue_clips.iter().enumerate()
    {
        let clip_duration = (clip_end - clip_start).max(0.0);
        if clip_duration <= 0.0 {
            continue;
        }

        for (cue_idx, cue) in cues.iter().enumerate() {
            let cue_start = cue.start.as_secs_f64();
            let cue_end = cue.end.as_secs_f64();
            let overlap = overlap_seconds(*clip_start, *clip_end, cue_start, cue_end);
            if overlap <= 0.0 {
                continue;
            }

            if overlap / clip_duration < 0.01 {
                continue;
            }

            // Convert to integer cost: maximize overlap, then prefer closer starts.
            // Costs are negated because the solver minimizes total cost.
            let overlap_cost = -(overlap * 1_000_000.0).round() as i64;
            let distance = (cue_start - *clip_start).abs();
            let distance_cost = (distance * 1_000.0).round() as i64;

            let cost = overlap_cost + distance_cost;
            add_edge(
                &mut graph,
                clip_offset + clip_idx,
                cue_offset + cue_idx,
                1,
                cost,
            );
        }
    }

    let (flow, _cost) = min_cost_max_flow(&mut graph, source, sink, clip_count as i64);
    if flow < clip_count as i64 {
        // Find a useful error pointing to the first clip with no candidate cues.
        for (timeline_idx, clip_start, clip_end, line, text) in dialogue_clips {
            let clip_duration = (clip_end - clip_start).max(0.0);
            if clip_duration <= 0.0 {
                bail!("Invalid segment duration for `{}` at line {}", text, line);
            }

            let mut has_candidate = false;
            for cue in cues {
                let cue_start = cue.start.as_secs_f64();
                let cue_end = cue.end.as_secs_f64();
                let overlap = overlap_seconds(*clip_start, *clip_end, cue_start, cue_end);
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
                bail!(
                    "Unable to locate subtitle entry for segment `{}` at line {}",
                    text,
                    line
                );
            }

            let _ = timeline_idx;
        }

        bail!("Unable to align subtitles: could not assign unique cues to every segment");
    }

    let mut result: Vec<(usize, usize)> = Vec::with_capacity(clip_count);

    for clip_idx in 0..clip_count {
        let clip_node = clip_offset + clip_idx;
        let timeline_idx = dialogue_clips[clip_idx].0;

        let mut matched: Option<usize> = None;
        for edge in &graph[clip_node] {
            let is_to_cue = edge.to >= cue_offset && edge.to < cue_offset + cue_count;
            if !is_to_cue {
                continue;
            }

            // If we sent 1 unit of flow along clip->cue, then forward edge cap is now 0.
            if edge.cap == 0 {
                matched = Some(edge.to - cue_offset);
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
    fn slide_applies_to_immediately_previous_clip_and_clears_on_separator() {
        let markdown = concat!(
            "`00:00:00.0-00:00:01.0` first\n",
            "slide 1\n\n",
            "---\n\n",
            "`00:00:01.0-00:00:02.0` second\n",
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
            "`00:00:00.0-00:00:01.0` first\n",
            "slide 1\n\n",
            "slide 2\n\n",
            "`00:00:01.0-00:00:02.0` second\n",
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
            "`00:00:00.0-00:00:01.0` first\n\n",
            "---\n\n",
            "short\n\n",
            "---\n\n",
            "`00:00:01.0-00:00:02.0` second\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let plan = plan_timeline(&document).unwrap();

        let pauses: Vec<_> = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Standalone(StandalonePlan::Pause { duration_seconds, .. }) => {
                    Some(*duration_seconds)
                }
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
                text: "third".to_string(),
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
        assert!(clip_segments[0].start >= prev_end + DEFAULT_PADDING_GUARD_SECONDS);
        assert!(clip_segments[0].end <= next_start - DEFAULT_PADDING_GUARD_SECONDS);
    }

    #[test]
    fn does_not_match_same_cue_twice() {
        // Two planned dialogue clips overlap the same single cue.
        // We should error rather than align both clips to identical cue bounds.
        let markdown = "`00:00:00.0-00:00:00.5` first\n`00:00:00.4-00:00:00.9` second\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![SrtCue {
            start: Duration::from_millis(0),
            end: Duration::from_millis(1000),
            text: "only".to_string(),
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
            "`00:00:09.7-00:00:11.6` I do not want to eat the following.\n",
            "`00:00:00.9-00:00:09.7` Hello, I want to eat a big, big orange.\n",
            "`00:00:14.4-00:00:16.0` A big pile of dog poo.\n",
            "`00:00:24.8-00:00:26.9` No, you don't say that.\n",
            "`00:00:19.2-00:00:24.8` Goodbye, this has been it.\n",
        );

        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = plan_timeline(&document).unwrap();

        let cues = vec![
            SrtCue {
                start: Duration::from_millis(866),
                end: Duration::from_millis(7274),
                text: "Hello".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(9677),
                end: Duration::from_millis(11559),
                text: "I do not want".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(14403),
                end: Duration::from_millis(16005),
                text: "A big pile".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(19189),
                end: Duration::from_millis(20730),
                text: "Goodbye".to_string(),
            },
            SrtCue {
                start: Duration::from_millis(20791),
                end: Duration::from_millis(26898),
                text: "No, you don't say".to_string(),
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
                    (clip_segments[i].start - clip_segments[j].start).abs() > 1e-9
                        || (clip_segments[i].end - clip_segments[j].end).abs() > 1e-9,
                    "clips {} and {} aligned to identical bounds",
                    i,
                    j
                );
            }
        }

        // Specifically: ensure the "No..." segment (4th authored) aligns to the last cue,
        // and the "Goodbye..." segment (5th authored) aligns to the prior cue.
        // This is the case that used to duplicate the last cue.
        let (no_start, no_end) = padded_cue_bounds(
            &cues,
            4,
            DEFAULT_DIALOGUE_PADDING_SECONDS,
            DEFAULT_PADDING_GUARD_SECONDS,
        );
        let (goodbye_start, goodbye_end) = padded_cue_bounds(
            &cues,
            3,
            DEFAULT_DIALOGUE_PADDING_SECONDS,
            DEFAULT_PADDING_GUARD_SECONDS,
        );

        assert!((clip_segments[3].start - no_start).abs() < 1e-6);
        assert!((clip_segments[3].end - no_end).abs() < 1e-6);
        assert!((clip_segments[4].start - goodbye_start).abs() < 1e-6);
        assert!((clip_segments[4].end - goodbye_end).abs() < 1e-6);
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
