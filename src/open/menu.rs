use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::model::{self, Choice, DiscoveryBatch};
use crate::applications::app_info::ApplicationInfo;
use crate::common::shell::{current_exe_command, shell_quote};
use crate::menu_utils::{
    DialogOutcome, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder, MenuItem,
};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;

#[derive(Clone, Debug)]
enum OpenEntry {
    App {
        app: Box<ApplicationInfo>,
        mime: String,
        path: PathBuf,
        recent: bool,
        recommended: bool,
    },
    Error(String),
}

impl FzfSelectable for OpenEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::App {
                app,
                recent,
                recommended,
                ..
            } => {
                let color = if app.is_default {
                    colors::GREEN
                } else if *recommended {
                    colors::BLUE
                } else {
                    colors::OVERLAY1
                };
                let mut text = app.display_with_badge_color(color);
                if *recent {
                    text.push_str("  ↗ recent");
                }
                text
            }
            Self::Error(error) => format!(
                "{} {error}",
                format_icon_colored(NerdFont::Warning, colors::RED)
            ),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::App { app, .. } => app.desktop_id.clone(),
            Self::Error(_) => "__open_error__".into(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::App {
                app, path, mime, ..
            } => {
                let mut command = match app.fzf_preview() {
                    FzfPreview::Command(command) => command,
                    FzfPreview::Text(details) => {
                        format!("printf '%s\\n' {}", shell_quote(&details))
                    }
                    FzfPreview::None => return FzfPreview::None,
                };
                if let Some(path) = path.to_str() {
                    command.push_str(&format!(
                        "\n{} preview --id file-suggestion --key {} --mime-type {}",
                        current_exe_command(),
                        shell_quote(path),
                        shell_quote(mime)
                    ));
                }
                FzfPreview::Command(command)
            }
            Self::Error(error) => FzfPreview::Text(error.clone()),
        }
    }
}

/// Pin the current default, then rank the remaining groups by MIME-specific usage.
/// Sending the default first also gives an immediately correct streaming cursor.
fn entries(
    snapshot: DiscoveryBatch,
    path: &Path,
    groups: &mut [bool; 3],
) -> Vec<MenuItem<OpenEntry>> {
    let mut rows = Vec::new();
    if let Some(warning) = snapshot.warning {
        rows.push(MenuItem::separator(&warning));
    }
    for (label, group) in [
        ("Current default", 0),
        ("Recommended · frequently and recently used", 1),
        ("Other applications", 2),
    ] {
        let mut applications = snapshot
            .apps
            .iter()
            .filter(|candidate| {
                let app_group = if candidate.app.is_default {
                    0
                } else if candidate.recommended {
                    1
                } else {
                    2
                };
                app_group == group
            })
            .peekable();
        if applications.peek().is_none() {
            continue;
        }
        if !groups[group] {
            rows.push(MenuItem::separator(label));
            groups[group] = true;
        }
        rows.extend(applications.map(|candidate| {
            MenuItem::entry(OpenEntry::App {
                app: Box::new(candidate.app.clone()),
                mime: snapshot.mime.clone(),
                path: path.to_owned(),
                recent: candidate.score > 0.0,
                recommended: candidate.recommended,
            })
        }));
    }

    rows
}

pub(super) fn choose(path: &Path) -> Result<Option<Choice>> {
    let (sender, receiver) = crossbeam_channel::bounded(128);
    let worker_path = path.to_owned();
    std::thread::Builder::new()
        .name("open-discovery".into())
        .spawn(move || {
            let mut groups = [false; 3];
            let mut any = false;
            let result = (|| {
                let mime = model::detect_mime(&worker_path)?;
                model::discover(mime, |snapshot| {
                    for row in entries(snapshot, &worker_path, &mut groups) {
                        any |= matches!(row, MenuItem::Entry(_));
                        if sender.send(row).is_err() {
                            return false;
                        }
                    }
                    true
                })
            })();
            let error = match result {
                Err(error) => Some(format!("{error:#}")),
                Ok(()) if !any => Some("No installed desktop applications found".into()),
                Ok(()) => None,
            };
            if let Some(error) = error {
                let _ = sender.send(MenuItem::entry(OpenEntry::Error(error)));
            }
        })
        .context("Failed to start application discovery")?;

    let outcome = FzfWrapper::builder()
        .prompt("Search applications")
        .header(
            HeaderBuilder::new(NerdFont::ExternalLink, "Open With")
                .field("File", path.to_string_lossy())
                .build(),
        )
        .responsive_layout()
        .stream(receiver)
        .padded()
        .checkbox("Use as default for this file type", false)
        .hint("use as default")
        .select()?;
    match outcome {
        DialogOutcome::Cancelled => Ok(None),
        DialogOutcome::Submitted((selection, use_default)) => match selection.into_single()? {
            MenuItem::Entry(OpenEntry::App { app, mime, .. }) => Ok(Some(Choice {
                app: *app,
                mime,
                use_default,
            })),
            MenuItem::Entry(OpenEntry::Error(error)) => bail!(error),
            MenuItem::Separator(_) => bail!("Menu submitted a non-selectable separator"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::Candidate;
    use super::*;

    fn candidate(id: &str, default: bool, recommended: bool, score: f64) -> Candidate {
        Candidate {
            app: ApplicationInfo {
                desktop_id: id.into(),
                name: Some(id.into()),
                comment: None,
                icon: None,
                exec: Some("app %f".into()),
                is_default: default,
                path: Some(PathBuf::from("/apps/app.desktop")),
                mime_types: vec![],
            },
            recommended,
            score,
        }
    }

    #[test]
    fn default_is_first_selectable_while_other_groups_keep_frecency_order() {
        let rows = entries(
            DiscoveryBatch {
                mime: "text/plain".into(),
                warning: None,
                apps: vec![
                    candidate("recent", false, true, 3.0),
                    candidate("default", true, true, 1.0),
                    candidate("other", false, false, 0.0),
                ],
            },
            Path::new("/tmp/a file.txt"),
            &mut [false; 3],
        );
        let ids = rows
            .iter()
            .filter(|row| row.fzf_is_selectable())
            .map(FzfSelectable::fzf_key)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["default", "recent", "other"]);
        assert_eq!(
            rows.iter().filter(|row| !row.fzf_is_selectable()).count(),
            3
        );
    }

    #[test]
    fn preview_reuses_application_details_and_shared_file_renderer() {
        let app = candidate("example", false, true, 0.0).app;
        let preview = OpenEntry::App {
            app: Box::new(app),
            mime: "text/plain".into(),
            path: PathBuf::from("/tmp/a 'file'; $literal"),
            recent: false,
            recommended: true,
        }
        .fzf_preview();
        let FzfPreview::Command(command) = preview else {
            panic!("Expected async command preview");
        };
        assert!(command.contains("preview --id file-suggestion --key"));
        assert!(command.contains("preview --id desktop-application --key"));
        assert!(command.contains("--mime-type text/plain"));
        assert!(command.contains("'\\''"));
    }
}
