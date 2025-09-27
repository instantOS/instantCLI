use std::{ffi::OsStr, fmt, path::Path};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use clap_complete::engine::CompletionCandidate;
use clap_complete::env::Shells;

use crate::dot::config::ConfigManager;
use crate::game::config::InstantGameConfig;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SupportedShell {
    Bash,
    Zsh,
}

impl SupportedShell {
    fn env_key(self) -> &'static str {
        match self {
            SupportedShell::Bash => "bash",
            SupportedShell::Zsh => "zsh",
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
                "Add this to your ~/.zshrc:\n  if [ -r \"{}\" ]; then\n      source \"{}\"\n  fi\nautoload -U compinit && compinit",
                install_path.display(),
                install_path.display()
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
        /// Print only the source snippet without additional text
        #[arg(long)]
        snippet_only: bool,
    },
}

pub fn generate(shell: SupportedShell) -> Result<String> {
    let mut command = crate::cli_command();
    command.build();

    let shells = Shells::builtins();
    let completer = shells
        .completer(shell.env_key())
        .ok_or_else(|| anyhow!("unsupported shell"))?;

    let name = command.get_name();
    let bin = command.get_bin_name().unwrap_or(name);

    let mut buffer = Vec::new();
    completer
        .write_registration("COMPLETE", name, bin, bin, &mut buffer)
        .context("writing dynamic completion stub")?;

    String::from_utf8(buffer).context("rendering completions")
}

pub fn install(shell: SupportedShell, snippet_only: bool) -> Result<String> {
    let snippet = match shell {
        SupportedShell::Bash => {
            "# Add to ~/.bashrc or ~/.bash_profile\nsource <(COMPLETE=bash instant)".to_string()
        }
        SupportedShell::Zsh => "# Add to ~/.zshrc\nsource <(COMPLETE=zsh instant)".to_string(),
    };

    if snippet_only {
        Ok(snippet)
    } else {
        Ok(format!(
            "To enable {} completions, add the following to your shell config:\n\n{}\n\nThis keeps dynamic completions in sync with the instant binary.",
            shell, snippet
        ))
    }
}

pub fn instructions(shell: SupportedShell, install_path: &Path) -> String {
    shell.install_instructions(install_path)
}

fn matches_prefix<'a>(value: &'a str, prefix: &str) -> bool {
    prefix.is_empty() || value.starts_with(prefix)
}

fn sort_and_filter(mut values: Vec<String>, prefix: &str) -> Vec<CompletionCandidate> {
    values.sort();
    values.dedup();
    values
        .into_iter()
        .filter(|value| matches_prefix(value, prefix))
        .map(CompletionCandidate::new)
        .collect()
}

fn lossy_prefix(input: &OsStr) -> String {
    input.to_string_lossy().to_string()
}

pub fn game_name_completion(current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = lossy_prefix(current);
    let Ok(config) = InstantGameConfig::load() else {
        return Vec::new();
    };

    let names = config.games.into_iter().map(|game| game.name.0).collect();

    sort_and_filter(names, &prefix)
}

pub fn repo_name_completion(current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = lossy_prefix(current);
    let Ok(manager) = ConfigManager::load() else {
        return Vec::new();
    };

    let names = manager
        .config
        .repos
        .into_iter()
        .map(|repo| repo.name)
        .collect();

    sort_and_filter(names, &prefix)
}
