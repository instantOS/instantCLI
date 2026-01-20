use anyhow::Result;
use colored::Colorize;

pub(super) fn handle_upload_logs(path: Option<std::path::PathBuf>) -> Result<()> {
    let log_path =
        path.unwrap_or_else(|| std::path::PathBuf::from(crate::arch::execution::paths::LOG_FILE));
    println!("Uploading logs from: {}", log_path.display());
    match crate::arch::logging::upload_logs(&log_path) {
        Ok(url) => println!("Logs uploaded successfully: {}", url.green().bold()),
        Err(e) => eprintln!("Failed to upload logs: {}", e),
    }
    Ok(())
}
