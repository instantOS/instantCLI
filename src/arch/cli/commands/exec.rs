use anyhow::{Context, Result};

use super::super::utils::ensure_root;
use crate::arch::engine::WizardStep;
use crate::arch::host::HostProfile;

pub(super) async fn handle_exec_command(
    steps: Vec<Box<dyn WizardStep>>,
    step: Option<String>,
    questions_file: std::path::PathBuf,
    dry_run: bool,
) -> Result<crate::arch::execution::ExecutionOutcome> {
    ensure_exec_host(
        HostProfile::detect()?,
        crate::arch::execution::is_chroot(),
        dry_run,
    )?;
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
        // Live media keeps logs under /var/log rather than the state directory;
        // that directory need not exist on a stock Arch ISO.
        if let Some(parent) = path.parent() {
            crate::arch::execution::paths::ensure_state_dir(parent)?;
        }
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

// Reject unsupported source hosts before creating state or touching a disk.
// Chroot re-entry runs inside the new target, whose os-release may differ.
fn ensure_exec_host(profile: HostProfile, in_chroot: bool, dry_run: bool) -> Result<()> {
    if !dry_run && !in_chroot && !profile.supports_installation() {
        anyhow::bail!(
            "Installation from this distribution is not supported. Boot the instantOS live ISO \
             or run the installer from an Arch Linux or instantOS system."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_hosts_are_refused_but_dry_runs_and_chroot_reentry_are_allowed() {
        assert!(ensure_exec_host(HostProfile::ForeignDistro, false, false).is_err());
        assert!(ensure_exec_host(HostProfile::ForeignDistro, false, true).is_ok());
        assert!(ensure_exec_host(HostProfile::ForeignDistro, true, false).is_ok());
        for profile in [
            HostProfile::LiveIso,
            HostProfile::RunningArch,
            HostProfile::RunningInstantOs,
        ] {
            assert!(ensure_exec_host(profile, false, false).is_ok());
        }
    }
}
