use std::process::Command;

use crate::ui::preview::PreviewBuilder;

pub(crate) fn command_output_optional(mut cmd: Command) -> Option<String> {
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

pub(crate) fn indent_lines(lines: &[String], indent: &str) -> Vec<String> {
    if lines.is_empty() {
        return vec![format!("{indent}(unavailable)")];
    }
    lines.iter().map(|line| format!("{indent}{line}")).collect()
}

pub(crate) fn push_raw_lines(mut builder: PreviewBuilder, lines: &[String]) -> PreviewBuilder {
    for line in lines {
        builder = builder.raw(line);
    }
    builder
}

pub(crate) fn push_bullets(mut builder: PreviewBuilder, lines: &[String]) -> PreviewBuilder {
    for line in lines {
        builder = builder.bullet(line);
    }
    builder
}

pub(crate) fn truncate_label(label: &str, limit: usize) -> String {
    let mut chars = label.chars();
    let count = label.chars().count();
    if count <= limit {
        return label.to_string();
    }
    let mut truncated = String::new();
    for _ in 0..limit.saturating_sub(3) {
        if let Some(c) = chars.next() {
            truncated.push(c);
        }
    }
    truncated.push_str("...");
    truncated
}

pub(crate) trait PreviewBuilderExt {
    fn raw_lines(self, lines: &[String]) -> Self;
}

impl PreviewBuilderExt for PreviewBuilder {
    fn raw_lines(self, lines: &[String]) -> Self {
        push_raw_lines(self, lines)
    }
}
