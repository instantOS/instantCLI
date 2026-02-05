use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::video::cli::RenderArgs;

pub(super) fn prepare_output_destination(
    output_path: &Path,
    args: &RenderArgs,
    video_path: &Path,
) -> Result<()> {
    if output_path == video_path {
        bail!(
            "Output path {} would overwrite the source video",
            output_path.display()
        );
    }

    if output_path.exists() {
        if args.force {
            fs::remove_file(output_path).with_context(|| {
                format!(
                    "Failed to remove existing output file {} before overwrite",
                    output_path.display()
                )
            })?;
        } else {
            bail!(
                "Output file {} already exists. Use --force to overwrite.",
                output_path.display()
            );
        }
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    Ok(())
}
