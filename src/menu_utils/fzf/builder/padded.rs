use anyhow::Result;
use std::io::Write;
use std::process::Command;

use crate::common::shell::shell_quote;

use super::FzfBuilder;
use super::shared::{
    FzfCommandOptions, apply_fzf_command_options, base_fzf_command, build_padded_item_from_lines,
    default_header_text, run_fzf_with_input,
};
use crate::menu_utils::fzf::types::{FzfPreview, FzfResult, FzfSelectable, InitialCursor};
use crate::menu_utils::fzf::wrapper::check_fzf_exit;

/// Invisible marker used to keep non-selectable padded rows visible while fzf
/// navigation only visits actual menu actions.
const SELECTABLE_MARKER: &str = "\u{2060}";

impl FzfBuilder {
    pub fn select_padded<T: FzfSelectable + Clone>(
        mut self,
        items: Vec<T>,
    ) -> Result<FzfResult<T>> {
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

        let has_non_selectable = items.iter().any(|item| !item.fzf_is_selectable());
        if has_non_selectable {
            let requested_index = self
                .shared
                .initial_cursor
                .as_ref()
                .map(|InitialCursor::Index(index)| *index);
            let Some(initial_index) = nearest_selectable_index(&items, requested_index) else {
                return Ok(FzfResult::Cancelled);
            };
            self.shared.initial_cursor = Some(InitialCursor::Index(initial_index));
        }

        let has_keywords = items
            .iter()
            .any(|item| !item.fzf_search_keywords().is_empty());

        let input_text = prepare_padded_input(&items, has_non_selectable);
        let has_preview = items
            .iter()
            .any(|item| !matches!(item.fzf_preview(), FzfPreview::None));
        let preview_dir = if has_preview {
            Some(prepare_padded_previews(&items)?)
        } else {
            None
        };

        let result = loop {
            let cmd = configure_padded_cmd(
                &self,
                preview_dir.as_deref(),
                has_keywords,
                has_non_selectable,
            );
            let output = match run_fzf_with_input(cmd, input_text.as_bytes()) {
                Ok(output) => output,
                Err(error) => break FzfResult::Error(error.to_string()),
            };

            if let Some(cancelled) = check_fzf_exit(&output) {
                break cancelled;
            }
            if !output.status.success() {
                break FzfResult::Cancelled;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let Some(index) = stdout.trim().parse::<usize>().ok() else {
                break FzfResult::Cancelled;
            };
            let Some(item) = items.get(index) else {
                break FzfResult::Cancelled;
            };

            if item.fzf_is_selectable() {
                break FzfResult::Selected(item.clone());
            }
            // Pointer selection can still land on a raw, non-matching row.
            // Reopen instead of returning a header as if it were an action.
        };

        if let Some(dir) = preview_dir.as_ref() {
            let _ = std::fs::remove_dir_all(dir);
        }

        Ok(result)
    }
}

fn nearest_selectable_index<T: FzfSelectable>(
    items: &[T],
    requested_index: Option<usize>,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }

    let requested = requested_index.unwrap_or(0).min(items.len() - 1);
    if items[requested].fzf_is_selectable() {
        return Some(requested);
    }
    if let Some(offset) = items[requested..]
        .iter()
        .position(FzfSelectable::fzf_is_selectable)
    {
        return Some(requested + offset);
    }
    items[..requested]
        .iter()
        .rposition(FzfSelectable::fzf_is_selectable)
}

fn prepare_padded_input<T: FzfSelectable>(items: &[T], mark_selectable: bool) -> String {
    let mut input_lines = Vec::new();
    const HIDDEN_PADDING: &str = "                                                                                                    ";
    const EXTRA_WIDE_PADDING: &str = "                                                                                                                                                                                                                                                                    ";

    let has_previews = items
        .iter()
        .any(|item| !matches!(item.fzf_preview(), FzfPreview::None));

    for item in items {
        let display = item.fzf_display_text();
        let keywords = item.fzf_search_keywords().join(" ");

        let mut middle_line = if keywords.is_empty() {
            format!("  {display}")
        } else if has_previews {
            format!("  {display}{HIDDEN_PADDING}\x1f{keywords}")
        } else {
            format!("  {display}{EXTRA_WIDE_PADDING}\x1f{keywords}")
        };
        if mark_selectable && item.fzf_is_selectable() {
            middle_line = format!("{SELECTABLE_MARKER}{middle_line}");
        }

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
    builder: &FzfBuilder,
    preview_dir: Option<&std::path::Path>,
    has_keywords: bool,
    has_non_selectable: bool,
) -> Command {
    let mut cmd = base_fzf_command();

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

    if has_non_selectable {
        cmd.arg("--raw")
            .arg(format!("--query={SELECTABLE_MARKER}"))
            .arg("--gutter-raw= ")
            .arg("--bind")
            .arg(
                [
                    "up:up-match",
                    "down:down-match",
                    "ctrl-p:up-match",
                    "ctrl-n:down-match",
                    "ctrl-k:up-match",
                    "ctrl-j:down-match",
                    "result:best",
                ]
                .join(","),
            );
    }

    if let Some(dir) = preview_dir {
        let preview_cmd = format!(
            "if [ -f {dir}/{{n}}.sh ]; then bash {dir}/{{n}}.sh; elif [ -f {dir}/{{n}}.txt ]; then cat {dir}/{{n}}.txt; fi",
            dir = dir.display()
        );
        cmd.arg("--preview").arg(&preview_cmd);
    }

    let cursor = builder
        .shared
        .initial_cursor
        .as_ref()
        .map(|InitialCursor::Index(index)| *index);
    apply_fzf_command_options(
        &mut cmd,
        &builder.shared,
        FzfCommandOptions {
            prompt_suffix: Some(" > "),
            header: default_header_text(&builder.shared),
            include_additional_args: true,
            cursor,
            responsive_layout: true,
        },
    );

    cmd
}

#[cfg(test)]
mod mock_tests {
    use crate::menu_utils::FzfSelectable;
    use crate::menu_utils::MockQueue;

    #[derive(Clone)]
    struct Item {
        label: &'static str,
        selectable: bool,
    }

    impl FzfSelectable for Item {
        fn fzf_display_text(&self) -> String {
            self.label.to_string()
        }

        fn fzf_is_selectable(&self) -> bool {
            self.selectable
        }
    }

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

    #[test]
    fn initial_cursor_skips_non_selectable_rows() {
        let items = vec![
            Item {
                label: "header",
                selectable: false,
            },
            Item {
                label: "first action",
                selectable: true,
            },
            Item {
                label: "second action",
                selectable: true,
            },
        ];

        assert_eq!(super::nearest_selectable_index(&items, None), Some(1));
        assert_eq!(super::nearest_selectable_index(&items, Some(0)), Some(1));
        assert_eq!(super::nearest_selectable_index(&items, Some(2)), Some(2));
    }

    #[test]
    fn no_cursor_exists_when_every_row_is_non_selectable() {
        let items = vec![Item {
            label: "header",
            selectable: false,
        }];

        assert_eq!(super::nearest_selectable_index(&items, None), None);
    }
}
