use anyhow::Result;

use super::FfmpegCompiler;
use super::FilterChain;
use super::inputs::SourceMap;
use super::util::format_time;
use crate::video::render::timeline::{Segment, SegmentData};

/// Crossfade discontinuous edits using real audio immediately outside the cut.
/// Half comes from each side, so the joined audio keeps its original duration.
const AUDIO_CUT_CROSSFADE_SECONDS: f64 = 0.010;

/// Source timestamps within half a 48 kHz sample are effectively contiguous.
const AUDIO_CONTIGUITY_TOLERANCE_SECONDS: f64 = 0.5 / 48_000.0;

struct VideoSubsetFilters {
    filters: Vec<String>,
    video_label: String,
    audio_label: String,
}

impl FfmpegCompiler {
    pub(super) fn build_padding_filter(
        &self,
        input_label: &str,
        output_label: &str,
    ) -> Option<String> {
        if !self.render_mode.requires_padding() {
            return None;
        }

        let offset_pct = self.render_mode.vertical_offset_pct();

        let filter = format!(
            "[{input}]scale={width}:-1:flags=lanczos,pad={width}:{height}:(ow-iw)/2:(oh-ih)*{offset}:0x1E1E2E,setsar=1[{output}]",
            input = input_label,
            width = self.target_width,
            height = self.target_height,
            offset = offset_pct,
            output = output_label
        );

        Some(filter)
    }

    pub(super) fn build_base_track_filters(
        &self,
        filters: &mut FilterChain,
        video_segments: &[&Segment],
        source_map: &SourceMap,
    ) -> Result<bool> {
        if video_segments.is_empty() {
            return Ok(false);
        }

        let mut concat_inputs = String::new();
        let mut audio_labels = Vec::new();
        let mut concat_count = 0usize;

        let playable_segments = video_segments
            .iter()
            .copied()
            .filter(|segment| segment.duration > 0.0)
            .collect::<Vec<_>>();
        if playable_segments.is_empty() {
            return Ok(false);
        }

        for (idx, segment) in playable_segments.iter().copied().enumerate() {
            let discontinuity_before =
                idx > 0 && !audio_is_contiguous(playable_segments[idx - 1], segment);
            let discontinuity_after = idx + 1 < playable_segments.len()
                && !audio_is_contiguous(segment, playable_segments[idx + 1]);

            if let Some(output) = self.build_video_subset_filters(
                segment,
                source_map,
                concat_count,
                discontinuity_before,
                discontinuity_after,
            )? {
                filters.extend(output.filters);
                concat_inputs.push_str(&format!("[{video}]", video = output.video_label));
                audio_labels.push(output.audio_label);
                concat_count += 1;
            }
        }

        filters.push(format!(
            "{inputs}concat=n={count}:v=1:a=0[concat_v]",
            inputs = concat_inputs,
            count = concat_count
        ));
        self.build_audio_joins(filters, &playable_segments, audio_labels);

        Ok(true)
    }

    fn build_video_subset_filters(
        &self,
        segment: &Segment,
        source_map: &SourceMap,
        idx: usize,
        extend_before: bool,
        extend_after: bool,
    ) -> Result<Option<VideoSubsetFilters>> {
        let SegmentData::VideoSubset {
            start_time,
            source,
            mute_audio,
            ..
        } = &segment.data
        else {
            return Ok(None);
        };

        let input_index = source_map.index(&source.video)?;
        let audio_input_index = source_map.index(&source.audio)?;

        // Subtract per-input -ss offset so trim times are relative to the
        // seeked input position. For render (no input seeking), offset is 0.
        let video_offset = source_map.offset(input_index);
        let audio_offset = source_map.offset(audio_input_index);

        let video_label = format!("v{idx}");
        let audio_label = format!("a{idx}");
        let adj_start = start_time - video_offset;
        let adj_end = adj_start + segment.duration;

        let trimmed_label = format!("v{idx}_raw");
        let mut filters = Vec::new();
        filters.push(self.build_trimmed_video_filter(
            &trimmed_label,
            input_index,
            adj_start,
            adj_end,
        ));
        filters.push(self.build_normalized_video_filter(&trimmed_label, &video_label));

        let audio_adj_start = start_time - audio_offset;
        let audio_adj_end = audio_adj_start + segment.duration;
        filters.push(self.build_audio_filter(
            *mute_audio,
            audio_input_index,
            audio_adj_start,
            audio_adj_end,
            segment.duration,
            &audio_label,
            extend_before,
            extend_after,
        ));

        Ok(Some(VideoSubsetFilters {
            filters,
            video_label,
            audio_label,
        }))
    }

