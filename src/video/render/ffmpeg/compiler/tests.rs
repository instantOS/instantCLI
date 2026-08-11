use std::path::PathBuf;
use std::process::Command;

use super::inputs::SourceMap;
use super::util::escape_ffmpeg_path;
use super::{FfmpegCompiler, RenderConfig, VideoDimensions};
use crate::video::config::VideoConfig;
use crate::video::render::mode::RenderMode;
use crate::video::render::timeline::{AvSourceRef, Segment, Timeline};

#[test]
fn compiler_includes_output_path_in_args() {
    let dimensions = VideoDimensions::new(1920, 1080);
    let render_config = RenderConfig::new(RenderMode::Standard, VideoConfig::default(), None);
    let compiler = FfmpegCompiler::new(dimensions, render_config);
    let timeline = Timeline::new();
    let output = compiler
        .compile(
            PathBuf::from("out.mp4"),
            &timeline,
            PathBuf::from("audio.mp4"),
        )
        .unwrap();
    assert_eq!(output.args.last().unwrap(), "out.mp4");
}

#[test]
fn concat_order_respects_timeline_order() {
    let dimensions = VideoDimensions::new(1920, 1080);
    let render_config = RenderConfig::new(RenderMode::Standard, VideoConfig::default(), None);
    let compiler = FfmpegCompiler::new(dimensions, render_config);

    let mut timeline = Timeline::new();
    timeline.add_segment(Segment::new_video_subset(
        0.0,
        1.0,
        5.0,
        AvSourceRef {
            video: PathBuf::from("video.mp4"),
            audio: PathBuf::from("audio.mp4"),
            id: "a".to_string(),
        },
        None,
        false,
    ));
    timeline.add_segment(Segment::new_video_subset(
        1.0,
        1.0,
        1.0,
        AvSourceRef {
            video: PathBuf::from("video.mp4"),
            audio: PathBuf::from("audio.mp4"),
            id: "a".to_string(),
        },
        None,
        false,
    ));
    timeline.add_segment(Segment::new_video_subset(
        2.0,
        1.0,
        3.0,
        AvSourceRef {
            video: PathBuf::from("video.mp4"),
            audio: PathBuf::from("audio.mp4"),
            id: "a".to_string(),
        },
        None,
        false,
    ));

    let output = compiler
        .compile(
            PathBuf::from("out.mp4"),
            &timeline,
            PathBuf::from("audio.mp4"),
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

    // Every source jump is a real edit. Both sides fade within their own
    // fixed-duration A/V unit, so smoothing cannot alter the timeline.
    assert_eq!(filter_complex.matches("afade=t=out").count(), 2);
    assert_eq!(filter_complex.matches("afade=t=in").count(), 2);
    assert!(!filter_complex.contains("acrossfade="));
    assert!(filter_complex.contains("atrim=start=5.000000:end=6.000000"));
    assert!(filter_complex.contains("atrim=start=1.000000:end=2.000000"));
    assert!(filter_complex.contains("atrim=start=3.000000:end=4.000000"));
    assert_eq!(
        filter_complex
            .matches("apad,atrim=duration=1.000000")
            .count(),
        3
    );
}

#[test]
fn contiguous_audio_is_not_crossfaded_at_internal_segment_boundary() {
    let dimensions = VideoDimensions::new(1920, 1080);
    let render_config = RenderConfig::new(RenderMode::Standard, VideoConfig::default(), None);
    let compiler = FfmpegCompiler::new(dimensions, render_config);
    let source = AvSourceRef {
        video: PathBuf::from("video.mp4"),
        audio: PathBuf::from("audio.wav"),
        id: "a".to_string(),
    };

    let mut timeline = Timeline::new();
    timeline.add_segment(Segment::new_video_subset(
        0.0,
        1.0,
        5.0,
        source.clone(),
        None,
        false,
    ));
    timeline.add_segment(Segment::new_video_subset(
        1.0, 1.0, 6.0, source, None, false,
    ));

    let output = compiler
        .compile(
            PathBuf::from("out.mp4"),
            &timeline,
            PathBuf::from("audio.wav"),
        )
        .unwrap();
    let filter_complex_idx = output
        .args
        .iter()
        .position(|arg| arg == "-filter_complex")
        .unwrap();
    let filter_complex = &output.args[filter_complex_idx + 1];

    // The contiguous join at source time 6.0 remains sample-for-sample
    // unchanged: no extension and no crossfade.
    assert!(!filter_complex.contains("acrossfade="));
    assert!(!filter_complex.contains("alimiter="));
    assert!(filter_complex.contains("[a_base]asetpts=PTS-STARTPTS[outa]"));
    assert!(filter_complex.contains("atrim=start=5.000000:end=6.000000"));
    assert!(filter_complex.contains("atrim=start=6.000000:end=7.000000"));
    assert!(filter_complex.contains("[v0][a0][v1][a1]concat=n=2:v=1:a=1[concat_v][concat_a]"));
}

#[test]
fn compiler_rejects_a_gapped_base_av_timeline() {
    let compiler = FfmpegCompiler::new(
        VideoDimensions::new(1920, 1080),
        RenderConfig::new(RenderMode::Standard, VideoConfig::default(), None),
    );
    let source = AvSourceRef {
        video: PathBuf::from("video.mp4"),
        audio: PathBuf::from("audio.wav"),
        id: "a".to_string(),
    };
    let mut timeline = Timeline::new();
    timeline.add_segment(Segment::new_video_subset(
        0.0,
        1.0,
        0.0,
        source.clone(),
        None,
        false,
    ));
    timeline.add_segment(Segment::new_video_subset(
        2.0, 1.0, 2.0, source, None, false,
    ));

    let error = compiler
        .compile(
            PathBuf::from("out.mp4"),
            &timeline,
            PathBuf::from("audio.wav"),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Base A/V timeline is not contiguous")
    );
}

#[test]
fn rendered_repeated_cuts_keep_audio_and_video_on_the_same_source_interval() {
    let temp = tempfile::tempdir().unwrap();
    let video = temp.path().join("source.mkv");
    let voice = temp.path().join("voice.wav");
    let music = temp.path().join("music.wav");
    let output = temp.path().join("output.mp4");

    run_ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "nullsrc=s=64x64:r=30:d=6",
        "-vf",
        "geq=lum='40+30*floor(T)':cb=128:cr=128",
        "-c:v",
        "ffv1",
        video.to_str().unwrap(),
    ]);
    run_ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "aevalsrc=0.8*sin(2*PI*(200+100*floor(t))*t):s=48000:d=6",
        "-c:a",
        "pcm_s16le",
        voice.to_str().unwrap(),
    ]);
    run_ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=50:sample_rate=48000:duration=3",
        "-c:a",
        "pcm_s16le",
        music.to_str().unwrap(),
    ]);

    let source = AvSourceRef {
        video: video.clone(),
        audio: voice.clone(),
        id: "a".to_string(),
    };
    let mut timeline = Timeline::new();
    for (output_start, source_start) in [(0.0, 0.0), (1.0, 2.0), (2.0, 4.0)] {
        timeline.add_segment(Segment::new_video_subset(
            output_start,
            1.0,
            source_start,
            source.clone(),
            None,
            false,
        ));
    }
    timeline.add_segment(Segment::new_music(0.0, 3.0, music));

    let compiler = FfmpegCompiler::new(
        VideoDimensions::new(64, 64),
        RenderConfig::new(RenderMode::Standard, VideoConfig::default(), None),
    );
    let compiled = compiler.compile(output.clone(), &timeline, voice).unwrap();
    let mut args = compiled.args;
    args.insert(args.len() - 1, "-y".to_string());
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());

    // Each output second must come from the same source second in both
    // streams. The source encodes time as video luminance and audio pitch.
    for (output_time, source_time) in [(0.5, 0.5), (1.5, 2.5), (2.5, 4.5)] {
        let expected_luma = sample_video_luma(&video, source_time);
        let luma = sample_video_luma(&output, output_time);
        assert!(luma.abs_diff(expected_luma) <= 4, "unexpected luma {luma}");

        let expected_hz = sample_audio_frequency(&source.audio, source_time);
        let frequency = sample_audio_frequency(&output, output_time);
        assert!(
            (frequency - expected_hz).abs() <= 8.0,
            "unexpected frequency {frequency:.1} Hz"
        );
    }
}

