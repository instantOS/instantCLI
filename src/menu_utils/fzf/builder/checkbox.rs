//! A persistent footer checkbox for existing fzf selections.
use std::io::Write;

use anyhow::{Context, Result};

use super::{FzfBuilder, StreamSelection};
use crate::common::shell::shell_quote;
use crate::menu_utils::fzf::types::{DialogOutcome, FzfSelectable, MenuHint, MenuSelection};

/// Selection modifier; fzf retains its query, cursor and preview when toggled.
pub struct CheckboxSelection<S> {
    selection: S,
    label: String,
    checked: bool,
    hint: String,
}

struct CheckboxFiles {
    state: tempfile::NamedTempFile,
    // fzf runs this small callback when Ctrl+D or the footer is clicked.
    _script: tempfile::NamedTempFile,
}

impl CheckboxFiles {
    fn configure(builder: &mut FzfBuilder, label: &str, checked: bool, hint: &str) -> Result<Self> {
        let label = label
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>();
        let mut state = tempfile::NamedTempFile::new().context("Create checkbox state")?;
        state.write_all(if checked { b"1" } else { b"0" })?;
        let state_path = shell_quote(&state.path().to_string_lossy());
        let off = shell_quote(&format!("[ ] {label}"));
        let on = shell_quote(&format!("[✓] {label}"));
        let mut script = tempfile::NamedTempFile::new().context("Create checkbox callback")?;
        write!(
            script,
            "if [ \"$(cat {state_path})\" = 1 ]; then\n  printf 0 > {state_path}\n  printf '%s\\n' {off}\nelse\n  printf 1 > {state_path}\n  printf '%s\\n' {on}\nfi\n"
        )?;
        let callback = format!("sh {}", shell_quote(&script.path().to_string_lossy()));
        builder.shared.hints.push(MenuHint::new("Ctrl D", hint));
        builder.shared.user_args.extend([
            "--footer".into(),
            format!("{} {label}", if checked { "[✓]" } else { "[ ]" }),
            "--footer-border=top".into(),
            "--bind".into(),
            format!("ctrl-d:transform-footer:{callback}"),
            "--bind".into(),
            format!("click-footer:transform-footer:{callback}"),
        ]);
        Ok(Self {
            state,
            _script: script,
        })
    }

    fn checked(&self) -> Result<bool> {
        match std::fs::read_to_string(self.state.path())?.as_str() {
            "1" => Ok(true),
            "0" => Ok(false),
            _ => anyhow::bail!("Invalid checkbox state returned by fzf"),
        }
    }
}

impl<'a, T, A> StreamSelection<'a, T, A> {
    pub fn checkbox(self, label: impl Into<String>, checked: bool) -> CheckboxSelection<Self> {
        let label = label.into();
        CheckboxSelection {
            selection: self,
            hint: label.clone(),
            label,
            checked,
        }
    }
}

impl<S> CheckboxSelection<S> {
    /// Describe what this option does when the footer label includes extra context.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
}

impl<T: FzfSelectable + Clone + Send + 'static, A: Clone>
    CheckboxSelection<StreamSelection<'_, T, A>>
{
    pub fn select(mut self) -> Result<DialogOutcome<(MenuSelection<T, A>, bool)>> {
        let files = CheckboxFiles::configure(
            &mut self.selection.builder,
            &self.label,
            self.checked,
            &self.hint,
        )?;
        let outcome = self.selection.select()?;
        let checked = files.checked()?;
        Ok(outcome.map(|selection| (selection, checked)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_toggles_without_interpreting_label_as_shell() -> Result<()> {
        let mut builder = FzfBuilder::new();
        let files = CheckboxFiles::configure(
            &mut builder,
            "Use $(exit 9); 'literal' as default",
            false,
            "use as default",
        )?;
        for (expected, marker) in [(true, "[✓]"), (false, "[ ]")] {
            let output = std::process::Command::new("sh")
                .arg(files._script.path())
                .output()?;
            assert!(output.status.success());
            let footer = String::from_utf8(output.stdout)?;
            assert!(footer.starts_with(marker));
            assert!(footer.contains("$(exit 9); 'literal'"));
            assert_eq!(files.checked()?, expected);
        }
        Ok(())
    }

    #[test]
    fn selection_modifier_preserves_mocked_selection_and_initial_state() -> Result<()> {
        let _guard = crate::menu_utils::MockQueue::new().select_index(1).guard();
        let (sender, receiver) = crossbeam_channel::unbounded();
        sender.send("one".to_owned())?;
        sender.send("two".to_owned())?;
        drop(sender);
        let outcome = FzfBuilder::new()
            .stream(receiver)
            .checkbox("Remember", true)
            .select()?;
        let DialogOutcome::Submitted((selection, checked)) = outcome else {
            panic!("Expected selection");
        };
        assert_eq!(selection.into_single()?, "two");
        assert!(checked);
        Ok(())
    }
}
