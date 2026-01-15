use anyhow::{Context, Result};
use freedesktop_file_parser::parse;
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::context::SettingsContext;

/// Information about a MIME type for display purposes
#[derive(Debug, Clone)]
struct MimeTypeInfo {
    mime_type: String,
    icon: NerdFont,
    description: Option<String>,
}

impl FzfSelectable for MimeTypeInfo {
    fn fzf_display_text(&self) -> String {
        let icon = char::from(self.icon);
        if let Some(desc) = &self.description {
            format!("{} {} - {}", icon, self.mime_type, desc)
        } else {
            format!("{} {}", icon, self.mime_type)
        }
    }

    fn fzf_key(&self) -> String {
        self.mime_type.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        // Use a command-based preview so FZF only runs it for the selected item
        // The {} placeholder will be replaced with fzf_key() (the MIME type)
        FzfPreview::Command(create_mime_preview_command())
    }
}

/// Create a shell command for previewing MIME type information
fn create_mime_preview_command() -> String {
    // This command will be run by FZF with the MIME type passed as $1
    // FZF extracts it from the tab-separated line: display\tmime_type
    r#"bash -c '
mime_type="$1"

echo "═══════════════════════════════════════════════════"
echo "MIME Type: $mime_type"
echo "═══════════════════════════════════════════════════"
echo ""

# Current default
default=$(xdg-mime query default "$mime_type" 2>/dev/null || echo "")
if [ -n "$default" ]; then
    echo "📌 Current Default:"
    # Try to get app name
    for dir in "$HOME/.local/share/applications" "/usr/share/applications" "/var/lib/flatpak/exports/share/applications" "$HOME/.local/share/flatpak/exports/share/applications"; do
        if [ -f "$dir/$default" ]; then
            name=$(grep "^Name=" "$dir/$default" 2>/dev/null | head -1 | cut -d= -f2)
            [ -n "$name" ] && echo "   $name ($default)" && break
        fi
    done
    [ -z "$name" ] && echo "   $default"
else
    echo "📌 Current Default:"
    echo "   (not set)"
fi
echo ""

# Common extensions
echo "📄 Common Extensions:"
case "$mime_type" in
    image/jpeg) echo "   .jpg, .jpeg" ;;
    image/png) echo "   .png" ;;
    image/gif) echo "   .gif" ;;
    image/webp) echo "   .webp" ;;
    image/svg+xml) echo "   .svg" ;;
    video/mp4) echo "   .mp4" ;;
    video/x-matroska) echo "   .mkv" ;;
    video/webm) echo "   .webm" ;;
    video/x-msvideo) echo "   .avi" ;;
    audio/mpeg) echo "   .mp3" ;;
    audio/ogg) echo "   .ogg, .opus" ;;
    audio/flac) echo "   .flac" ;;
    audio/x-wav) echo "   .wav" ;;
    application/pdf) echo "   .pdf" ;;
    application/zip) echo "   .zip" ;;
    application/x-tar) echo "   .tar" ;;
    application/gzip) echo "   .gz" ;;
    text/plain) echo "   .txt" ;;
    text/html) echo "   .html" ;;
    text/markdown) echo "   .md" ;;
    application/json) echo "   .json" ;;
    application/xml) echo "   .xml" ;;
    application/x-appimage) echo "   .AppImage" ;;
    *) echo "   (varies)" ;;
esac
echo ""

