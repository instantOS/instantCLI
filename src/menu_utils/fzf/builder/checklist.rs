use anyhow::Result;
use std::collections::HashMap;
use std::process::Output;

use super::shared::{
    FzfCommandOptions, apply_fzf_command_options, base_fzf_command, default_header_text,
    run_fzf_with_input,
};
use super::{ChecklistBuilder, ChecklistEntry};
use crate::menu_utils::fzf::preview::PreviewUtils;
use crate::menu_utils::fzf::types::{
    ChecklistAction, ChecklistConfirm, ChecklistItem, ChecklistResult, ChecklistSelection,
    ConfirmResult, FzfSelectable, ItemDisplayData,
};
use crate::menu_utils::fzf::wrapper::{FzfWrapper, check_fzf_exit, configure_preview_and_input};

impl ChecklistBuilder {
    pub fn checklist_dialog<T: FzfSelectable + Clone>(
        self,
        items: Vec<T>,
    ) -> Result<ChecklistResult<T>> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return match resp {
                crate::menu_utils::mock::MockResponse::ChecklistConfirm(indices) => {
                    let selected: Vec<T> = indices
                        .into_iter()
                        .map(|i| {
                            items
                                .iter()
                                .nth(i)
                                .unwrap_or_else(|| {
                                    panic!("MockResponse::ChecklistConfirm({i}) out of bounds")
                                })
                                .clone()
                        })
                        .collect();
                    Ok(ChecklistResult::Confirmed(selected))
                }
                crate::menu_utils::mock::MockResponse::ChecklistAction(key) => {
                    Ok(ChecklistResult::Action(
                        self.actions
                            .iter()
                            .find(|a| a.key == key)
                            .cloned()
                            .unwrap_or_else(|| {
                                panic!("MockResponse::ChecklistAction key '{key}' not found")
                            }),
                    ))
                }
                crate::menu_utils::mock::MockResponse::ChecklistCancelled => {
                    Ok(ChecklistResult::Cancelled)
                }
                other => panic!("Mock: expected checklist response, got {other:?}"),
            };
        }

        if items.is_empty() {
            return Ok(ChecklistResult::Cancelled);
        }

        let confirm_text = self.confirm_text.clone();
        let allow_empty = self.allow_empty;

        let mut checklist_items: Vec<ChecklistItem<T>> =
            items.into_iter().map(ChecklistItem::new).collect();
        let confirm_item = ChecklistConfirm::new(&confirm_text);

        let action_items = self.actions.clone();
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
        let mut cmd = base_fzf_command();
        cmd.arg("--ansi");
        cmd.arg("--tiebreak=index");
        cmd.arg("--layout=reverse");
        cmd.arg("--print-query");
        apply_fzf_command_options(
            &mut cmd,
            &self.shared,
            FzfCommandOptions {
                prompt_suffix: Some(" > "),
                header: default_header_text(&self.shared),
                include_additional_args: true,
                cursor,
                responsive_layout: true,
            },
        );

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

        let output = run_fzf_with_input(cmd, input_text.as_bytes())?;

        parse_checklist_output(output, key_to_index, action_map)
    }
}

fn parse_checklist_output(
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

#[cfg(test)]
mod mock_tests {
    use crate::menu_utils::MockQueue;

    #[test]
    fn test_mock_checklist_confirm_with_indices() {
        let _guard = MockQueue::new().checklist_confirm(vec![0, 2]).guard();
        let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let result = crate::menu_utils::FzfWrapper::builder()
            .checklist("Confirm")
            .checklist_dialog(items)
            .unwrap();
        match result {
            crate::menu_utils::ChecklistResult::Confirmed(selected) => {
                assert_eq!(selected, vec!["alpha", "gamma"]);
            }
            other => panic!("Expected Confirmed, got {other:?}"),
        }
    }
}
