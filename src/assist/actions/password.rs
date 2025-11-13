use anyhow::Result;
use duct::cmd;

pub fn open_password_manager() -> Result<()> {
    cmd!("instantpass").run()?;
    Ok(())
}
