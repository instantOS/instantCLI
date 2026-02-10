use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap};

use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuItem};
use crate::settings::SettingsContext;
use crate::settings::installable_packages::{
    self, ARCHIVE_MANAGERS, FILE_MANAGERS, IMAGE_VIEWERS, InstallableApp, PDF_VIEWERS,
    TEXT_EDITORS, VIDEO_PLAYERS, WEB_BROWSERS,
};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::prelude::*;

use super::app_info::{ApplicationInfo, get_application_info};
use super::mime_cache::{build_mime_to_apps_map, get_apps_for_mime};
use super::mime_info::{MimeTypeInfo, get_all_mime_types, get_mime_type_info};
use super::mime_sets::{ARCHIVE_MIME_TYPES, AUDIO_MIME_TYPES, IMAGE_MIME_TYPES, VIDEO_MIME_TYPES};
use super::system::{query_default_app, set_default_app};

pub fn manage_default_apps(ctx: &mut SettingsContext) -> Result<()> {
    if which::which("xdg-mime").is_err() {
        emit(
            Level::Error,
            "settings.defaultapps.no_xdg_mime",
            &format!(
                "{} xdg-mime command not found. Please install xdg-utils.",
                char::from(NerdFont::CrossCircle)
            ),
            None,
        );
        return Ok(());
    }

    let mime_map = build_mime_to_apps_map().context("Failed to build MIME type map")?;
    let mime_type_strings = get_all_mime_types(&mime_map);

    if mime_type_strings.is_empty() {
        emit(
            Level::Warn,
            "settings.defaultapps.no_mime_types",
            &format!(
                "{} No MIME types found in mimeinfo.cache files.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(());
    }

    let mime_types: Vec<MimeTypeInfo> = mime_type_strings
        .iter()
        .map(|mt| get_mime_type_info(mt))
        .collect();

    let selected_mime_info = match FzfWrapper::builder()
        .prompt("Select MIME type: ")
        .header(Header::fancy(
            "Default Applications\nSelect a MIME type to configure",
        ))
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(mime_types)?
    {
        FzfResult::Selected(info) => info,
        _ => {
            ctx.emit_info("settings.defaultapps.cancelled", "No MIME type selected.");
            return Ok(());
        }
    };

    let selected_mime = &selected_mime_info.mime_type;
    let apps = get_apps_for_mime(selected_mime, &mime_map);

    if apps.is_empty() {
        emit(
            Level::Warn,
            "settings.defaultapps.no_apps",
            &format!(
                "{} No applications found for {}",
                char::from(NerdFont::Warning),
                selected_mime
            ),
            None,
        );
        return Ok(());
    }

    let current_default = query_default_app(selected_mime)?;
    let current_default_id = current_default.as_deref();
    let header_text = if let Some(ref default) = current_default {
        format!(
            "MIME type: {} {}\nCurrent default: {}",
            char::from(selected_mime_info.icon),
            selected_mime,
            default
        )
    } else {
        format!(
            "MIME type: {} {}\nCurrent default: (none)",
            char::from(selected_mime_info.icon),
            selected_mime
        )
    };

    let app_infos: Vec<ApplicationInfo> = apps
        .iter()
        .map(|desktop_id| {
            let mut info = get_application_info(desktop_id);
            info.is_default = current_default_id == Some(desktop_id.as_str());
            info
        })
        .collect();

    let initial_index = app_infos.iter().position(|info| info.is_default);

    let mut app_menu = FzfWrapper::builder()
        .prompt("Select application: ")
        .header(Header::fancy(header_text))
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = initial_index {
        app_menu = app_menu.initial_index(index);
    }

    let selected_app_info = match app_menu.select(app_infos)? {
        FzfResult::Selected(app_info) => app_info,
        _ => {
            ctx.emit_info("settings.defaultapps.cancelled", "No application selected.");
            return Ok(());
        }
    };

    let desktop_file = &selected_app_info.desktop_id;

    set_default_app(selected_mime, desktop_file).context("Failed to set default application")?;

    ctx.notify(
        "Default application",
        &format!("Set {} as default for {}", desktop_file, selected_mime),
    );

    Ok(())
}

pub fn set_default_browser(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, &["text/html"], "Web Browser")
}

pub fn set_default_email(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, &["x-scheme-handler/mailto"], "Email Client")
}

