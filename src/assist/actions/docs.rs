use anyhow::Result;

use crate::assist::utils;

pub fn open_docs() -> Result<()> {
    utils::launch_detached("xdg-open", &["https://instantos.io/docs"])?;
    Ok(())
}
