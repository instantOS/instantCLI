use anyhow::Result;

use crate::assist::utils::launch_self_gui;

/// Open the instantOS settings manager
pub fn open_settings() -> Result<()> {
    launch_self_gui(&["settings"])
}
