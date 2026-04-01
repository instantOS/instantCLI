use anyhow::Result;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::common::shell::shell_quote;

use super::FzfBuilder;
use crate::menu_utils::fzf::types::{FzfPreview, FzfResult, FzfSelectable, InitialCursor};
use crate::menu_utils::fzf::utils::extract_icon_padding;
use crate::menu_utils::fzf::wrapper::check_fzf_exit;

impl FzfBuilder {
    pub fn select_padded<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
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

        let mut cmd = self.configure_padded_cmd(preview_dir.as_deref(), has_keywords);

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

        let output = child.wait_with_output();
        crate::menu::server::unregister_menu_process(pid);

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
            Err(e) => {
                super::super::utils::check_fzf_spawn_error_and_exit(&e);
                Ok(FzfResult::Error(format!("fzf execution failed: {e}")))
            }
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
            let (top_padding, bottom_with_shadow) = extract_icon_padding(&display);
            let keywords = item.fzf_search_keywords().join(" ");

            let middle_line = if keywords.is_empty() {
                format!("  {display}")
            } else if has_previews {
                format!("  {display}{HIDDEN_PADDING}\x1f{keywords}")
            } else {
                format!("  {display}{EXTRA_WIDE_PADDING}\x1f{keywords}")
            };

            let padded_item = format!("{top_padding}\n{middle_line}\n{bottom_with_shadow}");
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
        let mut cmd = Command::new("fzf");
        cmd.env_remove("FZF_DEFAULT_OPTS");

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

        if let Some(prompt) = &self.prompt {
            cmd.arg("--prompt").arg(format!("{prompt} > "));
        }
        if let Some(header) = &self.header {
            cmd.arg("--header").arg(header.to_fzf_string());
        }

        if let Some(InitialCursor::Index(index)) = self.initial_cursor {
            cmd.arg("--bind").arg(format!("load:pos({})", index + 1));
        }

        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        if self.responsive_layout {
            let layout = super::super::utils::get_responsive_layout();
            cmd.arg(layout.preview_window);
            cmd.arg("--margin").arg(layout.margin);
        }

        cmd
    }
}
