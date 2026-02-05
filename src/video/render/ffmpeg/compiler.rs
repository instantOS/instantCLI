use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};

use super::super::mode::RenderMode;
use crate::video::config::VideoConfig;
use crate::video::render::timeline::{Segment, SegmentData, Timeline};

#[derive(Debug, Clone)]
pub struct FfmpegCompileOutput {
    pub args: Vec<String>,
}

pub struct FfmpegCompiler {
    target_width: u32,
    target_height: u32,
    render_mode: RenderMode,
    config: VideoConfig,
    subtitle_path: Option<PathBuf>,
}

impl FfmpegCompiler {
    pub fn new(
        render_mode: RenderMode,
        source_width: u32,
        source_height: u32,
        config: VideoConfig,
        subtitle_path: Option<PathBuf>,
    ) -> Self {
        let (target_width, target_height) =
            render_mode.target_dimensions(source_width, source_height);
        Self {
            target_width,
            target_height,
            render_mode,
            config,
            subtitle_path,
        }
    }

    /// Build letterboxing/pillboxing filter chain when target != source aspect ratio
    fn build_padding_filter(&self, input_label: &str, output_label: &str) -> Option<String> {
        if !self.render_mode.requires_padding() {
            return None;
        }

        let offset_pct = self.render_mode.vertical_offset_pct();

        // Build the base scale+pad filter
        // Note: input_label is a filter label (e.g., "v0_raw"), not an input stream index
        // setsar=1 normalizes the sample aspect ratio for consistent concat
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

    pub fn compile(
        &self,
        output: PathBuf,
        timeline: &Timeline,
        audio_source: PathBuf,
        audio_map: &std::collections::HashMap<String, PathBuf>,
    ) -> Result<FfmpegCompileOutput> {
        let mut args = Vec::new();

        let (source_map, source_order) =
            self.build_input_source_map(timeline, &audio_source, audio_map);

        // Add all input files in the order they were discovered
        for source in &source_order {
            args.push("-i".to_string());
            args.push(source.to_string_lossy().into_owned());
        }

        let total_duration = timeline.total_duration();

        let filter_complex = self.build_filter_complex(timeline, &source_map, total_duration)?;
        args.push("-filter_complex".to_string());
        args.push(filter_complex);

        args.push("-map".to_string());
        args.push("[outv]".to_string());
        args.push("-map".to_string());
        args.push("[outa]".to_string());

        // Encoding settings
        args.push("-c:v".to_string());
        args.push("libx264".to_string());
        args.push("-preset".to_string());
        args.push("medium".to_string());
        args.push("-crf".to_string());
        args.push("18".to_string());
        args.push("-c:a".to_string());
        args.push("aac".to_string());
        args.push("-b:a".to_string());
        args.push("192k".to_string());
        args.push("-movflags".to_string());
        args.push("+faststart".to_string());
        args.push(output.to_string_lossy().into_owned());

        Ok(FfmpegCompileOutput { args })
    }

    fn build_input_source_map(
        &self,
        timeline: &Timeline,
        audio_source: &Path,
        audio_map: &std::collections::HashMap<String, PathBuf>,
    ) -> (HashMap<PathBuf, usize>, Vec<PathBuf>) {
        let mut source_map: HashMap<PathBuf, usize> = HashMap::new();
        let mut source_order: Vec<PathBuf> = Vec::new();
        let mut next_index = 0;

        for segment in &timeline.segments {
            if let Some(source) = segment.data.source_path()
                && !source_map.contains_key(source)
            {
                source_map.insert(source.clone(), next_index);
                source_order.push(source.clone());
                next_index += 1;
            }
            if let Some(audio) = segment.data.audio_source()
                && !source_map.contains_key(audio)
            {
                source_map.insert(audio.clone(), next_index);
                source_order.push(audio.clone());
                next_index += 1;
            }
        }

        for audio in audio_map.values() {
            if !source_map.contains_key(audio) {
                source_map.insert(audio.clone(), next_index);
                source_order.push(audio.clone());
                next_index += 1;
            }
        }

        if !source_map.contains_key(audio_source) {
            source_map.insert(audio_source.to_path_buf(), next_index);
            source_order.push(audio_source.to_path_buf());
        }

        (source_map, source_order)
    }

    fn build_filter_complex(
        &self,
        timeline: &Timeline,
        source_map: &HashMap<PathBuf, usize>,
        total_duration: f64,
    ) -> Result<String> {
        let mut filters: Vec<String> = Vec::new();

        let (video_segments, overlay_segments, music_segments, broll_segments) =
            categorize_segments(timeline);

        let has_base_track =
            self.build_base_track_filters(&mut filters, &video_segments, source_map)?;

        let mut current_video_label = "concat_v".to_string();

        if !broll_segments.is_empty() {
            current_video_label = self.apply_broll_overlays(
                &mut filters,
                &broll_segments,
                source_map,
                &current_video_label,
            )?;
        }

        if !overlay_segments.is_empty() {
            current_video_label = self.apply_overlays(
                &mut filters,
                &overlay_segments,
                source_map,
                &current_video_label,
            )?;
        }

        if let Some(ass_path) = &self.subtitle_path {
            let escaped_path = escape_ffmpeg_path(ass_path);
            let next_label = "subtitled_v";
            filters.push(format!(
                "[{input}]ass='{path}'[{output}]",
                input = current_video_label,
                path = escaped_path,
                output = next_label
            ));
            current_video_label = next_label.to_string();
        }

        filters.push(format!("[{}]copy[outv]", current_video_label));

        self.build_audio_mix_filters(
            &mut filters,
            &music_segments,
            source_map,
            has_base_track,
            total_duration,
        )?;

        Ok(filters.join("; "))
    }

    fn build_base_track_filters(
        &self,
        filters: &mut Vec<String>,
        video_segments: &[&Segment],
        source_map: &HashMap<PathBuf, usize>,
    ) -> Result<bool> {
        if video_segments.is_empty() {
            return Ok(false);
        }

        // Preserve the timeline-provided order; that is the user-authored edit order.
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
            source_video,
            audio_source,
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
            source_video,
            "No ffmpeg input available for source video",
        )?;

        let audio_input_index = get_ffmpeg_input_index(
            source_map,
            audio_source,
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

    fn apply_broll_overlays(
        &self,
        filters: &mut Vec<String>,
        broll_segments: &[&Segment],
        source_map: &HashMap<PathBuf, usize>,
        input_label: &str,
    ) -> Result<String> {
        let mut current_video_label = input_label.to_string();

        for (idx, segment) in broll_segments.iter().enumerate() {
            let SegmentData::Broll {
                start_time: source_start,
                source_video,
                ..
            } = &segment.data
            else {
                continue;
            };

            let input_index = source_map.get(source_video).ok_or_else(|| {
                anyhow!(
                    "No ffmpeg input available for B-roll video {}",
                    source_video.display()
                )
            })?;

            let trimmed_label = format!("broll_trim_{idx}");
            let scaled_label = format!("broll_scaled_{idx}");
            let output_label = format!("broll_out_{idx}");

            let trim_end = source_start + segment.duration;
            // Offset b-roll timestamps to match the main video timeline
            // This ensures the overlay filter syncs frames correctly when using enable='between(t,...)'
            filters.push(format!(
                "[{input}:v]trim=start={start}:end={end},setpts=PTS-STARTPTS+{offset}/TB[{out}]",
                input = input_index,
                start = source_start,
                end = trim_end,
                offset = segment.start_time,
                out = trimmed_label,
            ));

            // Scale b-roll to 90% of target size, add catppuccin blue border, then pad to full size
            let border_width = 4;
            let inner_width = (self.target_width as f64 * 0.9) as u32 - (border_width * 2);
            let inner_height = (self.target_height as f64 * 0.9) as u32 - (border_width * 2);
            let outer_width = (self.target_width as f64 * 0.9) as u32;
            let outer_height = (self.target_height as f64 * 0.9) as u32;
            let catppuccin_blue = "0x89B4FA"; // Catppuccin Mocha Blue

            filters.push(format!(
                "[{input}]scale={iw}:{ih}:force_original_aspect_ratio=decrease,pad={iw}:{ih}:(ow-iw)/2:(oh-ih)/2:0x1E1E2E,setsar=1,pad={ow}:{oh}:(ow-iw)/2:(oh-ih)/2:{color}[{out}]",
                input = trimmed_label,
                iw = inner_width,
                ih = inner_height,
                ow = outer_width,
                oh = outer_height,
                color = catppuccin_blue,
                out = scaled_label,
            ));

            let enable_condition =
                format!("between(t,{},{})", segment.start_time, segment.end_time());

            // Center the scaled b-roll on the main video
            let x_offset = (self.target_width - outer_width) / 2;
            let y_offset = (self.target_height - outer_height) / 2;

            filters.push(format!(
                "[{video}][{broll}]overlay=x={x}:y={y}:enable='{condition}'[{output}]",
                video = current_video_label,
                broll = scaled_label,
                x = x_offset,
                y = y_offset,
                condition = enable_condition,
                output = output_label,
            ));

            current_video_label = output_label;
        }

        Ok(current_video_label)
    }

    fn apply_overlays(
        &self,
        filters: &mut Vec<String>,
        overlay_segments: &[&Segment],
        source_map: &HashMap<PathBuf, usize>,
        input_label: &str,
    ) -> Result<String> {
        let mut current_video_label = input_label.to_string();

        for (idx, segment) in overlay_segments.iter().enumerate() {
            let SegmentData::Image {
                source_image,
                transform,
            } = &segment.data
            else {
                continue;
            };

            let input_index = source_map.get(source_image).ok_or_else(|| {
                anyhow!(
                    "No ffmpeg input available for overlay image {}",
                    source_image.display()
                )
            })?;

            let overlay_label = format!("overlay_{idx}");
            let output_label = format!("overlaid_{idx}");

            let scale_factor = transform.as_ref().and_then(|t| t.scale).unwrap_or(0.8);

            filters.push(format!(
                "[{input}:v]scale=w=ceil({width}*{scale}/2)*2:h=-1:flags=lanczos,setsar=1,format=rgba[{overlay}]",
                input = input_index,
                width = self.target_width,
                scale = scale_factor,
                overlay = overlay_label,
            ));

            let enable_condition =
                format!("between(t,{},{})", segment.start_time, segment.end_time());

            // Calculate overlay position based on render mode
            // In reels mode: center overlay on full frame at 30% from top
            // In standard mode: center in the frame
            let y_position = if self.render_mode.requires_padding() {
                // For reels: center overlay on full frame at 30% from top
                // This leaves ~70% of frame below for subtitles
                // Formula: (H - h) * 0.3
                "(H-h)*0.3".to_string()
            } else {
                "(H-h)/2".to_string()
            };

            filters.push(format!(
                "[{video}][{overlay}]overlay=x=(W-w)/2:y={y_pos}:enable='{condition}'[{output}]",
                video = current_video_label,
                overlay = overlay_label,
                y_pos = y_position,
                condition = enable_condition,
                output = output_label,
            ));

            current_video_label = output_label;
        }

        Ok(current_video_label)
    }

    fn build_audio_mix_filters(
        &self,
        filters: &mut Vec<String>,
        music_segments: &[&Segment],
        source_map: &HashMap<PathBuf, usize>,
        has_base_track: bool,
        total_duration: f64,
    ) -> Result<()> {
        let mut audio_label: Option<String> = None;

        if has_base_track {
            filters.push("[concat_a]anull[a_base]".to_string());
            audio_label = Some("a_base".to_string());
        }

        if !music_segments.is_empty() {
            let music_label = self.build_music_filters(filters, music_segments, source_map)?;
            audio_label = Some(match audio_label {
                Some(base) => {
                    let mixed = "a_mix".to_string();
                    filters.push(format!(
                        "[{base}][{music}]amix=inputs=2:normalize=0:dropout_transition=0[{mixed}]",
                        base = base,
                        music = music_label,
                        mixed = mixed,
                    ));
                    mixed
                }
                None => music_label,
            });
        }

        let final_audio = if let Some(label) = audio_label {
            label
        } else {
            let duration = format_time(total_duration);
            filters.push(format!(
                "anullsrc=r=48000:cl=stereo,atrim=duration={duration}[a_silence]",
            ));
            "a_silence".to_string()
        };

        filters.push(format!("[{label}]anull[outa]", label = final_audio));
        Ok(())
    }

    fn build_music_filters(
        &self,
        filters: &mut Vec<String>,
        music_segments: &[&Segment],
        source_map: &HashMap<PathBuf, usize>,
    ) -> Result<String> {
        let music_volume = f64::from(self.config.music_volume());
        let labels =
            collect_music_segment_labels(filters, music_segments, source_map, music_volume)?;
        mix_music_labels(filters, labels)
    }
}

fn categorize_segments(
    timeline: &Timeline,
) -> (Vec<&Segment>, Vec<&Segment>, Vec<&Segment>, Vec<&Segment>) {
    let mut video = Vec::new();
    let mut overlay = Vec::new();
    let mut music = Vec::new();
    let mut broll = Vec::new();

    for segment in &timeline.segments {
        match &segment.data {
            SegmentData::VideoSubset { .. } => video.push(segment),
            SegmentData::Image { .. } => overlay.push(segment),
            SegmentData::Music { .. } => music.push(segment),
            SegmentData::Broll { .. } => broll.push(segment),
        }
    }
    (video, overlay, music, broll)
}

fn get_ffmpeg_input_index(
    source_map: &HashMap<PathBuf, usize>,
    source: &Path,
    error_prefix: &str,
) -> Result<usize> {
    source_map
        .get(source)
        .copied()
        .ok_or_else(|| anyhow!("{error_prefix} {}", source.display()))
}

fn collect_music_segment_labels(
    filters: &mut Vec<String>,
    music_segments: &[&Segment],
    source_map: &HashMap<PathBuf, usize>,
    music_volume: f64,
) -> Result<Vec<String>> {
    let mut labels = Vec::new();

    for (idx, segment) in music_segments.iter().enumerate() {
        if segment.duration <= 0.0 {
            continue;
        }

        let SegmentData::Music { audio_source } = &segment.data else {
            continue;
        };

        let input_index = source_map.get(audio_source).ok_or_else(|| {
            anyhow!(
                "No ffmpeg input available for background music {}",
                audio_source.display()
            )
        })?;

        let label = format!("music_{idx}");
        push_single_music_filter(filters, segment, *input_index, music_volume, &label);
        labels.push(label);
    }

    Ok(labels)
}

fn push_single_music_filter(
    filters: &mut Vec<String>,
    segment: &Segment,
    input_index: usize,
    music_volume: f64,
    label: &str,
) {
    let duration_str = format_time(segment.duration);
    let delay_ms = ((segment.start_time * 1000.0).round()).max(0.0) as u64;

    filters.push(format!(
        "[{input}:a]atrim=start=0:end={duration},asetpts=PTS-STARTPTS,apad=pad_dur={duration},atrim=duration={duration},aresample=async=1:first_pts=0,adelay={delay}|{delay},volume={volume}[{label}]",
        input = input_index,
        duration = duration_str,
        delay = delay_ms,
        volume = format!("{:.6}", music_volume),
        label = label,
    ));
}

fn mix_music_labels(filters: &mut Vec<String>, labels: Vec<String>) -> Result<String> {
    match labels.as_slice() {
        [] => bail!("No music segments available to build audio filters"),
        [label] => Ok(label.to_string()),
        _ => {
            let inputs = labels
                .iter()
                .map(|label| format!("[{label}]"))
                .collect::<String>();
            let output_label = "music_mix".to_string();
            filters.push(format!(
                "{inputs}amix=inputs={count}:normalize=0:dropout_transition=0[{output}]",
                inputs = inputs,
                count = labels.len(),
                output = output_label,
            ));
            Ok(output_label)
        }
    }
}

fn format_time(value: f64) -> String {
    format!("{value:.6}")
}

/// Escape special characters in a path for use in FFmpeg filter expressions.
/// FFmpeg filter syntax requires escaping of ', \, and : characters.
fn escape_ffmpeg_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('\'', "'\\''")
        .replace(':', "\\:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_includes_output_path_in_args() {
        let compiler = FfmpegCompiler::new(
            RenderMode::Standard,
            1920,
            1080,
            VideoConfig::default(),
            None,
        );
        let timeline = Timeline::new();
        let audio_map = HashMap::new();
        let output = compiler
            .compile(
                PathBuf::from("out.mp4"),
                &timeline,
                PathBuf::from("audio.mp4"),
                &audio_map,
            )
            .unwrap();
        assert_eq!(output.args.last().unwrap(), "out.mp4");
    }

    #[test]
    fn concat_order_respects_timeline_order() {
        let compiler = FfmpegCompiler::new(
            RenderMode::Standard,
            1920,
            1080,
            VideoConfig::default(),
            None,
        );

        // Deliberately add segments whose *source* start times are out-of-order.
        // The user expects their authored order to be preserved.
        let mut timeline = Timeline::new();
        timeline.add_segment(Segment::new_video_subset(
            0.0,
            1.0,
            5.0,
            PathBuf::from("video.mp4"),
            PathBuf::from("audio.mp4"),
            "a".to_string(),
            None,
            false,
        ));
        timeline.add_segment(Segment::new_video_subset(
            1.0,
            1.0,
            1.0,
            PathBuf::from("video.mp4"),
            PathBuf::from("audio.mp4"),
            "a".to_string(),
            None,
            false,
        ));
        timeline.add_segment(Segment::new_video_subset(
            2.0,
            1.0,
            3.0,
            PathBuf::from("video.mp4"),
            PathBuf::from("audio.mp4"),
            "a".to_string(),
            None,
            false,
        ));

        let audio_map = HashMap::new();
        let output = compiler
            .compile(
                PathBuf::from("out.mp4"),
                &timeline,
                PathBuf::from("audio.mp4"),
                &audio_map,
            )
            .unwrap();

        let filter_complex_idx = output
            .args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let filter_complex = &output.args[filter_complex_idx + 1];

        let concat_pos = filter_complex
            .find("concat=n=3:v=1:a=1[concat_v][concat_a]")
            .unwrap();
        let before_concat = &filter_complex[..concat_pos];

        let pos_v0 = before_concat.find("[v0]").unwrap();
        let pos_v1 = before_concat.find("[v1]").unwrap();
        let pos_v2 = before_concat.find("[v2]").unwrap();
        assert!(pos_v0 < pos_v1);
        assert!(pos_v1 < pos_v2);

        let pos_start_5 = before_concat.find("trim=start=5.000000").unwrap();
        let pos_start_1 = before_concat.find("trim=start=1.000000").unwrap();
        let pos_start_3 = before_concat.find("trim=start=3.000000").unwrap();
        assert!(pos_start_5 < pos_start_1);
        assert!(pos_start_1 < pos_start_3);
    }

    #[test]
    fn test_reels_mode_generates_padding_filter() {
        let compiler =
            FfmpegCompiler::new(RenderMode::Reels, 1920, 1080, VideoConfig::default(), None);
        let padding = compiler.build_padding_filter("v0_raw", "v0");
        assert!(padding.is_some());

        let filter = padding.unwrap();
        assert!(filter.contains("scale=1080:-1"));
        assert!(filter.contains("pad=1080:1920"));
        assert!(filter.contains("(oh-ih)*0.1")); // 10% offset
        assert!(filter.contains(":0x1E1E2E")); // Catppuccin Base background
        assert!(!filter.contains("ass=")); // No subtitles without path
    }

    #[test]
    fn test_reels_mode_padding_excludes_subtitles() {
        let compiler = FfmpegCompiler::new(
            RenderMode::Reels,
            1920,
            1080,
            VideoConfig::default(),
            Some(PathBuf::from("/tmp/subs.ass")),
        );
        let padding = compiler.build_padding_filter("v0_raw", "v0");
        assert!(padding.is_some());

        let filter = padding.unwrap();
        assert!(filter.contains("scale=1080:-1"));
        assert!(filter.contains("pad=1080:1920"));
        assert!(!filter.contains("ass=")); // Subtitles moved to global filter_complex
    }

    #[test]
    fn test_filter_complex_includes_subtitles() {
        let compiler = FfmpegCompiler::new(
            RenderMode::Reels,
            1920,
            1080,
            VideoConfig::default(),
            Some(PathBuf::from("/tmp/subs.ass")),
        );

        let mut timeline = Timeline::new();
        // Add a dummy segment so we have video content
        timeline.add_segment(Segment::new_video_subset(
            0.0,
            0.0,
            5.0,
            PathBuf::from("video.mp4"),
            PathBuf::from("audio.mp4"),
            "a".to_string(),
            None,
            false,
        ));

        // Create a dummy source map
        let mut source_map = HashMap::new();
        source_map.insert(PathBuf::from("video.mp4"), 0);
        let filter_complex = compiler
            .build_filter_complex(&timeline, &source_map, 5.0)
            .unwrap();

        assert!(filter_complex.contains("ass='/tmp/subs.ass'"));
        // Ensure subtitles are applied after concat/overlays
        // The structure should involve [concat_v]...[subtitled_v]...[outv]
        assert!(filter_complex.contains("[concat_v]ass='/tmp/subs.ass'[subtitled_v]"));
    }

    #[test]
    fn test_standard_mode_no_padding() {
        let compiler = FfmpegCompiler::new(
            RenderMode::Standard,
            1920,
            1080,
            VideoConfig::default(),
            None,
        );
        let padding = compiler.build_padding_filter("v0_raw", "v0");
        assert!(padding.is_none());
    }

    #[test]
    fn test_escape_ffmpeg_path() {
        assert_eq!(
            escape_ffmpeg_path(&PathBuf::from("/simple/path.ass")),
            "/simple/path.ass"
        );
        assert_eq!(
            escape_ffmpeg_path(&PathBuf::from("/path/with spaces/file.ass")),
            "/path/with spaces/file.ass"
        );
        assert_eq!(
            escape_ffmpeg_path(&PathBuf::from("/path/with'quote/file.ass")),
            "/path/with'\\''quote/file.ass"
        );
    }
}
