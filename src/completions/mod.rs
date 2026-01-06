use std::{ffi::OsStr, fmt};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use clap_complete::engine::CompletionCandidate;
use clap_complete::env::Shells;

use crate::assist::registry::{self, AssistEntry};
use crate::dot::config::Config;
use crate::doctor::registry::REGISTRY;
use crate::game::config::InstantGameConfig;
use crate::settings::setting::Category;

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
            format!(
                "# Add to ~/.bashrc or ~/.bash_profile\nsource <(COMPLETE=bash {})",
                env!("CARGO_BIN_NAME")
            )
        }
        SupportedShell::Zsh => format!(
            "# Add to ~/.zshrc\nsource <(COMPLETE=zsh {})",
            env!("CARGO_BIN_NAME")
        ),
    };

    if snippet_only {
        Ok(snippet)
    } else {
        Ok(format!(
            "To enable {shell} completions, add the following to your shell config:\n\n{snippet}\n\nThis keeps dynamic completions in sync with the {} binary.",
            env!("CARGO_BIN_NAME")
        ))
    }
}

fn matches_prefix(value: &str, prefix: &str) -> bool {
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

fn sort_and_filter_with_descriptions(mut values: Vec<(String, &'static str)>, prefix: &str) -> Vec<CompletionCandidate> {
    values.sort_by(|a, b| a.0.cmp(&b.0));
    values.dedup_by(|a, b| a.0 == b.0);
    values
        .into_iter()
        .filter(|value| matches_prefix(&value.0, prefix))
        .map(|(key, description)| CompletionCandidate::new(key).help(Some(description.to_string().into())))
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
    let Ok(config) = Config::load(None) else {
        return Vec::new();
    };

    let names = config.repos.into_iter().map(|repo| repo.name).collect();

    sort_and_filter(names, &prefix)
}

pub fn check_name_completion(current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = lossy_prefix(current);

    let checks = REGISTRY.all_checks();
    let names: Vec<String> = checks.into_iter().map(|check| check.id().to_string()).collect();

    sort_and_filter(names, &prefix)
}

/// Collect all assist key sequences with their descriptions from the registry
fn collect_assist_keys(entries: &[AssistEntry]) -> Vec<(String, &'static str)> {
    let mut keys = Vec::new();

    for entry in entries {
        match entry {
            AssistEntry::Action(action) => {
                keys.push((action.key.to_string(), action.description));
            }
            AssistEntry::Group(group) => {
                // Add the group key itself
                keys.push((group.key.to_string(), group.description));

                // Add all child keys with the group prefix
                for child in group.children {
                    if let AssistEntry::Action(action) = child {
                        keys.push((format!("{}{}", group.key, action.key), action.description));
                    }
                }
            }
        }
    }

    keys
}

pub fn assist_key_sequence_completion(current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = lossy_prefix(current);

    let keys = collect_assist_keys(registry::ASSISTS);

    sort_and_filter_with_descriptions(keys, &prefix)
}

pub fn settings_category_completion(current: &OsStr) -> Vec<CompletionCandidate> {
    let prefix = lossy_prefix(current);

    let categories: Vec<String> = Category::all().iter().map(|cat| cat.id().to_string()).collect();

    sort_and_filter(categories, &prefix)
}

pub fn handle_completions_command(command: &CompletionCommands) -> Result<()> {
    match command {
        CompletionCommands::Generate { shell } => {
            let script = generate(*shell)?;
            print!("{script}");
        }
        CompletionCommands::Install {
            shell,
            snippet_only,
        } => {
            let instructions = install(*shell, *snippet_only)?;
            println!("{instructions}");
        }
    }
    Ok(())
}
