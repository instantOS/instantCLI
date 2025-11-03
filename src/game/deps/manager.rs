use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

use crate::dot::path_serde::TildePath;
use crate::game::config::{
    Game, GameDependency, GameInstallation, InstallationsConfig, InstalledDependency,
    InstantGameConfig, PathContentKind,
};
use crate::game::deps::{display, selection};
use crate::game::games::selection::select_game_interactive;
use crate::game::restic::dependencies::{backup_dependency, restore_dependency};
use crate::game::utils::save_files::get_save_directory_info;
use crate::game::utils::validation;
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfSelectable, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

pub struct AddDependencyOptions {
    pub game_name: Option<String>,
    pub dependency_id: Option<String>,
    pub source_path: Option<String>,
}

pub struct InstallDependencyOptions {
    pub game_name: Option<String>,
    pub dependency_id: Option<String>,
    pub install_path: Option<String>,
}

pub struct UninstallDependencyOptions {
    pub game_name: Option<String>,
    pub dependency_id: Option<String>,
}

struct DependencyContext {
    game_config: InstantGameConfig,
    installations: InstallationsConfig,
}

impl DependencyContext {
    fn load() -> Result<Self> {
        Ok(Self {
            game_config: InstantGameConfig::load().context("Failed to load game configuration")?,
            installations: InstallationsConfig::load()
                .context("Failed to load installations configuration")?,
        })
    }

    fn load_with_validation() -> Result<Self> {
        let context = Self::load()?;
        validation::check_restic_and_game_manager(&context.game_config)?;
        Ok(context)
    }

    fn game_config(&self) -> &InstantGameConfig {
        &self.game_config
    }

    fn game_index(&self, game_name: &str) -> Result<usize> {
        self.game_config
            .games
            .iter()
            .position(|game| game.name.0 == game_name)
            .ok_or_else(|| anyhow!("Game '{}' not found in configuration", game_name))
    }

    fn game(&self, game_name: &str) -> Result<&Game> {
        let index = self.game_index(game_name)?;
        Ok(&self.game_config.games[index])
    }

    fn game_mut(&mut self, game_name: &str) -> Result<&mut Game> {
        let index = self.game_index(game_name)?;
        Ok(&mut self.game_config.games[index])
    }

    fn ensure_local_installation_exists(&self, game_name: &str) -> Result<()> {
        if self
            .installations
            .installations
            .iter()
            .any(|inst| inst.game_name.0 == game_name)
        {
            Ok(())
        } else {
            Err(anyhow!(
                "Game '{}' is not configured on this device. Run '{} game setup' to configure it before adding dependencies.",
                game_name,
                env!("CARGO_BIN_NAME")
            ))
        }
    }

    fn installation_mut(&mut self, game_name: &str) -> Result<&mut GameInstallation> {
        self.installations
            .installations
            .iter_mut()
            .find(|inst| inst.game_name.0 == game_name)
            .ok_or_else(|| {
                anyhow!(
                    "Game '{}' is not configured on this device. Run '{} game setup' to configure it before adding dependencies.",
                    game_name,
                    env!("CARGO_BIN_NAME")
                )
            })
    }

    fn upsert_installed_dependency(
        installation: &mut GameInstallation,
        dependency_id: &str,
        install_path: TildePath,
    ) {
        let install_path_type = if let Ok(metadata) = fs::metadata(install_path.as_path()) {
            PathContentKind::from(metadata)
        } else {
            PathContentKind::Directory
        };

        if let Some(existing) = installation
            .dependencies
            .iter_mut()
            .find(|dep| dep.dependency_id == dependency_id)
        {
            existing.install_path = install_path;
            existing.install_path_type = install_path_type;
        } else {
            installation.dependencies.push(InstalledDependency {
                dependency_id: dependency_id.to_string(),
                install_path,
                install_path_type,
            });
        }
    }

    fn save_installations(&self) -> Result<()> {
        self.installations.save()
    }

    fn save_all(&self) -> Result<()> {
        self.installations.save()?;
        self.game_config.save()
    }
}

