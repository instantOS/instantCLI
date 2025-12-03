use crate::common::compositor::sway;
use anyhow::Result;

pub fn apply_wallpaper(path: &str) -> Result<()> {
    // swaymsg output "*" bg <path> fill
    let command = format!("output \"*\" bg \"{}\" fill", path);
    sway::swaymsg(&command)?;
    Ok(())
}
