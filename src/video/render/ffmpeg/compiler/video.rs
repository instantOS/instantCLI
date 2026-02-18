use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use super::FfmpegCompiler;
use super::inputs::get_ffmpeg_input_index;
use super::util::format_time;
use crate::video::render::timeline::{Segment, SegmentData};

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
        filters: &mut Vec<String>,
        video_segments: &[&Segment],
        source_map: &HashMap<PathBuf, usize>,
    ) -> Result<bool> {
        if video_segments.is_empty() {
            return Ok(false);
        }

        let mut concat_inputs = String::new();
        let mut concat_count = 0usize;

        for segment in video_segments.iter().copied() {
            if segment.duration <= 0.0 {
                continue;
            }

            if self.push_single_video_subset_filters(
                filters,
                &mut concat_inputs,
                &mut concat_count,
                segment,
                source_map,
            )? {
                // segment added
            }
        }

        filters.push(format!(
            "{inputs}concat=n={count}:v=1:a=1[concat_v][concat_a]",
            inputs = concat_inputs,
            count = concat_count
        ));

        Ok(true)
    }

    fn push_single_video_subset_filters(
        &self,
        filters: &mut Vec<String>,
        concat_inputs: &mut String,
        concat_count: &mut usize,
        segment: &Segment,
        source_map: &HashMap<PathBuf, usize>,
    ) -> Result<bool> {
        let SegmentData::VideoSubset {
            start_time,
            source,
            mute_audio,
            ..
        } = &segment.data
        else {
            return Ok(false);
        };

        let idx = *concat_count;
        *concat_count += 1;

        let input_index = get_ffmpeg_input_index(
            source_map,
            &source.video,
            "No ffmpeg input available for source video",
        )?;

        let audio_input_index = get_ffmpeg_input_index(
            source_map,
            &source.audio,
            "No ffmpeg input available for audio source",
        )?;

        let video_label = format!("v{idx}");
        let audio_label = format!("a{idx}");
        let end_time = start_time + segment.duration;

        let trimmed_label =
            self.push_trimmed_video_filters(filters, idx, input_index, *start_time, end_time);
        self.push_normalized_video_filters(filters, &trimmed_label, &video_label);
        self.push_audio_filters(
            filters,
            *mute_audio,
            audio_input_index,
            *start_time,
            end_time,
            segment.duration,
            &audio_label,
        );

        concat_inputs.push_str(&format!(
            "[{video}][{audio}]",
            video = video_label,
            audio = audio_label
        ));
        Ok(true)
    }

    fn push_trimmed_video_filters(
        &self,
        filters: &mut Vec<String>,
        idx: usize,
        input_index: usize,
        start_time: f64,
        end_time: f64,
    ) -> String {
        let trimmed_label = format!("v{idx}_raw");
        filters.push(format!(
            "[{input}:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[{trimmed}]",
            input = input_index,
            start = format_time(start_time),
            end = format_time(end_time),
            trimmed = trimmed_label,
        ));
        trimmed_label
    }

    fn push_normalized_video_filters(
        &self,
        filters: &mut Vec<String>,
        trimmed_label: &str,
        video_label: &str,
    ) {
        if let Some(padding_filter) = self.build_padding_filter(trimmed_label, video_label) {
            filters.push(padding_filter);
        } else {
            filters.push(format!(
                "[{trimmed}]setsar=1[{video}]",
                trimmed = trimmed_label,
                video = video_label
            ));
        }
    }

    fn push_audio_filters(
        &self,
        filters: &mut Vec<String>,
        mute_audio: bool,
        audio_input_index: usize,
        start_time: f64,
        end_time: f64,
        segment_duration: f64,
        audio_label: &str,
    ) {
        if mute_audio {
            filters.push(format!(
                "anullsrc=r=48000:cl=stereo,atrim=duration={dur}[{audio}]",
                dur = format_time(segment_duration),
                audio = audio_label,
            ));
        } else {
            filters.push(format!(
                "[{input}:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[{audio}]",
                input = audio_input_index,
                start = format_time(start_time),
                end = format_time(end_time),
                audio = audio_label,
            ));
        }
    }
}