struct DependencySource {
    expanded: PathBuf,
    kind: PathContentKind,
    tilde: TildePath,
}

impl DependencySource {
    fn display_path(&self) -> String {
        self.tilde
            .to_tilde_string()
            .unwrap_or_else(|_| self.tilde.as_path().display().to_string())
    }
}

fn prepare_dependency_source(
    game_name: &str,
    source_path: Option<String>,
) -> Result<DependencySource> {
    let raw = resolve_source_path(source_path, game_name)?;
    let expanded = PathBuf::from(shellexpand::tilde(&raw).to_string());
    let metadata = fs::metadata(&expanded)
        .with_context(|| format!("Failed to read metadata for dependency path: {}", raw))?;

    let tilde = TildePath::from_str(&raw).with_context(|| {
        format!(
            "Failed to convert dependency path '{}' into a storable representation",
            raw
        )
    })?;

    Ok(DependencySource {
        expanded,
        kind: PathContentKind::from(metadata),
        tilde,
    })
}

pub fn add_dependency(options: AddDependencyOptions) -> Result<()> {
    let mut context = DependencyContext::load_with_validation()?;

    let AddDependencyOptions {
        game_name: game_name_arg,
        dependency_id: dependency_id_arg,
        source_path: source_path_arg,
    } = options;

    let game_name = resolve_game_name(game_name_arg, Some("Select game to add dependency"))?;
    context.ensure_local_installation_exists(&game_name)?;

    let game = context.game(&game_name)?;
    let dependency_id = resolve_dependency_id(dependency_id_arg, &game_name, game)?;
    let source = prepare_dependency_source(&game_name, source_path_arg)?;

    println!(
        "{} Creating snapshot for '{}' dependency. This may take a while...",
        char::from(NerdFont::Info),
        dependency_id
    );

    let backup = backup_dependency(
        &game_name,
        &dependency_id,
        &source.expanded,
        context.game_config(),
    )?;

    let dependency = GameDependency {
        id: dependency_id.clone(),
        source_path: source.expanded.to_string_lossy().to_string(),
        source_type: source.kind,
    };

    context.game_mut(&game_name)?.dependencies.push(dependency);

    let installation = context.installation_mut(&game_name)?;
    DependencyContext::upsert_installed_dependency(
        installation,
        &dependency_id,
        source.tilde.clone(),
    );

    context.save_all()?;

    emit(
        Level::Success,
        "game.deps.add",
        &format!(
            "{} Added dependency '{}' for '{}' (snapshot: {}). Installed at {}.",
            char::from(NerdFont::Check),
            dependency_id,
            game_name,
            backup.snapshot_id,
            source.display_path()
        ),
        Some(serde_json::json!({
            "game": game_name,
            "dependency": dependency_id,
            "snapshot": backup.snapshot_id,
            "reused_existing": backup.reused_existing,
            "install_path": source.display_path()
        })),
    );

    Ok(())
}

