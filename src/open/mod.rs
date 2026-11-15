//! Interactive Open With picker. Discovery and preview work never run on the UI thread.
mod menu;
mod model;

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::applications::desktop::load_desktop_details;
use crate::applications::system::set_default_app;

#[derive(clap::Args, Debug)]
pub(crate) struct OpenArgs {
    /// Local file or directory to open
    pub path: PathBuf,
    /// Open the picker in the configured graphical terminal
    #[arg(long)]
    pub gui: bool,
}

pub(crate) fn run(args: &OpenArgs) -> Result<()> {
    let path = args
        .path
        .canonicalize()
        .with_context(|| format!("Cannot open {}", args.path.display()))?;
    let metadata = path.metadata()?;
    if !metadata.is_file() && !metadata.is_dir() {
        bail!("Only regular files and directories can be opened");
    }
    if args.gui {
        instantos_terminal::Launcher::new(std::env::current_exe()?)
            .args([std::ffi::OsStr::new("open"), path.as_os_str()])
            .title("ins")
            .app_id("ins-open")
            .spawn_detached()?;
        return Ok(());
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("ins open needs an interactive terminal; use --gui to open one");
    }
    let Some(choice) = menu::choose(&path)? else {
        return Ok(());
    };
    let desktop_path = choice
        .app
        .path
        .as_ref()
        .context("Application has no desktop file")?;
    load_desktop_details(desktop_path)?
        .open_file(&path)
        .with_context(|| format!("Failed to open file with {}", choice.app.desktop_id))?;
    // Persist only launches that were successfully handed to the application.
    if let Err(error) = model::record(&choice.mime, &choice.app.desktop_id) {
        eprintln!("Warning: could not remember application usage: {error:#}");
    }
    if choice.use_default {
        set_default_app(&choice.mime, &choice.app.desktop_id)
            .context("File opened, but setting the default application failed")?;
    }
    Ok(())
}
