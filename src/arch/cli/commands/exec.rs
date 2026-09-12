use anyhow::Result;

use super::super::utils::ensure_root;
use crate::arch::engine::WizardStep;

pub(super) async fn handle_exec_command(
    steps: Vec<Box<dyn WizardStep>>,
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
        // A full installation gets a fresh log so an upload cannot include
        // output left behind by an earlier installation attempt. Explicit
        // single-step execution continues appending to the current attempt.
        if step.is_none() {
            std::fs::File::create(&path)?;
        }
        Some(path)
    } else {
        None
    };

    crate::arch::execution::execute_installation(&steps, questions_file, step, dry_run, log_file)
        .await
}
