use super::CommandRunner;
use crate::arch::engine::{InstallContext, StepId};
use anyhow::{Context, Result};

pub async fn install_post(context: &InstallContext, executor: &dyn CommandRunner) -> Result<()> {
    println!("Running post-installation setup (inside chroot)...");

    let username = context
        .get_answer(&StepId::Username)
        .context("Username not set")?;

    super::setup::setup_instantos(context, executor, Some(username.clone())).await?;

    Ok(())
}
