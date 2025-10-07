use std::fs;

use anyhow::{Context, Result, bail};

use super::cli::TitlecardArgs;
use super::document::{DocumentBlock, parse_video_document};
use super::title_card::TitleCardGenerator;

pub fn handle_titlecard(args: TitlecardArgs) -> Result<()> {
    let markdown_path = args.markdown.canonicalize().with_context(|| {
        format!("Failed to resolve markdown path {}", args.markdown.display())
    })?;

    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    // Find the first heading in the document
    let heading = document.blocks.iter().find_map(|block| {
        if let DocumentBlock::Heading(h) = block {
            Some(h)
        } else {
            None
        }
    });

    let heading = match heading {
        Some(h) => h,
        None => bail!("No heading found in markdown file {}", markdown_path.display()),
    };

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
    
    generator.generate_image(heading.level, &heading.text, &output_path)?;

    println!("Title card generated: {}", output_path.display());

    Ok(())
}
