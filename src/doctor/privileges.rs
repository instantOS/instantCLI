use super::{DoctorCheck, PrivilegeLevel};
use sudo::RunningAs;
use thiserror::Error;

pub fn check_privilege_requirements(
    check: &dyn DoctorCheck,
    is_fix: bool,
) -> Result<(), PrivilegeError> {
    let required = if is_fix {
        check.fix_privilege_level()
    } else {
        check.check_privilege_level()
    };

    let current = sudo::check();

    match (required, current) {
        (PrivilegeLevel::Root, RunningAs::User) => Err(PrivilegeError::NeedRoot),
        (PrivilegeLevel::User, RunningAs::Root) => Err(PrivilegeError::MustNotBeRoot),
        _ => Ok(()),
    }
}

/// Check if a check should be skipped based on privilege requirements and current state.
/// Returns Some(reason) if the check should be skipped, None if it can run.
pub fn skip_reason_for_privilege_level(
    required: PrivilegeLevel,
    is_root: bool,
) -> Option<&'static str> {
    match (required, is_root) {
        (PrivilegeLevel::Root, false) => Some("Requires root privileges"),
        (PrivilegeLevel::User, true) => Some("Cannot run as root"),
        _ => None,
    }
}

/// Escalate privileges and fix a batch of checks
/// Replaces the current process with sudo, passing check IDs as arguments
pub fn escalate_for_fix(check_ids: Vec<String>) -> Result<(), anyhow::Error> {
    use std::process::Command;

    // Join check IDs with commas for the command line
    let batch_ids = check_ids.join(",");

    // Get the current program path and args
    let current_exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("Failed to get current executable: {}", e))?;

    // Build the sudo command with the batch IDs
    let status = Command::new("sudo")
        .arg(&current_exe)
        .args(std::env::args().skip(1)) // Pass through existing args
        .arg("--batch-ids")
        .arg(&batch_ids)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to execute sudo: {}", e))?;

    // Exit with the same code as the sudo command
    std::process::exit(status.code().unwrap_or(1));
}

#[derive(Debug, Error)]
pub enum PrivilegeError {
    #[error("This operation requires root privileges")]
    NeedRoot,
    #[error("This operation must not run as root for security reasons")]
    MustNotBeRoot,
}