pub fn install_dependency(options: InstallDependencyOptions) -> Result<()> {
    let mut context = DependencyContext::load_with_validation()?;

    let InstallDependencyOptions {
        game_name: game_name_arg,
        dependency_id: dependency_id_arg,
        install_path: install_path_arg,
    } = options;

    let game_name = resolve_game_name(game_name_arg, Some("Select game to install dependency"))?;
    let game = context.game(&game_name)?;

    if game.dependencies.is_empty() {
        println!(
            "{} Game '{}' has no registered dependencies.",
            char::from(NerdFont::Info),
            game_name
        );
        return Ok(());
    }

    let selected_dependency = select_dependency_for_install(game, dependency_id_arg, &game_name)?;

    let dependency_id = selected_dependency.id.clone();
    let target_path_input = resolve_install_path(
        install_path_arg,
        &game_name,
        &dependency_id,
        &selected_dependency,
        selected_dependency.source_type,
    )?;

    let install_path_tilde = TildePath::from_str(&target_path_input).with_context(|| {
        format!(
            "Invalid install path provided for dependency '{}': {}",
            dependency_id, target_path_input
        )
    })?;

    if !prepare_install_target(&install_path_tilde, selected_dependency.source_type)? {
        return Ok(());
    }

    let restore_outcome = restore_dependency(
        &game_name,
        &selected_dependency,
        context.game_config(),
        install_path_tilde.as_path(),
    )?;

    let installation = context.installation_mut(&game_name)?;
    DependencyContext::upsert_installed_dependency(
        installation,
        &dependency_id,
        install_path_tilde.clone(),
    );
    context.save_installations()?;

    let install_display = install_path_tilde
        .to_tilde_string()
        .unwrap_or_else(|_| install_path_tilde.as_path().display().to_string());

    emit(
        Level::Success,
        "game.deps.install",
        &format!(
            "{} Installed dependency '{}' for '{}' into {} (snapshot: {}).",
            char::from(NerdFont::Check),
            dependency_id,
            game_name,
            install_display,
            restore_outcome.snapshot_id
        ),
        Some(serde_json::json!({
            "game": game_name,
            "dependency": dependency_id,
            "snapshot": restore_outcome.snapshot_id,
            "summary": restore_outcome.summary,
            "install_path": install_display
        })),
    );

    if let Some(summary) = &restore_outcome.summary {
        println!("{} {}", char::from(NerdFont::Info), summary);
    }

    Ok(())
}

pub fn uninstall_dependency(options: UninstallDependencyOptions) -> Result<()> {
    let mut context = DependencyContext::load()?;

    let game_name = resolve_game_name(
        options.game_name,
        Some("Select game to uninstall dependency"),
    )?;

    let game = context
        .game(&game_name)
        .with_context(|| format!("Game '{}' not found in configuration", game_name))?;

    if game.dependencies.is_empty() {
        println!(
            "{} Game '{}' has no registered dependencies.",
            char::from(NerdFont::Info),
            game_name
        );
        return Ok(());
    }

    let dependency_id = if let Some(id) = options.dependency_id {
        id
    } else {
        selection::select_dependency(&game_name, &game.dependencies)?
            .ok_or_else(|| anyhow!("Dependency selection cancelled"))?
            .id
            .clone()
    };

    let installation = context
        .installation_mut(&game_name)
        .map_err(|_| anyhow!("Game '{}' is not configured on this device.", game_name))?;

    if let Some(index) = installation
        .dependencies
        .iter()
        .position(|dep| dep.dependency_id == dependency_id)
    {
        let removed = installation.dependencies.remove(index);
        context.save_installations()?;

        emit(
            Level::Success,
            "game.deps.uninstall",
            &format!(
                "{} Uninstalled dependency '{}' for '{}'.",
                char::from(NerdFont::Check),
                dependency_id,
                game_name
            ),
            Some(serde_json::json!({
                "game": game_name,
                "dependency": dependency_id,
                "install_path": removed
                    .install_path
                    .to_tilde_string()
                    .unwrap_or_else(|_| removed.install_path.as_path().display().to_string())
            })),
        );
    } else {
        println!(
            "{} Dependency '{}' was not installed for '{}' on this device.",
            char::from(NerdFont::Info),
            dependency_id,
            game_name
        );
    }

    Ok(())
}

pub fn list_dependencies(game_name: Option<String>) -> Result<()> {
    let context = DependencyContext::load()?;

    let game_name = resolve_game_name(game_name, None)?;
    let game = context.game(&game_name)?;
    let installation = context
        .installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name);

    display::show_dependency_list(&game_name, &game.dependencies, installation)
}

fn resolve_game_name(game_name: Option<String>, prompt: Option<&str>) -> Result<String> {
    if let Some(name) = game_name {
        return Ok(name);
    }

    match select_game_interactive(prompt)? {
        Some(name) => Ok(name),
        None => Err(anyhow!("Game selection cancelled")),
    }
}

