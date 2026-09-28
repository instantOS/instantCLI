use anyhow::{Context, Result};

use super::super::utils::ensure_root;
use crate::arch::engine::WizardStep;

pub(super) async fn handle_exec_command(
    steps: Vec<Box<dyn WizardStep>>,
    step: Option<String>,
    questions_file: std::path::PathBuf,
    dry_run: bool,
) -> Result<crate::arch::execution::ExecutionOutcome> {
    if !dry_run {
        ensure_root()?;
    }

    let log_file = if !dry_run {
        // The log for *this* run. On a running system that is a file in the
        // installer's ephemeral state directory, not the source system's
        // `/var/log/instantos/install.log`: truncating the latter would
        // destroy the log of the machine the user is installing from.
        let path = crate::arch::execution::paths::host_log_file();
        crate::arch::execution::paths::ensure_host_state_dir()?;
        // A full installation gets a fresh log so an upload cannot include
        // output left behind by an earlier installation attempt. Explicit
        // single-step execution continues appending to the current attempt.
        if step.is_none() {
            std::fs::File::create(&path)
                .with_context(|| format!("Failed to create install log {}", path.display()))?;
        }
        Some(path)
    } else {
        None
    };

    crate::arch::execution::execute_installation(&steps, questions_file, step, dry_run, log_file)
        .await
}
