use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::common::console_font::{ConsoleFont, DEFAULT_NAME, FONT_DIRECTORY, discover};
use crate::common::shell::shell_quote;
use crate::menu_utils::{DialogOutcome, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::nerd_font::NerdFont;

/// Restores the font and its Unicode map if a dialog errors or is cancelled.
pub(crate) struct FontSession {
    elevated: bool,
    console: PathBuf,
    loader: PathBuf,
    original: tempfile::NamedTempFile,
    confirmed: bool,
}

fn set_font(loader: &Path, console: &Path, font: &Path, elevated: bool) -> Result<()> {
    let output = loader_command(loader, elevated)
        .arg("-C")
        .arg(console)
        .arg(font)
        .output()
        .context("Could not run setfont")?;
    if !output.status.success() {
        bail!(
            "Could not load console font: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

impl FontSession {
    pub(crate) fn start(console: PathBuf, elevated: bool) -> Result<Self> {
        Self::save(console, PathBuf::from("setfont"), elevated)
    }

    pub(crate) fn confirm(&mut self) {
        self.confirmed = true;
    }

    #[cfg(test)]
    fn start_with_loader(console: PathBuf, loader: PathBuf) -> Result<Self> {
        Self::save(console, loader, false)
    }

    fn save(console: PathBuf, loader: PathBuf, elevated: bool) -> Result<Self> {
        let original = tempfile::Builder::new().suffix(".psf").tempfile()?;
        let output = loader_command(&loader, elevated)
            .arg("-C")
            .arg(&console)
            .arg("-O")
            .arg(original.path())
            .output()
            .context("Could not save the current console font")?;
        if !output.status.success() {
            bail!(
                "Could not save the current console font: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(Self {
            elevated,
            console,
            loader,
            original,
            confirmed: false,
        })
    }
}

impl Drop for FontSession {
    fn drop(&mut self) {
        if !self.confirmed
            && let Err(error) = set_font(
                &self.loader,
                &self.console,
                self.original.path(),
                self.elevated,
            )
        {
            eprintln!("Warning: could not restore the previous console font: {error:#}");
        }
    }
}

#[derive(Clone)]
struct FontOption {
    current: bool,
    font: ConsoleFont,
    path: PathBuf,
    preview: FzfPreview,
}

impl FzfSelectable for FontOption {
    fn fzf_display_text(&self) -> String {
        let label = if self.font.name() == DEFAULT_NAME {
            format!("Default - {}", self.font.label())
        } else {
            self.font.label()
        };
        if self.current {
            format!("{label} [current]")
        } else {
            label
        }
    }
    fn fzf_key(&self) -> String {
        self.font.name().to_string()
    }
    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

#[derive(Clone)]
enum FontChoice {
    Font(FontOption),
    AllFonts(usize),
    Back,
}

impl FzfSelectable for FontChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Font(option) => option.fzf_display_text(),
            Self::AllFonts(count) => format!("All fonts ({count} available)"),
            Self::Back => "Back to suggested fonts".to_string(),
        }
    }
    fn fzf_key(&self) -> String {
        match self {
            Self::Font(option) => option.fzf_key(),
            Self::AllFonts(_) => "__all_fonts__".to_string(),
            Self::Back => "__back__".to_string(),
        }
    }
    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Font(option) => option.fzf_preview(),
            Self::AllFonts(_) => FzfPreview::Text("Browse every available console font, including custom fonts.\n\nFont sizes are read from the files. The selected font is saved for Linux TTYs.".to_string()),
            Self::Back => FzfPreview::Text("Return to the suggested fonts.".to_string()),
        }
    }
}

fn font_preview(font: &ConsoleFont, path: &Path, session: Option<&FontSession>) -> FzfPreview {
    let mut information = format!(
        "{}\n\nFont: {}\n\n{}\n\nThe selected font will be used by Linux TTYs, including after reboot.\n",
        font.label(),
        font.name(),
        if session.is_some() {
            "Use Up/Down to try fonts live. Enter keeps the selected font.\nEscape restores the previous font."
        } else {
            "Live preview is only available on a Linux console.\nThis choice does not change your graphical terminal's font."
        }
    );
    if session.is_some() {
        information.push_str("\nThe quick brown fox jumps over the lazy dog.\nABCDEFGHIJKLMNOPQRSTUVWXYZ\nabcdefghijklmnopqrstuvwxyz\n0123456789  !@#$%&*()[]{}<>/?\n");
    }
    if let Some(info) = font.info() {
        information.push_str(&format!(
            "\nSize: {}x{} pixels\nGlyphs: {}\n",
            info.width, info.height, info.glyphs
        ));
        if info.glyphs > 256 {
            information.push_str(
                "\nThis font has more than 256 glyphs; some consoles lose bright colors with it.\n",
            );
        }
    }
    match session {
        Some(session) => FzfPreview::Command(format!(
            "{}{} -C {} {} 2>&1; printf '%s' {}",
            if session.elevated { "sudo -n -- " } else { "" },
            shell_quote(&session.loader.to_string_lossy()),
            shell_quote(&session.console.to_string_lossy()),
            shell_quote(&path.to_string_lossy()),
            shell_quote(&information),
        )),
        None => FzfPreview::Text(information),
    }
}

