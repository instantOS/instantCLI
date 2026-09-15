use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use colored::Colorize;
use tempfile::NamedTempFile;

use crate::arch::engine::{AnswerPrivacy, InstallContext};
use crate::arch::execution::upload_state::{UploadRecord, UploadScope, fingerprint_bytes};
use crate::menu_utils::{DialogOutcome, FzfPreview, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

const SNIPS_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACDRzUC9CRt7es9BJmUrI+sDt6nG6CsSBvtfOeAvcR/J7gAAAJBM/19nTP9f
ZwAAAAtzc2gtZWQyNTUxOQAAACDRzUC9CRt7es9BJmUrI+sDt6nG6CsSBvtfOeAvcR/J7g
AAAEA+b6NfYeO8B3xNNqiixJPfcRrw2zQhmdA8uCFodPK4etHNQL0JG3t6z0EmZSsj6wO3
qcboKxIG+1854C9xH8nuAAAADWJlbmphbWluQHJ4cGM=
-----END OPENSSH PRIVATE KEY-----";

#[derive(Clone)]
struct MenuItem<T> {
    value: T,
    label: String,
    preview: FzfPreview,
}

impl<T: Clone> FzfSelectable for MenuItem<T> {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

#[derive(Clone, Copy)]
enum FailureLogAction {
    Upload,
    View,
    Exit,
}

/// Entry points offered by the interactive log upload menu.
#[derive(Clone)]
enum UploadMenuAction {
    Upload(UploadScope),
    /// Show the URL of an earlier upload without uploading again.
    PreviousUpload(UploadRecord),
    ViewLogs,
    Cancel,
}

#[derive(Clone, Copy)]
enum ExistingUploadPolicy {
    Reuse,
    Replace,
}

enum UploadResult {
    Uploaded(UploadRecord),
    Reused(UploadRecord),
}

impl UploadResult {
    fn into_record(self) -> UploadRecord {
        match self {
            Self::Uploaded(record) | Self::Reused(record) => record,
        }
    }
}

fn reusable_upload(
    record: Option<UploadRecord>,
    log_sha256: &str,
    policy: ExistingUploadPolicy,
) -> Option<UploadRecord> {
    if !matches!(policy, ExistingUploadPolicy::Reuse) {
        return None;
    }
    record.filter(|record| record.log_sha256 == log_sha256)
}

/// Upload after a successful install only when the user explicitly enabled it
/// in Advanced Options. This always uses the least-detailed report scope.
pub fn process_requested_log_upload(context: &InstallContext) {
    if !context.get_answer_bool(crate::arch::engine::StepId::LogUpload) {
        return;
    }

    match upload_install_report(
        context,
        UploadScope::InstallLog,
        ExistingUploadPolicy::Reuse,
    ) {
        Ok(UploadResult::Uploaded(record)) => {
            println!("Logs uploaded successfully: {}", record.url.green().bold())
        }
        Ok(UploadResult::Reused(record)) => {
            println!("Logs were already uploaded: {}", record.url.green().bold())
        }
        Err(error) => eprintln!("Failed to upload logs: {error}"),
    }
}

fn format_upload_time(timestamp: chrono::DateTime<chrono::Utc>) -> String {
    timestamp.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// Build the interactive upload menu entries.
///
/// When an upload of the current install log is remembered, an entry
/// reporting it (with the URL) is added, so the user can retrieve the link
/// without uploading a duplicate report. Uploading a fresh report stays
/// possible.
fn build_upload_options(previous: Option<&UploadRecord>) -> Vec<MenuItem<UploadMenuAction>> {
    // The first entry aborts the menu. Before any upload that role is the
    // privacy choice "Do Not Upload"; afterwards it is a plain exit, since
    // declining to upload no longer describes the situation.
    let cancel = if previous.is_some() {
        MenuItem {
            value: UploadMenuAction::Cancel,
            label: format!(
                "{} Exit",
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::CrossCircle, "Exit")
                .text("Return to the previous menu without uploading again.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Lock),
                    "No network request will be made.",
                )
                .build(),
        }
    } else {
        MenuItem {
            value: UploadMenuAction::Cancel,
            label: format!(
                "{} Do Not Upload",
                format_icon_colored(NerdFont::CrossCircle, colors::BLUE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::CrossCircle, "Do Not Upload")
                .text("Return without sharing any logs or system information.")
                .blank()
                .line(
                    colors::GREEN,
                    Some(NerdFont::Lock),
                    "No network request will be made.",
                )
                .build(),
        }
    };

    let mut options = vec![cancel];

    if let Some(record) = previous {
        options.push(MenuItem {
            value: UploadMenuAction::PreviousUpload(record.clone()),
            label: format!(
                "{} Already uploaded",
                format_icon_colored(NerdFont::Link, colors::TEAL),
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Link, "Previous Upload")
                .text("A privacy-filtered report for this install log was already uploaded.")
                .blank()
                .field("URL", &record.url)
                .field("Contents", record.scope.label())
                .field("Uploaded", &format_upload_time(record.uploaded_at))
                .blank()
                .subtext("Select to show the URL again. A new report can still be uploaded.")
                .build(),
        });
    }

    options.push(MenuItem {
        value: UploadMenuAction::Upload(UploadScope::InstallLog),
        label: format!(
            "{} {}",
            format_icon_colored(NerdFont::Upload, colors::GREEN),
            UploadScope::InstallLog.label()
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Upload, "Upload Support Report")
            .text("Upload the sanitized install log and non-personal configuration choices.")
            .blank()
            .line(colors::GREEN, Some(NerdFont::Lock), "Excluded")
            .bullets([
                "Username and hostname",
                "Passwords and encryption passphrases",
                "Disk and partition identifiers",
                "Detected hardware and system specifications",
            ])
            .blank()
            .subtext("Nothing is uploaded until you select this option.")
            .build(),
    });

    options.push(MenuItem {
        value: UploadMenuAction::Upload(UploadScope::InstallLogAndSystemDetails),
        label: format!(
            "{} Include system and hardware details",
            format_icon_colored(NerdFont::CloudUpload, colors::YELLOW)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::CloudUpload, "Upload Detailed Support Report")
            .text("Also include detected CPU/GPU, RAM, architecture, distro, boot mode, and selected disk/partitions.")
            .blank()
            .line(colors::GREEN, Some(NerdFont::Lock), "Still excluded")
            .bullets([
                "Username and hostname",
                "Passwords and encryption passphrases",
            ])
            .blank()
            .subtext("Review the local log first if command output may contain personal data.")
            .build(),
    });

    options.push(view_logs_menu_item(UploadMenuAction::ViewLogs));

    options
}

/// Menu entry for opening the local installation log.
fn view_logs_menu_item<T>(value: T) -> MenuItem<T> {
    MenuItem {
        value,
        label: format!(
            "{} View Logs",
            format_icon_colored(NerdFont::FileText, colors::BLUE)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::FileText, "View Logs")
            .text("Open the local installation log in nvim, or less when nvim is unavailable.")
            .field("File", crate::arch::execution::paths::LOG_FILE)
            .build(),
    }
}

/// Open the local installation log, reporting failure in a dialog because
/// the surrounding menus cover plain terminal output.
pub fn show_install_log_dialog() -> Result<()> {
    if let Err(error) = view_install_log() {
        FzfWrapper::message(&format!("Failed to view logs: {error}"))?;
    }
    Ok(())
}

/// Let the user select exactly what will be included before uploading.
///
/// Re-prompts after each action so the user can upload and then immediately
/// review the log or retrieve the URL, without leaving the menu. Upload
/// results are reported in a dialog because the immediately reopening menu
/// would otherwise cover plain terminal output. Whether a report was
/// uploaded is tracked for this menu run: once one succeeds the first entry
/// becomes an explicit "Exit" and the remembered upload is offered again.
pub fn prompt_log_upload(context: &InstallContext) -> Result<()> {
    let mut previous = UploadRecord::current();
    loop {
        let options = build_upload_options(previous.as_ref());

        let header = if previous.is_some() {
            Header::fancy("Logs Already Uploaded")
        } else {
            Header::fancy("Choose Log Upload Contents")
        };

        let result = FzfWrapper::builder()
            .header(header)
            .prompt("Select")
            .responsive_layout()
            .items(options)
            .padded()
            .select_one()?;

        let DialogOutcome::Submitted(item) = result else {
            return Ok(());
        };

        match item.value {
            UploadMenuAction::Cancel => return Ok(()),
            UploadMenuAction::Upload(scope) => {
                println!("Preparing privacy-filtered support report...");
                match upload_install_report(context, scope, ExistingUploadPolicy::Replace) {
                    Ok(result) => {
                        let record = result.into_record();
                        FzfWrapper::message(&format!(
                            "Logs uploaded successfully.\n\nURL: {}\nContents: {}\n\nSelect \"View Logs\" to inspect the local log, or \"Exit\" to leave this menu.",
                            record.url,
                            record.scope.label(),
                        ))?;
                        previous = Some(record);
                    }
                    Err(error) => {
                        FzfWrapper::message(&format!(
                            "Failed to upload logs:\n\n{error}\n\nNothing was shared. You can retry, view the local log, or exit.",
                        ))?;
                    }
                }
            }
            UploadMenuAction::PreviousUpload(record) => FzfWrapper::message(&format!(
                "These logs were already uploaded.\n\nURL: {}\n\nContents: {}\nUploaded: {}",
                record.url,
                record.scope.label(),
                format_upload_time(record.uploaded_at),
            ))?,
            UploadMenuAction::ViewLogs => show_install_log_dialog()?,
        }
    }
}

/// Keep a failed installation interactive so the user can inspect or
/// explicitly upload its log before returning to the shell.
pub fn show_failed_install_log_menu(context: Option<&InstallContext>) -> Result<()> {
    loop {
        let options = vec![
            MenuItem {
                value: FailureLogAction::Upload,
                label: format!(
                    "{} Upload Logs",
                    format_icon_colored(NerdFont::Upload, colors::GREEN)
                ),
                preview: PreviewBuilder::new()
                    .header(NerdFont::Upload, "Upload Logs")
                    .text("Choose a privacy-filtered support report to upload to snips.sh.")
                    .blank()
                    .subtext("Uploading is optional and requires another explicit selection.")
                    .build(),
            },
            view_logs_menu_item(FailureLogAction::View),
            MenuItem {
                value: FailureLogAction::Exit,
                label: format!(
                    "{} Exit",
                    format_icon_colored(NerdFont::CrossCircle, colors::RED)
                ),
                preview: PreviewBuilder::new()
                    .header(NerdFont::CrossCircle, "Exit")
                    .text("Return to the shell.")
                    .build(),
            },
        ];

        let result = FzfWrapper::builder()
            .header(Header::fancy("Installation Failed"))
            .prompt("Select")
            .responsive_layout()
            .items(options)
            .padded()
            .select_one()?;

        match result {
            DialogOutcome::Submitted(item) => match item.value {
                FailureLogAction::Upload => {
                    if let Some(context) = context {
                        prompt_log_upload(context)?;
                    } else {
                        FzfWrapper::message(
                            "The saved answers could not be loaded, so a safely redacted report cannot be created. You can still view the local log.",
                        )?;
                    }
                }
                FailureLogAction::View => show_install_log_dialog()?,
                FailureLogAction::Exit => return Ok(()),
            },
            DialogOutcome::Cancelled => return Ok(()),
        }
    }
}

pub fn view_install_log() -> Result<()> {
    let log_path = Path::new(crate::arch::execution::paths::LOG_FILE);
    if !log_path.exists() {
        anyhow::bail!("Log file not found: {}", log_path.display());
    }

    let (viewer, args): (&str, &[&str]) = if command_exists("nvim") {
        ("nvim", &[])
    } else {
        ("less", &["-R"])
    };
    let status = Command::new(viewer)
        .args(args)
        .arg(log_path)
        .status()
        .with_context(|| format!("Failed to open log with {viewer}"))?;
    if !status.success() {
        anyhow::bail!("{viewer} exited unsuccessfully");
    }
    Ok(())
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(command).is_file())
    })
}

/// Upload the privacy-filtered report and remember the upload.
///
/// Remembering happens here so every report upload is recorded; callers
/// cannot forget it. The raw [`upload_logs`] path stays unrecorded on
/// purpose: it uploads an explicitly chosen file, not this report. A failing
/// record is only a warning because the upload itself already succeeded.
fn upload_install_report(
    context: &InstallContext,
    scope: UploadScope,
    existing_upload: ExistingUploadPolicy,
) -> Result<UploadResult> {
    let log_path = Path::new(crate::arch::execution::paths::LOG_FILE);
    let (report, log_sha256) = build_support_report(context, log_path, scope)?;

    if let Some(record) = reusable_upload(UploadRecord::load(), &log_sha256, existing_upload) {
        return Ok(UploadResult::Reused(record));
    }

    println!("Uploading the privacy-filtered installation report as requested...");
    let url = upload_logs(report.path())?;

    let record = UploadRecord::new(url, scope, log_sha256);
    if let Err(error) = record.save() {
        eprintln!("Warning: could not save the upload record: {error}");
    }
    Ok(UploadResult::Uploaded(record))
}

fn build_support_report(
    context: &InstallContext,
    log_path: &Path,
    scope: UploadScope,
) -> Result<(NamedTempFile, String)> {
    let log = std::fs::read_to_string(log_path)
        .with_context(|| format!("Failed to read log file: {}", log_path.display()))?;
    let log_sha256 = fingerprint_bytes(log.as_bytes());
    let sanitized_log = redact_log(context, &log, scope);

    let mut report = NamedTempFile::new().context("Failed to create support report")?;
    writeln!(report, "instantOS installation support report")?;
    writeln!(report, "scope = {scope:?}")?;
    writeln!(report, "known_identity_and_secret_answers_included = false")?;
    writeln!(report, "\n[included choices]")?;

    let mut answers: Vec<_> = context.answers().collect();
    answers.sort_by_key(|(id, _)| **id);
    for (id, answer) in answers {
        let include = id.answer_privacy() == AnswerPrivacy::Anonymous
            || (scope == UploadScope::InstallLogAndSystemDetails
                && id.answer_privacy() == AnswerPrivacy::SystemDetail);
        if include {
            writeln!(report, "{id:?} = {answer:?}")?;
        }
    }

    if scope == UploadScope::InstallLogAndSystemDetails {
        let info = &context.system_info;
        writeln!(report, "\n[system details]")?;
        writeln!(report, "boot_mode = {:?}", info.boot_mode)?;
        writeln!(report, "architecture = {:?}", info.architecture)?;
        writeln!(report, "distro = {:?}", info.distro)?;
        writeln!(report, "has_amd_cpu = {}", info.has_amd_cpu)?;
        writeln!(report, "has_intel_cpu = {}", info.has_intel_cpu)?;
        writeln!(report, "gpus = {:?}", info.gpus)?;
        writeln!(report, "virtual_machine = {:?}", info.vm_type)?;
        writeln!(report, "total_ram_gb = {:?}", info.total_ram_gb)?;
    }

    writeln!(report, "\n[install log]")?;
    report.write_all(sanitized_log.as_bytes())?;
    report.flush()?;
    Ok((report, log_sha256))
}

fn redact_log(context: &InstallContext, log: &str, scope: UploadScope) -> String {
    let mut redactions: Vec<_> = context
        .answers()
        .filter_map(|(id, answer)| {
            if answer.is_empty() {
                return None;
            }
            let privacy = id.answer_privacy();
            let should_redact = matches!(privacy, AnswerPrivacy::Personal | AnswerPrivacy::Secret)
                || (privacy == AnswerPrivacy::SystemDetail && scope == UploadScope::InstallLog);
            should_redact.then_some((answer, privacy))
        })
        .collect();
    redactions.sort_by_key(|(answer, _)| std::cmp::Reverse(answer.len()));

    let mut sanitized = log.to_string();
    for (answer, privacy) in redactions {
        let replacement = match privacy {
            AnswerPrivacy::Secret => "[REDACTED SECRET]",
            AnswerPrivacy::Personal => "[REDACTED PERSONAL]",
            AnswerPrivacy::SystemDetail => "[REDACTED SYSTEM DETAIL]",
            AnswerPrivacy::Anonymous => continue,
        };
        sanitized = sanitized.replace(answer, replacement);
    }
    sanitized
}

pub fn upload_logs(log_path: &Path) -> Result<String> {
    if !log_path.exists() {
        anyhow::bail!("Log file not found: {}", log_path.display());
    }

    let mut key_file = NamedTempFile::new().context("Failed to create temporary key file")?;
    key_file
        .write_all(SNIPS_KEY.as_bytes())
        .context("Failed to write key to temporary file")?;
    if !SNIPS_KEY.ends_with('\n') {
        key_file.write_all(b"\n")?;
    }
    key_file.flush()?;

    let output = Command::new("ssh")
        .arg("-i")
        .arg(key_file.path())
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("instantos@snips.sh")
        .stdin(std::fs::File::open(log_path)?)
        .output()
        .context("Failed to execute ssh command")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to upload logs: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::engine::StepId;

    #[test]
    fn upload_menu_reports_a_remembered_upload() {
        let record = UploadRecord {
            url: "https://snips.sh/f/abc123".to_string(),
            scope: UploadScope::InstallLog,
            uploaded_at: chrono::Utc::now(),
            log_sha256: fingerprint_bytes(b"install log"),
        };

        let fresh = build_upload_options(None);
        assert!(
            fresh
                .iter()
                .all(|item| !matches!(item.value, UploadMenuAction::PreviousUpload(_))),
            "no previous upload should be offered before the first upload"
        );
        let fresh_cancel = fresh
            .iter()
            .find(|item| matches!(item.value, UploadMenuAction::Cancel))
            .expect("a do-not-upload entry should exist before the first upload");
        assert!(fresh_cancel.label.contains("Do Not Upload"));

        let repeated = build_upload_options(Some(&record));
        let repeated_cancel = repeated
            .iter()
            .find(|item| matches!(item.value, UploadMenuAction::Cancel))
            .expect("an exit entry should exist after an upload");
        assert!(
            repeated_cancel.label.contains("Exit")
                && !repeated_cancel.label.contains("Do Not Upload"),
            "the abort entry should become an explicit exit once a report was uploaded"
        );
        let previous = repeated
            .iter()
            .find(|item| matches!(item.value, UploadMenuAction::PreviousUpload(_)))
            .expect("a remembered upload should be offered");

        assert!(previous.label.contains("Already uploaded"));
        assert!(
            !previous.label.contains(&record.url),
            "the URL belongs in the preview, not the one-line menu label"
        );
        let FzfPreview::Text(preview) = previous.fzf_preview() else {
            panic!("expected a text preview");
        };
        assert!(preview.contains(&record.url));

        assert!(
            repeated
                .iter()
                .any(|item| matches!(item.value, UploadMenuAction::ViewLogs)),
            "viewing the logs should always be offered"
        );
    }

    #[test]
    fn automatic_upload_reuses_only_a_record_for_identical_log_contents() {
        let log_sha256 = fingerprint_bytes(b"install log");
        let record = UploadRecord::new(
            "https://snips.sh/f/abc123",
            UploadScope::InstallLog,
            &log_sha256,
        );

        assert!(
            reusable_upload(
                Some(record.clone()),
                &log_sha256,
                ExistingUploadPolicy::Reuse,
            )
            .is_some()
        );
        assert!(
            reusable_upload(
                Some(record.clone()),
                &fingerprint_bytes(b"changed log"),
                ExistingUploadPolicy::Reuse,
            )
            .is_none()
        );
        assert!(
            reusable_upload(Some(record), &log_sha256, ExistingUploadPolicy::Replace,).is_none()
        );
    }

    #[test]
    fn key_has_openssh_structure() {
        assert!(SNIPS_KEY.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(SNIPS_KEY.ends_with("-----END OPENSSH PRIVATE KEY-----"));
    }

    #[test]
    fn basic_report_redacts_identity_secrets_and_system_details() {
        let mut context = InstallContext::new();
        context.set_answer(StepId::Username, "alice".to_string());
        context.set_answer(StepId::Hostname, "homebox".to_string());
        context.set_answer(StepId::Password, "secret phrase".to_string());
        context.set_answer(StepId::Disk, "/dev/nvme0n1".to_string());
        context.set_answer(StepId::Kernel, "linux-zen".to_string());

        let sanitized = redact_log(
            &context,
            "user alice on homebox password secret phrase disk /dev/nvme0n1 kernel linux-zen",
            UploadScope::InstallLog,
        );

        assert!(!sanitized.contains("alice"));
        assert!(!sanitized.contains("homebox"));
        assert!(!sanitized.contains("secret phrase"));
        assert!(!sanitized.contains("/dev/nvme0n1"));
        assert!(sanitized.contains("linux-zen"));
    }

    #[test]
    fn detailed_report_still_redacts_identity_and_secrets() {
        let mut context = InstallContext::new();
        context.set_answer(StepId::Username, "alice".to_string());
        context.set_answer(StepId::Password, "secret phrase".to_string());
        context.set_answer(StepId::Disk, "/dev/nvme0n1".to_string());

        let sanitized = redact_log(
            &context,
            "alice secret phrase /dev/nvme0n1",
            UploadScope::InstallLogAndSystemDetails,
        );

        assert!(!sanitized.contains("alice"));
        assert!(!sanitized.contains("secret phrase"));
        assert!(sanitized.contains("/dev/nvme0n1"));
    }

    #[test]
    fn support_report_includes_only_the_selected_answer_classes() {
        let mut context = InstallContext::new();
        context.set_answer(StepId::Username, "alice".to_string());
        context.set_answer(StepId::Password, "secret phrase".to_string());
        context.set_answer(StepId::Disk, "/dev/nvme0n1".to_string());
        context.set_answer(StepId::Kernel, "linux-zen".to_string());

        let mut log = NamedTempFile::new().unwrap();
        writeln!(log, "alice secret phrase /dev/nvme0n1 linux-zen").unwrap();

        let (basic, _) =
            build_support_report(&context, log.path(), UploadScope::InstallLog).unwrap();
        let basic_text = std::fs::read_to_string(basic.path()).unwrap();
        assert!(basic_text.contains("Kernel = \"linux-zen\""));
        assert!(!basic_text.contains("alice"));
        assert!(!basic_text.contains("secret phrase"));
        assert!(!basic_text.contains("/dev/nvme0n1"));

        let (detailed, _) = build_support_report(
            &context,
            log.path(),
            UploadScope::InstallLogAndSystemDetails,
        )
        .unwrap();
        let detailed_text = std::fs::read_to_string(detailed.path()).unwrap();
        assert!(detailed_text.contains("Disk = \"/dev/nvme0n1\""));
        assert!(!detailed_text.contains("alice"));
        assert!(!detailed_text.contains("secret phrase"));
    }
}
