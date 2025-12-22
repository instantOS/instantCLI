use std::fs;

use anyhow::{Context, Result};

use super::super::cli::TitlecardArgs;
use super::super::markdown_utils::strip_yaml_frontmatter;
use super::TitleCardGenerator;

pub fn handle_titlecard(args: TitlecardArgs) -> Result<()> {
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

    // Use default 1920x1080 resolution for title cards
    let generator = TitleCardGenerator::new(1920, 1080)?;

    let asset = generator.markdown_card(content)?;

    if asset.was_cached {
        println!("Using cached title card: {}", asset.image_path.display());
    } else {
        println!("Generated new title card: {}", asset.image_path.display());
    }

    fs::copy(&asset.image_path, &output_path)
        .with_context(|| format!("Failed to copy title card to {}", output_path.display()))?;

    println!("Title card saved to: {}", output_path.display());

    Ok(())
}
