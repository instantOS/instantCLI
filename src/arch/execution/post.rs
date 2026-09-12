use super::CommandRunner;
use crate::arch::engine::InstallPlan;
use anyhow::Result;

pub async fn install_post(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Running post-installation setup (inside chroot)...");

    super::setup::setup_instantos_for_install(plan, executor).await?;

    Ok(())
}
