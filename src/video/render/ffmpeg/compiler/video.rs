use anyhow::{Result, bail};

use super::FfmpegCompiler;
use super::FilterChain;
use super::inputs::SourceMap;
use super::util::format_time;
use crate::video::render::timeline::{AvSourceRef, Segment, SegmentData};

/// Fade each side of a discontinuous edit to zero. Keeping the fades inside
/// the segment means audio and video retain exactly the same nominal duration.
const AUDIO_CUT_FADE_SECONDS: f64 = 0.005;

/// Source timestamps within half a 48 kHz sample are effectively contiguous.
const AUDIO_CONTIGUITY_TOLERANCE_SECONDS: f64 = 0.5 / 48_000.0;

struct PairedSegmentFilters {
    filters: Vec<String>,
    video_label: String,
    audio_label: String,
}

/// A validated base-track unit. Video and dialogue audio cannot be represented
/// independently once they enter the compiler.
struct PairedAvSegment<'a> {
    output_start: f64,
    duration: f64,
    source_start: f64,
    source: &'a AvSourceRef,
    mute_audio: bool,
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
        let mut concat_count = 0usize;

        let playable_segments = video_segments
            .iter()
            .copied()
            .filter(|segment| segment.duration > 0.0)
            .map(PairedAvSegment::try_from)
            .collect::<Result<Vec<_>>>()?;
        if playable_segments.is_empty() {
            return Ok(false);
        }
        validate_base_track(&playable_segments)?;

        for (idx, segment) in playable_segments.iter().enumerate() {
            let discontinuity_before =
                idx > 0 && !audio_is_contiguous(&playable_segments[idx - 1], segment);
            let discontinuity_after = idx + 1 < playable_segments.len()
                && !audio_is_contiguous(segment, &playable_segments[idx + 1]);

            let output = self.build_paired_segment_filters(
                segment,
                source_map,
                concat_count,
                discontinuity_before,
                discontinuity_after,
            )?;
            filters.extend(output.filters);
            concat_inputs.push_str(&format!(
                "[{video}][{audio}]",
                video = output.video_label,
                audio = output.audio_label,
            ));
            concat_count += 1;
        }

        filters.push(format!(
            "{inputs}concat=n={count}:v=1:a=1[concat_v][concat_a]",
            inputs = concat_inputs,
            count = concat_count
        ));

        Ok(true)
    }

    fn build_paired_segment_filters(
        &self,
        segment: &PairedAvSegment<'_>,
        source_map: &SourceMap,
        idx: usize,
        extend_before: bool,
        extend_after: bool,
    ) -> Result<PairedSegmentFilters> {
        let input_index = source_map.index(&segment.source.video)?;
        let audio_input_index = source_map.index(&segment.source.audio)?;

        // Subtract per-input -ss offset so trim times are relative to the
        // seeked input position. For render (no input seeking), offset is 0.
        let video_offset = source_map.offset(input_index);
        let audio_offset = source_map.offset(audio_input_index);

        let video_label = format!("v{idx}");
        let audio_label = format!("a{idx}");
        let adj_start = segment.source_start - video_offset;
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

        let audio_adj_start = segment.source_start - audio_offset;
        let audio_adj_end = audio_adj_start + segment.duration;
        filters.push(self.build_audio_filter(
            segment.mute_audio,
            audio_input_index,
            audio_adj_start,
            audio_adj_end,
            segment.duration,
            &audio_label,
            extend_before,
            extend_after,
        ));

        Ok(PairedSegmentFilters {
            filters,
            video_label,
            audio_label,
        })
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
            let mut fade_filters = String::new();
            let fade_duration = AUDIO_CUT_FADE_SECONDS.min(segment_duration / 2.0);
            if extend_before && fade_duration > 0.0 {
                fade_filters.push_str(&format!(
                    ",afade=t=in:st=0:d={}",
                    format_time(fade_duration)
                ));
            }
            if extend_after && fade_duration > 0.0 {
                fade_filters.push_str(&format!(
                    ",afade=t=out:st={}:d={}",
                    format_time(segment_duration - fade_duration),
                    format_time(fade_duration)
                ));
            }
            format!(
                "[{input}:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS,aformat=sample_rates=48000:channel_layouts=mono,apad,atrim=duration={duration}{fades}[{audio}]",
                input = audio_input_index,
                start = format_time(start_time),
                end = format_time(end_time),
                duration = format_time(segment_duration),
                fades = fade_filters,
                audio = audio_label,
            )
        }
    }
}

impl<'a> TryFrom<&'a Segment> for PairedAvSegment<'a> {
    type Error = anyhow::Error;

    fn try_from(segment: &'a Segment) -> Result<Self> {
        let SegmentData::VideoSubset {
            start_time,
            source,
            mute_audio,
            ..
        } = &segment.data
        else {
            bail!("Only paired video/audio segments may enter the base A/V track");
        };
        Ok(Self {
            output_start: segment.start_time,
            duration: segment.duration,
            source_start: *start_time,
            source,
            mute_audio: *mute_audio,
        })
    }
}

fn validate_base_track(segments: &[PairedAvSegment<'_>]) -> Result<()> {
    let mut expected_start = 0.0;
    for segment in segments {
        if (segment.output_start - expected_start).abs() > AUDIO_CONTIGUITY_TOLERANCE_SECONDS {
            bail!(
                "Base A/V timeline is not contiguous: expected a segment at {:.6}s, found one at {:.6}s",
                expected_start,
                segment.output_start,
            );
        }
        expected_start += segment.duration;
    }
    Ok(())
}

fn audio_is_contiguous(left: &PairedAvSegment<'_>, right: &PairedAvSegment<'_>) -> bool {
    if left.mute_audio && right.mute_audio {
        return true;
    }
    if left.mute_audio || right.mute_audio || left.source.audio != right.source.audio {
        return false;
    }

    (left.source_start + left.duration - right.source_start).abs()
        <= AUDIO_CONTIGUITY_TOLERANCE_SECONDS
}
