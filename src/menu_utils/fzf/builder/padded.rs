use anyhow::Result;
use std::io::Write;
use std::process::Command;

use base64::{Engine as _, engine::general_purpose};

use super::FzfBuilder;
use super::shared::{
    FzfCommandOptions, apply_fzf_command_options, base_fzf_command, build_padded_item_from_lines,
    default_header_text, run_fzf_with_input,
};
use crate::menu_utils::fzf::types::{DialogOutcome, FzfPreview, FzfSelectable, InitialCursor};
#[cfg(test)]
use crate::menu_utils::fzf::wrapper::NO_KEYBINDS;
use crate::menu_utils::fzf::wrapper::fzf_was_cancelled;

/// Invisible marker used to keep non-selectable padded rows visible while fzf
/// navigation only visits actual menu actions.
const SELECTABLE_MARKER: &str = "\u{2060}";

impl FzfBuilder {
    pub(crate) fn select_with_padded_presentation<T: FzfSelectable + Clone>(
        mut self,
        items: Vec<T>,
    ) -> Result<DialogOutcome<Vec<T>>> {
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            let selection = crate::menu_utils::mock::resolve_selection(resp, items, NO_KEYBINDS);
            return Ok(selection.map(|sel| sel.items));
        }

        if items.is_empty() {
            return Ok(DialogOutcome::Cancelled);
        }

        let has_non_selectable = items.iter().any(|item| !item.fzf_is_selectable());
        if has_non_selectable {
            let requested_index = self
                .shared
                .initial_cursor
                .as_ref()
                .map(|InitialCursor::Index(index)| *index);
            let Some(initial_index) = nearest_selectable_index(&items, requested_index) else {
                return Ok(DialogOutcome::Cancelled);
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
        let preview_manifest = if has_preview {
            Some(prepare_padded_preview_manifest(&items)?)
        } else {
            None
        };

        let result = loop {
            let cmd = configure_padded_cmd(
                &self,
                preview_manifest.as_ref().map(tempfile::NamedTempFile::path),
                has_keywords,
                has_non_selectable,
            );
            let output = run_fzf_with_input(cmd, input_text.as_bytes())?;

            if fzf_was_cancelled(&output)? {
                break DialogOutcome::Cancelled;
            }
            if !output.status.success() {
                break DialogOutcome::Cancelled;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let index = stdout
                .trim()
                .parse::<usize>()
                .map_err(|error| anyhow::anyhow!("fzf returned an invalid item index: {error}"))?;
            let item = items
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("fzf returned out-of-range item index {index}"))?;

            if item.fzf_is_selectable() {
                break DialogOutcome::Submitted(vec![item.clone()]);
            }
            // Pointer selection can still land on a raw, non-matching row.
            // Reopen instead of returning a header as if it were an action.
        };

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

fn prepare_padded_preview_manifest<T: FzfSelectable>(
    items: &[T],
) -> Result<tempfile::NamedTempFile> {
    let mut manifest = tempfile::NamedTempFile::new()?;
    for item in items {
        let (kind, content) = match item.fzf_preview() {
            FzfPreview::Text(text) => ("T", text),
            FzfPreview::Command(command) => ("C", command),
            FzfPreview::None => ("N", String::new()),
        };
        writeln!(
            manifest,
            "{kind}\t{}\t{}",
            general_purpose::STANDARD.encode(content),
            general_purpose::STANDARD.encode(item.fzf_key())
        )?;
    }
    Ok(manifest)
}

fn configure_padded_cmd(
    builder: &FzfBuilder,
    preview_manifest: Option<&std::path::Path>,
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

    if let Some(manifest) = preview_manifest {
        cmd.arg("--preview").arg(padded_preview_command(manifest));
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

fn padded_preview_command(manifest: &std::path::Path) -> String {
    format!(
        "row=$(sed -n \"$(({{n}} + 1))p\" {manifest}); \
         kind=$(printf '%s' \"$row\" | cut -f1); \
         content=$(printf '%s' \"$row\" | cut -f2 | base64 -d); \
         if [ \"$kind\" = C ]; then \
             key=$(printf '%s' \"$row\" | cut -f3 | base64 -d); \
             printf '%s' \"$content\" | bash -s -- \"$key\"; \
         elif [ \"$kind\" = T ]; then printf '%s' \"$content\"; fi",
        manifest = crate::common::shell::shell_quote(&manifest.display().to_string())
    )
}

#[cfg(test)]
mod mock_tests {
    use crate::menu_utils::MockQueue;
    use crate::menu_utils::{FzfPreview, FzfSelectable, MenuPresentation};

    #[derive(Clone)]
    struct Item {
        label: &'static str,
        selectable: bool,
    }

    #[derive(Clone)]
    struct PreviewItem {
        label: &'static str,
        preview: FzfPreview,
    }

    impl FzfSelectable for PreviewItem {
        fn fzf_display_text(&self) -> String {
            self.label.to_string()
        }

        fn fzf_preview(&self) -> FzfPreview {
            self.preview.clone()
        }
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
    fn padded_presentation_returns_selected_item() {
        let _guard = MockQueue::new().select_index(0).guard();
        let items = vec!["first".to_string(), "second".to_string()];
        let result = crate::menu_utils::FzfWrapper::builder()
            .presentation(MenuPresentation::Padded)
            .select(items)
            .unwrap();
        match result {
            crate::menu_utils::DialogOutcome::Submitted(s) => {
                assert_eq!(s.items, vec!["first".to_string()])
            }
            other => panic!("Expected Submitted, got {other:?}"),
        }
    }

    #[test]
    fn padded_previews_share_one_manifest() {
        let items = vec![
            PreviewItem {
                label: "text",
                preview: FzfPreview::Text("hello\nworld".to_string()),
            },
            PreviewItem {
                label: "command",
                preview: FzfPreview::Command("printf command".to_string()),
            },
            PreviewItem {
                label: "none",
                preview: FzfPreview::None,
            },
        ];

        let manifest = super::prepare_padded_preview_manifest(&items).unwrap();
        assert!(manifest.path().is_file());

        let rows = std::fs::read_to_string(manifest.path()).unwrap();
        let rows = rows.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), items.len());
        assert!(rows[0].starts_with("T\t"));
        assert!(rows[1].starts_with("C\t"));
        assert!(rows[2].starts_with("N\t"));
    }

    #[test]
    fn padded_preview_command_reads_selected_row_and_passes_key() {
        let items = vec![
            PreviewItem {
                label: "text",
                preview: FzfPreview::Text("first\npreview".to_string()),
            },
            PreviewItem {
                label: "command-key",
                preview: FzfPreview::Command("printf 'command:%s' \"$1\"".to_string()),
            },
            PreviewItem {
                label: "none",
                preview: FzfPreview::None,
            },
        ];
        let manifest = super::prepare_padded_preview_manifest(&items).unwrap();
        let command = super::padded_preview_command(manifest.path());

        let run_row = |index: usize| {
            std::process::Command::new("bash")
                .arg("-c")
                .arg(command.replace("{n}", &index.to_string()))
                .output()
                .unwrap()
        };

        let text = run_row(0);
        assert!(text.status.success());
        assert_eq!(text.stdout, b"first\npreview");

        let dynamic = run_row(1);
        assert!(dynamic.status.success());
        assert_eq!(dynamic.stdout, b"command:command-key");

        let none = run_row(2);
        assert!(none.status.success());
        assert!(none.stdout.is_empty());
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
