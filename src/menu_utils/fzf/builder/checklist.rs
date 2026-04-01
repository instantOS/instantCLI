use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use super::{ChecklistEntry, DialogType, FzfBuilder};
use crate::menu_utils::fzf::preview::PreviewUtils;
use crate::menu_utils::fzf::types::{
    ChecklistAction, ChecklistConfirm, ChecklistItem, ChecklistResult, ChecklistSelection,
    ConfirmResult, FzfSelectable, ItemDisplayData,
};
use crate::menu_utils::fzf::wrapper::{FzfWrapper, check_fzf_exit, configure_preview_and_input};

impl FzfBuilder {
    pub(super) fn execute_checklist<T: FzfSelectable + Clone>(
        self,
        items: Vec<T>,
    ) -> Result<ChecklistResult<T>> {
        if items.is_empty() {
            return Ok(ChecklistResult::Cancelled);
        }

        let (confirm_text, allow_empty) = match &self.dialog_type {
            DialogType::Checklist {
                confirm_text,
                allow_empty,
            } => (confirm_text.clone(), *allow_empty),
            _ => return Err(anyhow!("Not a checklist dialog")),
        };

        let mut checklist_items: Vec<ChecklistItem<T>> =
            items.into_iter().map(ChecklistItem::new).collect();
        let confirm_item = ChecklistConfirm::new(&confirm_text);

        let action_items = self.checklist_actions.clone();
        let mut action_map: HashMap<String, ChecklistAction> = HashMap::new();
        for action in &action_items {
            action_map.insert(action.fzf_key(), action.clone());
        }

        let mut cursor: Option<usize> = None;

        loop {
            let mut key_to_index: HashMap<String, usize> = HashMap::new();
            let mut entries = Vec::new();

            for (idx, item) in checklist_items.iter().enumerate() {
                let display = item.fzf_display_text();
                let key = item.fzf_key();
                let preview = item.fzf_preview();
                key_to_index.insert(key.clone(), idx);
                entries.push(ChecklistEntry::new(display, key, preview));
            }

            for action in &action_items {
                entries.push(ChecklistEntry::new(
                    action.fzf_display_text(),
                    action.fzf_key(),
                    action.fzf_preview(),
                ));
            }

            entries.push(ChecklistEntry::new(
                confirm_item.fzf_display_text(),
                confirm_item.fzf_key(),
                confirm_item.fzf_preview(),
            ));

            let result = self.run_checklist_fzf(&entries, &key_to_index, &action_map, cursor)?;

            match result {
                ChecklistSelection::Cancelled => return Ok(ChecklistResult::Cancelled),
                ChecklistSelection::EmptyQuery => {
                    let has_checked = checklist_items.iter().any(|item| item.checked);

                    if has_checked {
                        match FzfWrapper::builder()
                            .confirm("Discard selections and exit?")
                            .yes_text("Discard")
                            .no_text("Keep")
                            .confirm_dialog()?
                        {
                            ConfirmResult::Yes => return Ok(ChecklistResult::Cancelled),
                            ConfirmResult::No | ConfirmResult::Cancelled => continue,
                        }
                    } else {
                        continue;
                    }
                }
                ChecklistSelection::NotFound => {
                    FzfWrapper::message("No matching items found")?;
                    continue;
                }
                ChecklistSelection::Toggled(index) => {
                    if let Some(item) = checklist_items.get_mut(index) {
                        item.toggle();
                    }
                    cursor = Some(index);
                }
                ChecklistSelection::Confirmed => {
                    let checked_indices: Vec<usize> = checklist_items
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| item.checked)
                        .map(|(idx, _)| idx)
                        .collect();

                    if !allow_empty && checked_indices.is_empty() {
                        FzfWrapper::message("Please select at least one item before confirming.")?;
                        continue;
                    }

                    let checked: Vec<T> = checklist_items
                        .into_iter()
                        .enumerate()
                        .filter(|(idx, _)| checked_indices.contains(idx))
                        .map(|(_, item)| item.item)
                        .collect();

                    return Ok(ChecklistResult::Confirmed(checked));
                }
                ChecklistSelection::Action(key) => {
                    if let Some(action) = action_map.get(&key) {
                        return Ok(ChecklistResult::Action(action.clone()));
                    }
                }
            }
        }
    }

    fn run_checklist_fzf(
        &self,
        entries: &[ChecklistEntry],
        key_to_index: &HashMap<String, usize>,
        action_map: &HashMap<String, ChecklistAction>,
        cursor: Option<usize>,
    ) -> Result<ChecklistSelection> {
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");
        cmd.arg("--ansi");
        cmd.arg("--tiebreak=index");
        cmd.arg("--layout=reverse");
        cmd.arg("--print-query");

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }

        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header.to_fzf_string());
        }

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        if let Some(index) = cursor {
            cmd.arg("--bind").arg(format!("load:pos({})", index + 1));
        }

        if self.responsive_layout {
            let layout = super::super::utils::get_responsive_layout();
            cmd.arg(layout.preview_window);
            cmd.arg("--margin").arg(layout.margin);
        }

        let preview_strategy = PreviewUtils::analyze_preview_strategy(entries)?;
        let display_data: Vec<ItemDisplayData> = entries
            .iter()
            .map(|entry| ItemDisplayData {
                display_text: entry.fzf_display_text(),
                key: entry.fzf_key(),
                keywords: entry.fzf_search_keywords().join(" "),
                is_selectable: true,
            })
            .collect();
        let input_text =
            configure_preview_and_input(&mut cmd, preview_strategy, &display_data, false);

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(input_text.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        crate::menu::server::unregister_menu_process(pid);

        self.parse_checklist_output(output, key_to_index, action_map)
    }

    fn parse_checklist_output(
        &self,
        result: Output,
        key_to_index: &HashMap<String, usize>,
        action_map: &HashMap<String, ChecklistAction>,
    ) -> Result<ChecklistSelection> {
        let exit_code = result.status.code();

        if exit_code != Some(1) && check_fzf_exit::<()>(&result).is_some() {
            return Ok(ChecklistSelection::Cancelled);
        }

        let stdout = String::from_utf8_lossy(&result.stdout);
        let lines: Vec<&str> = stdout.trim_end().split('\n').collect();

        let query = lines.first().map(|s| s.trim()).unwrap_or("");
        let selected = lines.get(1).map(|s| s.trim()).unwrap_or("");

        if selected.is_empty() {
            if query.is_empty() {
                return Ok(ChecklistSelection::EmptyQuery);
            } else {
                return Ok(ChecklistSelection::NotFound);
            }
        }

        if let Some(key) = selected.split('\x1f').nth(2) {
            if key == ChecklistConfirm::confirm_key() {
                return Ok(ChecklistSelection::Confirmed);
            }

            if action_map.contains_key(key) {
                return Ok(ChecklistSelection::Action(key.to_string()));
            }

            if let Some(&index) = key_to_index.get(key) {
                return Ok(ChecklistSelection::Toggled(index));
            }
        }

        Ok(ChecklistSelection::NotFound)
    }
}
