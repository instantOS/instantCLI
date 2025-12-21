use anyhow::{Result, anyhow};

/// Strips YAML frontmatter from content and returns the body content.
///
/// This function handles the common pattern of YAML frontmatter surrounded by `---` delimiters.
/// It returns the content after the frontmatter, or the original content if no frontmatter is found.
///
/// # Arguments
/// * `content` - The markdown content to process
///
/// # Returns
/// * `&str` - The content with frontmatter removed
pub fn strip_yaml_frontmatter(content: &str) -> &str {
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

    while cursor <= content.len() {
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

/// Checks if content has YAML frontmatter.
///
/// # Arguments
/// * `content` - The content to check
///
/// # Returns
/// * `bool` - true if frontmatter is present, false otherwise
pub fn has_frontmatter(content: &str) -> bool {
    content.starts_with("---\n") || content.starts_with("---\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter_with_content() {
        let content = "---\ntitle: Test\n---\n# Hello World\nThis is content.";
        let result = strip_yaml_frontmatter(content);
        assert_eq!(result, "# Hello World\nThis is content.");
    }

    #[test]
    fn test_strip_frontmatter_without_frontmatter() {
        let content = "# Hello World\nThis is content.";
        let result = strip_yaml_frontmatter(content);
        assert_eq!(result, "# Hello World\nThis is content.");
    }

    #[test]
    fn test_strip_frontmatter_empty() {
        let content = "";
        let result = strip_yaml_frontmatter(content);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_frontmatter_with_crlf() {
        let content = "---\r\ntitle: Test\r\n---\r\n# Hello World";
        let result = strip_yaml_frontmatter(content);
        assert_eq!(result, "# Hello World");
    }

    #[test]
    fn test_strip_frontmatter_malformed_no_end() {
        let content = "---\ntitle: Test\n# Hello World";
        let result = strip_yaml_frontmatter(content);
        // When malformed, return the original content
        assert_eq!(result, "---\ntitle: Test\n# Hello World");
    }

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

    #[test]
    fn test_has_frontmatter_true() {
        let content = "---\ntitle: Test\n---\n# Hello World";
        assert!(has_frontmatter(content));
    }

    #[test]
    fn test_has_frontmatter_false() {
        let content = "# Hello World";
        assert!(!has_frontmatter(content));
    }

    #[test]
    fn test_has_frontmatter_crlf() {
        let content = "---\r\ntitle: Test\r\n---\r\n# Hello World";
        assert!(has_frontmatter(content));
    }
}
