use anyhow::Result;

use super::super::utils::ensure_root;

pub(super) fn handle_upload_logs(path: Option<std::path::PathBuf>) -> Result<()> {
    if let Some(log_path) = path {
        // Supplying a path is an explicit request to upload that exact file.
        // This is retained for support/debugging use outside an installation.
        println!(
            "Uploading the explicitly selected file: {}",
            log_path.display()
        );
        let url = crate::arch::logging::upload_logs(&log_path)?;
        println!("Logs uploaded successfully: {url}");
        return Ok(());
    }

    // The default flow reads the root-owned install log and records the
    // upload in /etc, so it needs the same privileges as the installer.
    ensure_root()?;
    let context =
        crate::arch::engine::InstallContext::load(crate::arch::cli::DEFAULT_QUESTIONS_FILE)?;
    crate::arch::logging::prompt_log_upload(&context)
}
