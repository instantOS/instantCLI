use anyhow::Result;

use crate::launch::types::LaunchItem;

/// Execute a launch item
pub fn execute_launch_item(item: &LaunchItem, in_terminal: bool) -> Result<()> {
    match item {
        LaunchItem::DesktopApp { path, .. } => {
            let details = crate::launch::desktop::load_desktop_details(path)?;
            details.execute()?;
        }
        LaunchItem::PathExecutable { name, .. } => {
            if in_terminal {
                instantos_terminal::Launcher::new(name).spawn_detached()?;
            } else {
                execute_path_executable(name)?;
            }
        }
    }
    Ok(())
}

/// Execute a path executable
fn execute_path_executable(name: &str) -> Result<()> {
    let mut cmd = std::process::Command::new(name);

    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch path executable: {}", e))?;

    Ok(())
}