fn menu_choices(options: &[FontOption], all_fonts: bool) -> Vec<FontChoice> {
    if all_fonts {
        let mut choices: Vec<_> = options.iter().cloned().map(FontChoice::Font).collect();
        choices.push(FontChoice::Back);
        choices
    } else {
        let mut choices: Vec<_> = options
            .iter()
            .filter(|option| option.current || option.font.suggested_rank().is_some())
            .cloned()
            .map(FontChoice::Font)
            .collect();
        choices.push(FontChoice::AllFonts(options.len()));
        choices
    }
}

fn select_font(
    mut fonts: Vec<ConsoleFont>,
    previous: Option<&str>,
    session: Option<&FontSession>,
) -> Result<DialogOutcome<FontOption>> {
    let previous = previous.and_then(|answer| {
        fonts
            .iter()
            .find(|font| font.name() == answer)
            .cloned()
            .or_else(|| ConsoleFont::parse(answer).ok())
    });
    if let Some(previous) = previous.as_ref().filter(|font| font.data().is_some()) {
        // An imported/resumed snapshot may no longer exist on this live ISO.
        // Keep its exact bytes selectable and preselect it, including in All fonts.
        match fonts.iter().position(|font| font.name() == previous.name()) {
            Some(index) => fonts[index] = previous.clone(),
            None => fonts.push(previous.clone()),
        }
    }
    let preview_directory = tempfile::tempdir().context("Preparing font previews")?;
    let options: Vec<_> = fonts
        .into_iter()
        .enumerate()
        .map(|(index, font)| {
            let path = font.preview_path(preview_directory.path(), index)?;
            let preview = font_preview(&font, &path, session);
            Ok(FontOption {
                current: previous
                    .as_ref()
                    .is_some_and(|previous| previous.name() == font.name()),
                font,
                path,
                preview,
            })
        })
        .collect::<Result<_>>()?;
    let initial_name = previous.as_ref().map_or(DEFAULT_NAME, |font| font.name());
    // Keep a custom current font in the compact menu alongside suggestions.
    let mut all_fonts = false;
    loop {
        let choices = menu_choices(&options, all_fonts);
        let index = choices
            .iter()
            .position(|choice| choice.fzf_key() == initial_name)
            .unwrap_or(0);
        let outcome = FzfWrapper::builder()
            .header(
                HeaderBuilder::new(
                    NerdFont::Terminal,
                    if all_fonts {
                        "All TTY Fonts"
                    } else {
                        "Select TTY Font"
                    },
                )
                .build(),
            )
            .items(choices)
            .initial_index(index)
            .select_one()?;
        match outcome {
            DialogOutcome::Submitted(FontChoice::Font(option)) => {
                // Reapply inside this scope: the staged PSF and preview command
                // files still exist here, and the final choice wins over previews.
                if let Some(session) = session {
                    set_font(
                        &session.loader,
                        &session.console,
                        &option.path,
                        session.elevated,
                    )?;
                }
                return Ok(DialogOutcome::Submitted(option));
            }
            DialogOutcome::Submitted(FontChoice::AllFonts(_)) => all_fonts = true,
            DialogOutcome::Submitted(FontChoice::Back) => all_fonts = false,
            DialogOutcome::Cancelled => return Ok(DialogOutcome::Cancelled),
        }
    }
}

fn loader_command(loader: &Path, elevated: bool) -> Command {
    if elevated {
        let mut command = Command::new("sudo");
        // Foreground loads and restoration run after the picker closes, so
        // reauthentication is safe if sudo credentials expired while browsing.
        // Preview commands separately use -n to avoid prompts inside fzf.
        command.arg("--").arg(loader);
        command
    } else {
        Command::new(loader)
    }
}

