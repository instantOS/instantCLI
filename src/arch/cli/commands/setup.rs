use anyhow::Result;

use crate::common::distro::is_live_iso;

use super::super::utils::{detect_single_user, ensure_root};

pub(super) async fn handle_setup_command(user: Option<String>, dry_run: bool) -> Result<()> {
    // Check if running on live CD
    if is_live_iso() {
        anyhow::bail!("This command cannot be run on a live CD/ISO.");
    }

    if !dry_run {
        ensure_root()?;
    }

    // Try to infer user:
    // 1. Provided argument
    // 2. SUDO_USER env var
    // 3. Smart detection (single user in /home)
    let target_user = user
        .or_else(|| std::env::var("SUDO_USER").ok())
        .or_else(detect_single_user);

    // Create a context for setup by detecting existing system settings
    let context = crate::arch::engine::InstallContext::for_setup(target_user.clone());

    let executor = crate::arch::execution::CommandExecutor::new(dry_run, None);
    crate::arch::execution::setup::setup_instantos(&context, &executor, target_user).await
}
