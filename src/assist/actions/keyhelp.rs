use anyhow::Result;

use crate::assist::utils::launch_self_gui;

/// Open the instantWM keyhelp viewer in a GUI terminal window
pub fn open_keyhelp() -> Result<()> {
    launch_self_gui(&["keyhelp"])
}