# Available applications (limit to top 8)
echo "📋 Available Applications:"
count=0
for dir in "$HOME/.local/share/applications" "/usr/share/applications" "/var/lib/flatpak/exports/share/applications" "$HOME/.local/share/flatpak/exports/share/applications"; do
    cache="$dir/mimeinfo.cache"
    [ ! -f "$cache" ] && continue
    
    apps=$(grep "^$mime_type=" "$cache" 2>/dev/null | cut -d= -f2 | tr ";" "\n" | grep -v "^$")
    [ -z "$apps" ] && continue
    
    echo "$apps" | while IFS= read -r app; do
        [ -z "$app" ] || [ $count -ge 8 ] && continue
        
        # Find desktop file and get name
        for adir in "$HOME/.local/share/applications" "/usr/share/applications" "/var/lib/flatpak/exports/share/applications" "$HOME/.local/share/flatpak/exports/share/applications"; do
            dfile="$adir/$app"
            if [ -f "$dfile" ]; then
                name=$(grep "^Name=" "$dfile" 2>/dev/null | head -1 | cut -d= -f2)
                if [ -n "$name" ]; then
                    if [ "$app" = "$default" ]; then
                        echo "   ✓ $name (current)"
                    else
                        echo "   • $name"
                    fi
                    count=$((count + 1))
                    break
                fi
            fi
        done
    done
done
[ $count -eq 0 ] && echo "   (none registered)"
echo ""

