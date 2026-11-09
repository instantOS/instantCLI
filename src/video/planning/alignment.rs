use super::assignment_solver::{McmfEdge, add_edge, min_cost_max_flow};
use super::core::{TimelinePlan, TimelinePlanItem};
use super::cut_snap::{AudioEnvelope, snapped_cue_bounds};
use crate::video::document::SegmentKind;
use crate::video::render::timeline::SourceRange;
use crate::video::support::transcript::TranscriptCue;
use anyhow::{Context, Result, bail, ensure};
use std::collections::{HashMap, HashSet};

/// Loudness envelopes keyed by transcript source id. A source without an
/// envelope falls back to plain fixed padding.
pub type SourceEnvelopes = HashMap<String, AudioEnvelope>;

/// Match planned dialogue clips to transcript cues and set their source windows.
///
/// When an envelope is available for a clip's source, each cut is snapped to the
/// quietest nearby point of the audio that is actually rendered, reducing the
/// risk of clipped words or leaked deleted speech from timestamp error.
/// `cues` must retain the original acoustic word timings, not subtitle edits.
pub fn align_plan_with_subtitles(
    plan: &mut TimelinePlan,
    cues: &[TranscriptCue],
    envelopes: &SourceEnvelopes,
) -> Result<()> {
    let dialogue_indices = match_dialogue_clips_to_cues(plan, cues)?;
    if dialogue_indices.is_empty() {
        return Ok(());
    }

    pad_dialogue_clips(plan, cues, &dialogue_indices, envelopes)?;

    let silence_updates = collect_silence_time_updates(&plan.items, &dialogue_indices);
    apply_clip_time_updates(plan, silence_updates);
    validate_dialogue_runs(plan, cues)?;

    Ok(())
}

const MAX_SILENCE_STRETCH_RATIO: f64 = 2.5;
/// Cue padding may move the dialogue surrounding an authored silence slightly,
/// but a large boundary change means a source interval was moved elsewhere in
/// document order and must not be swallowed back into the silence.
const MAX_SILENCE_BOUNDARY_ADJUSTMENT_SECONDS: f64 = 0.5;

#[derive(Debug, Clone, Copy)]
struct ClipTimeUpdate {
    idx: usize,
    time_window: SourceRange,
}

const DEFAULT_DIALOGUE_PADDING_SECONDS: f64 = 0.08;
const DEFAULT_PADDING_GUARD_SECONDS: f64 = 0.0;

fn match_dialogue_clips_to_cues(
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

        let mut dialogue_clips: Vec<(usize, SourceRange, String)> = Vec::new();
        let mut identity_assignments = Vec::new();
        let mut fallback_clips = Vec::new();
        for idx in clip_indices {
            let Some(TimelinePlanItem::Clip(clip)) = plan.items.get(idx) else {
                continue;
            };
            let entry = (idx, clip.time_window, clip.text.clone());
            if let Some(cue_id) = clip.cue_id {
                let cue_idx = source_cues
                    .iter()
                    .position(|cue| cue.cue_id == cue_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Source `{source_id}` has no transcript cue #{cue_id} referenced by `{}`",
                            clip.text
                        )
                    })?;
                identity_assignments.push((idx, cue_idx));
            } else {
                fallback_clips.push(entry.clone());
            }
            dialogue_clips.push(entry);
        }

        if dialogue_clips.is_empty() {
            continue;
        }

        let mut claimed = HashSet::new();
        for (_, cue_idx) in &identity_assignments {
            if !claimed.insert(*cue_idx) {
                bail!(
                    "Transcript cue #{} from source `{source_id}` is referenced more than once",
                    source_cues[*cue_idx].cue_id
                );
            }
        }
        let fallback_cues = source_cues
            .iter()
            .enumerate()
            .filter(|(cue_idx, _)| !claimed.contains(cue_idx))
            .map(|(cue_idx, cue)| (cue_idx, cue.clone()))
            .collect::<Vec<_>>();
        let mut assignments = identity_assignments;
        if !fallback_clips.is_empty() {
            let fallback_values = fallback_cues
                .iter()
                .map(|(_, cue)| cue.clone())
                .collect::<Vec<_>>();
            assignments.extend(
                assign_cues_max_overlap(&fallback_clips, &fallback_values)?
                    .into_iter()
                    .map(|(clip_idx, local_cue_idx)| (clip_idx, fallback_cues[local_cue_idx].0)),
            );
        }

        for (clip_idx, cue_idx) in assignments {
            let Some(TimelinePlanItem::Clip(clip)) = plan.items.get_mut(clip_idx) else {
                continue;
            };

            let cue = &source_cues[cue_idx];
            clip.cue_id = Some(cue.cue_id);
            clip.time_window = SourceRange::new(cue.start.as_secs_f64(), cue.end.as_secs_f64());
            dialogue_indices.push(clip_idx);
        }
    }

    Ok(dialogue_indices)
}

