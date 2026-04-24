use anyhow::Result;
use std::process::Command;

pub fn open_password_manager() -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let status = Command::new(current_exe).args(["pass", "--gui"]).status()?;
    if !status.success() {
        anyhow::bail!("`ins pass` exited with status {status}");
    }
    Ok(())
}
