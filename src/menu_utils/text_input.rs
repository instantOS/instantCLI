use anyhow::Result;

use super::fzf::{ConfirmResult, FzfResult, FzfWrapper};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEditOutcome {
    Cancelled,
    Unchanged,
    Updated(Option<String>),
}

#[derive(Debug, Clone)]
pub struct TextEditPrompt<'a> {
    label: &'a str,
    current: Option<&'a str>,
    header: Option<String>,
    ghost: Option<String>,
}

impl<'a> TextEditPrompt<'a> {
    pub fn new(label: &'a str, current: Option<&'a str>) -> Self {
        Self {
            label,
            current,
            header: None,
            ghost: None,
        }
    }

    pub fn header(mut self, header: impl Into<String>) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn ghost(mut self, ghost: impl Into<String>) -> Self {
        self.ghost = Some(ghost.into());
        self
    }
}

pub fn prompt_text_edit(prompt: TextEditPrompt<'_>) -> Result<TextEditOutcome> {
    let TextEditPrompt {
        label,
        current,
        header,
        ghost,
    } = prompt;

    let mut builder = FzfWrapper::builder()
        .input()
        .prompt(label)
        .ghost(ghost.as_deref().unwrap_or("Leave empty to clear"));

    if let Some(header) = header {
        builder = builder.header(header);
    }

    if let Some(value) = current {
        builder = builder.query(value);
    }

    match builder.input_result()? {
        FzfResult::Cancelled => Ok(TextEditOutcome::Cancelled),
        FzfResult::Selected(value) => handle_text_selection(label, current, value),
        _ => Ok(TextEditOutcome::Cancelled),
    }
}

fn handle_text_selection(
    label: &str,
    current: Option<&str>,
    value: String,
) -> Result<TextEditOutcome> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return confirm_empty_input(label);
    }

    if let Some(current) = current {
        if trimmed == current.trim() {
            return Ok(TextEditOutcome::Unchanged);
        }
    }

    Ok(TextEditOutcome::Updated(Some(trimmed)))
}

fn confirm_empty_input(label: &str) -> Result<TextEditOutcome> {
    let confirm = FzfWrapper::builder()
        .confirm(format!(
            "{} was left empty.\n\nChoose \"Clear Value\" to remove it or \"Go Back\" to keep the current value.",
            label
        ))
        .yes_text("Clear Value")
        .no_text("Go Back")
        .confirm_dialog()?;

    Ok(match confirm {
        ConfirmResult::Yes => TextEditOutcome::Updated(None),
        ConfirmResult::No => TextEditOutcome::Unchanged,
        ConfirmResult::Cancelled => TextEditOutcome::Cancelled,
    })
}