fn pad_dialogue_clips(
    plan: &mut TimelinePlan,
    cues: &[TranscriptCue],
    dialogue_indices: &[usize],
    envelopes: &SourceEnvelopes,
) -> Result<()> {
    let mut cues_by_source: HashMap<&str, Vec<TranscriptCue>> = HashMap::new();
    for cue in cues {
        cues_by_source
            .entry(&cue.source_id)
            .or_default()
            .push(cue.clone());
    }
    for source_cues in cues_by_source.values_mut() {
        source_cues.sort_by_key(|cue| cue.start);
    }

    for &idx in dialogue_indices {
        let Some(TimelinePlanItem::Clip(clip)) = plan.items.get_mut(idx) else {
            continue;
        };
        let cue_id = clip
            .cue_id
            .ok_or_else(|| anyhow::anyhow!("Matched dialogue clip has no cue identity"))?;
        let source_cues = cues_by_source
            .get(clip.source_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("No transcript cues loaded for `{}`", clip.source_id))?;
        let cue_idx = source_cues
            .iter()
            .position(|cue| cue.cue_id == cue_id)
            .ok_or_else(|| {
                anyhow::anyhow!("Transcript cue #{cue_id} disappeared during padding")
            })?;
        let time_window = match envelopes.get(clip.source_id.as_str()) {
            Some(envelope) => snapped_cue_bounds(
                source_cues,
                cue_idx,
                DEFAULT_DIALOGUE_PADDING_SECONDS,
                envelope,
            )
            .with_context(|| {
                format!(
                    "Cannot refine audio cuts for source `{}` cue #{cue_id}",
                    clip.source_id
                )
            })?,
            None => padded_cue_bounds(
                source_cues,
                cue_idx,
                DEFAULT_DIALOGUE_PADDING_SECONDS,
                DEFAULT_PADDING_GUARD_SECONDS,
            ),
        };
        validate_positive_dialogue_bounds(&source_cues[cue_idx], time_window)?;
        clip.time_window = time_window;
    }
    Ok(())
}

fn validate_positive_dialogue_bounds(cue: &TranscriptCue, bounds: SourceRange) -> Result<()> {
    ensure!(
        bounds.end > bounds.start,
        "Cuts collapse dialogue for source `{}` cue #{}: [{}, {}]. Review transcript timings before rendering.",
        cue.source_id,
        cue.cue_id,
        bounds.start,
        bounds.end
    );
    Ok(())
}

fn validate_dialogue_bounds(cue: &TranscriptCue, bounds: SourceRange) -> Result<()> {
    validate_positive_dialogue_bounds(cue, bounds)?;
    // Check both ends of the retained source run: adjacent clips can preserve
    // word parts that lie outside this cue's own clip window.
    for word in cue.words.first().into_iter().chain(cue.words.last()) {
        let start = word.start.as_secs_f64();
        let end = word.end.as_secs_f64();
        let retained = (bounds.end.min(end) - bounds.start.max(start)).max(0.0);
        ensure!(
            retained + 1e-9 >= (end - start) / 2.0,
            "Cuts retain less than half of edge word `{}` for source `{}` cue #{}. Review transcript timings before rendering.",
            word.word,
            cue.source_id,
            cue.cue_id
        );
    }
    Ok(())
}