fn run_ffmpeg(args: &[&str]) {
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn sample_video_luma(path: &std::path::Path, time: f64) -> u8 {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-ss"])
        .arg(format!("{time:.3}"))
        .arg("-i")
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=1:1,format=gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    output.stdout[0]
}

fn sample_audio_frequency(path: &std::path::Path, center: f64) -> f64 {
    const SAMPLE_RATE: f64 = 48_000.0;
    const WINDOW: f64 = 0.4;
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-ss"])
        .arg(format!("{:.3}", center - WINDOW / 2.0))
        .arg("-i")
        .arg(path)
        .args(["-t", "0.400", "-map", "0:a:0", "-ac", "1", "-ar", "48000"])
        .args(["-f", "s16le", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let samples = output
        .stdout
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0 && pair[1] > 0)
        .count();
    crossings as f64 / (samples.len() as f64 / SAMPLE_RATE)
}

#[test]
fn voice_is_mono_and_music_mix_is_stereo() {
    let compiler = FfmpegCompiler::new(
        VideoDimensions::new(1920, 1080),
        RenderConfig::new(RenderMode::Standard, VideoConfig::default(), None),
    );
    let mut timeline = Timeline::new();
    timeline.add_segment(Segment::new_video_subset(
        0.0,
        2.0,
        0.0,
        AvSourceRef {
            video: PathBuf::from("video.mp4"),
            audio: PathBuf::from("voice.wav"),
            id: "a".to_string(),
        },
        None,
        false,
    ));
    timeline.add_segment(Segment::new_music(0.0, 2.0, PathBuf::from("music.mp3")));

    let output = compiler
        .compile(
            PathBuf::from("out.mp4"),
            &timeline,
            PathBuf::from("voice.wav"),
        )
        .unwrap();
    let filter_complex = &output.args[output
        .args
        .iter()
        .position(|arg| arg == "-filter_complex")
        .unwrap()
        + 1];

    assert!(filter_complex.contains("channel_layouts=mono[a_base]"));
    assert!(filter_complex.matches("channel_layouts=stereo").count() >= 2);
    assert!(filter_complex.contains("[music_0]"));
    assert!(
        filter_complex.contains("channel_layouts=stereo,asplit=2[a_voice_mix][a_voice_sidechain]")
    );
    assert!(filter_complex.contains("[music_0][a_voice_sidechain]sidechaincompress="));
    assert!(filter_complex.contains("[a_voice_mix][a_duck]amix=inputs=2:duration=first"));
    assert!(filter_complex.contains("alimiter=limit=0.8913:level=false:latency=true"));
    assert!(
        filter_complex
            .contains("alimiter=limit=0.8913:level=false:latency=true,asetpts=PTS-STARTPTS[outa]")
    );
    assert!(output.args.iter().any(|arg| arg == "-shortest"));
}

#[test]
fn test_reels_mode_generates_padding_filter() {
    let dimensions = VideoDimensions::new(1080, 1920);
    let render_config = RenderConfig::new(RenderMode::Reels, VideoConfig::default(), None);
    let compiler = FfmpegCompiler::new(dimensions, render_config);
    let padding = compiler.build_padding_filter("v0_raw", "v0");
    assert!(padding.is_some());

    let filter = padding.unwrap();
    assert!(filter.contains("scale=1080:-1"));
    assert!(filter.contains("pad=1080:1920"));
    assert!(filter.contains("(oh-ih)*0.1"));
    assert!(filter.contains(":0x1E1E2E"));
    assert!(!filter.contains("ass="));
}

#[test]
fn test_reels_mode_padding_excludes_subtitles() {
    let dimensions = VideoDimensions::new(1080, 1920);
    let render_config = RenderConfig::new(
        RenderMode::Reels,
        VideoConfig::default(),
        Some(PathBuf::from("/tmp/subs.ass")),
    );
    let compiler = FfmpegCompiler::new(dimensions, render_config);
    let padding = compiler.build_padding_filter("v0_raw", "v0");
    assert!(padding.is_some());

    let filter = padding.unwrap();
    assert!(filter.contains("scale=1080:-1"));
    assert!(filter.contains("pad=1080:1920"));
    assert!(!filter.contains("ass="));
}

#[test]
fn test_filter_complex_includes_subtitles() {
    let dimensions = VideoDimensions::new(1080, 1920);
    let render_config = RenderConfig::new(
        RenderMode::Reels,
        VideoConfig::default(),
        Some(PathBuf::from("/tmp/subs.ass")),
    );
    let compiler = FfmpegCompiler::new(dimensions, render_config);

    let mut timeline = Timeline::new();
    timeline.add_segment(Segment::new_video_subset(
        0.0,
        0.0,
        5.0,
        AvSourceRef {
            video: PathBuf::from("video.mp4"),
            audio: PathBuf::from("audio.mp4"),
            id: "a".to_string(),
        },
        None,
        false,
    ));

    let source_map = SourceMap::build(&timeline, PathBuf::from("audio.mp4").as_path(), false);
    let filter_complex = compiler
        .build_filter_complex(&timeline, &source_map, 5.0)
        .unwrap();

    assert!(filter_complex.contains("ass='/tmp/subs.ass'"));
    assert!(filter_complex.contains("[concat_v]ass='/tmp/subs.ass'[subtitled_v]"));
}

#[test]
fn test_standard_mode_no_padding() {
    let dimensions = VideoDimensions::new(1920, 1080);
    let render_config = RenderConfig::new(RenderMode::Standard, VideoConfig::default(), None);
    let compiler = FfmpegCompiler::new(dimensions, render_config);
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
