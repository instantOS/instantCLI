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

    // A dry run only plans and prints the command. It must not remove an
    // existing output or create its parent directory, even with --force.
    if args.dry_run {
        return Ok(());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::cli::VideoProcessArgs;

    #[test]
    fn forced_dry_run_preserves_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("edit.mp4");
        let source = dir.path().join("source.mp4");
        fs::write(&output, b"existing render").unwrap();
        let args = RenderArgs {
            markdown: dir.path().join("video.md"),
            out_file: None,
            force: true,
            dry_run: true,
            common: VideoProcessArgs {
                precache_slides: false,
                reels: false,
                subtitles: false,
                verbose: false,
            },
        };

        prepare_output_destination(&output, &args, &source).unwrap();

        assert_eq!(fs::read(&output).unwrap(), b"existing render");
    }
}
