use std::process::Command;

use anyhow::{Context, Result};

use crate::assist::utils::launch_self_gui;

pub fn open_password_manager() -> Result<()> {
    if which::which("instantmenu").is_ok() {
        Command::new(std::env::current_exe()?)
            .args(["pass", "--instantmenu"])
            .spawn()
            .context("Failed to launch the password picker")?;
        return Ok(());
    }

    launch_self_gui(&["pass"])
}