fn validate_dialogue_runs(plan: &TimelinePlan, cues: &[TranscriptCue]) -> Result<()> {
    let cue_map: HashMap<_, _> = cues
        .iter()
        .map(|cue| ((cue.source_id.as_str(), cue.cue_id), cue))
        .collect();
    let mut start_idx = 0;
    while start_idx < plan.items.len() {
        let TimelinePlanItem::Clip(first) = &plan.items[start_idx] else {
            start_idx += 1;
            continue;
        };
        let mut bounds = first.time_window;
        let mut end_idx = start_idx + 1;
        while let Some(item) = plan.items.get(end_idx) {
            match item {
                TimelinePlanItem::Music(_) => {}
                TimelinePlanItem::Clip(clip)
                    if clip.source_id == first.source_id
                        && (clip.time_window.start - bounds.end).abs() <= 1e-9
                        && clip.time_window.end >= clip.time_window.start =>
                {
                    bounds.end = clip.time_window.end;
                }
                _ => break,
            }
            end_idx += 1;
        }

        // Each item belongs to one run, so grouping and validation stay linear.
        // Source silence clips contribute audio too; standalone content does not.
        for item in &plan.items[start_idx..end_idx] {
            let TimelinePlanItem::Clip(clip) = item else {
                continue;
            };
            if clip.kind == SegmentKind::Silence {
                continue;
            }
            let cue = clip
                .cue_id
                .and_then(|cue_id| cue_map.get(&(clip.source_id.as_str(), cue_id)))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No matched transcript cue for dialogue in source `{}` (cue {:?})",
                        clip.source_id,
                        clip.cue_id
                    )
                })?;
            validate_positive_dialogue_bounds(cue, clip.time_window)?;
            validate_dialogue_bounds(cue, bounds)?;
        }
        start_idx = end_idx;
    }
    Ok(())
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
            clip.time_window = SourceRange::new(
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
        let authored_start = clip_start(items, self.start_idx)?;
        let authored_end = clip_end(items, self.end_idx.checked_sub(1)?)?;
        if (prev_end - authored_start).abs() > MAX_SILENCE_BOUNDARY_ADJUSTMENT_SECONDS
            || (next_start - authored_end).abs() > MAX_SILENCE_BOUNDARY_ADJUSTMENT_SECONDS
        {
            return None;
        }

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
                time_window: SourceRange::new(current, current + actual_duration),
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
) -> SourceRange {
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
        SourceRange::new(padded_start, padded_start)
    } else {
        SourceRange::new(padded_start, padded_end)
    }
}

