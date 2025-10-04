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
        format!("{}/.local/share/applications", std::env::var("HOME").unwrap_or_default()),
        format!("{}/.local/share/flatpak/exports/share/applications", std::env::var("HOME").unwrap_or_default()),
        "/var/lib/flatpak/exports/share/applications".to_string(),
        "/usr/share/applications".to_string(),
    ];

    for dir in &directories {
        let path = PathBuf::from(dir).join(desktop_id);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(desktop_file) = parse(&content) {
                    use freedesktop_file_parser::EntryType;
                    
                    let exec = match &desktop_file.entry.entry_type {
                        EntryType::Application(app) => app.exec.clone(),
                        _ => None,
                    };
                    
                    return ApplicationInfo {
                        desktop_id: desktop_id.to_string(),
                        name: Some(desktop_file.entry.name.default.clone()),
                        comment: desktop_file.entry.comment.as_ref().map(|c| c.default.clone()),
                        icon: desktop_file.entry.icon.as_ref().map(|i| i.content.clone()),
                        exec,
                    };
                }
            }
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
            "image" => (NerdFont::Image, Some("Image file")),
            "video" => (NerdFont::Video, Some("Video file")),
            "audio" => (NerdFont::Music, Some("Audio file")),
            "text" => (NerdFont::FileText, Some("Text file")),
            "application" => (NerdFont::Package, Some("Application file")),
            "inode" => (NerdFont::Folder, Some("Directory/Inode")),
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
fn get_exact_mime_info(mime_type: &str) -> Option<(NerdFont, &'static str)> {
    let mapping = match mime_type {
        // Images
        "image/jpeg" | "image/jpg" => (NerdFont::Image, "JPEG image"),
        "image/png" => (NerdFont::Image, "PNG image"),
        "image/gif" => (NerdFont::Image, "GIF animation"),
        "image/svg+xml" => (NerdFont::Image, "SVG vector image"),
        "image/webp" => (NerdFont::Image, "WebP image"),
        "image/bmp" => (NerdFont::Image, "Bitmap image"),
        "image/tiff" => (NerdFont::Image, "TIFF image"),
        
        // Documents
        "application/pdf" => (NerdFont::FilePdf, "PDF document"),
        "application/vnd.oasis.opendocument.text" => (NerdFont::FileText, "OpenDocument text"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => (NerdFont::FileWord, "Microsoft Word document"),
        "application/msword" => (NerdFont::FileWord, "Microsoft Word document"),
        "application/vnd.ms-excel" => (NerdFont::FileExcel, "Microsoft Excel spreadsheet"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => (NerdFont::FileExcel, "Excel spreadsheet"),
        "application/vnd.ms-powerpoint" => (NerdFont::FilePresentation, "PowerPoint presentation"),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => (NerdFont::FilePresentation, "PowerPoint presentation"),
        
        // Archives
        "application/zip" => (NerdFont::Archive, "ZIP archive"),
        "application/x-tar" => (NerdFont::Archive, "TAR archive"),
        "application/x-7z-compressed" => (NerdFont::Archive, "7-Zip archive"),
        "application/x-rar" => (NerdFont::Archive, "RAR archive"),
        "application/gzip" => (NerdFont::Archive, "GZIP archive"),
        "application/x-bzip2" => (NerdFont::Archive, "BZIP2 archive"),
        "application/x-xz" => (NerdFont::Archive, "XZ archive"),
        
        // Video
        "video/mp4" => (NerdFont::Video, "MP4 video"),
        "video/x-matroska" => (NerdFont::Video, "Matroska video"),
        "video/webm" => (NerdFont::Video, "WebM video"),
        "video/mpeg" => (NerdFont::Video, "MPEG video"),
        "video/x-msvideo" => (NerdFont::Video, "AVI video"),
        
        // Audio
        "audio/mpeg" => (NerdFont::Music, "MP3 audio"),
        "audio/ogg" => (NerdFont::Music, "OGG audio"),
        "audio/flac" => (NerdFont::Music, "FLAC audio"),
        "audio/x-wav" => (NerdFont::Music, "WAV audio"),
        "audio/aac" => (NerdFont::Music, "AAC audio"),
        
        // Text
        "text/plain" => (NerdFont::FileText, "Plain text"),
        "text/html" => (NerdFont::Code, "HTML document"),
        "text/css" => (NerdFont::Code, "CSS stylesheet"),
        "text/javascript" => (NerdFont::Code, "JavaScript code"),
        "application/json" => (NerdFont::Code, "JSON data"),
        "application/xml" => (NerdFont::Code, "XML document"),
        "text/x-python" => (NerdFont::Code, "Python script"),
        "text/x-rust" => (NerdFont::Code, "Rust source code"),
        "text/x-c" => (NerdFont::Code, "C source code"),
        "text/x-c++" => (NerdFont::Code, "C++ source code"),
        "text/markdown" => (NerdFont::FileText, "Markdown document"),
        
        // System
        "application/x-executable" => (NerdFont::Gear, "Executable file"),
        "application/x-sharedlib" => (NerdFont::Gear, "Shared library"),
        "application/x-shellscript" => (NerdFont::Terminal, "Shell script"),
        "inode/directory" => (NerdFont::Folder, "Directory"),
        
        // Special
        "application/vnd.appimage" => (NerdFont::Package, "AppImage application"),
        "application/vnd.flatpak.ref" => (NerdFont::Package, "Flatpak reference"),
        "application/x-iso9660-image" => (NerdFont::Archive, "ISO disk image"),
        
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
        paths.push(home_path.join(".local/share/flatpak/exports/share/applications/mimeinfo.cache"));
    }

    // System flatpak apps
    paths.push(PathBuf::from("/var/lib/flatpak/exports/share/applications/mimeinfo.cache"));

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
        if in_mime_cache {
            if let Some((mime_type, apps)) = line.split_once('=') {
                let apps: Vec<String> = apps
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();

                if !apps.is_empty() {
                    map.entry(mime_type.to_string())
                        .or_default()
                        .extend(apps);
                }
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
                    mime_map
                        .entry(mime_type)
                        .or_default()
                        .extend(apps);
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

/// Get all available MIME types from mimeinfo.cache files
fn get_all_mime_types(mime_map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mime_types: BTreeSet<String> = mime_map.keys().cloned().collect();
    mime_types.into_iter().collect()
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
    if Command::new("which")
        .arg("xdg-mime")
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(()) } else { None })
        .is_none()
    {
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
    let apps = get_apps_for_mime(&selected_mime, &mime_map);

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
    let current_default = query_default_app(&selected_mime)?;
    let header_text = if let Some(ref default) = current_default {
        format!("MIME type: {} {}\nCurrent default: {}", 
                char::from(selected_mime_info.icon), 
                selected_mime, 
                default)
    } else {
        format!("MIME type: {} {}\nCurrent default: (none)", 
                char::from(selected_mime_info.icon),
                selected_mime)
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
    set_default_app(&selected_mime, desktop_file)
        .context("Failed to set default application")?;

    ctx.notify(
        "Default application",
        &format!("Set {} as default for {}", desktop_file, selected_mime),
    );

    Ok(())
}
