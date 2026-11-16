mod instantmenu;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::model::{self, Application, Choice, DiscoveryBatch};
#[cfg(test)]
use crate::applications::app_info::ApplicationInfo;
use crate::applications::system::set_default_app;
use crate::common::shell::{current_exe_command, shell_quote};
use crate::menu_utils::{
    DialogOutcome, FilePickerBuilder, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder,
    MenuItem, MenuKey, MenuKeybind,
};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;

fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[derive(Clone, Debug)]
enum OpenEntry {
    App {
        app: Box<Application>,
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
                let color = if app.is_default() {
                    colors::GREEN
                } else if *recommended {
                    colors::BLUE
                } else {
                    colors::OVERLAY1
                };
                let mut text = match app.as_ref() {
                    Application::Desktop(app) => app.display_with_badge_color(color),
                    Application::Executable(executable) => format!(
                        "{} {} (custom · {})",
                        format_icon_colored(NerdFont::Application, color),
                        plain(&app.name()),
                        plain(&executable.to_string_lossy())
                    ),
                };
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
            Self::App { app, .. } => app.key(),
            Self::Error(_) => "__open_error__".into(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::App {
                app, path, mime, ..
            } => {
                let preview = match app.as_ref() {
                    Application::Desktop(app) => app.fzf_preview(),
                    Application::Executable(executable) => FzfPreview::Text(format!(
                        "Custom executable\nExecutable: {}\nOpens this item by passing its path as a single argument.",
                        executable.display()
                    )),
                };
                let mut command = match preview {
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
                let app_group = if candidate.app.is_default() {
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

#[derive(Clone, Copy)]
enum OpenAction {
    SetDefault,
    CustomExecutable,
    RunFile,
}

#[derive(Clone, Copy)]
pub(super) enum Backend {
    Terminal,
    Instantmenu,
}

impl Backend {
    fn confirm_execution(self, path: &Path, reason: &str) -> Result<bool> {
        use crate::menu_utils::ConfirmResult;
        let message = format!(
            "This file is unlikely to execute successfully.\n{reason}\n\nFile: {}\nMake it executable and try anyway?",
            path.display()
        );
        let response = match self {
            Self::Terminal => FzfWrapper::confirm(&message)?,
            Self::Instantmenu => crate::menu::instantmenu::InstantmenuBackend::confirm(&message)?,
        };
        Ok(response == ConfirmResult::Yes)
    }

    fn pick_executable(self) -> Result<DialogOutcome<PathBuf>> {
        let start = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/usr/bin"));
        match self {
            Self::Terminal => FilePickerBuilder::new()
                .start_dir(start)
                .hint("Choose an executable to open the item")
                .pick_one(),
            Self::Instantmenu => {
                use crate::menu::client::HostedMenuClient;
                use crate::menu_utils::FilePickerScope;
                match HostedMenuClient::new().file_picker(
                    Some(start.to_string_lossy().into_owned()),
                    FilePickerScope::Files,
                    false,
                )? {
                    DialogOutcome::Cancelled => Ok(DialogOutcome::Cancelled),
                    DialogOutcome::Submitted(paths) => Ok(DialogOutcome::Submitted(
                        paths
                            .into_iter()
                            .next()
                            .context("File picker returned no executable")?,
                    )),
                }
            }
        }
    }
}

pub(super) fn choose(path: &Path, backend: Backend) -> Result<Option<Choice>> {
    let mut status = String::new();
    loop {
        let Some(selection) = choose_action(path, &status, backend)? else {
            return Ok(None);
        };
        if matches!(selection.action, Some(OpenAction::RunFile)) {
            if path.is_file() {
                if let Some(reason) = super::executable::warning(path)?
                    && !backend.confirm_execution(path, &reason)?
                {
                    continue;
                }
                return Ok(Some(Choice::RunFile));
            }
            status = "Only regular files can be executed".into();
            continue;
        }
        if matches!(selection.action, Some(OpenAction::CustomExecutable)) {
            if let DialogOutcome::Submitted(path) = backend.pick_executable()? {
                match which::which(&path) {
                    Ok(executable) => {
                        let mime = selection.items.iter().find_map(|item| match item {
                            MenuItem::Entry(OpenEntry::App { mime, .. }) => Some(mime.clone()),
                            _ => None,
                        });
                        return Ok(Some(Choice::Executable { executable, mime }));
                    }
                    Err(error) => {
                        status = format!("Cannot execute {}: {error}", path.display());
                    }
                }
            }
            continue;
        }
        let Some(item) = selection.items.into_iter().next() else {
            continue;
        };
        match item {
            MenuItem::Entry(OpenEntry::App { app, mime, .. }) => match (*app, selection.action) {
                (Application::Desktop(app), Some(OpenAction::SetDefault)) => {
                    set_default_app(&mime, &app.desktop_id)
                        .context("Failed to set default application")?;
                    status = format!(
                        "Default set to {}",
                        app.name.as_deref().unwrap_or(&app.desktop_id)
                    );
                }
                (Application::Desktop(app), _) => return Ok(Some(Choice::Desktop { app, mime })),
                (Application::Executable(_), Some(OpenAction::SetDefault)) => {
                    status = "Custom executables have no desktop entry for an XDG default; press Enter to open".into();
                }
                (Application::Executable(executable), _) => {
                    return Ok(Some(Choice::Executable {
                        executable,
                        mime: Some(mime),
                    }));
                }
            },
            MenuItem::Entry(OpenEntry::Error(error)) if selection.action.is_none() => bail!(error),
            _ => {}
        }
    }
}

fn choose_action(
    path: &Path,
    status: &str,
    backend: Backend,
) -> Result<Option<crate::menu_utils::MenuSelection<MenuItem<OpenEntry>, OpenAction>>> {
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

    let keybinds = keybinds(path.is_file())?;
    if matches!(backend, Backend::Instantmenu) {
        return instantmenu::select(path, status, receiver, &keybinds);
    }
    let outcome = FzfWrapper::builder()
        .prompt("Search applications")
        .header(
            HeaderBuilder::new(NerdFont::ExternalLink, "Open With")
                .field("File", path.to_string_lossy())
                .subtitle(status)
                .build(),
        )
        .responsive_layout()
        .stream(receiver)
        .padded()
        .keybinds(&keybinds)
        .select()?;
    match outcome {
        DialogOutcome::Cancelled => Ok(None),
        DialogOutcome::Submitted(selection) => Ok(Some(selection)),
    }
}

fn keybinds(allow_execute: bool) -> Result<Vec<MenuKeybind<OpenAction>>> {
    let mut bindings = vec![
        MenuKeybind::new(
            MenuKey::new("ctrl-d")?,
            "set default",
            OpenAction::SetDefault,
        ),
        MenuKeybind::new(
            MenuKey::new("ctrl-e")?,
            "custom executable",
            OpenAction::CustomExecutable,
        ),
    ];
    if allow_execute {
        bindings.push(MenuKeybind::new(
            MenuKey::new("ctrl-x")?,
            "make executable and run",
            OpenAction::RunFile,
        ));
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::super::model::Candidate;
    use super::*;

    fn candidate(id: &str, default: bool, recommended: bool, score: f64) -> Candidate {
        Candidate {
            app: Application::Desktop(ApplicationInfo {
                desktop_id: id.into(),
                name: Some(id.into()),
                comment: None,
                icon: None,
                exec: Some("app %f".into()),
                is_default: default,
                path: Some(PathBuf::from("/apps/app.desktop")),
                mime_types: vec![],
            }),
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
