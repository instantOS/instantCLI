use std::fs;

use anyhow::{Context, Result};

use super::super::cli::SlideArgs;
use super::super::markdown_utils::strip_yaml_frontmatter;
use super::SlideGenerator;

pub fn handle_slide(args: SlideArgs) -> Result<()> {
    let markdown_path = args.markdown.canonicalize().with_context(|| {
        format!(
            "Failed to resolve markdown path {}",
            args.markdown.display()
        )
    })?;

    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    // Strip YAML frontmatter if present
    let content = strip_yaml_frontmatter(&markdown_contents);

    // Determine output path
    let output_path = if let Some(out) = args.out_file {
        out
    } else {
        // Default to <markdownfilename>.jpg
        let mut path = markdown_path.clone();
        path.set_extension("jpg");
        path
    };

    // Use default 1920x1080 resolution for slides
    let generator = SlideGenerator::new(1920, 1080)?;

    let asset = generator.markdown_slide(content)?;

    if asset.was_cached {
        println!("Using cached slide: {}", asset.image_path.display());
    } else {
        println!("Generated new slide: {}", asset.image_path.display());
    }

    fs::copy(&asset.image_path, &output_path)
        .with_context(|| format!("Failed to copy slide to {}", output_path.display()))?;

    println!("Slide saved to: {}", output_path.display());

    Ok(())
}
