use anyhow::{Result, anyhow};

/// Splits content into frontmatter and body components.
///
/// This is a more comprehensive version that returns both the frontmatter content
/// (if present) and the body content, along with the byte offset where the body starts.
///
/// # Arguments
/// * `content` - The markdown content to split
///
/// # Returns
/// * `Result<(Option<&str>, &str, usize)>` - A tuple containing:
///   - `Option<&str>` - The frontmatter content (if present)
///   - `&str` - The body content
///   - `usize` - The byte offset where the body starts
pub fn split_frontmatter(content: &str) -> Result<(Option<&str>, &str, usize)> {
    if !(content.starts_with("---\n") || content.starts_with("---\r\n")) {
        return Ok((None, content, 0));
    }

    let first_newline = content
        .find('\n')
        .ok_or_else(|| anyhow!("Front matter start delimiter without newline"))?;
    let mut cursor = first_newline + 1;
    let front_start = cursor;

    while cursor < content.len() {
        let next_newline = content[cursor..].find('\n');
        let line_end = match next_newline {
            Some(offset) => cursor + offset + 1,
            None => content.len(),
        };
        let line = &content[cursor..line_end];

        if line.trim_end_matches(['\r', '\n']) == "---" {
            let front = &content[front_start..cursor];
            let body_start = line_end;
            return Ok((Some(front), &content[body_start..], body_start));
        }

        cursor = line_end;
    }

    Err(anyhow!("Closing front matter delimiter '---' not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_frontmatter_with_content() {
        let content = "---\ntitle: Test\n---\n# Hello World\nThis is content.";
        let result = split_frontmatter(content).unwrap();
        assert_eq!(result.0, Some("title: Test\n"));
        assert_eq!(result.1, "# Hello World\nThis is content.");
        assert_eq!(result.2, 20); // Offset after the closing ---
    }

    #[test]
    fn test_split_frontmatter_without_frontmatter() {
        let content = "# Hello World\nThis is content.";
        let result = split_frontmatter(content).unwrap();
        assert_eq!(result.0, None);
        assert_eq!(result.1, "# Hello World\nThis is content.");
        assert_eq!(result.2, 0);
    }
}
