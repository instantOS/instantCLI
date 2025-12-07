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

/// Check if a check should be skipped based on current privileges.
/// Returns Some(reason) if the check should be skipped, None if it can run.
pub fn should_skip_for_privileges(required: PrivilegeLevel) -> Option<&'static str> {
    match (required, sudo::check()) {
        (PrivilegeLevel::Root, RunningAs::User) => Some("Requires root privileges"),
        (PrivilegeLevel::User, RunningAs::Root) => Some("Cannot run as root"),
        _ => None,
    }
}

pub fn escalate_for_fix(_check_id: &str) -> Result<(), anyhow::Error> {
    // Use sudo crate to restart with privileges
    match sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"]) {
        Ok(_) => {
            // This should never be reached as process restarts
            unreachable!("sudo::with_env should restart the process")
        }
        Err(e) => Err(anyhow::anyhow!("Failed to escalate privileges: {}", e)),
    }
}

#[derive(Debug, Error)]
pub enum PrivilegeError {
    #[error("This operation requires root privileges")]
    NeedRoot,
    #[error("This operation must not run as root for security reasons")]
    MustNotBeRoot,
}
