use anyhow::Result;

pub fn caffeine() -> Result<()> {
    use crate::assist::utils;
    use crate::common::display_server::DisplayServer;

    match DisplayServer::detect() {
        DisplayServer::Wayland | DisplayServer::X11 => {
            let command = "echo 'Caffeine running - press Ctrl+C to quit' && systemd-inhibit --what=idle --who=Caffeine --why=Caffeine --mode=block sleep inf";
            utils::launch_in_terminal(command)?;
            Ok(())
        }
        DisplayServer::Unknown => {
            anyhow::bail!("Unknown display server. Caffeine requires a running display server.");
        }
    }
}

pub fn volume() -> Result<()> {
    crate::assist::utils::menu_command(&["slide", "--preset", "audio", "--gui"])
}

pub fn brightness() -> Result<()> {
    crate::assist::utils::menu_command(&["slide", "--preset", "brightness", "--gui"])
}

pub fn theme_settings() -> Result<()> {
    crate::settings::apply::run_nonpersistent_apply(false, false)
}