fn assign_cues_max_overlap(
    dialogue_clips: &[(usize, SourceRange, String)],
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
    dialogue_clips: &[(usize, SourceRange, String)],
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
        dialogue_clips: &[(usize, SourceRange, String)],
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
                let overlap = clip_window.overlap_seconds(SourceRange::new(cue_start, cue_end));

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
        dialogue_clips: &[(usize, SourceRange, String)],
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
    dialogue_clips: &[(usize, SourceRange, String)],
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
            let overlap = clip_window.overlap_seconds(SourceRange::new(cue_start, cue_end));
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
    use crate::video::planning::TimelinePlanItem;
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
        let plan = document.plan_timeline().unwrap();

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
        let plan = document.plan_timeline().unwrap();

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
        let plan = document.plan_timeline().unwrap();

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
        let plan = document.plan_timeline().unwrap();

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
        let mut plan = document.plan_timeline().unwrap();

        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_millis(0),
                end: Duration::from_millis(950),
                text: "first".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_millis(1200),
                end: Duration::from_millis(2450),
                text: "second".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();

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
    fn audio_snapping_keeps_kept_neighbours_contiguous_and_clear_of_deleted_words() {
        use crate::video::support::transcript::WordTiming;

        // Three abutting cues. The 1.00 s stamp is early: the first word
        // sounds until 1.02 s, followed by a dip at 1.02-1.04 s. The 2.00 s
        // stamp is late: the retained last word ends at 1.98 s, followed by
        // a dip at 1.98-2.00 s before the deleted next cue sounds at 2.00 s.
        let tone = |seconds: f64, amplitude: f64| -> Vec<u8> {
            (0..(seconds * 16_000.0) as usize)
                .flat_map(|n| {
                    let phase = std::f64::consts::TAU * 200.0 * n as f64 / 16_000.0;
                    ((amplitude * phase.sin() * 32767.0) as i16).to_le_bytes()
                })
                .collect::<Vec<u8>>()
        };
        let mut pcm = tone(1.02, 0.5);
        pcm.extend(tone(0.02, 0.0));
        pcm.extend(tone(0.94, 0.5));
        pcm.extend(tone(0.02, 0.0));
        pcm.extend(tone(1.0, 0.5));
        let envelope = AudioEnvelope::from_s16le_reader(std::io::Cursor::new(pcm), 16_000).unwrap();
        let mut envelopes = SourceEnvelopes::new();
        envelopes.insert("a".to_string(), envelope);

        let word = |text: &str, start: u64, end: u64| WordTiming {
            word: text.to_string(),
            start: Duration::from_millis(start),
            end: Duration::from_millis(end),
        };
        let cue = |cue_id: usize, words: Vec<WordTiming>| TranscriptCue {
            cue_id,
            start: words.first().unwrap().start,
            end: words.last().unwrap().end,
            text: "x".to_string(),
            words,
            source_id: "a".to_string(),
        };
        let cues = vec![
            cue(0, vec![word("one", 200, 1000)]),
            cue(1, vec![word("two", 1000, 1800), word("last", 1800, 2000)]),
            cue(2, vec![word("three", 2000, 2900)]),
        ];

        // Keep lines 0 and 1 (contiguous neighbours); delete line 2.
        let aligned_windows = |markdown: &str| {
            let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
            let mut plan = document.plan_timeline().unwrap();
            align_plan_with_subtitles(&mut plan, &cues, &envelopes).unwrap();
            plan.items
                .iter()
                .filter_map(|item| match item {
                    TimelinePlanItem::Clip(clip) => Some(clip.time_window),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let lines = [
            "`a#0@00:00:00.2-00:00:01.0` one\n",
            "`a#1@00:00:01.0-00:00:02.0` two last\n",
            "`a#2@00:00:02.0-00:00:02.9` three\n",
        ];
        let windows = aligned_windows(&lines[..2].concat());
        assert_eq!(windows.len(), 2);
        assert_eq!(
            windows[0].end, windows[1].start,
            "kept neighbours must share one cut"
        );
        assert!(
            (1.02..=1.04).contains(&windows[0].end),
            "cut {} is not in the dip between the words",
            windows[0].end
        );
        assert!(
            windows[1].end >= 1.98,
            "cut {} clips the retained last word's tail",
            windows[1].end
        );
        assert!(
            windows[1].end < 2.0,
            "cut {} misses the dip and risks leaking the deleted next cue",
            windows[1].end
        );

        let all_windows = aligned_windows(&lines.concat());
        for indices in [vec![0, 1], vec![0, 2], vec![2, 0], vec![1, 2]] {
            let markdown = indices.iter().map(|&idx| lines[idx]).collect::<String>();
            let edited_windows = aligned_windows(&markdown);
            assert_eq!(edited_windows.len(), indices.len());
            for (window, idx) in edited_windows.iter().zip(indices) {
                assert_eq!(window.start, all_windows[idx].start);
                assert_eq!(window.end, all_windows[idx].end);
            }
        }
        // Deleting the first cue must not pull the next clip's beginning back
        // into the deleted word, which really continues through 1.02 s.
        assert!(all_windows[1].start >= 1.02);
        assert!(all_windows[2].start >= 1.98 && all_windows[2].start < 2.0);
    }

    #[test]
    fn two_sided_snapping_cannot_shave_away_a_short_middle_word() {
        use crate::video::support::transcript::WordTiming;

        let pcm = (0..16_000)
            .flat_map(|sample| {
                let time = f64::from(sample) / 16_000.0;
                let amplitude = if (0.51..0.53).contains(&time) {
                    0.0
                } else {
                    0.5
                };
                let value = amplitude * (std::f64::consts::TAU * 200.0 * time).sin();
                ((value * 32767.0) as i16).to_le_bytes()
            })
            .collect::<Vec<_>>();
        let envelope = AudioEnvelope::from_s16le_reader(std::io::Cursor::new(pcm), 16_000).unwrap();
        let envelopes = SourceEnvelopes::from([("a".into(), envelope)]);
        let cues = [(0.2, 0.5), (0.5, 0.54), (0.54, 0.9)]
            .into_iter()
            .enumerate()
            .map(|(cue_id, (start, end))| {
                let start = Duration::from_secs_f64(start);
                let end = Duration::from_secs_f64(end);
                TranscriptCue {
                    cue_id,
                    start,
                    end,
                    text: "word".into(),
                    words: vec![WordTiming {
                        word: "word".into(),
                        start,
                        end,
                    }],
                    source_id: "a".into(),
                }
            })
            .collect::<Vec<_>>();
        let document =
            parse_video_document("`a#1@00:00:00.5-00:00:00.54` word\n", Path::new("test.md"))
                .unwrap();
        let mut plan = document.plan_timeline().unwrap();
        let error = align_plan_with_subtitles(&mut plan, &cues, &envelopes).unwrap_err();
        assert!(error.to_string().contains("less than half"));
        assert!(error.to_string().contains("source `a` cue #1"));

        let document = parse_video_document(
            concat!(
                "`a#0@00:00:00.2-00:00:00.5` first\n",
                "`a#1@00:00:00.5-00:00:00.54` word\n",
                "`a#2@00:00:00.54-00:00:00.9` last\n",
            ),
            Path::new("test.md"),
        )
        .unwrap();
        let empty_envelopes = SourceEnvelopes::new();
        for envelopes in [&envelopes, &empty_envelopes] {
            let mut plan = document.plan_timeline().unwrap();
            align_plan_with_subtitles(&mut plan, &cues, envelopes).unwrap();
            let windows = plan
                .items
                .iter()
                .filter_map(|item| match item {
                    TimelinePlanItem::Clip(clip) => Some(clip.time_window),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(windows.len(), 3);
            assert_eq!(windows[0].end, windows[1].start);
            assert_eq!(windows[1].end, windows[2].start);
            assert!(windows[0].start <= 0.5 && windows[2].end >= 0.54);
            if !envelopes.is_empty() {
                assert!(windows[1].end - windows[1].start < 0.02);
            }
        }
    }

    #[test]
    fn dialogue_retention_uses_contiguous_source_silence_but_not_other_runs() {
        use crate::video::support::transcript::WordTiming;

        let document = parse_video_document(
            concat!(
                "`a#0@00:00:00.2-00:00:00.5` first\n",
                "`a#1@00:00:00.5-00:00:00.54` middle\n",
                "`a#2@00:00:00.54-00:00:00.9` last\n",
            ),
            Path::new("test.md"),
        )
        .unwrap();
        let mut plan = document.plan_timeline().unwrap();
        let windows = [(0.2, 0.525), (0.525, 0.535), (0.535, 0.9)];
        for (idx, item) in plan.items.iter_mut().enumerate() {
            let TimelinePlanItem::Clip(clip) = item else {
                panic!("expected a source clip");
            };
            clip.time_window = SourceRange::new(windows[idx].0, windows[idx].1);
            if idx != 1 {
                clip.kind = SegmentKind::Silence;
            }
        }
        let cues = vec![TranscriptCue {
            cue_id: 1,
            start: Duration::from_secs_f64(0.5),
            end: Duration::from_secs_f64(0.54),
            text: "middle".into(),
            words: vec![WordTiming {
                word: "middle".into(),
                start: Duration::from_secs_f64(0.5),
                end: Duration::from_secs_f64(0.54),
            }],
            source_id: "a".into(),
        }];
        validate_dialogue_runs(&plan, &cues).unwrap();

        for case in 0..3 {
            let mut separated = plan.clone();
            if case == 2 {
                separated.items.insert(
                    1,
                    TimelinePlanItem::Standalone(super::super::core::StandalonePlan {
                        markdown: "pause".into(),
                        duration_seconds: 1.0,
                    }),
                );
            } else if let TimelinePlanItem::Clip(clip) = &mut separated.items[0] {
                if case == 0 {
                    clip.source_id = "other".into();
                } else {
                    clip.time_window.end = 0.52;
                }
            }
            let error = validate_dialogue_runs(&separated, &cues).unwrap_err();
            assert!(error.to_string().contains("less than half"));
        }
        if let TimelinePlanItem::Clip(clip) = &mut plan.items[1] {
            clip.time_window.end = clip.time_window.start;
        }
        let error = validate_dialogue_runs(&plan, &cues).unwrap_err();
        assert!(error.to_string().contains("collapse dialogue"));
    }

    #[test]
    fn empty_word_timings_require_only_a_positive_dialogue_window() {
        let cue = TranscriptCue {
            cue_id: 1,
            start: Duration::from_secs(1),
            end: Duration::from_secs(2),
            text: "untimed words".into(),
            words: vec![],
            source_id: "a".into(),
        };
        validate_dialogue_bounds(&cue, SourceRange::new(1.4, 1.5)).unwrap();
        assert!(validate_dialogue_bounds(&cue, SourceRange::new(1.5, 1.5)).is_err());
    }

    #[test]
    fn fixed_padding_cannot_silently_collapse_dialogue() {
        let document = parse_video_document(
            "`a#1@00:00:00.5-00:00:00.54` middle\n",
            Path::new("test.md"),
        )
        .unwrap();
        let mut plan = document.plan_timeline().unwrap();
        let cues = [(0.2, 0.6), (0.5, 0.54), (0.51, 0.9)]
            .into_iter()
            .enumerate()
            .map(|(cue_id, (start, end))| TranscriptCue {
                cue_id,
                start: Duration::from_secs_f64(start),
                end: Duration::from_secs_f64(end),
                text: "word".into(),
                words: vec![],
                source_id: "a".into(),
            })
            .collect::<Vec<_>>();
        let error =
            align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap_err();
        assert!(error.to_string().contains("collapse dialogue"));
        assert!(error.to_string().contains("source `a` cue #1"));
    }

    #[test]
    fn cue_identity_survives_reordering_text_edits_and_inaccurate_display_times() {
        let markdown = concat!(
            "`a#1@00:00:00.0-00:00:00.1` corrected second\n",
            "`a#0@00:00:09.0-00:00:09.1` corrected first\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = document.plan_timeline().unwrap();
        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_secs_f64(1.0),
                end: Duration::from_secs_f64(2.0),
                text: "first".into(),
                words: vec![],
                source_id: "a".into(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_secs_f64(5.0),
                end: Duration::from_secs_f64(6.0),
                text: "second".into(),
                words: vec![],
                source_id: "a".into(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();
        let clips = plan
            .items
            .iter()
            .filter_map(|item| match item {
                TimelinePlanItem::Clip(clip) => Some(clip),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(clips[0].cue_id, Some(1));
        assert!(clips[0].time_window.start > 4.8);
        assert_eq!(clips[1].cue_id, Some(0));
        assert!(clips[1].time_window.start < 1.1);
    }

    #[test]
    fn duplicate_authoritative_cue_references_are_rejected() {
        let markdown = concat!(
            "`a#0@00:00:00.0-00:00:01.0` first\n",
            "`a#0@00:00:01.0-00:00:02.0` duplicate\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = document.plan_timeline().unwrap();
        let cues = vec![TranscriptCue {
            cue_id: 0,
            start: Duration::from_secs_f64(0.0),
            end: Duration::from_secs_f64(1.0),
            text: "first".into(),
            words: vec![],
            source_id: "a".into(),
        }];

        let error =
            align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap_err();
        assert!(error.to_string().contains("referenced more than once"));
    }

    #[test]
    fn aligns_using_time_overlap_not_text() {
        let markdown = "`a@00:00:00.0-00:00:01.0` hello\n`a@00:00:01.0-00:00:02.0` world\n";
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = document.plan_timeline().unwrap();

        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_millis(0),
                end: Duration::from_millis(1100),
                text: "completely different".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_millis(1100),
                end: Duration::from_millis(2000),
                text: "also different".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();

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
        let mut plan = document.plan_timeline().unwrap();

        // Cues are tightly packed with a 20ms gap.
        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_millis(0),
                end: Duration::from_millis(1000),
                text: "first".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_millis(1020),
                end: Duration::from_millis(2000),
                text: "mid".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 2,
                start: Duration::from_millis(2020),
                end: Duration::from_millis(3000),
                text: "third".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();

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
        let mut plan = document.plan_timeline().unwrap();

        let cues = vec![TranscriptCue {
            cue_id: 0,
            start: Duration::from_millis(0),
            end: Duration::from_millis(1000),
            text: "only".to_string(),
            words: vec![],
            source_id: "a".to_string(),
        }];

        let err = align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap_err();
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
        let mut plan = document.plan_timeline().unwrap();

        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_millis(866),
                end: Duration::from_millis(7274),
                text: "Hello".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_millis(9677),
                end: Duration::from_millis(11559),
                text: "I do not want".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 2,
                start: Duration::from_millis(14403),
                end: Duration::from_millis(16005),
                text: "A big pile".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 3,
                start: Duration::from_millis(19189),
                end: Duration::from_millis(20730),
                text: "Goodbye".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 4,
                start: Duration::from_millis(20791),
                end: Duration::from_millis(26898),
                text: "No, you don't say".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();

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
        let mut plan = document.plan_timeline().unwrap();

        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_millis(0),
                end: Duration::from_millis(1234),
                text: "intro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_millis(6789),
                end: Duration::from_millis(8000),
                text: "outro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();

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
        let mut plan = document.plan_timeline().unwrap();

        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_millis(0),
                end: Duration::from_millis(1000),
                text: "intro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_millis(50_000),
                end: Duration::from_millis(51_000),
                text: "outro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();

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
    fn reordered_dialogue_is_not_absorbed_into_later_silence() {
        let markdown = concat!(
            "`a@00:00:02.0-00:00:03.0` moved earlier\n",
            "`a@00:00:00.0-00:00:01.0` intro\n",
            "`a@00:00:01.0-00:00:02.0` SILENCE\n",
            "`a@00:00:03.0-00:00:04.0` outro\n",
        );
        let document = parse_video_document(markdown, Path::new("test.md")).unwrap();
        let mut plan = document.plan_timeline().unwrap();
        let cues = vec![
            TranscriptCue {
                cue_id: 0,
                start: Duration::from_secs(0),
                end: Duration::from_secs(1),
                text: "intro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 1,
                start: Duration::from_secs(2),
                end: Duration::from_secs(3),
                text: "moved earlier".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
            TranscriptCue {
                cue_id: 2,
                start: Duration::from_secs(3),
                end: Duration::from_secs(4),
                text: "outro".to_string(),
                words: vec![],
                source_id: "a".to_string(),
            },
        ];

        align_plan_with_subtitles(&mut plan, &cues, &SourceEnvelopes::new()).unwrap();

        let silence = plan
            .items
            .iter()
            .find_map(|item| match item {
                TimelinePlanItem::Clip(clip) if clip.kind == SegmentKind::Silence => Some(clip),
                _ => None,
            })
            .unwrap();
        assert!((silence.time_window.start - 1.0).abs() < 1e-6);
        assert!((silence.time_window.end - 2.0).abs() < 1e-6);
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
        let plan = document.plan_timeline().unwrap();

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
        let plan = document.plan_timeline().unwrap();

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
