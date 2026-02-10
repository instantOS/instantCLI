//! Subdirectory selection menu for a repo.

use anyhow::Result;

use crate::dot::config::{Config, Repo};
use crate::dot::db::Database;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::action_menu::handle_subdir_actions;
use super::defaults::{format_default_active_label, handle_edit_default_subdirs};
use super::orphaned::handle_orphaned_subdir_actions;

const ADD_NEW_SENTINEL: &str = "__add_new__";
const EDIT_DEFAULTS_SENTINEL: &str = "__edit_defaults__";
const BACK_SENTINEL: &str = "..";

#[derive(Clone)]
struct SubdirMenuItem {
    subdir: String,
    is_active: bool,
    is_orphaned: bool,
    priority: Option<usize>,
    total_active: usize,
    default_label: Option<String>,
}

impl FzfSelectable for SubdirMenuItem {
    fn fzf_display_text(&self) -> String {
        if self.subdir == BACK_SENTINEL {
            format!("{} Back", format_back_icon())
        } else if self.subdir == ADD_NEW_SENTINEL {
            format!(
                "{} Add Dotfile Dir",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            )
        } else if self.subdir == EDIT_DEFAULTS_SENTINEL {
            format!(
                "{} Edit Default Enabled",
                format_icon_colored(NerdFont::Star, colors::YELLOW)
            )
        } else if self.is_orphaned {
            // Orphaned: enabled in config but not in metadata
            let mismatch_label = format_icon_colored(NerdFont::Warning, colors::YELLOW);
            format!("{} {} [mismatch]", mismatch_label, self.subdir)
        } else {
            let icon = if self.is_active {
                format_icon_colored(NerdFont::Check, colors::GREEN)
            } else {
                format_icon_colored(NerdFont::CrossCircle, colors::RED)
            };
            // Show priority if active and there are multiple active subdirs
            let priority_text = if let Some(p) = self.priority {
                if self.total_active > 1 {
                    format!(" [P{}]", p)
                } else {
                    String::new()
                }
            } else if self.is_active {
                " [default]".to_string()
            } else {
                String::new()
            };
            format!("{} {}{}", icon, self.subdir, priority_text)
        }
    }

    fn fzf_key(&self) -> String {
        self.subdir.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;

        if self.subdir == BACK_SENTINEL {
            FzfPreview::Text("Return to repo menu".to_string())
        } else if self.subdir == ADD_NEW_SENTINEL {
            FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Plus, "Add Dotfile Directory")
                    .text("Create a new dotfile directory in this repository.")
                    .blank()
                    .text("This will:")
                    .bullet("Create the directory in the repository")
                    .bullet("Add it to instantdots.toml")
                    .bullet("You can then enable it from this menu")
                    .build_string(),
            )
        } else if self.subdir == EDIT_DEFAULTS_SENTINEL {
            let current = self
                .default_label
                .as_deref()
                .unwrap_or("Auto (first subdir)");
            FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Star, "Default Enabled")
                    .text("Defaults are only used when you haven't enabled subdirs for this repo.")
                    .blank()
                    .field("Current", current)
                    .blank()
                    .subtext(
                        "Select Auto (first subdir) to remove defaults and use the first subdir.",
                    )
                    .subtext("Select none to disable the repo by default.")
                    .build_string(),
            )
        } else if self.is_orphaned {
            FzfPreview::Text(
                PreviewBuilder::new()
                    .line(
                        colors::YELLOW,
                        Some(NerdFont::Warning),
                        "Configuration Mismatch",
                    )
                    .blank()
                    .text(&format!(
                        "'{}' is enabled in config but not declared",
                        self.subdir
                    ))
                    .text("in the repository's instantdots.toml metadata.")
                    .blank()
                    .text("To resolve this, you can:")
                    .bullet("Disable - Remove from enabled subdirs")
                    .bullet("Add to Metadata - Add to instantdots.toml")
                    .build_string(),
            )
        } else {
            let status = if self.is_active { "Active" } else { "Inactive" };
            let status_color = if self.is_active {
                colors::GREEN
            } else {
                colors::RED
            };
            let mut builder =
                PreviewBuilder::new().line(status_color, None, &format!("Status: {}", status));

            // Add priority info if active
            if let Some(p) = self.priority {
                let priority_hint = if p == 1 && self.total_active > 1 {
                    " (highest priority)"
                } else if p == self.total_active && self.total_active > 1 {
                    " (lowest priority)"
                } else {
                    ""
                };
                builder = builder.line(
                    colors::PEACH,
                    Some(NerdFont::ArrowUp),
                    &format!("Priority: P{}{}", p, priority_hint),
                );
            } else if self.is_active {
                builder = builder.line(colors::PEACH, Some(NerdFont::Info), "Default active");
            }

            builder = builder.indented_line(
                colors::TEXT,
                None,
                &format!("Path: {}/dots/{}", self.subdir, self.subdir),
            );

            FzfPreview::Text(builder.build_string())
        }
    }
}

