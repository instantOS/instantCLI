use std::fs;

use anyhow::{Context, Result};

use super::super::cli::TitlecardArgs;
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
    let content = strip_frontmatter(&markdown_contents);

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

    generator.generate_image_from_markdown(content, &output_path)?;

    println!("Title card generated: {}", output_path.display());

    Ok(())
}

fn strip_frontmatter(content: &str) -> &str {
    if !(content.starts_with("---\n") || content.starts_with("---\r\n")) {
        return content;
    }

    let first_newline = match content.find('\n') {
        Some(n) => n,
        None => return content,
    };

    let mut cursor = first_newline + 1;

    while cursor <= content.len() {
        let next_newline = content[cursor..].find('\n');
        let line_end = match next_newline {
            Some(offset) => cursor + offset + 1,
            None => content.len(),
        };
        let line = &content[cursor..line_end];

        if line.trim_end_matches(['\r', '\n']) == "---" {
            return &content[line_end..];
        }

        cursor = line_end;
    }

    content
}
