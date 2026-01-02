//! ASS Subtitle generation for video rendering.
//!
//! This module provides functionality to:
//! - Generate ASS (Advanced SubStation Alpha) subtitle files
//! - Remap transcript cue timings from source video time to final timeline time
//!
//! The remapping is critical for reels mode where segments may be reordered
//! or pauses inserted between segments.

mod ass;
mod remap;

pub use ass::{AssStyle, generate_ass_file};
pub use remap::remap_subtitles_to_timeline;
