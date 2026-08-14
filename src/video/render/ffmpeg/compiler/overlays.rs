use anyhow::Result;

use super::FfmpegCompiler;
use super::FilterGraphBuilder;
use super::graph::{VideoInput, VideoPad};
use super::inputs::SourceMap;
use crate::video::render::timeline::{BrollClip, ImageOverlay, Position, TimelineRange, Transform};

const OVERLAY_FRAME_SCALE: f64 = 0.9;
const OVERLAY_FRAME_BORDER_WIDTH: u32 = 4;
const OVERLAY_FRAME_BORDER_COLOR: &str = "0x89B4FA";
const OVERLAY_FRAME_BACKGROUND_COLOR: &str = "0x1E1E2E";

impl FfmpegCompiler {
    fn compute_overlay_position(
        &self,
        transform: Option<&Transform>,
        overlay_width: u32,
        overlay_height: u32,
    ) -> (i32, i32) {
        const MARGIN: i32 = 16;

        let tw = self.target_width as i32;
        let th = self.target_height as i32;
        let ow = overlay_width as i32;
        let oh = overlay_height as i32;

        let position = transform
            .and_then(|t| t.position)
            .unwrap_or(Position::Center);

        let (mut x, mut y) = match position {
            Position::Center => {
                let cy = if self.render_mode.requires_padding() {
                    ((th - oh) as f64 * 0.3) as i32
                } else {
                    (th - oh) / 2
                };
                ((tw - ow) / 2, cy)
            }
            Position::TopLeft => (MARGIN, MARGIN),
            Position::Top => ((tw - ow) / 2, MARGIN),
            Position::TopRight => (tw - ow - MARGIN, MARGIN),
            Position::Right => (tw - ow - MARGIN, (th - oh) / 2),
            Position::BottomRight => (tw - ow - MARGIN, th - oh - MARGIN),
            Position::Bottom => ((tw - ow) / 2, th - oh - MARGIN),
            Position::BottomLeft => (MARGIN, th - oh - MARGIN),
            Position::Left => (MARGIN, (th - oh) / 2),
        };

        if let Some(t) = transform
            && let Some((tx, ty)) = t.translate
        {
            x += tx as i32;
            y += ty as i32;
        }

        (x, y)
    }

    fn build_broll_prep(
        &self,
        ctx: &mut FilterGraphBuilder,
        input_index: usize,
        source_start: f64,
        duration: f64,
        timeline_start: f64,
        idx: usize,
    ) -> VideoPad {
        let trimmed_label = format!("broll_trim_{idx}");
        let trim_end = source_start + duration;
        ctx.graph().video_from_input(
            VideoInput(input_index),
            format!(
                "trim=start={start}:end={end},setpts=PTS-STARTPTS+{offset}/TB",
                start = source_start,
                end = trim_end,
                offset = timeline_start,
            ),
            trimmed_label,
        )
    }

    fn build_image_prep(
        &self,
        ctx: &mut FilterGraphBuilder,
        input_index: usize,
        idx: usize,
    ) -> VideoPad {
        let overlay_input = format!("overlay_raw_{idx}");
        ctx.graph()
            .video_from_input(VideoInput(input_index), "format=rgba", overlay_input)
    }

    fn apply_overlay_segment(
        &self,
        ctx: &mut FilterGraphBuilder,
        prep: VideoPad,
        transform: Option<&Transform>,
        time_window: TimelineRange,
        current_video: VideoPad,
        label: &str,
    ) -> VideoPad {
        let scaled_label = label.to_string();
        let output_label = format!("{label}_out");

        let scale_factor = transform
            .and_then(|t| t.scale)
            .map(|s| s as f64)
            .unwrap_or(OVERLAY_FRAME_SCALE);

        let outer_width = (self.target_width as f64 * scale_factor) as u32;
        let outer_height = (self.target_height as f64 * scale_factor) as u32;
        let inner_width = outer_width - (OVERLAY_FRAME_BORDER_WIDTH * 2);
        let inner_height = outer_height - (OVERLAY_FRAME_BORDER_WIDTH * 2);
        let scaled = ctx.graph().video_filter(
            prep,
            format!(
                "scale={inner_width}:{inner_height}:force_original_aspect_ratio=decrease,pad={inner_width}:{inner_height}:(ow-iw)/2:(oh-ih)/2:{OVERLAY_FRAME_BACKGROUND_COLOR},setsar=1,pad={outer_width}:{outer_height}:(ow-iw)/2:(oh-ih)/2:{OVERLAY_FRAME_BORDER_COLOR}"
            ),
            scaled_label,
        );
        let enable_condition = format!("between(t,{},{})", time_window.start, time_window.end);
        let (x_offset, y_offset) =
            self.compute_overlay_position(transform, outer_width, outer_height);
        ctx.graph().overlay(
            current_video,
            scaled,
            format!("overlay=x={x_offset}:y={y_offset}:enable='{enable_condition}'"),
            output_label,
        )
    }

    pub(super) fn apply_broll_overlays(
        &self,
        ctx: &mut FilterGraphBuilder,
        broll_segments: &[BrollClip],
        source_map: &SourceMap,
        input: VideoPad,
    ) -> Result<VideoPad> {
        let mut current_video = input;

        for (idx, segment) in broll_segments.iter().enumerate() {
            let input_index = source_map.index(&segment.source_video)?;
            let offset = source_map.offset(input_index);
            let adj_start = segment.source_start.seconds() - offset;

            let prep = self.build_broll_prep(
                ctx,
                input_index,
                adj_start,
                segment.duration.seconds(),
                segment.timeline_start.seconds(),
                idx,
            );

            current_video = self.apply_overlay_segment(
                ctx,
                prep,
                segment.transform.as_ref(),
                TimelineRange::new(
                    segment.timeline_start.seconds(),
                    segment.timeline_start.seconds() + segment.duration.seconds(),
                ),
                current_video,
                &format!("broll_{idx}"),
            );
        }

        Ok(current_video)
    }

    pub(super) fn apply_overlays(
        &self,
        ctx: &mut FilterGraphBuilder,
        overlay_segments: &[ImageOverlay],
        source_map: &SourceMap,
        input: VideoPad,
    ) -> Result<VideoPad> {
        let mut current_video = input;

        for (idx, segment) in overlay_segments.iter().enumerate() {
            let input_index = source_map.index(&segment.source_image)?;
            let prep = self.build_image_prep(ctx, input_index, idx);

            current_video = self.apply_overlay_segment(
                ctx,
                prep,
                segment.transform.as_ref(),
                TimelineRange::new(
                    segment.timeline_start.seconds(),
                    segment.timeline_start.seconds() + segment.duration.seconds(),
                ),
                current_video,
                &format!("overlay_{idx}"),
            );
        }

        Ok(current_video)
    }
}