fn resolve_dependency_id(
    dependency_id: Option<String>,
    game_name: &str,
    game: &Game,
) -> Result<String> {
    if let Some(id) = dependency_id {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("Dependency ID cannot be empty"));
        }
        if game
            .dependencies
            .iter()
            .any(|dependency| dependency.id == trimmed)
        {
            return Err(anyhow!(
                "Dependency '{}' already exists for '{}'",
                trimmed,
                game_name
            ));
        }
        return Ok(trimmed.to_string());
    }

    let input = FzfWrapper::input(&format!("Enter dependency ID for '{}':", game_name))
        .context("Failed to read dependency id input")?;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Dependency ID cannot be empty"));
    }

    if game
        .dependencies
        .iter()
        .any(|dependency| dependency.id == trimmed)
    {
        return Err(anyhow!(
            "Dependency '{}' already exists for '{}'",
            trimmed,
            game_name
        ));
    }

    Ok(trimmed.to_string())
}

fn select_dependency_for_install(
    game: &Game,
    dependency_id: Option<String>,
    game_name: &str,
) -> Result<GameDependency> {
    if let Some(id) = dependency_id {
        let trimmed = id.trim();
        return game
            .dependencies
            .iter()
            .find(|dep| dep.id == trimmed)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "Dependency '{}' not found for game '{}'",
                    trimmed,
                    game_name
                )
            });
    }

    selection::select_dependency(game_name, &game.dependencies)?
        .ok_or_else(|| anyhow!("Dependency selection cancelled"))
        .cloned()
}

fn resolve_source_path(path: Option<String>, game_name: &str) -> Result<String> {
    if let Some(path) = path {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("Dependency path cannot be empty"));
        }
        return Ok(trimmed.to_string());
    }

    let selection = PathInputBuilder::new()
        .header(format!(
            "{} Select the dependency path for '{}'",
            char::from(NerdFont::Package),
            game_name
        ))
        .manual_prompt("Enter dependency path (file or directory):")
        .scope(FilePickerScope::FilesAndDirectories)
        .picker_hint("Select the file or directory to capture as a dependency")
        .choose()?;

    match selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                Err(anyhow!("Dependency path cannot be empty"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        PathInputSelection::Picker(path) => Ok(path.display().to_string()),
        PathInputSelection::WinePrefix(path) => Ok(path.display().to_string()),
        PathInputSelection::Cancelled => Err(anyhow!("Dependency path selection cancelled")),
    }
}

fn resolve_install_path(
    path: Option<String>,
    game_name: &str,
    dependency_id: &str,
    dependency: &GameDependency,
    expected_kind: PathContentKind,
) -> Result<String> {
    if let Some(path) = path {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("Install path cannot be empty"));
        }
        return Ok(trimmed.to_string());
    }

    let source_display = format_path_for_display(&dependency.source_path);
    let options = vec![
        InstallPathOption::new(
            format!(
                "{} Use source path ({})",
                char::from(NerdFont::Folder),
                source_display
            ),
            Some(dependency.source_path.clone()),
        ),
        InstallPathOption::new(
            format!("{} Choose a different path", char::from(NerdFont::Edit)),
            None,
        ),
    ];

    match FzfWrapper::select_one(options)? {
        Some(selection) => match selection.value {
            Some(value) => Ok(value),
            None => prompt_custom_install_path(game_name, dependency_id, expected_kind),
        },
        None => Err(anyhow!("Install path selection cancelled")),
    }
}

