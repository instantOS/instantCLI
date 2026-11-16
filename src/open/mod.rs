//! Interactive Open With picker. Discovery and preview work never run on the UI thread.
mod executable;
mod menu;
mod model;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::applications::desktop::load_desktop_details;

#[derive(clap::Args, Debug)]
pub(crate) struct OpenArgs {
    /// Local file or directory to open
    pub path: PathBuf,
    /// Open the picker in the configured graphical terminal
    #[arg(long, conflicts_with = "instantmenu")]
    pub gui: bool,
    /// Open a compact graphical picker using Instant Menu (no terminal required)
    #[arg(long)]
    pub instantmenu: bool,
}

pub(crate) fn run(args: &OpenArgs) -> Result<()> {
    let result = run_inner(args);
    if args.instantmenu
        && let Err(error) = &result
    {
        if let Err(dialog_error) = crate::menu::instantmenu::InstantmenuBackend::message(
            Some("INS Open"),
            &format!("{error:#}"),
        ) {
            eprintln!("Could not display error: {dialog_error:#}");
        }
    }
    result
}

fn run_inner(args: &OpenArgs) -> Result<()> {
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
    let backend = if args.instantmenu {
        menu::Backend::Instantmenu
    } else {
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            bail!("ins open needs an interactive terminal; use --instantmenu or --gui");
        }
        menu::Backend::Terminal
    };
    let Some(choice) = menu::choose(&path, backend)? else {
        return Ok(());
    };
    let (app, mime) = match choice {
        model::Choice::Desktop { app, mime } => (app, mime),
        model::Choice::RunFile => return execute_file(&path),
        model::Choice::Executable { executable, mime } => {
            let mut command = Command::new(&executable);
            command.arg(&path);
            spawn_detached(command)
                .with_context(|| format!("Failed to open file with {}", executable.display()))?;
            let remembered = (|| {
                let mime = match mime {
                    Some(mime) => mime,
                    None => model::detect_mime(&path)?,
                };
                model::record_custom(&mime, &executable)
            })();
            if let Err(error) = remembered {
                eprintln!("Warning: could not remember custom executable: {error:#}");
            }
            return Ok(());
        }
    };
    let desktop_path = app
        .path
        .as_ref()
        .context("Application has no desktop file")?;
    load_desktop_details(desktop_path)?
        .open_file(&path)
        .with_context(|| format!("Failed to open file with {}", app.desktop_id))?;
    // Persist only launches that were successfully handed to the application.
    if let Err(error) = model::record(&mime, &app.desktop_id) {
        eprintln!("Warning: could not remember application usage: {error:#}");
    }
    Ok(())
}

/// Execute the opened file itself without passing it back as an argument.
fn execute_file(path: &Path) -> Result<()> {
    let metadata = path
        .metadata()
        .with_context(|| format!("Cannot inspect {}", path.display()))?;
    if !metadata.is_file() {
        bail!("Only regular files can be executed");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = metadata.permissions();
        let mode = permissions.mode();
        if mode & 0o100 == 0 && which::which(path).is_err() {
            // Grant only owner execution; preserve the remaining permissions.
            permissions.set_mode(mode | 0o100);
            std::fs::set_permissions(path, permissions)
                .with_context(|| format!("Failed to make {} executable", path.display()))?;
        }
    }
    spawn_detached(Command::new(path))
        .with_context(|| format!("Failed to execute {}", path.display()))
}

fn spawn_detached(mut command: Command) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}
