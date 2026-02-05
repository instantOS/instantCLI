/// Rendering mode for the output video
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    /// Standard rendering (same dimensions as source)
    #[default]
    Standard,
    /// Instagram Reels/TikTok (9:16 vertical, 1080x1920)
    Reels,
}

impl RenderMode {
    /// Get target dimensions for this render mode
    pub fn target_dimensions(&self, source_width: u32, source_height: u32) -> (u32, u32) {
        match self {
            RenderMode::Standard => (source_width, source_height),
            RenderMode::Reels => (1080, 1920),
        }
    }

    /// Get output file suffix for this render mode
    pub fn output_suffix(&self) -> &str {
        match self {
            RenderMode::Standard => "_edit",
            RenderMode::Reels => "_reels",
        }
    }

    /// Whether this mode requires letterboxing/pillboxing
    pub fn requires_padding(&self) -> bool {
        matches!(self, RenderMode::Reels)
    }

    /// Get vertical position offset as percentage (0.0 = top, 0.5 = center)
    pub fn vertical_offset_pct(&self) -> f64 {
        match self {
            RenderMode::Standard => 0.5,
            RenderMode::Reels => 0.1, // 10% from top
        }
    }
}
