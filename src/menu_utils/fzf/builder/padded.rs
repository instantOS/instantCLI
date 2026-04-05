use anyhow::Result;
use std::io::Write;
use std::process::Command;

use crate::common::shell::shell_quote;

use super::FzfBuilder;
use super::shared::{FzfCommandOptions, build_padded_item_from_lines, run_fzf_with_input};
use crate::menu_utils::fzf::types::{FzfPreview, FzfResult, FzfSelectable, InitialCursor};
use crate::menu_utils::fzf::wrapper::check_fzf_exit;

impl FzfBuilder {
    pub fn select_padded<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return match resp {
                crate::menu_utils::mock::MockResponse::SelectIndex(i) => Ok(FzfResult::Selected(
                    items
                        .into_iter()
                        .nth(i)
                        .unwrap_or_else(|| panic!("MockResponse::SelectIndex({i}) out of bounds")),
                )),
                crate::menu_utils::mock::MockResponse::CancelSelection => Ok(FzfResult::Cancelled),
                other => panic!("Mock: expected select response, got {other:?}"),
            };
        }

        if items.is_empty() {
            return Ok(FzfResult::Cancelled);
        }

        let has_keywords = items
            .iter()
            .any(|item| !item.fzf_search_keywords().is_empty());

        let input_text = Self::prepare_padded_input(&items);
        let has_preview = items
            .iter()
            .any(|item| !matches!(item.fzf_preview(), FzfPreview::None));
        let preview_dir = if has_preview {
            Some(Self::prepare_padded_previews(&items)?)
        } else {
            None
        };

        let cmd = self.configure_padded_cmd(preview_dir.as_deref(), has_keywords);

        let output = run_fzf_with_input(cmd, input_text.as_bytes());

        if let Some(dir) = preview_dir.as_ref() {
            let _ = std::fs::remove_dir_all(dir);
        }

        match output {
            Ok(result) => {
                if let Some(cancelled) = check_fzf_exit(&result) {
                    return Ok(cancelled);
                }

                if !result.status.success() {
                    return Ok(FzfResult::Cancelled);
                }

                let stdout = String::from_utf8_lossy(&result.stdout);
                let index_str = stdout.trim();

                if index_str.is_empty() {
                    return Ok(FzfResult::Cancelled);
                }

                if let Ok(index) = index_str.parse::<usize>()
                    && let Some(item) = items.get(index)
                {
                    return Ok(FzfResult::Selected(item.clone()));
                }

                Ok(FzfResult::Cancelled)
            }
            Err(e) => Ok(FzfResult::Error(e.to_string())),
        }
    }

    fn prepare_padded_input<T: FzfSelectable>(items: &[T]) -> String {
        let mut input_lines = Vec::new();
        const HIDDEN_PADDING: &str = "                                                                                                    ";
        const EXTRA_WIDE_PADDING: &str = "                                                                                                                                                                                                                                                                    ";

        let has_previews = items
            .iter()
            .any(|item| !matches!(item.fzf_preview(), FzfPreview::None));

        for item in items {
            let display = item.fzf_display_text();
            let keywords = item.fzf_search_keywords().join(" ");

            let middle_line = if keywords.is_empty() {
                format!("  {display}")
            } else if has_previews {
                format!("  {display}{HIDDEN_PADDING}\x1f{keywords}")
            } else {
                format!("  {display}{EXTRA_WIDE_PADDING}\x1f{keywords}")
            };

            let padded_item = build_padded_item_from_lines(&display, &middle_line);
            input_lines.push(padded_item);
        }

        input_lines.join("\0")
    }

    fn prepare_padded_previews<T: FzfSelectable>(items: &[T]) -> Result<std::path::PathBuf> {
        let preview_dir = std::env::temp_dir().join(format!("fzf_preview_{}", std::process::id()));
        std::fs::create_dir_all(&preview_dir)?;

        for (idx, item) in items.iter().enumerate() {
            match item.fzf_preview() {
                FzfPreview::Text(preview) => {
                    let preview_path = preview_dir.join(format!("{}.txt", idx));
                    if let Ok(mut file) = std::fs::File::create(&preview_path) {
                        let _ = file.write_all(preview.as_bytes());
                    }
                }
                FzfPreview::Command(cmd) => {
                    let preview_path = preview_dir.join(format!("{}.sh", idx));
                    if let Ok(mut file) = std::fs::File::create(&preview_path) {
                        let key = shell_quote(&item.fzf_key());
                        let script = format!("set -- {key}\n{cmd}");
                        let _ = file.write_all(script.as_bytes());
                    }
                }
                FzfPreview::None => {}
            }
        }

        Ok(preview_dir)
    }

    fn configure_padded_cmd(
        &self,
        preview_dir: Option<&std::path::Path>,
        has_keywords: bool,
    ) -> Command {
        let mut cmd = self.base_fzf_command();

        cmd.arg("--read0");
        cmd.arg("--ansi");
        cmd.arg("--highlight-line");
        cmd.arg("--layout=reverse");
        cmd.arg("--tiebreak=index");
        cmd.arg("--info=inline-right");

        if has_keywords {
            cmd.arg("--delimiter=\x1f").arg("--no-hscroll");
        }

        cmd.arg("--bind").arg("enter:become(echo {n})");

        if let Some(dir) = preview_dir {
            let preview_cmd = format!(
                "if [ -f {dir}/{{n}}.sh ]; then bash {dir}/{{n}}.sh; elif [ -f {dir}/{{n}}.txt ]; then cat {dir}/{{n}}.txt; fi",
                dir = dir.display()
            );
            cmd.arg("--preview").arg(&preview_cmd);
        }

        let cursor = match self.initial_cursor {
            Some(InitialCursor::Index(index)) => Some(index),
            None => None,
        };
        self.apply_fzf_command_options(
            &mut cmd,
            FzfCommandOptions {
                prompt_suffix: Some(" > "),
                header: self.default_header_text(),
                include_additional_args: true,
                cursor,
                responsive_layout: true,
            },
        );

        cmd
    }
}

#[cfg(test)]
mod mock_tests {
    use crate::menu_utils::MockQueue;

    #[test]
    fn test_mock_select_padded_returns_canned_item() {
        let _guard = MockQueue::new().select_index(0).guard();
        let items = vec!["first".to_string(), "second".to_string()];
        let result = crate::menu_utils::FzfWrapper::builder()
            .select_padded(items)
            .unwrap();
        match result {
            crate::menu_utils::FzfResult::Selected(s) => assert_eq!(s, "first"),
            other => panic!("Expected Selected, got {other:?}"),
        }
    }
}
