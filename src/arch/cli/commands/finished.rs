use anyhow::Result;

use crate::arch::cli::DEFAULT_QUESTIONS_FILE;
use crate::arch::engine::build_install_summary;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, Header, MenuPresentation};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// The three actions offered after installation completes.
#[derive(Clone)]
enum FinishedMenuOption {
    Reboot,
    Shutdown,
    Continue,
}

impl FinishedMenuOption {
    fn icon(&self) -> (&'static str, NerdFont) {
        match self {
            Self::Reboot => (colors::GREEN, NerdFont::Reboot),
            Self::Shutdown => (colors::RED, NerdFont::PowerOff),
            Self::Continue => (colors::BLUE, NerdFont::Continue),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Reboot => "Reboot",
            Self::Shutdown => "Shutdown",
            Self::Continue => "Continue in Live Session",
        }
    }

    fn description_lines(&self) -> &'static [&'static str] {
        match self {
            Self::Reboot => &[
                "Restart the system and boot into your",
                "newly installed instantOS system.",
            ],
            Self::Shutdown => &[
                "Power off the system. Boot into your",
                "new installation when you are ready.",
            ],
            Self::Continue => &[
                "Return to the live environment without",
                "rebooting or powering off.",
            ],
        }
    }
}

/// Wrapper that pairs a menu option with its pre-built preview.
#[derive(Clone)]
struct FinishedMenuItem {
    option: FinishedMenuOption,
    preview: FzfPreview,
}

impl FzfSelectable for FinishedMenuItem {
    fn fzf_display_text(&self) -> String {
        let (color, icon) = self.option.icon();
        format!(
            "{} {}",
            format_icon_colored(icon, color),
            self.option.label()
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

/// Format the installation duration as `HH:MM:SS`.
fn format_duration(state: &crate::arch::execution::state::InstallState) -> Option<String> {
    let start = state.start_time?;
    let elapsed = chrono::Utc::now() - start;
    let hours = elapsed.num_hours();
    let minutes = elapsed.num_minutes() % 60;
    let seconds = elapsed.num_seconds() % 60;
    Some(format!("{hours:02}:{minutes:02}:{seconds:02}"))
}

/// Query actual disk usage on `/mnt` via `df`.
fn query_storage_used() -> Option<String> {
    let output = std::process::Command::new("df")
        .arg("-h")
        .arg("/mnt")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().nth(1)?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    (parts.len() >= 3).then(|| parts[2].to_string())
}

/// Load the full install configuration summary text.
fn load_install_summary() -> Option<String> {
    let context = crate::arch::engine::InstallContext::load(DEFAULT_QUESTIONS_FILE).ok()?;
    Some(build_install_summary(&context).text)
}

/// Build the preview for a finished-menu option.
///
/// Each preview shows the action description at the top, followed by runtime
/// results (duration, storage) and the full installation configuration
/// summary so the user can confirm the install regardless of which option they
/// hover over.
fn build_finished_preview(
    option: &FinishedMenuOption,
    duration: Option<&str>,
    storage: Option<&str>,
    summary: Option<&str>,
) -> FzfPreview {
    let (color, icon) = option.icon();

    let mut builder = PreviewBuilder::new()
        .line(color, Some(icon), option.label())
        .separator()
        .blank();

    for line in option.description_lines() {
        builder = builder.text(line);
    }

    // Runtime results
    if duration.is_some() || storage.is_some() {
        builder = builder
            .blank()
            .line(colors::TEAL, Some(NerdFont::Clock), "Installation Results");
        if let Some(d) = duration {
            builder = builder.field("Duration", d);
        }
        if let Some(s) = storage {
            builder = builder.field("Storage Used", s);
        }
    }

    // Full configuration summary
    if let Some(summary) = summary {
        builder = builder.blank().separator().blank().raw(summary);
    }

    builder.build()
}

/// Handle the installation finished menu
pub(super) async fn handle_finished_command() -> Result<()> {
    let state = crate::arch::execution::state::InstallState::load()?;

    // Check if we should upload logs
    if let Ok(context) = crate::arch::engine::InstallContext::load(DEFAULT_QUESTIONS_FILE) {
        crate::arch::logging::process_log_upload(&context);
    }

    // Compute summary data once so every preview shares it
    let duration = format_duration(&state);
    let storage = query_storage_used();
    let summary_text = load_install_summary();

    let options = [
        FinishedMenuOption::Reboot,
        FinishedMenuOption::Shutdown,
        FinishedMenuOption::Continue,
    ];

    let items: Vec<FinishedMenuItem> = options
        .into_iter()
        .map(|opt| {
            let preview = build_finished_preview(
                &opt,
                duration.as_deref(),
                storage.as_deref(),
                summary_text.as_deref(),
            );
            FinishedMenuItem {
                option: opt,
                preview,
            }
        })
        .collect();

    let result = FzfWrapper::menu()
        .header(Header::fancy("Installation Finished!"))
        .presentation(MenuPresentation::Padded)
        .select_one(items)?;

    match result {
        crate::menu_utils::DialogOutcome::Submitted(item) => match item.option {
            FinishedMenuOption::Reboot => {
                println!("Rebooting...");
                std::process::Command::new("reboot").spawn()?;
            }
            FinishedMenuOption::Shutdown => {
                println!("Shutting down...");
                std::process::Command::new("poweroff").spawn()?;
            }
            FinishedMenuOption::Continue => {
                println!("Exiting to live session...");
            }
        },
        crate::menu_utils::DialogOutcome::Cancelled => println!("Exiting..."),
    }

    Ok(())
}
