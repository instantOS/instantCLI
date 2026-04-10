use crate::common::instantwmctl;
use anyhow::{Context, Result};

pub fn apply_wallpaper(path: &str) -> Result<()> {
    instantwmctl::run(["wallpaper", path]).context("Failed to set wallpaper with instantwmctl")
}
