use anyhow::{Result, bail};

use super::FfmpegCompiler;
use super::FilterGraphBuilder;
use super::graph::{AudioInput, AudioPad, FilterGraph};
use super::inputs::SourceMap;
use super::util::format_time;
use crate::video::render::timeline::MusicClip;

impl FfmpegCompiler {
    pub(super) fn build_audio_mix_filters(
        &self,
        ctx: &mut FilterGraphBuilder,
        music_segments: &[MusicClip],
        source_map: &SourceMap,
        base_audio: Option<AudioPad>,
        total_duration: f64,
    ) -> Result<()> {
        let graph = ctx.graph();
        let has_base_track = base_audio.is_some();
        let mut audio = base_audio.map(|base| {
            // Voice is always a centered mono stem, regardless of how the
            // recording device labelled or duplicated its input channels.
            graph.audio_filter(
                base,
                "aformat=sample_rates=48000:channel_layouts=mono",
                "a_base",
            )
        });

        if !music_segments.is_empty() {
            let music = self.build_music_filters(graph, music_segments, source_map)?;
            audio = Some(match audio {
                Some(base) => {
                    let stereo = graph.audio_filter(
                        base,
                        "aformat=sample_rates=48000:channel_layouts=stereo",
                        "a_voice_stereo",
                    );
                    let (voice_mix, voice_sidechain) =
                        graph.split_audio(stereo, "a_voice_mix", "a_voice_sidechain");
                    // Duck the music bed against the (already enhanced) voice:
                    // the music dips only while someone speaks, then returns
                    // untouched. Deliberate, musical ducking - not a compressor
                    // that re-masters the track (music is already mastered).
                    let ducked = graph.audio_two_input_filter(
                        music,
                        voice_sidechain,
                        "sidechaincompress=threshold=0.05:ratio=8:attack=15:release=300",
                        "a_duck",
                    );
                    graph.mix_audio(
                        vec![voice_mix, ducked],
                        "duration=first:normalize=0:dropout_transition=0",
                        "a_mix",
                    )
                }
                None => music,
            });
        }

        let final_audio = if let Some(audio) = audio {
            audio
        } else {
            let duration = format_time(total_duration);
            graph.audio_source(
                format!("anullsrc=r=48000:cl=stereo,atrim=duration={duration}"),
                "a_silence",
            )
        };

        let final_audio = if has_base_track && !music_segments.is_empty() {
            // Limit only the voice+music sum. The enhanced voice stem is
            // already normalized to -1 dBTP, so processing it again adds no
            // protection and needlessly changes timing/state.
            graph.audio_filter(
                final_audio,
                "alimiter=limit=0.8913:level=false:latency=true,asetpts=PTS-STARTPTS",
                "a_final",
            )
        } else if has_base_track {
            graph.audio_filter(final_audio, "asetpts=PTS-STARTPTS", "a_final")
        } else {
            graph.audio_filter(
                final_audio,
                format!(
                    "atrim=duration={},asetpts=PTS-STARTPTS",
                    format_time(total_duration)
                ),
                "a_final",
            )
        };
        graph.output_audio(final_audio);
        Ok(())
    }

    fn build_music_filters(
        &self,
        graph: &mut FilterGraph,
        music_segments: &[MusicClip],
        source_map: &SourceMap,
    ) -> Result<AudioPad> {
        let music_volume = f64::from(self.config.music_volume());
        let labels = collect_music_segment_labels(graph, music_segments, source_map, music_volume)?;
        mix_music_labels(graph, labels)
    }
}

fn collect_music_segment_labels(
    graph: &mut FilterGraph,
    music_segments: &[MusicClip],
    source_map: &SourceMap,
    music_volume: f64,
) -> Result<Vec<AudioPad>> {
    let mut labels = Vec::new();

    for (idx, segment) in music_segments.iter().enumerate() {
        if segment.duration.seconds() <= 0.0 {
            continue;
        }
        let input_index = source_map.index(&segment.audio_source)?;

        let label = format!("music_{idx}");
        labels.push(build_single_music_filter(
            graph,
            segment,
            input_index,
            music_volume,
            &label,
        ));
    }

    Ok(labels)
}

fn build_single_music_filter(
    graph: &mut FilterGraph,
    segment: &MusicClip,
    input_index: usize,
    music_volume: f64,
    label: &str,
) -> AudioPad {
    let duration_str = format_time(segment.duration.seconds());
    let delay_ms = ((segment.timeline_start.seconds() * 1000.0).round()).max(0.0) as u64;

    graph.audio_from_input(
        AudioInput(input_index),
        format!(
        "atrim=start={source_start}:end={source_end},asetpts=PTS-STARTPTS,apad=pad_dur={duration},atrim=duration={duration},aresample=async=1:first_pts=0,aformat=sample_rates=48000:channel_layouts=stereo,adelay={delay}|{delay},volume={volume:.6}",
        source_start = format_time(segment.source_start.seconds()),
        source_end = format_time(segment.source_start.seconds() + segment.duration.seconds()),
        duration = duration_str,
        delay = delay_ms,
        volume = music_volume,
        ),
        label,
    )
}

fn mix_music_labels(graph: &mut FilterGraph, mut labels: Vec<AudioPad>) -> Result<AudioPad> {
    match labels.len() {
        0 => bail!("No music segments available to build audio filters"),
        1 => Ok(labels.remove(0)),
        _ => Ok(graph.mix_audio(labels, "normalize=0:dropout_transition=0", "music_mix")),
    }
}