pub fn set_default_file_manager(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, &["inode/directory"], "File Manager")
}

pub fn set_default_text_editor(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, &["text/plain"], "Text Editor")
}

pub fn set_default_image_viewer(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, IMAGE_MIME_TYPES, "Image Viewer")
}

pub fn set_default_video_player(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, VIDEO_MIME_TYPES, "Video Player")
}

pub fn set_default_audio_player(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, AUDIO_MIME_TYPES, "Audio Player")
}

pub fn set_default_pdf_viewer(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, &["application/pdf"], "PDF Viewer")
}

pub fn set_default_archive_manager(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mimes(ctx, ARCHIVE_MIME_TYPES, "Archive Manager")
}

#[derive(Clone, Debug)]
enum DefaultAppMenuEntry {
    App(ApplicationInfo),
    InstallMore,
}

impl FzfSelectable for DefaultAppMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            DefaultAppMenuEntry::App(app) => app.fzf_display_text(),
            DefaultAppMenuEntry::InstallMore => format!("{} Install more...", NerdFont::Package),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            DefaultAppMenuEntry::App(app) => app.fzf_key(),
            DefaultAppMenuEntry::InstallMore => "__install_more__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            DefaultAppMenuEntry::App(app) => app.fzf_preview(),
            DefaultAppMenuEntry::InstallMore => PreviewBuilder::new()
                .header(NerdFont::Package, "Install More")
                .text("Search and install more applications")
                .text("from the official repositories.")
                .build(),
        }
    }
}

fn manage_default_app_for_mimes(
    ctx: &mut SettingsContext,
    mime_types: &[&str],
    app_name: &str,
) -> Result<()> {
    let Some(primary_mime) = mime_types.first() else {
        ctx.emit_info("settings.defaultapps.cancelled", "No MIME types provided.");
        return Ok(());
    };

    let installable_apps = installable_apps_for(app_name);

    loop {
        let mime_map = build_mime_to_apps_map().context("Failed to build MIME type map")?;
        let app_desktop_ids = collect_supported_apps(mime_types, &mime_map);
        let current_default = query_default_app(primary_mime).ok().flatten();
        let header_text = selection_header(app_name, current_default.clone(), mime_types);

        let mut entries: Vec<MenuItem<DefaultAppMenuEntry>> = Vec::new();

        if let Some(apps) = installable_apps {
            entries.push(MenuItem::entry(DefaultAppMenuEntry::InstallMore));
            if !app_desktop_ids.is_empty() {
                entries.push(MenuItem::line());
            }
        }

        let app_infos: Vec<ApplicationInfo> = app_desktop_ids
            .iter()
            .map(|desktop_id| {
                let mut info = get_application_info(desktop_id);
                info.is_default = current_default.as_deref() == Some(desktop_id.as_str());
                info
            })
            .collect();

        for app_info in &app_infos {
            entries.push(MenuItem::entry(DefaultAppMenuEntry::App(app_info.clone())));
        }

        if entries.is_empty() {
            handle_missing_apps(ctx, app_name, mime_types, installable_apps.is_some());
            return Ok(());
        }

        // Check if only "Install more" is available
        if entries.len() == 1 && installable_apps.is_some() {
            handle_missing_apps(ctx, app_name, mime_types, true);
        }

        let initial_index = app_infos.iter().position(|info| info.is_default);

        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select {}: ", app_name))
            .header(Header::fancy(header_text))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = initial_index {
            // Adjust index if we have InstallMore and line separator
            let offset = if installable_apps.is_some() {
                if app_desktop_ids.is_empty() { 1 } else { 2 }
            } else {
                0
            };
            builder = builder.initial_index(index + offset);
        }

        let selected = builder.select_menu(entries)?;

        match selected {
            FzfResult::Selected(entry) => match entry {
                DefaultAppMenuEntry::InstallMore => {
                    if let Some(apps) = installable_apps {
                        let installed =
                            installable_packages::show_install_more_menu(app_name, apps)?;
                        if installed {
                            continue;
                        }
                    }
                    continue;
                }
                DefaultAppMenuEntry::App(app_info) => {
                    apply_default_for_mimes(mime_types, &app_info.desktop_id)?;
                    notify_success(ctx, app_name, mime_types.len(), &app_info);
                    return Ok(());
                }
            },
            _ => {
                ctx.emit_info("settings.defaultapps.cancelled", "No changes made.");
                return Ok(());
            }
        }
    }
}

