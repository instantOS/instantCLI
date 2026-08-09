use std::path::PathBuf;

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

    let concat_pos = filter_complex.find("concat=n=3:v=1:a=0[concat_v]").unwrap();
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

    // Every source jump is a real edit. Each side contributes 5 ms of
    // surrounding audio to a 10 ms crossfade, preserving total duration.
    assert_eq!(filter_complex.matches("acrossfade=d=0.010000").count(), 2);
    assert!(filter_complex.contains("atrim=start=5.000000:end=6.005000"));
    assert!(filter_complex.contains("atrim=start=0.995000:end=2.005000"));
    assert!(filter_complex.contains("atrim=start=2.995000:end=4.000000"));
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
    assert!(filter_complex.contains("[a_base]anull[outa]"));
    assert!(filter_complex.contains("atrim=start=5.000000:end=6.000000"));
    assert!(filter_complex.contains("atrim=start=6.000000:end=7.000000"));
    assert!(filter_complex.contains("[a0][a1]concat=n=2:v=0:a=1[concat_a]"));
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
    assert!(filter_complex.contains("channel_layouts=stereo[a_base_stereo]"));
    assert!(filter_complex.contains("[a_base_stereo][a_duck]amix="));
    assert!(filter_complex.contains("alimiter=limit=0.8913:level=false:latency=true[outa]"));
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
