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

/// Check if a check should be skipped based on current privileges.
/// Returns Some(reason) if the check should be skipped, None if it can run.
pub fn should_skip_for_privileges(required: PrivilegeLevel) -> Option<&'static str> {
    let is_root = matches!(sudo::check(), RunningAs::Root);
    skip_reason_for_privilege_level(required, is_root)
}

/// Get the predictable temp file path for batch fix data
/// Uses the current user's UID which remains the same after sudo escalation
pub fn batch_fix_temp_file() -> std::path::PathBuf {
    use sudo::RunningAs;

    // Get the real user ID (the user who ran sudo, not root)
    // When running as root, we need to find the original user
    let uid = if matches!(sudo::check(), RunningAs::Root) {
        // Running as root - try to get the real user from SUDO_UID or use root (0)
        std::env::var("SUDO_UID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    } else {
        // Not running as root - use current user
        // Use username as identifier since we can't safely get UID without unsafe
        std::env::var("USER")
            .unwrap_or_else(|_| "default".to_string())
            .chars()
            .map(|c| c as u32)
            .fold(0u32, |acc, val| acc.wrapping_add(val))
    };

    let temp_dir = std::env::temp_dir();
    // Include current PID for uniqueness, and user ID for predictability
    temp_dir.join(format!("instant_doctor_fix_{}_{}", uid, std::process::id()))
}

pub fn escalate_for_fix(check_ids: Vec<String>) -> Result<(), anyhow::Error> {
    // Pass check IDs via a temporary file with a predictable path
    // No environment variables needed - the escalated process scans for the right file
    let ids_json = serde_json::to_string(&check_ids)?;
    let temp_file = batch_fix_temp_file();

    std::fs::write(&temp_file, ids_json)
        .map_err(|e| anyhow::anyhow!("Failed to write temp file: {}", e))?;

    // Use sudo crate to restart with privileges
    // The escalated process will scan for temp files matching the pattern
    match sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"]) {
        Ok(_) => {
            // This should never be reached as process restarts
            unreachable!("sudo::with_env should restart the process")
        }
        Err(e) => {
            // Clean up temp file on error
            let _ = std::fs::remove_file(&temp_file);
            Err(anyhow::anyhow!("Failed to escalate privileges: {}", e))
        }
    }
}

#[derive(Debug, Error)]
pub enum PrivilegeError {
    #[error("This operation requires root privileges")]
    NeedRoot,
    #[error("This operation must not run as root for security reasons")]
    MustNotBeRoot,
}
