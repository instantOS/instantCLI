use anyhow::Result;

use super::super::utils::ensure_root;

pub(super) async fn handle_exec_command(
    step: Option<String>,
    questions_file: std::path::PathBuf,
    dry_run: bool,
) -> Result<()> {
    if !dry_run {
        ensure_root()?;
    }

    let log_file = if !dry_run {
        let path = std::path::PathBuf::from(crate::arch::execution::paths::LOG_FILE);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Some(path)
    } else {
        None
    };

    crate::arch::execution::execute_installation(questions_file, step, dry_run, log_file).await
}
