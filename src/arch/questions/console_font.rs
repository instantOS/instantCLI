use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::arch::annotations::AnnotatedValue;
use crate::arch::console_font::ConsoleFont;
use crate::arch::engine::{InstallContext, StepId, StepOutcome, WizardStep};
use crate::common::shell::shell_quote;
use crate::menu_utils::{DialogOutcome, FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::nerd_font::NerdFont;

const FONT_DIRECTORY: &str = "/usr/share/kbd/consolefonts";

/// Restores the font and its Unicode map if a dialog errors or is cancelled.
struct FontSession {
    console: PathBuf,
    loader: PathBuf,
    original: tempfile::NamedTempFile,
    confirmed: bool,
}

fn set_font(loader: &Path, console: &Path, font: &Path) -> Result<()> {
    let output = Command::new(loader)
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
    fn start(console: PathBuf) -> Result<Self> {
        Self::start_with_loader(console, PathBuf::from("setfont"))
    }

    fn start_with_loader(console: PathBuf, loader: PathBuf) -> Result<Self> {
        let original = tempfile::Builder::new().suffix(".psf").tempfile()?;
        let output = Command::new(&loader)
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
            && let Err(error) = set_font(&self.loader, &self.console, self.original.path())
        {
            eprintln!("Warning: could not restore the previous console font: {error:#}");
        }
    }
}

#[derive(Clone)]
struct FontOption {
    font: ConsoleFont,
    preview: FzfPreview,
}

impl FzfSelectable for FontOption {
    fn fzf_display_text(&self) -> String {
        AnnotatedValue::new(
            self.font.label.to_string(),
            (self.font == ConsoleFont::DEFAULT).then(|| "Default".to_string()),
        )
        .fzf_display_text()
    }

    fn fzf_key(&self) -> String {
        self.font.name.to_string()
    }
    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

fn font_preview(font: ConsoleFont, session: Option<&FontSession>) -> FzfPreview {
    let information = format!(
        "{}\n\nFont: {}\n\n{}\n\nThe selected font will also be used by the installed system's TTYs.\n",
        font.label,
        font.name,
        if session.is_some() {
            "Use Up/Down to try fonts live. Enter keeps the selected font.\nEscape restores the previous font."
        } else {
            "Live preview is only available on a Linux console.\nThis choice does not change your graphical terminal's font."
        }
    );
    let information = if font.name == "latarcyrheb-sun32" {
        format!(
            "{information}\nThis font has 512 glyphs; some consoles lose bright colors with it.\n"
        )
    } else {
        information
    };
    match session {
        Some(session) => FzfPreview::Command(format!(
            "{} -C {} {} 2>&1; printf '%s' {}",
            shell_quote(&session.loader.to_string_lossy()),
            shell_quote(&session.console.to_string_lossy()),
            shell_quote(font.name),
            shell_quote(&information),
        )),
        None => FzfPreview::Text(information),
    }
}

pub struct ConsoleFontQuestion;

#[async_trait::async_trait]
impl WizardStep for ConsoleFontQuestion {
    fn id(&self) -> StepId {
        StepId::ConsoleFont
    }
    fn description(&self) -> Option<&str> {
        Some("Choose the font size for Linux TTYs")
    }
    fn preselect_answer(&self, _context: &InstallContext) -> Option<String> {
        Some(ConsoleFont::DEFAULT.name.to_string())
    }
    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        ConsoleFont::parse(answer)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn run(&self, context: &InstallContext) -> Result<StepOutcome> {
        let mut session = crate::ui::catppuccin::linux_console_device()
            .map(|console| FontSession::start(console.to_path_buf()))
            .transpose()
            .unwrap_or_else(|error| {
                eprintln!("Warning: live font preview is unavailable: {error:#}");
                None
            });
        let options: Vec<_> = ConsoleFont::ALL
            .iter()
            .copied()
            .filter(|font| {
                *font == ConsoleFont::DEFAULT || font.file_in(Path::new(FONT_DIRECTORY)).is_some()
            })
            .map(|font| FontOption {
                font,
                preview: font_preview(font, session.as_ref()),
            })
            .collect();
        let result = super::select_one_for_step(
            context,
            self,
            FzfWrapper::builder()
                .header(HeaderBuilder::new(NerdFont::Terminal, "Select TTY Font").build())
                .items(options),
        )?;
        match result {
            DialogOutcome::Submitted(option) => {
                if let Some(session) = session.as_mut() {
                    if let Err(error) = set_font(
                        &session.loader,
                        &session.console,
                        Path::new(option.font.name),
                    ) {
                        return Ok(StepOutcome::Retry(format!("{error:#}")));
                    }
                    session.confirmed = true;
                }
                Ok(StepOutcome::Answer(option.font.name.to_string()))
            }
            DialogOutcome::Cancelled => Ok(StepOutcome::Pause),
        }
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
            console: PathBuf::from("/dev/tty2"),
            loader,
            original: tempfile::NamedTempFile::new().unwrap(),
            confirmed: true, // The fixture must never attempt to restore a real console.
        };
        let FzfPreview::Command(command) =
            font_preview(ConsoleFont::parse("sun12x22").unwrap(), Some(&session))
        else {
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
        set_font(&session.loader, &session.console, Path::new("sun12x22")).unwrap();
        session.confirmed = true;
        drop(session);
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.ends_with("-C\n/dev/tty3\nsun12x22\n"));
        assert_eq!(calls.matches("/dev/tty3").count(), 2);
    }

    #[test]
    fn graphical_preview_has_no_console_side_effects_and_default_is_labelled() {
        let font = ConsoleFont::DEFAULT;
        let preview = font_preview(font, None);
        let FzfPreview::Text(text) = &preview else {
            panic!("GUI preview must be text")
        };
        assert!(text.contains("only available on a Linux console"));
        let option = FontOption { font, preview };
        assert!(option.fzf_display_text().contains("Default"));
        assert_eq!(option.fzf_key(), "default8x16");
        assert_eq!(
            ConsoleFontQuestion.preselect_answer(&InstallContext::new()),
            Some(option.fzf_key())
        );
    }
}
