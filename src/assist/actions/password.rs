use anyhow::Result;

use crate::assist::utils::launch_self_gui;

pub fn open_password_manager() -> Result<()> {
    launch_self_gui(&["pass"])
}
