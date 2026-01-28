use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::preview::PreviewContext;
use crate::preview::default_apps::display_app_name;
use crate::preview::helpers::push_bullets;
use crate::settings::defaultapps::{build_mime_to_apps_map, get_apps_for_mime, query_default_app};
use crate::ui::catppuccin::colors;
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_mime_type_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(mime_type) = ctx.key() else {
        return Ok(String::new());
    };

    let category = mime_category(mime_type);
    let extensions = mime_extensions(mime_type);
    let default = query_default_app(mime_type)
        .ok()
        .flatten()
        .map(|desktop_id| display_app_name(&desktop_id))
        .unwrap_or_else(|| "(not set)".to_string());

    let app_map = build_mime_to_apps_map().unwrap_or_default();
    let mut apps = get_apps_for_mime(mime_type, &app_map);
    let current_default = query_default_app(mime_type).ok().flatten();

    apps.sort();
    let app_lines: Vec<String> = apps
        .into_iter()
        .take(8)
        .map(|desktop_id| {
            let label = display_app_name(&desktop_id);
            if current_default.as_deref() == Some(desktop_id.as_str()) {
                format!("{label} (current)")
            } else {
                label
            }
        })
        .collect();

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::File, "MIME Type")
        .subtext("Select a default application for this MIME type.")
        .blank()
        .line(colors::TEAL, Some(NerdFont::ChevronRight), "Details")
        .field_indented("Type", mime_type)
        .raw(&format!("  Category: {category}"))
        .blank()
        .line(
            colors::TEAL,
            Some(NerdFont::ChevronRight),
            "Common Extensions",
        );

    if extensions.is_empty() {
        builder = builder.bullet("(none registered)");
    } else {
        builder = push_bullets(builder, &extensions);
    }

    builder = builder
        .blank()
        .line(
            colors::TEAL,
            Some(NerdFont::ChevronRight),
            "Current Default",
        )
        .field_indented(mime_type, &default)
        .blank()
        .line(
            colors::TEAL,
            Some(NerdFont::ChevronRight),
            "Available Applications",
        );

    if app_lines.is_empty() {
        builder = builder.bullet("(none registered)");
    } else {
        builder = push_bullets(builder, &app_lines);
    }

    Ok(builder.build_string())
}

fn mime_category(mime_type: &str) -> &'static str {
    if mime_type.starts_with("image/") {
        return "Image file";
    }
    if mime_type.starts_with("video/") {
        return "Video file";
    }
    if mime_type.starts_with("audio/") {
        return "Audio file";
    }
    if mime_type.starts_with("text/") {
        return "Text document";
    }
    if mime_type == "application/pdf" {
        return "PDF document";
    }
    if mime_type.contains("zip") || mime_type.contains("tar") || mime_type.contains("rar") {
        return "Archive file";
    }
    if mime_type.contains("7z") {
        return "Archive file";
    }
    if mime_type == "application/x-appimage" {
        return "AppImage executable";
    }
    if mime_type.starts_with("application/") {
        return "Application data";
    }
    "Other"
}

fn mime_extensions(mime_type: &str) -> Vec<String> {
    let canonical = canonical_mime_type(mime_type);
    let mut entries: Vec<GlobEntry> = Vec::new();

    for path in mime_globs2_paths() {
        if let Ok(mut list) = parse_globs2(&path, &canonical) {
            entries.append(&mut list);
        }
    }

    entries.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| a.pattern.cmp(&b.pattern))
    });

    let mut seen = HashSet::new();
    let mut extensions = Vec::new();

    for entry in entries {
        let trimmed = entry.pattern.trim_start_matches('*').to_string();
        let label = if trimmed.is_empty() {
            entry.pattern
        } else {
            trimmed
        };
        if seen.insert(label.clone()) {
            extensions.push(label);
            if extensions.len() >= 8 {
                break;
            }
        }
    }

    extensions
}

fn canonical_mime_type(mime_type: &str) -> String {
    for path in mime_alias_paths() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let alias = parts.next().unwrap_or("");
                let canonical = parts.next().unwrap_or("");
                if alias == mime_type && !canonical.is_empty() {
                    return canonical.to_string();
                }
            }
        }
    }
    mime_type.to_string()
}

fn mime_alias_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/mime/aliases"));
    }
    paths.push(PathBuf::from("/usr/local/share/mime/aliases"));
    paths.push(PathBuf::from("/usr/share/mime/aliases"));
    paths.into_iter().filter(|p| p.exists()).collect()
}

fn mime_globs2_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/mime/globs2"));
    }
    paths.push(PathBuf::from("/usr/local/share/mime/globs2"));
    paths.push(PathBuf::from("/usr/share/mime/globs2"));
    paths.into_iter().filter(|p| p.exists()).collect()
}

fn parse_globs2(path: &Path, mime_type: &str) -> Result<Vec<GlobEntry>> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ':');
        let weight = parts.next().unwrap_or("0");
        let mime = parts.next().unwrap_or("");
        let pattern = parts.next().unwrap_or("");
        if mime != mime_type || pattern.is_empty() {
            continue;
        }
        let weight = weight.parse::<i32>().unwrap_or(0);
        entries.push(GlobEntry {
            weight,
            pattern: pattern.to_string(),
        });
    }

    Ok(entries)
}

#[derive(Debug, Clone)]
struct GlobEntry {
    weight: i32,
    pattern: String,
}
