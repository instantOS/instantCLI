use anyhow::Result;

use super::FfmpegCompiler;
use super::inputs::SourceMap;
use crate::video::render::timeline::{Segment, SegmentData, TimeWindow, Transform};

const OVERLAY_FRAME_SCALE: f64 = 0.9;
const OVERLAY_FRAME_BORDER_WIDTH: u32 = 4;
const OVERLAY_FRAME_BORDER_COLOR: &str = "0x89B4FA";
const OVERLAY_FRAME_BACKGROUND_COLOR: &str = "0x1E1E2E";

impl FfmpegCompiler {
    pub(super) fn build_scaled_overlay_filters(
        &self,
        input_label: &str,
        output_label: &str,
        scale: f64,
    ) -> String {
        let outer_width = (self.target_width as f64 * scale) as u32;
        let outer_height = (self.target_height as f64 * scale) as u32;
        let inner_width = outer_width - (OVERLAY_FRAME_BORDER_WIDTH * 2);
        let inner_height = outer_height - (OVERLAY_FRAME_BORDER_WIDTH * 2);

        format!(
            "[{input}]scale={iw}:{ih}:force_original_aspect_ratio=decrease,pad={iw}:{ih}:(ow-iw)/2:(oh-ih)/2:{background},setsar=1,pad={ow}:{oh}:(ow-iw)/2:(oh-ih)/2:{border}[{out}]",
            input = input_label,
            iw = inner_width,
            ih = inner_height,
            ow = outer_width,
            oh = outer_height,
            background = OVERLAY_FRAME_BACKGROUND_COLOR,
            border = OVERLAY_FRAME_BORDER_COLOR,
            out = output_label,
        )
    }

    fn compute_overlay_position(
        &self,
        transform: Option<&Transform>,
        overlay_width: u32,
        overlay_height: u32,
    ) -> (i32, i32) {
        let base_x = (self.target_width as i32 - overlay_width as i32) / 2;
        let base_y = if self.render_mode.requires_padding() {
            ((self.target_height as i32 - overlay_height as i32) as f64 * 0.3) as i32
        } else {
            (self.target_height as i32 - overlay_height as i32) / 2
        };

        if let Some(t) = transform
            && let Some((tx, ty)) = t.translate
        {
            (base_x + tx as i32, base_y + ty as i32)
        } else {
            (base_x, base_y)
        }
    }

    fn build_overlay_label_triplet(&self, prefix: &str, idx: usize) -> (String, String, String) {
        (
            format!("{prefix}_raw_{idx}"),
            format!("{prefix}_{idx}"),
            format!("{prefix}_out_{idx}"),
        )
    }

    fn build_overlay_chain(
        &self,
        filters: &mut Vec<String>,
        base_label: &str,
        overlay_scaled_label: &str,
        output_label: &str,
        transform: Option<&Transform>,
        scale_factor: f64,
        time_window: TimeWindow,
    ) -> String {
        let enable_condition = format!("between(t,{},{})", time_window.start, time_window.end);
        let overlay_width = (self.target_width as f64 * scale_factor) as u32;
        let overlay_height = (self.target_height as f64 * scale_factor) as u32;
        let (x_offset, y_offset) =
            self.compute_overlay_position(transform, overlay_width, overlay_height);

        filters.push(format!(
            "[{video}][{overlay}]overlay=x={x}:y={y}:enable='{condition}'[{output}]",
            video = base_label,
            overlay = overlay_scaled_label,
            x = x_offset,
            y = y_offset,
            condition = enable_condition,
            output = output_label,
        ));

        output_label.to_string()
    }

    pub(super) fn apply_broll_overlays(
        &self,
        filters: &mut Vec<String>,
        broll_segments: &[&Segment],
        source_map: &SourceMap,
        input_label: &str,
    ) -> Result<String> {
        let mut current_video_label = input_label.to_string();

        for (idx, segment) in broll_segments.iter().enumerate() {
            let SegmentData::Broll {
                start_time: source_start,
                source_video,
                transform,
                ..
            } = &segment.data
            else {
                continue;
            };

            let input_index = source_map.index(source_video)?;

            let trimmed_label = format!("broll_trim_{idx}");
            let (scaled_label, output_label, _) = self.build_overlay_label_triplet("broll", idx);

            let trim_end = source_start + segment.duration;
            filters.push(format!(
                "[{input}:v]trim=start={start}:end={end},setpts=PTS-STARTPTS+{offset}/TB[{out}]",
                input = input_index,
                start = source_start,
                end = trim_end,
                offset = segment.start_time,
                out = trimmed_label,
            ));

            let scale_factor = transform
                .as_ref()
                .and_then(|t| t.scale)
                .map(|s| s as f64)
                .unwrap_or(OVERLAY_FRAME_SCALE);

            let scaled_broll =
                self.build_scaled_overlay_filters(&trimmed_label, &scaled_label, scale_factor);
            filters.push(scaled_broll);

            current_video_label = self.build_overlay_chain(
                filters,
                &current_video_label,
                &scaled_label,
                &output_label,
                transform.as_ref(),
                scale_factor,
                segment.time_window(),
            );
        }

        Ok(current_video_label)
    }

    pub(super) fn apply_overlays(
        &self,
        filters: &mut Vec<String>,
        overlay_segments: &[&Segment],
        source_map: &SourceMap,
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

            let input_index = source_map.index(source_image)?;

            let (overlay_input, overlay_label, output_label) =
                self.build_overlay_label_triplet("overlay", idx);

            filters.push(format!(
                "[{input}:v]format=rgba[{overlay}]",
                input = input_index,
                overlay = overlay_input,
            ));

            let scale_factor = transform
                .as_ref()
                .and_then(|t| t.scale)
                .map(|s| s as f64)
                .unwrap_or(OVERLAY_FRAME_SCALE);

            let framed_overlay =
                self.build_scaled_overlay_filters(&overlay_input, &overlay_label, scale_factor);
            filters.push(framed_overlay);

            current_video_label = self.build_overlay_chain(
                filters,
                &current_video_label,
                &overlay_label,
                &output_label,
                transform.as_ref(),
                scale_factor,
                segment.time_window(),
            );
        }

        Ok(current_video_label)
    }
}
