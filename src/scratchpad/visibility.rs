use crate::common::compositor::{CompositorType, hyprland, sway};
use super::{config::ScratchpadConfig, operations::{check_window_exists, create_and_configure_sway_scratchpad, create_and_configure_hyprland_scratchpad}};
use anyhow::Result;

/// Check if scratchpad terminal is currently visible
pub fn is_scratchpad_visible(
    compositor: &CompositorType,
    config: &ScratchpadConfig,
) -> Result<bool> {
    let window_class = config.window_class();
    match compositor {
        CompositorType::Sway => sway::is_window_visible(&window_class),
        CompositorType::Hyprland => {
            // For Hyprland, check if special workspace is active AND window exists using direct IPC
            let workspace_name = config.workspace_name();

            // Check if special workspace is active using direct IPC
            let special_workspace_active =
                hyprland::is_special_workspace_active(&workspace_name).unwrap_or(false);

            // Check if window exists using direct IPC
            let window_exists = check_window_exists(compositor, &window_class)?;

            Ok(special_workspace_active && window_exists)
        }
        CompositorType::Other(_) => {
            eprintln!("TODO: Scratchpad visibility check not implemented for this compositor");
            Ok(false)
        }
    }
}

/// Show scratchpad terminal for Sway
pub fn show_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let window_class = config.window_class();
    let window_exists = check_window_exists(&CompositorType::Sway, &window_class)?;

    if window_exists {
        // Terminal exists, show it
        sway::show_scratchpad(&window_class)?;
        println!("Showed scratchpad terminal '{}'", config.name);
    } else {
        // Terminal doesn't exist, create and configure it
        create_and_configure_sway_scratchpad(config)?;

        // Show it immediately
        sway::show_scratchpad(&window_class)?;

        println!("Scratchpad terminal '{}' created and shown", config.name);
    }

    Ok(())
}

/// Hide scratchpad terminal for Sway
pub fn hide_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let window_class = config.window_class();
    let window_exists = check_window_exists(&CompositorType::Sway, &window_class)?;

    if window_exists {
        // Check if it's currently visible (not in scratchpad)
        let is_visible = sway::is_window_visible(&window_class)?;

        if is_visible {
            // Terminal is visible, move it to scratchpad (hide it)
            sway::hide_scratchpad(&window_class)?;
            println!("Hid scratchpad terminal '{}'", config.name);
        } else {
            println!("Scratchpad terminal '{}' is already hidden", config.name);
        }
    } else {
        println!("Scratchpad terminal '{}' does not exist", config.name);
    }

    Ok(())
}

/// Show scratchpad terminal for Hyprland
pub fn show_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Check if terminal with specific class exists using direct IPC
    let window_exists = check_window_exists(&CompositorType::Hyprland, &window_class)?;

    if window_exists {
        // Terminal exists, show special workspace using direct IPC
        hyprland::show_special_workspace(&workspace_name)?;
        println!("Showed scratchpad terminal '{}'", config.name);
    } else {
        // Terminal doesn't exist, create it with proper rules
        create_and_configure_hyprland_scratchpad(config)?;

        // Show the special workspace using direct IPC
        hyprland::show_special_workspace(&workspace_name)?;

        println!("Scratchpad terminal '{}' created and shown", config.name);
    }

    Ok(())
}

/// Hide scratchpad terminal for Hyprland
pub fn hide_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Check if terminal with specific class exists using direct IPC
    let window_exists = check_window_exists(&CompositorType::Hyprland, &window_class)?;

    if window_exists {
        // Check if special workspace is currently active
        let is_visible =
            hyprland::is_special_workspace_active(&workspace_name).unwrap_or(false);

        if is_visible {
            // Terminal is visible, hide special workspace using direct IPC
            hyprland::hide_special_workspace(&workspace_name)?;
            println!("Hid scratchpad terminal '{}'", config.name);
        } else {
            println!("Scratchpad terminal '{}' is already hidden", config.name);
        }
    } else {
        println!("Scratchpad terminal '{}' does not exist", config.name);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::terminal::Terminal;

    #[test]
    fn test_show_hide_functions_exist() {
        // Test that the public API functions exist and can be called
        // This is a basic smoke test to ensure the functions are properly exposed
        let config = ScratchpadConfig::default();
        let compositor = CompositorType::Other("test".to_string());
        
        // These should not panic and should return Ok(()) for unsupported compositor
        let visible_result = is_scratchpad_visible(&compositor, &config);
        
        assert!(visible_result.is_ok());
        assert_eq!(visible_result.unwrap(), false); // Should return false for unsupported compositor
    }
}