    fn build_trimmed_video_filter(
        &self,
        trimmed_label: &str,
        input_index: usize,
        start_time: f64,
        end_time: f64,
    ) -> String {
        format!(
            "[{input}:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[{trimmed}]",
            input = input_index,
            start = format_time(start_time),
            end = format_time(end_time),
            trimmed = trimmed_label,
        )
    }

    fn build_normalized_video_filter(&self, trimmed_label: &str, video_label: &str) -> String {
        if let Some(padding_filter) = self.build_padding_filter(trimmed_label, video_label) {
            padding_filter
        } else {
            format!(
                "[{trimmed}]setsar=1[{video}]",
                trimmed = trimmed_label,
                video = video_label
            )
        }
    }

    fn build_audio_filter(
        &self,
        mute_audio: bool,
        audio_input_index: usize,
        start_time: f64,
        end_time: f64,
        segment_duration: f64,
        audio_label: &str,
        extend_before: bool,
        extend_after: bool,
    ) -> String {
        if mute_audio {
            format!(
                "anullsrc=r=48000:cl=mono,atrim=duration={dur}[{audio}]",
                dur = format_time(segment_duration),
                audio = audio_label,
            )
        } else {
            let half_crossfade = AUDIO_CUT_CROSSFADE_SECONDS / 2.0;
            let trim_start = if extend_before {
                (start_time - half_crossfade).max(0.0)
            } else {
                start_time
            };
            let trim_end = if extend_after {
                end_time + half_crossfade
            } else {
                end_time
            };
            format!(
                "[{input}:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS,aformat=sample_rates=48000:channel_layouts=mono[{audio}]",
                input = audio_input_index,
                start = format_time(trim_start),
                end = format_time(trim_end),
                audio = audio_label,
            )
        }
    }

    fn build_audio_joins(
        &self,
        filters: &mut FilterChain,
        segments: &[&Segment],
        audio_labels: Vec<String>,
    ) {
        if audio_labels.len() == 1 {
            filters.push(format!("[{}]anull[concat_a]", audio_labels[0]));
            return;
        }
        let mut current = audio_labels[0].clone();
        for idx in 1..audio_labels.len() {
            let joined = if idx + 1 == audio_labels.len() {
                "concat_a".to_string()
            } else {
                format!("a_join_{idx}")
            };
            if audio_is_contiguous(segments[idx - 1], segments[idx]) {
                filters.push(format!(
                    "[{current}][{next}]concat=n=2:v=0:a=1[{joined}]",
                    next = audio_labels[idx]
                ));
            } else {
                filters.push(format!(
                    "[{current}][{next}]acrossfade=d={duration}:c1=tri:c2=tri[{joined}]",
                    next = audio_labels[idx],
                    duration = format_time(AUDIO_CUT_CROSSFADE_SECONDS),
                ));
            }
            current = joined;
        }
    }
}

fn audio_is_contiguous(left: &Segment, right: &Segment) -> bool {
    let (
        SegmentData::VideoSubset {
            start_time: left_start,
            source: left_source,
            mute_audio: left_muted,
            ..
        },
        SegmentData::VideoSubset {
            start_time: right_start,
            source: right_source,
            mute_audio: right_muted,
            ..
        },
    ) = (&left.data, &right.data)
    else {
        return false;
    };

    if *left_muted && *right_muted {
        return true;
    }
    if *left_muted || *right_muted || left_source.audio != right_source.audio {
        return false;
    }

    (left_start + left.duration - right_start).abs() <= AUDIO_CONTIGUITY_TOLERANCE_SECONDS
}
