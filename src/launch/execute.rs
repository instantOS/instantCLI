use anyhow::Result;

use crate::launch::types::LaunchItem;

/// Execute a launch item
pub async fn execute_launch_item(item: &LaunchItem) -> Result<()> {
    match item {
        LaunchItem::DesktopApp(desktop_id) => {
            // For desktop apps, we need to load details first
            let mut loader = crate::launch::desktop::DesktopLoader::new();
            let details = loader.get_desktop_details(desktop_id).await?;
            crate::launch::desktop::execute_desktop_app(&details)?;
        }
        LaunchItem::PathExecutable(name) => {
            // For path executables, execute directly
            execute_path_executable(name)?;
        }
    }
    Ok(())
}

/// Execute a path executable
fn execute_path_executable(name: &str) -> Result<()> {
    // Remove "path:" prefix if present
    let clean_name = name.strip_prefix("path:").unwrap_or(name);

    let mut cmd = std::process::Command::new(clean_name);

    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch path executable: {}", e))?;

    Ok(())
}
