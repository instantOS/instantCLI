use anyhow::Result;

use super::fzf::{ConfirmResult, FzfResult, FzfWrapper};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEditOutcome {
    Cancelled,
    Unchanged,
    Updated(Option<String>),
}

pub fn prompt_text_edit(label: &str, current: Option<&str>) -> Result<TextEditOutcome> {
    let mut builder = FzfWrapper::builder()
        .input()
        .prompt(label)
        .ghost("Leave empty to clear");

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