/// Handle managing subdirs
pub(crate) fn handle_manage_subdirs(
    repo_name: &str,
    config: &mut Config,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        // Load the repo to get available subdirs
        let dotfile_repo = match DotfileRepo::new(config, repo_name.to_string()) {
            Ok(repo) => repo,
            Err(e) => {
                FzfWrapper::message(&format!("Failed to load repository: {}", e))?;
                return Ok(());
            }
        };

        let active_subdirs = config.get_active_subdirs(repo_name);
        let repo_config = config.repos.iter().find(|r| r.name == repo_name);
        let configured_subdirs = repo_config.and_then(|repo| repo.active_subdirectories.clone());

        let mut subdir_items =
            build_base_items(&dotfile_repo, &active_subdirs, configured_subdirs.as_ref());

        let is_read_only = repo_config.map(|r| r.read_only).unwrap_or(false);
        let is_external = dotfile_repo.is_external(config);

        if !is_read_only && !is_external && dotfile_repo.meta.dots_dirs.len() > 1 {
            subdir_items.push(SubdirMenuItem {
                subdir: EDIT_DEFAULTS_SENTINEL.to_string(),
                is_active: false,
                is_orphaned: false,
                priority: None,
                total_active: 0,
                default_label: Some(format_default_active_label(&dotfile_repo.meta)),
            });
        }

        if !is_read_only && !is_external {
            subdir_items.push(SubdirMenuItem {
                subdir: ADD_NEW_SENTINEL.to_string(),
                is_active: false,
                is_orphaned: false,
                priority: None,
                total_active: 0,
                default_label: None,
            });
        }

        // Add orphaned subdirs (enabled in config but not in metadata)
        let orphaned = dotfile_repo.get_orphaned_active_subdirs(config);
        for subdir in orphaned {
            subdir_items.push(SubdirMenuItem {
                subdir,
                is_active: true,
                is_orphaned: true,
                priority: None,
                total_active: 0,
                default_label: None,
            });
        }

        // Add back option
        subdir_items.push(SubdirMenuItem {
            subdir: BACK_SENTINEL.to_string(),
            is_active: false,
            is_orphaned: false,
            priority: None,
            total_active: 0,
            default_label: None,
        });

        let header_text = build_header_text(repo_name, repo_config, &dotfile_repo);
        let selection = select_subdir(&mut cursor, &header_text, &subdir_items)?;

        let Some(selected) = selection else {
            return Ok(());
        };

        if selected.subdir == BACK_SENTINEL {
            return Ok(());
        }

        if selected.subdir == EDIT_DEFAULTS_SENTINEL {
            handle_edit_default_subdirs(repo_name, &dotfile_repo, config)?;
            continue;
        }

        // Handle add new subdirectory
        if selected.subdir == ADD_NEW_SENTINEL {
            handle_add_new_subdir(&dotfile_repo, config)?;
            continue;
        }

        // Handle orphaned subdir with special resolution actions
        if selected.is_orphaned {
            handle_orphaned_subdir_actions(repo_name, &selected.subdir, &dotfile_repo, config)?;
            continue;
        }

        // Show action menu for the selected subdirectory
        handle_subdir_actions(repo_name, &selected.subdir, config, db, debug)?;
    }
}

fn build_base_items(
    dotfile_repo: &DotfileRepo,
    active_subdirs: &[String],
    configured_subdirs: Option<&Vec<String>>,
) -> Vec<SubdirMenuItem> {
    dotfile_repo
        .meta
        .dots_dirs
        .iter()
        .map(|subdir| {
            let is_active = active_subdirs.contains(subdir);
            let is_configured = configured_subdirs
                .map(|subdirs| subdirs.contains(subdir))
                .unwrap_or(false);
            let priority = if is_active {
                active_subdirs
                    .iter()
                    .position(|s| s == subdir)
                    .map(|i| i + 1)
            } else {
                None
            };
            SubdirMenuItem {
                subdir: subdir.clone(),
                is_active,
                is_orphaned: false,
                priority,
                total_active: if is_configured {
                    active_subdirs.len()
                } else {
                    0
                },
                default_label: None,
            }
        })
        .collect()
}

fn build_header_text(
    repo_name: &str,
    repo_config: Option<&Repo>,
    dotfile_repo: &DotfileRepo,
) -> String {
    let defaults_disabled = repo_config
        .map(|repo| repo.active_subdirectories.is_none())
        .unwrap_or(false)
        && dotfile_repo
            .meta
            .default_active_subdirs
            .as_ref()
            .map(|dirs| dirs.is_empty())
            .unwrap_or(false);

    if defaults_disabled {
        format!(
            "Subdirectories: {}\nDefaults disabled - repo inactive until you enable subdirs",
            repo_name
        )
    } else {
        format!("Subdirectories: {}", repo_name)
    }
}

fn select_subdir(
    cursor: &mut MenuCursor,
    header_text: &str,
    subdir_items: &[SubdirMenuItem],
) -> Result<Option<SubdirMenuItem>> {
    let mut builder = FzfWrapper::builder()
        .header(Header::fancy(header_text))
        .prompt("Select subdirectory")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(subdir_items) {
        builder = builder.initial_index(index);
    }

    let selection = builder.select(subdir_items.to_vec())?;

    match selection {
        FzfResult::Selected(item) => {
            cursor.update(&item, subdir_items);
            Ok(Some(item))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

fn handle_add_new_subdir(dotfile_repo: &DotfileRepo, config: &Config) -> Result<()> {
    // Prompt for new directory name
    let new_dir = match FzfWrapper::builder()
        .input()
        .prompt("New dotfile directory name")
        .ghost("e.g. themes, config, scripts")
        .input_result()?
    {
        FzfResult::Selected(s) if !s.trim().is_empty() => s.trim().to_string(),
        FzfResult::Cancelled => return Ok(()),
        _ => return Ok(()),
    };

    // Get repo path and add the directory
    let local_path = dotfile_repo.local_path(config)?;
    match crate::dot::meta::add_dots_dir(&local_path, &new_dir) {
        Ok(()) => {
            FzfWrapper::message(&format!(
                "Created dotfile directory '{}'. Enable it to start using.",
                new_dir
            ))?;
        }
        Err(e) => {
            FzfWrapper::message(&format!("Error: {}", e))?;
        }
    }

    Ok(())
}