# Category description
echo "ℹ️  Category:"
case "$mime_type" in
    image/*) echo "   Image file" ;;
    video/*) echo "   Video file" ;;
    audio/*) echo "   Audio file" ;;
    text/*) echo "   Text document" ;;
    application/pdf) echo "   PDF document" ;;
    application/*zip*|application/*tar*|application/*rar*|application/*7z*) echo "   Archive file" ;;
    application/x-appimage) echo "   AppImage executable" ;;
    application/*) echo "   Application data" ;;
    *) echo "   Other" ;;
esac
'"#.to_string()
}

/// Information about an application for display purposes
#[derive(Debug, Clone)]
struct ApplicationInfo {
    desktop_id: String,
    name: Option<String>,
    comment: Option<String>,
    icon: Option<String>,
    exec: Option<String>,
}

impl FzfSelectable for ApplicationInfo {
    fn fzf_display_text(&self) -> String {
        if let Some(name) = &self.name {
            format!("󰘔 {} ({})", name, self.desktop_id)
        } else {
            format!("󰘔 {}", self.desktop_id)
        }
    }

    fn fzf_key(&self) -> String {
        self.desktop_id.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        // Build preview text from already-loaded data (no external calls)
        let mut preview = String::new();

        if let Some(name) = &self.name {
            preview.push_str(&format!("Application: {}\n", name));
        } else {
            preview.push_str(&format!("Desktop ID: {}\n", self.desktop_id));
        }

        if let Some(comment) = &self.comment {
            preview.push_str(&format!("\nDescription:\n{}\n", comment));
        }

        if let Some(exec) = &self.exec {
            preview.push_str(&format!("\nCommand:\n{}\n", exec));
        }

        if let Some(icon) = &self.icon {
            preview.push_str(&format!("\nIcon: {}\n", icon));
        }

        preview.push_str(&format!("\nDesktop File:\n{}\n", self.desktop_id));

        FzfPreview::Text(preview)
    }
}

/// Get application info by parsing its desktop file
fn get_application_info(desktop_id: &str) -> ApplicationInfo {
    // Try to find and parse the desktop file
    let directories = [
        format!(
            "{}/.local/share/applications",
            std::env::var("HOME").unwrap_or_default()
        ),
        format!(
            "{}/.local/share/flatpak/exports/share/applications",
            std::env::var("HOME").unwrap_or_default()
        ),
        "/var/lib/flatpak/exports/share/applications".to_string(),
        "/usr/share/applications".to_string(),
    ];

    for dir in &directories {
        let path = PathBuf::from(dir).join(desktop_id);
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(desktop_file) = parse(&content)
        {
            use freedesktop_file_parser::EntryType;

            let exec = match &desktop_file.entry.entry_type {
                EntryType::Application(app) => app.exec.clone(),
                _ => None,
            };

            return ApplicationInfo {
                desktop_id: desktop_id.to_string(),
                name: Some(desktop_file.entry.name.default.clone()),
                comment: desktop_file
                    .entry
                    .comment
                    .as_ref()
                    .map(|c| c.default.clone()),
                icon: desktop_file.entry.icon.as_ref().map(|i| i.content.clone()),
                exec,
            };
        }
    }

    // Fallback if desktop file not found or can't be parsed
    ApplicationInfo {
        desktop_id: desktop_id.to_string(),
        name: None,
        comment: None,
        icon: None,
        exec: None,
    }
}

/// Get icon and description for a MIME type
fn get_mime_type_info(mime_type: &str) -> MimeTypeInfo {
    // Check for exact matches first
    if let Some((icon, desc)) = get_exact_mime_info(mime_type) {
        return MimeTypeInfo {
            mime_type: mime_type.to_string(),
            icon,
            description: Some(desc.to_string()),
        };
    }

    // Check for category matches (e.g., image/*, video/*)
    if let Some((prefix, _)) = mime_type.split_once('/') {
        let (icon, desc) = match prefix {
            "image" => (NerdFont::Image, Some("Image Viewer")),
            "video" => (NerdFont::Video, Some("Video Player")),
            "audio" => (NerdFont::Music, Some("Music Player")),
            "text" => (NerdFont::FileText, Some("Text Editor")),
            "application" => (NerdFont::Package, Some("Application")),
            "inode" => (NerdFont::Folder, Some("File Manager")),
            "x-scheme-handler" => (NerdFont::Link, Some("URL Handler")),
            "message" => (NerdFont::ExternalLink, Some("Email Client")),
            _ => (NerdFont::File, None),
        };

        return MimeTypeInfo {
            mime_type: mime_type.to_string(),
            icon,
            description: desc.map(String::from),
        };
    }

    // Default fallback
    MimeTypeInfo {
        mime_type: mime_type.to_string(),
        icon: NerdFont::File,
        description: None,
    }
}

/// Exact mappings for common MIME types
/// This makes it easy to add new specific mappings with nice DX
/// Uses user-friendly names that non-technical users would search for
fn get_exact_mime_info(mime_type: &str) -> Option<(NerdFont, &'static str)> {
    let mapping = match mime_type {
        // Common applications - what users actually search for
        "inode/directory" => (NerdFont::Folder, "File Manager"),
        "text/html" => (NerdFont::Globe, "Web Browser"),
        "x-scheme-handler/http" => (NerdFont::Globe, "Web Browser (HTTP)"),
        "x-scheme-handler/https" => (NerdFont::Globe, "Web Browser (HTTPS)"),
        "x-scheme-handler/mailto" => (NerdFont::ExternalLink, "Email Client"),
        "message/rfc822" => (NerdFont::ExternalLink, "Email Client"),

        // Images
        "image/jpeg" | "image/jpg" => (NerdFont::Image, "Image Viewer (JPEG)"),
        "image/png" => (NerdFont::Image, "Image Viewer (PNG)"),
        "image/gif" => (NerdFont::Image, "Image Viewer (GIF)"),
        "image/svg+xml" => (NerdFont::Image, "Image Viewer (SVG)"),
        "image/webp" => (NerdFont::Image, "Image Viewer (WebP)"),
        "image/bmp" => (NerdFont::Image, "Image Viewer (BMP)"),
        "image/tiff" => (NerdFont::Image, "Image Viewer (TIFF)"),

        // Documents
        "application/pdf" => (NerdFont::FilePdf, "PDF Viewer"),
        "application/vnd.oasis.opendocument.text" => (NerdFont::FileText, "Document Editor (ODT)"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            (NerdFont::FileWord, "Document Editor (Word)")
        }
        "application/msword" => (NerdFont::FileWord, "Document Editor (Word)"),
        "application/vnd.ms-excel" => (NerdFont::FileExcel, "Spreadsheet Editor (Excel)"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            (NerdFont::FileExcel, "Spreadsheet Editor (Excel)")
        }
        "application/vnd.ms-powerpoint" => (
            NerdFont::FilePresentation,
            "Presentation Editor (PowerPoint)",
        ),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => (
            NerdFont::FilePresentation,
            "Presentation Editor (PowerPoint)",
        ),

        // Archives
        "application/zip" => (NerdFont::Archive, "Archive Manager (ZIP)"),
        "application/x-tar" => (NerdFont::Archive, "Archive Manager (TAR)"),
        "application/x-7z-compressed" => (NerdFont::Archive, "Archive Manager (7-Zip)"),
        "application/x-rar" => (NerdFont::Archive, "Archive Manager (RAR)"),
        "application/gzip" => (NerdFont::Archive, "Archive Manager (GZIP)"),
        "application/x-bzip2" => (NerdFont::Archive, "Archive Manager (BZIP2)"),
        "application/x-xz" => (NerdFont::Archive, "Archive Manager (XZ)"),

        // Video
        "video/mp4" => (NerdFont::Video, "Video Player (MP4)"),
        "video/x-matroska" => (NerdFont::Video, "Video Player (MKV)"),
        "video/webm" => (NerdFont::Video, "Video Player (WebM)"),
        "video/mpeg" => (NerdFont::Video, "Video Player (MPEG)"),
        "video/x-msvideo" => (NerdFont::Video, "Video Player (AVI)"),

        // Audio
        "audio/mpeg" => (NerdFont::Music, "Music Player (MP3)"),
        "audio/ogg" => (NerdFont::Music, "Music Player (OGG)"),
        "audio/flac" => (NerdFont::Music, "Music Player (FLAC)"),
        "audio/x-wav" => (NerdFont::Music, "Music Player (WAV)"),
        "audio/aac" => (NerdFont::Music, "Music Player (AAC)"),

        // Text
        "text/plain" => (NerdFont::FileText, "Text Editor"),
        "text/css" => (NerdFont::Code, "Code Editor (CSS)"),
        "text/javascript" => (NerdFont::Code, "Code Editor (JavaScript)"),
        "application/json" => (NerdFont::Code, "Code Editor (JSON)"),
        "application/xml" => (NerdFont::Code, "Code Editor (XML)"),
        "text/x-python" => (NerdFont::Code, "Code Editor (Python)"),
        "text/x-rust" => (NerdFont::Code, "Code Editor (Rust)"),
        "text/x-c" => (NerdFont::Code, "Code Editor (C)"),
        "text/x-c++" => (NerdFont::Code, "Code Editor (C++)"),
        "text/markdown" => (NerdFont::FileText, "Text Editor (Markdown)"),

        // System
        "application/x-executable" => (NerdFont::Gear, "Executable Program"),
        "application/x-sharedlib" => (NerdFont::Gear, "Shared Library"),
        "application/x-shellscript" => (NerdFont::Terminal, "Terminal (Shell Script)"),

        // Special
        "application/vnd.appimage" => (NerdFont::Package, "AppImage Launcher"),
        "application/vnd.flatpak.ref" => (NerdFont::Package, "Flatpak Installer"),
        "application/x-iso9660-image" => (NerdFont::Archive, "Disk Image Viewer (ISO)"),

        _ => return None,
    };

    Some(mapping)
}

/// Get all XDG data directories where mimeinfo.cache files may be located
fn get_mimeinfo_cache_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // User local applications (highest priority)
    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        paths.push(home_path.join(".local/share/applications/mimeinfo.cache"));

        // User flatpak apps
        paths
            .push(home_path.join(".local/share/flatpak/exports/share/applications/mimeinfo.cache"));
    }

    // System flatpak apps
    paths.push(PathBuf::from(
        "/var/lib/flatpak/exports/share/applications/mimeinfo.cache",
    ));

    // System applications directory
    paths.push(PathBuf::from("/usr/share/applications/mimeinfo.cache"));

    // Additional XDG data dirs from environment
    if let Ok(xdg_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in xdg_dirs.split(':') {
            if !dir.is_empty() {
                paths.push(PathBuf::from(dir).join("applications/mimeinfo.cache"));
            }
        }
    }

    // Filter to only existing files
    paths.into_iter().filter(|p| p.exists()).collect()
}

/// Parse a mimeinfo.cache file and return a mapping of MIME types to desktop files
fn parse_mimeinfo_cache(path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open mimeinfo.cache at {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    let mut in_mime_cache = false;
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Check for [MIME Cache] section header
        if line == "[MIME Cache]" {
            in_mime_cache = true;
            continue;
        }

        // Check for other section headers
        if line.starts_with('[') && line.ends_with(']') {
            in_mime_cache = false;
            continue;
        }

        // Parse entries only if we're in the [MIME Cache] section
        if in_mime_cache && let Some((mime_type, apps)) = line.split_once('=') {
            let apps: Vec<String> = apps
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            if !apps.is_empty() {
                map.entry(mime_type.to_string()).or_default().extend(apps);
            }
        }
    }

    Ok(map)
}

/// Build a mapping of MIME types to desktop files by reading mimeinfo.cache files
/// This is the fast approach that Thunar uses
fn build_mime_to_apps_map() -> Result<HashMap<String, Vec<String>>> {
    let mut mime_map: HashMap<String, Vec<String>> = HashMap::new();
    let cache_paths = get_mimeinfo_cache_paths();

    for cache_path in cache_paths {
        match parse_mimeinfo_cache(&cache_path) {
            Ok(cache) => {
                // Merge this cache into our map
                for (mime_type, apps) in cache {
                    mime_map.entry(mime_type).or_default().extend(apps);
                }
            }
            Err(_) => {
                // Silently skip cache files we can't read
                continue;
            }
        }
    }

    Ok(mime_map)
}

/// Get all available MIME types from mimeinfo.cache files, sorted by priority
/// Common MIME types (with manual descriptions) come first, then others alphabetically
fn get_all_mime_types(mime_map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut mime_types: Vec<String> = mime_map.keys().cloned().collect();

    // Sort with custom comparator:
    // 1. MIME types with exact descriptions (from get_exact_mime_info) come first
    // 2. Within each group, sort alphabetically
    mime_types.sort_by(|a, b| {
        let a_has_exact = has_exact_mime_info(a);
        let b_has_exact = has_exact_mime_info(b);

        match (a_has_exact, b_has_exact) {
            (true, false) => std::cmp::Ordering::Less, // a comes first
            (false, true) => std::cmp::Ordering::Greater, // b comes first
            _ => a.cmp(b),                             // Both same priority, alphabetical
        }
    });

    mime_types
}

/// Check if a MIME type has an exact manual description mapping
/// Single source of truth: delegates to get_exact_mime_info()
fn has_exact_mime_info(mime_type: &str) -> bool {
    get_exact_mime_info(mime_type).is_some()
}

/// Get applications for a specific MIME type
fn get_apps_for_mime(mime_type: &str, mime_map: &HashMap<String, Vec<String>>) -> Vec<String> {
    // Use BTreeSet to remove duplicates and sort
    let apps: BTreeSet<String> = mime_map
        .get(mime_type)
        .map(|apps| apps.iter().cloned().collect())
        .unwrap_or_default();

    apps.into_iter().collect()
}

/// Query the current default application for a MIME type using xdg-mime
fn query_default_app(mime_type: &str) -> Result<Option<String>> {
    let output = Command::new("xdg-mime")
        .arg("query")
        .arg("default")
        .arg(mime_type)
        .output()
        .context("Failed to execute xdg-mime query")?;

    if !output.status.success() {
        return Ok(None);
    }

    let default_app = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if default_app.is_empty() {
        Ok(None)
    } else {
        Ok(Some(default_app))
    }
}

/// Set the default application for a MIME type using xdg-mime
fn set_default_app(mime_type: &str, desktop_file: &str) -> Result<()> {
    let status = Command::new("xdg-mime")
        .arg("default")
        .arg(desktop_file)
        .arg(mime_type)
        .status()
        .context("Failed to execute xdg-mime default")?;

    if !status.success() {
        anyhow::bail!("xdg-mime default command failed");
    }

    Ok(())
}

/// Get a human-readable name for a desktop file
fn get_app_name(desktop_file: &str) -> String {
    // Try to read the desktop file and get the Name field
    let desktop_paths = [
        format!("/usr/share/applications/{}", desktop_file),
        format!(
            "{}/.local/share/applications/{}",
            std::env::var("HOME").unwrap_or_default(),
            desktop_file
        ),
    ];

    for path in &desktop_paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(name) = line.strip_prefix("Name=") {
                    return format!("{} ({})", name.trim(), desktop_file);
                }
            }
        }
    }

    // Fallback to just the desktop file name
    desktop_file.to_string()
}

/// Main action for managing default applications
pub fn manage_default_apps(ctx: &mut SettingsContext) -> Result<()> {
    use crate::menu_utils::FzfResult;

    // Check if xdg-mime is available
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

    // Build the MIME map once and reuse it
    let mime_map = build_mime_to_apps_map().context("Failed to build MIME type map")?;

    // Get all MIME types with enhanced info
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

    // Convert to MimeTypeInfo with icons and descriptions
    let mime_types: Vec<MimeTypeInfo> = mime_type_strings
        .iter()
        .map(|mt| get_mime_type_info(mt))
        .collect();

    // Let user select a MIME type with enhanced display
    let selected_mime_info = match FzfWrapper::builder()
        .prompt("Select MIME type: ")
        .select(mime_types)?
    {
        FzfResult::Selected(info) => info,
        _ => {
            ctx.emit_info("settings.defaultapps.cancelled", "No MIME type selected.");
            return Ok(());
        }
    };

    let selected_mime = &selected_mime_info.mime_type;

    // Get applications for this MIME type
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

    // Show current default in header
    let current_default = query_default_app(selected_mime)?;
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

    // Convert desktop IDs to ApplicationInfo with enhanced display
    let app_infos: Vec<ApplicationInfo> = apps
        .iter()
        .map(|desktop_id| get_application_info(desktop_id))
        .collect();

    // Let user select an application with preview
    let selected_app_info = match FzfWrapper::builder()
        .prompt("Select application: ")
        .header(&header_text)
        .select(app_infos)?
    {
        FzfResult::Selected(app_info) => app_info,
        _ => {
            ctx.emit_info("settings.defaultapps.cancelled", "No application selected.");
            return Ok(());
        }
    };

    let desktop_file = &selected_app_info.desktop_id;

    // Set the default application
    set_default_app(selected_mime, desktop_file).context("Failed to set default application")?;

    ctx.notify(
        "Default application",
        &format!("Set {} as default for {}", desktop_file, selected_mime),
    );

    Ok(())
}

/// Helper function to manage default app for a specific MIME type
fn manage_default_app_for_mime(
    ctx: &mut SettingsContext,
    mime_type: &str,
    app_name: &str,
) -> Result<()> {
    use crate::menu_utils::FzfResult;
    use crate::settings::installable_packages::{
        self, ARCHIVE_MANAGERS, FILE_MANAGERS, IMAGE_VIEWERS, InstallableApp, PDF_VIEWERS,
        TEXT_EDITORS, VIDEO_PLAYERS, WEB_BROWSERS,
    };

    // Map app_name to corresponding installable packages
    let installable_apps: Option<&[InstallableApp]> = match app_name {
        "PDF Viewer" => Some(PDF_VIEWERS),
        "Image Viewer" => Some(IMAGE_VIEWERS),
        "Video Player" => Some(VIDEO_PLAYERS),
        "Text Editor" => Some(TEXT_EDITORS),
        "Archive Manager" => Some(ARCHIVE_MANAGERS),
        "File Manager" => Some(FILE_MANAGERS),
        "Web Browser" => Some(WEB_BROWSERS),
        _ => None,
    };

    loop {
        // Build the MIME map
        let mime_map = build_mime_to_apps_map().context("Failed to build MIME type map")?;

        // Get applications for this MIME type
        let app_desktop_ids = mime_map.get(mime_type).cloned().unwrap_or_default();

        // Get current default
        let current_default = query_default_app(mime_type).ok().flatten();

        // Create header text
        let header_text = format!(
            "Select default {} application\nCurrent: {}",
            app_name,
            current_default.as_deref().unwrap_or("(none)")
        );

        // Build options list - start with "Install more..." if available
        let mut options: Vec<String> = Vec::new();
        let install_more_key = format!("{} Install more...", NerdFont::Package);

        if installable_apps.is_some() {
            options.push(install_more_key.to_string());
        }

        // Add separator after install more option
        if !options.is_empty() && !app_desktop_ids.is_empty() {
            options.push("─────────────────────".to_string());
        }

        // Convert to ApplicationInfo with preview data
        let app_infos: Vec<ApplicationInfo> = app_desktop_ids
            .iter()
            .map(|desktop_id| get_application_info(desktop_id))
            .collect();

        // Add all app display texts
        for app_info in &app_infos {
            options.push(app_info.fzf_display_text());
        }

        if options.is_empty() || (options.len() == 1 && installable_apps.is_some()) {
            // Only have install more option (or nothing at all)
            if installable_apps.is_some() {
                ctx.emit_info(
                    "settings.defaultapps.no_apps_install",
                    &format!(
                        "No {} applications installed. Select 'Install more...' to install one.",
                        app_name
                    ),
                );
            } else {
                ctx.emit_info(
                    "settings.defaultapps.no_apps",
                    &format!(
                        "No applications found for {}. Install an application first.",
                        app_name
                    ),
                );
                return Ok(());
            }
        }

        // Let user select an option
        let selected = FzfWrapper::builder()
            .prompt(format!("Select {}: ", app_name))
            .header(&header_text)
            .select(options)?;

        match selected {
            FzfResult::Selected(selection) => {
                if selection == install_more_key {
                    if let Some(apps) = installable_apps {
                        // Show install more menu
                        let installed =
                            installable_packages::show_install_more_menu(app_name, apps)?;
                        if installed {
                            // Loop back to show updated app list
                            continue;
                        }
                    }
                    // User cancelled or nothing installed, loop back
                    continue;
                } else if selection.starts_with('─') {
                    // Separator selected, ignore and loop back
                    continue;
                } else {
                    // Find the matching app info
                    let selected_app_info = app_infos
                        .iter()
                        .find(|info| info.fzf_display_text() == selection);

                    if let Some(app_info) = selected_app_info {
                        let desktop_file = &app_info.desktop_id;

                        // Set the default application
                        set_default_app(mime_type, desktop_file)
                            .context("Failed to set default application")?;

                        ctx.notify(
                            &format!("Default {}", app_name),
                            &format!(
                                "Set {} as default",
                                app_info.name.as_deref().unwrap_or(desktop_file)
                            ),
                        );
                        return Ok(());
                    }
                }
            }
            _ => {
                ctx.emit_info("settings.defaultapps.cancelled", "No changes made.");
                return Ok(());
            }
        }
    }
}

/// Set default web browser
pub fn set_default_browser(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "text/html", "Web Browser")
}

/// Set default email client
pub fn set_default_email(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "x-scheme-handler/mailto", "Email Client")
}

/// Set default file manager
pub fn set_default_file_manager(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "inode/directory", "File Manager")
}

/// Set default text editor
pub fn set_default_text_editor(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "text/plain", "Text Editor")
}

/// Set default image viewer
pub fn set_default_image_viewer(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "image/png", "Image Viewer")
}

/// Set default video player
pub fn set_default_video_player(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "video/mp4", "Video Player")
}

/// Set default music player
pub fn set_default_music_player(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "audio/mpeg", "Music Player")
}

/// Set default PDF viewer
pub fn set_default_pdf_viewer(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "application/pdf", "PDF Viewer")
}

/// Set default archive manager
pub fn set_default_archive_manager(ctx: &mut SettingsContext) -> Result<()> {
    manage_default_app_for_mime(ctx, "application/zip", "Archive Manager")
}
