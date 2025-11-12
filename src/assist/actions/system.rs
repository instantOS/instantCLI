use anyhow::Result;

pub fn caffeine() -> Result<()> {
    use crate::assist::utils;
    use crate::common::display_server::DisplayServer;

    match DisplayServer::detect() {
        DisplayServer::Wayland => {
            let command = "echo 'Caffeine running - press Ctrl+C to quit' && systemd-inhibit --what=idle --who=Caffeine --why=Caffeine --mode=block sleep inf";
            utils::launch_in_terminal(command)?;
            Ok(())
        }
        DisplayServer::X11 => {
            anyhow::bail!(
                "X11 support is work in progress. Caffeine currently only supports Wayland."
            );
        }
        DisplayServer::Unknown => {
            anyhow::bail!("Unknown display server. Caffeine currently only supports Wayland.");
        }
    }
}

pub fn volume() -> Result<()> {
    use crate::assist::utils;
    utils::menu_command(&["slide", "--preset", "audio", "--gui"])
}