fn prompt_custom_install_path(
    game_name: &str,
    dependency_id: &str,
    expected_kind: PathContentKind,
) -> Result<String> {
    let selection = PathInputBuilder::new()
        .header(format!(
            "{} Choose install {} for dependency '{}'",
            char::from(NerdFont::Folder),
            if expected_kind.is_file() {
                "file"
            } else {
                "directory"
            },
            dependency_id
        ))
        .manual_prompt(format!(
            "Enter destination {} for '{}' dependency of '{}'",
            if expected_kind.is_file() {
                "file"
            } else {
                "directory"
            },
            dependency_id,
            game_name
        ))
        .scope(match expected_kind {
            PathContentKind::File => FilePickerScope::FilesAndDirectories,
            PathContentKind::Directory => FilePickerScope::Directories,
        })
        .picker_hint(match expected_kind {
            PathContentKind::File => {
                "Select the file location where the dependency should be restored"
            }
            PathContentKind::Directory => {
                "Select the directory where the dependency should be installed"
            }
        })
        .choose()?;

    match selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                Err(anyhow!("Install path cannot be empty"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        PathInputSelection::Picker(path) => Ok(path.display().to_string()),
        PathInputSelection::WinePrefix(path) => Ok(path.display().to_string()),
        PathInputSelection::Cancelled => Err(anyhow!("Install path selection cancelled")),
    }
}

fn prepare_install_target(
    path: &crate::dot::path_serde::TildePath,
    expected_kind: PathContentKind,
) -> Result<bool> {
    let display = path
        .to_tilde_string()
        .unwrap_or_else(|_| path.as_path().display().to_string());

    if expected_kind.is_directory() {
        if path.as_path().exists() {
            if !path.as_path().is_dir() {
                return Err(anyhow!(
                    "Target path {} exists but is not a directory",
                    display
                ));
            }

            let info = get_save_directory_info(path.as_path())?;
            if info.file_count > 0 {
                let prompt = format!(
                    "{} Directory '{}' contains {} file(s). Overwrite contents?",
                    char::from(NerdFont::Warning),
                    display,
                    info.file_count
                );
                match FzfWrapper::builder()
                    .confirm(prompt)
                    .yes_text("Overwrite directory")
                    .no_text("Cancel")
                    .show_confirmation()?
                {
                    ConfirmResult::Yes => {}
                    ConfirmResult::No | ConfirmResult::Cancelled => {
                        println!("{} Installation cancelled.", char::from(NerdFont::Warning));
                        return Ok(false);
                    }
                }
            }
        } else {
            match FzfWrapper::confirm(&format!("Create directory '{}'?", display))? {
                ConfirmResult::Yes => {
                    fs::create_dir_all(path.as_path())
                        .with_context(|| format!("Failed to create directory '{}'.", display))?;
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("{} Installation cancelled.", char::from(NerdFont::Warning));
                    return Ok(false);
                }
            }
        }
    } else {
        let path_ref = path.as_path();

        if path_ref.exists() {
            if path_ref.is_dir() {
                return Err(anyhow!(
                    "Target path {} exists but is a directory; expected a file",
                    display
                ));
            }

            let prompt = format!(
                "{} File '{}' already exists. Overwrite it?",
                char::from(NerdFont::Warning),
                display
            );

            match FzfWrapper::builder()
                .confirm(prompt)
                .yes_text("Overwrite file")
                .no_text("Cancel")
                .show_confirmation()?
            {
                ConfirmResult::Yes => {}
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("{} Installation cancelled.", char::from(NerdFont::Warning));
                    return Ok(false);
                }
            }
        } else if let Some(parent) = path_ref.parent()
            && !parent.exists()
        {
            let parent_display = parent.display();
            match FzfWrapper::confirm(&format!(
                "Parent directory '{}' does not exist. Create it?",
                parent_display
            ))? {
                ConfirmResult::Yes => {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create parent directory '{}'.", parent_display)
                    })?;
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("{} Installation cancelled.", char::from(NerdFont::Warning));
                    return Ok(false);
                }
            }
        }
    }

    Ok(true)
}

fn format_path_for_display(path: &str) -> String {
    let buf = PathBuf::from(path);
    crate::dot::path_serde::TildePath::new(buf)
        .to_tilde_string()
        .unwrap_or_else(|_| path.to_string())
}

#[derive(Clone)]
struct InstallPathOption {
    label: String,
    value: Option<String>,
}

impl InstallPathOption {
    fn new(label: String, value: Option<String>) -> Self {
        Self { label, value }
    }
}

impl FzfSelectable for InstallPathOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_key(&self) -> String {
        self.label.clone()
    }
}