/// Keep the session alive until the caller has successfully saved the choice.
/// Dropping it before confirmation restores the font shown before the picker.
pub(crate) fn pick_font(
    previous: Option<&str>,
    session: Option<&FontSession>,
) -> Result<DialogOutcome<ConsoleFont>> {
    match select_font(discover(Path::new(FONT_DIRECTORY))?, previous, session)? {
        DialogOutcome::Submitted(option) => Ok(DialogOutcome::Submitted(option.font)),
        DialogOutcome::Cancelled => Ok(DialogOutcome::Cancelled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_preview_loads_the_highlighted_font_and_displays_failures() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let loader = directory.path().join("setfont");
        std::fs::write(
            &loader,
            "#!/bin/sh\nprintf '%s\\n' \"$@\"\nprintf 'fixture load failed\\n' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&loader, std::fs::Permissions::from_mode(0o755)).unwrap();
        let session = FontSession {
            elevated: false,
            console: PathBuf::from("/dev/tty2"),
            loader,
            original: tempfile::NamedTempFile::new().unwrap(),
            confirmed: true, // The fixture must never attempt to restore a real console.
        };
        let FzfPreview::Command(command) = font_preview(
            &ConsoleFont::parse("sun12x22").unwrap(),
            Path::new("sun12x22"),
            Some(&session),
        ) else {
            panic!("console preview must execute a command")
        };
        let output = Command::new("sh").arg("-c").arg(command).output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("-C\n/dev/tty2\nsun12x22\n"));
        assert!(text.contains("fixture load failed"));
        assert!(text.contains("Use Up/Down"));
    }

    #[test]
    fn cancelled_sessions_restore_saved_font_and_confirmed_sessions_keep_selection() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("calls");
        let loader = directory.path().join("setfont");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\n",
            shell_quote(&log.to_string_lossy())
        );
        std::fs::write(&loader, script).unwrap();
        std::fs::set_permissions(&loader, std::fs::Permissions::from_mode(0o755)).unwrap();
        let session =
            FontSession::start_with_loader(PathBuf::from("/dev/tty3"), loader.clone()).unwrap();
        let original = session.original.path().to_string_lossy().to_string();
        drop(session);
        let calls = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            calls,
            format!("-C\n/dev/tty3\n-O\n{original}\n-C\n/dev/tty3\n{original}\n")
        );

        std::fs::write(&log, "").unwrap();
        let mut session =
            FontSession::start_with_loader(PathBuf::from("/dev/tty3"), loader).unwrap();
        set_font(
            &session.loader,
            &session.console,
            Path::new("sun12x22"),
            false,
        )
        .unwrap();
        session.confirmed = true;
        drop(session);
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.ends_with("-C\n/dev/tty3\nsun12x22\n"));
        assert_eq!(calls.matches("/dev/tty3").count(), 2);
    }

    #[test]
    fn all_fonts_can_select_custom_fonts_and_preserve_previous_snapshots() {
        use crate::menu_utils::{MockQueue, scripted_responses_remaining};
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("custom.psf"),
            crate::common::console_font::tests::psf1(),
        )
        .unwrap();
        let fonts = discover(directory.path()).unwrap();
        let _mock = MockQueue::new().select_index(1).select_index(1).guard();
        let DialogOutcome::Submitted(option) = select_font(fonts, None, None).unwrap() else {
            panic!("expected custom font")
        };
        assert_eq!(option.font.name(), "custom");
        assert_eq!(scripted_responses_remaining(), 0);
        let answer = option.font.to_answer().unwrap();
        std::fs::remove_file(directory.path().join("custom.psf")).unwrap();
        let _mock = MockQueue::new().select_index(1).guard();
        let DialogOutcome::Submitted(resumed) =
            select_font(discover(directory.path()).unwrap(), Some(&answer), None).unwrap()
        else {
            panic!("expected saved font")
        };
        assert_eq!(resumed.font, option.font);
    }

    #[test]
    fn configured_custom_font_is_marked_current_in_suggested_menu() {
        use crate::menu_utils::MockQueue;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("custom.psf"),
            crate::common::console_font::tests::psf1(),
        )
        .unwrap();
        let _mock = MockQueue::new().select_index(1).guard();
        let DialogOutcome::Submitted(option) =
            select_font(discover(directory.path()).unwrap(), Some("custom"), None).unwrap()
        else {
            panic!("expected current custom font")
        };
        assert_eq!(option.font.name(), "custom");
        assert!(option.fzf_display_text().contains("[current]"));
    }

    #[test]
    fn picker_cancellation_returns_without_a_selection() {
        let _mock = crate::menu_utils::MockQueue::new()
            .cancel_selection()
            .guard();
        assert!(matches!(
            select_font(vec![ConsoleFont::default()], None, None).unwrap(),
            DialogOutcome::Cancelled
        ));
    }

    #[test]
    fn graphical_preview_has_no_console_side_effects_and_default_is_labelled() {
        let font = ConsoleFont::default();
        let preview = font_preview(&font, Path::new(DEFAULT_NAME), None);
        let FzfPreview::Text(text) = &preview else {
            panic!("GUI preview must be text")
        };
        assert!(text.contains("only available on a Linux console"));
        let option = FontOption {
            current: false,
            font,
            preview,
            path: PathBuf::from(DEFAULT_NAME),
        };
        assert!(option.fzf_display_text().contains("Default"));
        assert_eq!(option.fzf_key(), "default8x16");
    }
}
