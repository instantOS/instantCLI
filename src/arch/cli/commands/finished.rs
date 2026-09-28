use anyhow::Result;

use crate::arch::engine::build_install_summary;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// What the machine will boot after a reboot.
///
/// The finished menu used to say "boot into your newly installed instantOS
/// system" unconditionally. That is only true when the installer runs from
/// RAM: from a running system, rebooting returns to *that* system, and the
/// new install has to be booted from its own disk or the firmware boot menu.
/// Saying otherwise sends the user looking for a default boot entry that does
/// not exist.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterReboot {
    /// The installer ran from RAM-resident media; the next boot is the target.
    Target,
    /// The machine still runs the source system after a reboot.
    SourceSystem,
}

impl AfterReboot {
    fn detect() -> Self {
        if crate::arch::host::HostProfile::detect().is_ok_and(|profile| profile.etc_is_ephemeral())
        {
            Self::Target
        } else {
            Self::SourceSystem
        }
    }

    fn reboot_label(self) -> &'static str {
        match self {
            Self::Target => "Reboot",
            Self::SourceSystem => "Reboot into the current system",
        }
    }

    fn shutdown_label(self) -> &'static str {
        match self {
            Self::Target => "Shutdown",
            Self::SourceSystem => "Shutdown (into the current system)",
        }
    }

    fn continue_label(self) -> &'static str {
        match self {
            Self::Target => "Continue in Live Session",
            Self::SourceSystem => "Keep using the current system",
        }
    }

    fn reboot_description(self) -> &'static [&'static str] {
        match self {
            Self::Target => &[
                "Restart the system and boot into your",
                "newly installed instantOS system.",
            ],
            Self::SourceSystem => &[
                "Restart into the system you are running now.",
                "instantOS was installed to a different disk:",
                "select that disk in the firmware boot menu.",
            ],
        }
    }

    fn shutdown_description(self) -> &'static [&'static str] {
        match self {
            Self::Target => &[
                "Power off the system. Boot into your",
                "new installation when you are ready.",
            ],
            Self::SourceSystem => &[
                "Power off. The next power-on boots the current",
                "system again; pick the new disk in the firmware",
                "boot menu to boot instantOS from it.",
            ],
        }
    }

    fn continue_description(self) -> &'static [&'static str] {
        match self {
            Self::Target => &[
                "Return to the live environment without",
                "rebooting or powering off.",
            ],
            Self::SourceSystem => &[
                "Close the installer and carry on with the",
                "system you are running now; installing to a",
                "different disk left it untouched.",
            ],
        }
    }
}

/// Actions offered after installation completes.
#[derive(Clone)]
enum FinishedMenuOption {
    Reboot,
    Shutdown,
    Continue,
    UploadLogs,
    ViewLogs,
}

impl FinishedMenuOption {
    fn icon(&self) -> (&'static str, NerdFont) {
        match self {
            Self::Reboot => (colors::GREEN, NerdFont::Reboot),
            Self::Shutdown => (colors::RED, NerdFont::PowerOff),
            Self::Continue => (colors::BLUE, NerdFont::Continue),
            Self::UploadLogs => (colors::GREEN, NerdFont::Upload),
            Self::ViewLogs => (colors::BLUE, NerdFont::FileText),
        }
    }

    fn label(&self, reboot: AfterReboot) -> &'static str {
        match self {
            Self::Reboot => reboot.reboot_label(),
            Self::Shutdown => reboot.shutdown_label(),
            Self::Continue => reboot.continue_label(),
            Self::UploadLogs => "Upload Logs",
            Self::ViewLogs => "View Logs",
        }
    }

    fn description_lines(&self, reboot: AfterReboot) -> &'static [&'static str] {
        match self {
            Self::Reboot => reboot.reboot_description(),
            Self::Shutdown => reboot.shutdown_description(),
            Self::Continue => reboot.continue_description(),
            Self::UploadLogs => &[
                "Choose what to include, then upload a",
                "privacy-filtered report to snips.sh.",
            ],
            Self::ViewLogs => &[
                "Inspect the local installation log in nvim,",
                "or less when nvim is unavailable.",
            ],
        }
    }
}

/// Wrapper that pairs a menu option with its pre-built preview.
#[derive(Clone)]
struct FinishedMenuItem {
    option: FinishedMenuOption,
    reboot: AfterReboot,
    preview: FzfPreview,
}

impl FzfSelectable for FinishedMenuItem {
    fn fzf_display_text(&self) -> String {
        let (color, icon) = self.option.icon();
        format!(
            "{} {}",
            format_icon_colored(icon, color),
            self.option.label(self.reboot)
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
    let context =
        crate::arch::engine::InstallContext::load(super::default_questions_file()).ok()?;
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
    reboot: AfterReboot,
    duration: Option<&str>,
    storage: Option<&str>,
    summary: Option<&str>,
) -> FzfPreview {
    let (color, icon) = option.icon();

    let mut builder = PreviewBuilder::new()
        .line(color, Some(icon), option.label(reboot))
        .separator()
        .blank();

    for line in option.description_lines(reboot) {
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
    if let Ok(context) = crate::arch::engine::InstallContext::load(super::default_questions_file())
    {
        crate::arch::logging::process_requested_log_upload(&context);
    }

    // Compute summary data once so every preview shares it
    let duration = format_duration(&state);
    let storage = query_storage_used();
    let summary_text = load_install_summary();
    // Which system a reboot lands on decides how every power-related option
    // reads, so it is resolved once and threaded through the menu.
    let reboot = AfterReboot::detect();

    let options = [
        FinishedMenuOption::Reboot,
        FinishedMenuOption::Shutdown,
        FinishedMenuOption::Continue,
        FinishedMenuOption::UploadLogs,
        FinishedMenuOption::ViewLogs,
    ];

    let items: Vec<FinishedMenuItem> = options
        .into_iter()
        .map(|opt| {
            let preview = build_finished_preview(
                &opt,
                reboot,
                duration.as_deref(),
                storage.as_deref(),
                summary_text.as_deref(),
            );
            FinishedMenuItem {
                option: opt,
                reboot,
                preview,
            }
        })
        .collect();

    loop {
        let result = FzfWrapper::menu()
            .header(Header::fancy("Installation Finished!"))
            .items(items.clone())
            .padded()
            .select_one()?;

        match result {
            crate::menu_utils::DialogOutcome::Submitted(item) => match item.option {
                FinishedMenuOption::Reboot => {
                    println!("Rebooting...");
                    std::process::Command::new("reboot").spawn()?;
                    break;
                }
                FinishedMenuOption::Shutdown => {
                    println!("Shutting down...");
                    std::process::Command::new("poweroff").spawn()?;
                    break;
                }
                FinishedMenuOption::Continue => {
                    match item.reboot {
                        AfterReboot::Target => println!("Exiting to live session..."),
                        AfterReboot::SourceSystem => {
                            println!("Closing the installer; the current system is unchanged.")
                        }
                    }
                    break;
                }
                FinishedMenuOption::UploadLogs => {
                    let context =
                        crate::arch::engine::InstallContext::load(super::default_questions_file())?;
                    crate::arch::logging::prompt_log_upload(&context)?;
                }
                FinishedMenuOption::ViewLogs => {
                    crate::arch::logging::show_install_log_dialog()?;
                }
            },
            crate::menu_utils::DialogOutcome::Cancelled => {
                println!("Exiting...");
                break;
            }
        }
    }

    Ok(())
}
