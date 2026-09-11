use anyhow::Result;

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

    let context =
        crate::arch::engine::InstallContext::load(crate::arch::cli::DEFAULT_QUESTIONS_FILE)?;
    crate::arch::logging::prompt_log_upload(&context)
}
