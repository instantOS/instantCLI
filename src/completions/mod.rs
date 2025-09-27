use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use clap_complete::Shell;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SupportedShell {
    Bash,
    Zsh,
}

impl SupportedShell {
    fn as_complete_shell(self) -> Shell {
        match self {
            SupportedShell::Bash => Shell::Bash,
            SupportedShell::Zsh => Shell::Zsh,
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            SupportedShell::Bash => "instant.bash",
            SupportedShell::Zsh => "_instant",
        }
    }

    fn install_instructions(self, install_path: &Path) -> String {
        match self {
            SupportedShell::Bash => format!(
                "Add this to your ~/.bashrc or ~/.bash_profile:\n  if [ -r \"{}\" ]; then\n      source \"{}\"\n  fi",
                install_path.display(),
                install_path.display()
            ),
            SupportedShell::Zsh => format!(
                "Add this directory to your ~/.zshrc:\n  fpath=(\"{}\" $fpath)\nThen reload your shell or run: autoload -U compinit && compinit",
                install_path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| install_path.to_string_lossy().into())
            ),
        }
    }
}

impl fmt::Display for SupportedShell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SupportedShell::Bash => write!(f, "bash"),
            SupportedShell::Zsh => write!(f, "zsh"),
        }
    }
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum CompletionCommands {
    Generate {
        #[arg(value_enum)]
        shell: SupportedShell,
    },
    Install {
        #[arg(value_enum)]
        shell: SupportedShell,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
}

pub fn generate(shell: SupportedShell) -> Result<String> {
    let mut command = crate::cli_command();
    let mut buffer = Vec::new();
    clap_complete::generate(
        shell.as_complete_shell(),
        &mut command,
        "instant",
        &mut buffer,
    );
    String::from_utf8(buffer).context("rendering completions")
}

pub fn install(shell: SupportedShell, output: Option<PathBuf>, force: bool) -> Result<PathBuf> {
    let default_dir = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("instantos")
        .join("completions");
    let target_path = output.unwrap_or_else(|| default_dir.join(shell.file_name()));

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating completions directory {}", parent.display()))?;
    }

    if target_path.exists() && !force {
        return Err(anyhow!(
            "{} already exists, pass --force to overwrite",
            target_path.display()
        ));
    }

    let script = generate(shell)?;
    fs::write(&target_path, script)
        .with_context(|| format!("writing completion script to {}", target_path.display()))?;

    Ok(target_path)
}

pub fn instructions(shell: SupportedShell, install_path: &Path) -> String {
    shell.install_instructions(install_path)
}
