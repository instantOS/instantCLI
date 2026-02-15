use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::mode::RenderMode;

pub fn resolve_output_path(
    out_file: Option<&PathBuf>,
    video_path: &Path,
    project_dir: &Path,
    render_mode: RenderMode,
) -> Result<PathBuf> {
    if let Some(provided) = out_file {
        let resolved = if provided.is_absolute() {
            provided.clone()
        } else {
            project_dir.join(provided)
        };
        return Ok(resolved);
    }

    let mut output = video_path.to_path_buf();
    let stem = video_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("Video path {} has no valid file name", video_path.display()))?;

    let suffix = render_mode.output_suffix();
    output.set_file_name(format!("{stem}{suffix}.mp4"));
    Ok(output)
}