fn installable_apps_for(app_name: &str) -> Option<&'static [InstallableApp]> {
    match app_name {
        "PDF Viewer" => Some(PDF_VIEWERS),
        "Image Viewer" => Some(IMAGE_VIEWERS),
        "Video Player" => Some(VIDEO_PLAYERS),
        "Text Editor" => Some(TEXT_EDITORS),
        "Archive Manager" => Some(ARCHIVE_MANAGERS),
        "File Manager" => Some(FILE_MANAGERS),
        "Web Browser" => Some(WEB_BROWSERS),
        "Audio Player" => None,
        _ => None,
    }
}

fn collect_supported_apps(
    mime_types: &[&str],
    mime_map: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut sets: Vec<BTreeSet<String>> = mime_types
        .iter()
        .map(|mime_type| {
            mime_map
                .get(*mime_type)
                .map(|apps| apps.iter().cloned().collect())
                .unwrap_or_default()
        })
        .collect();

    let mut intersection = match sets.len() {
        0 => return Vec::new(),
        _ => sets.remove(0),
    };

    for set in sets {
        intersection = intersection.intersection(&set).cloned().collect();
    }

    intersection.into_iter().collect()
}

fn selection_header(
    app_name: &str,
    current_default: Option<String>,
    mime_types: &[&str],
) -> String {
    let current = current_default.as_deref().unwrap_or("(none)");

    if mime_types.len() > 1 {
        format!(
            "Select default {} application\nCurrent: {}\n(Sets default for {} MIME types)",
            app_name,
            current,
            mime_types.len()
        )
    } else {
        format!(
            "Select default {} application\nCurrent: {}",
            app_name, current
        )
    }
}

fn handle_missing_apps(
    ctx: &mut SettingsContext,
    app_name: &str,
    mime_types: &[&str],
    can_install: bool,
) {
    if can_install {
        ctx.emit_info(
            "settings.defaultapps.no_apps_install",
            &format!(
                "No {} applications installed that support all required MIME types ({}). Select 'Install more...' to install one.",
                app_name,
                mime_types.join(", ")
            ),
        );
    } else {
        ctx.emit_info(
            "settings.defaultapps.no_apps",
            &format!(
                "No applications found for {} that support all required MIME types ({}). Install an application first.",
                app_name,
                mime_types.join(", ")
            ),
        );
    }
}

fn apply_default_for_mimes(mime_types: &[&str], desktop_file: &str) -> Result<()> {
    for mime_type in mime_types {
        set_default_app(mime_type, desktop_file)
            .with_context(|| format!("Failed to set default for {}", mime_type))?;
    }

    Ok(())
}

fn notify_success(
    ctx: &mut SettingsContext,
    app_name: &str,
    mime_count: usize,
    app_info: &ApplicationInfo,
) {
    let desktop_file = &app_info.desktop_id;
    let app_display = app_info.name.as_deref().unwrap_or(desktop_file);

    let message = if mime_count > 1 {
        format!(
            "Set {} as default for {} MIME types",
            app_display, mime_count
        )
    } else {
        format!("Set {} as default", app_display)
    };

    ctx.notify(&format!("Default {}", app_name), &message);
}
