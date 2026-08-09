use anyhow::{Result, bail};

use super::FfmpegCompiler;
use super::FilterChain;
use super::inputs::SourceMap;
use super::util::format_time;
use crate::video::render::timeline::{Segment, SegmentData};

impl FfmpegCompiler {
    pub(super) fn build_audio_mix_filters(
        &self,
        filters: &mut FilterChain,
        music_segments: &[&Segment],
        source_map: &SourceMap,
        has_base_track: bool,
        total_duration: f64,
    ) -> Result<()> {
        let mut audio_label: Option<String> = None;

        if has_base_track {
            // Voice is always a centered mono stem, regardless of how the
            // recording device labelled or duplicated its input channels.
            filters.push(
                "[concat_a]aformat=sample_rates=48000:channel_layouts=mono[a_base]".to_string(),
            );
            audio_label = Some("a_base".to_string());
        }

        if !music_segments.is_empty() {
            let music_label = self.build_music_filters(filters, music_segments, source_map)?;
            audio_label = Some(match audio_label {
                Some(base) => {
                    let stereo_base = "a_base_stereo".to_string();
                    filters.push(format!(
                        "[{base}]aformat=sample_rates=48000:channel_layouts=stereo[{stereo_base}]"
                    ));
                    // Duck the music bed against the (already enhanced) voice:
                    // the music dips only while someone speaks, then returns
                    // untouched. Deliberate, musical ducking - not a compressor
                    // that re-masters the track (music is already mastered).
                    let ducked = "a_duck".to_string();
                    filters.push(format!(
                        "[{music}][{base}]sidechaincompress=threshold=0.05:ratio=8:attack=15:release=300[{ducked}]",
                        music = music_label,
                        base = stereo_base,
                        ducked = ducked,
                    ));
                    let mixed = "a_mix".to_string();
                    filters.push(format!(
                        "[{base}][{ducked}]amix=inputs=2:normalize=0:dropout_transition=0[{mixed}]",
                        base = stereo_base,
                        ducked = ducked,
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

        if has_base_track && !music_segments.is_empty() {
            // Limit only the voice+music sum. The enhanced voice stem is
            // already normalized to -1 dBTP, so processing it again adds no
            // protection and needlessly changes timing/state.
            filters.push(format!(
                "[{label}]alimiter=limit=0.8913:level=false:latency=true[outa]",
                label = final_audio
            ));
        } else {
            filters.push(format!("[{final_audio}]anull[outa]"));
        }
        Ok(())
    }

    fn build_music_filters(
        &self,
        filters: &mut FilterChain,
        music_segments: &[&Segment],
        source_map: &SourceMap,
    ) -> Result<String> {
        let music_volume = f64::from(self.config.music_volume());
        let labels =
            collect_music_segment_labels(filters, music_segments, source_map, music_volume)?;
        mix_music_labels(filters, labels)
    }
}

fn collect_music_segment_labels(
    filters: &mut FilterChain,
    music_segments: &[&Segment],
    source_map: &SourceMap,
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

        let input_index = source_map.index(audio_source)?;

        let label = format!("music_{idx}");
        filters.push(build_single_music_filter(
            segment,
            input_index,
            music_volume,
            &label,
        ));
        labels.push(label);
    }

    Ok(labels)
}

fn build_single_music_filter(
    segment: &Segment,
    input_index: usize,
    music_volume: f64,
    label: &str,
) -> String {
    let duration_str = format_time(segment.duration);
    let delay_ms = ((segment.start_time * 1000.0).round()).max(0.0) as u64;

    format!(
        "[{input}:a]atrim=start=0:end={duration},asetpts=PTS-STARTPTS,apad=pad_dur={duration},atrim=duration={duration},aresample=async=1:first_pts=0,aformat=sample_rates=48000:channel_layouts=stereo,adelay={delay}|{delay},volume={volume:.6}[{label}]",
        input = input_index,
        duration = duration_str,
        delay = delay_ms,
        volume = music_volume,
        label = label,
    )
}

fn mix_music_labels(filters: &mut FilterChain, labels: Vec<String>) -> Result<String> {
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
