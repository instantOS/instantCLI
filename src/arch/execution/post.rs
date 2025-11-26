use super::CommandExecutor;
use crate::arch::engine::{InstallContext, QuestionId};
use anyhow::{Context, Result};

pub async fn install_post(context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Running post-installation setup (inside chroot)...");

    let username = context
        .get_answer(&QuestionId::Username)
        .context("Username not set")?;

    super::setup::setup_instantos(executor, Some(username.clone())).await?;

    Ok(())
}
