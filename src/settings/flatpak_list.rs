//! Internal flatpak list generation for fast menu loading
//!
//! Uses local appstream metadata (~15x faster than `flatpak remote-ls`)
//! with fallback to remote-ls if appstream is unavailable.
//! Outputs apps incrementally for streaming fzf integration.

use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Generate and print the flatpak app list to stdout
/// If keyword is provided, only matching apps are printed
/// Apps are printed incrementally as they're found (streaming)
pub fn generate_and_print_list(keyword: Option<&str>) -> Result<()> {
    eprintln!("Using local appstream cache (fast)");

    let keyword_lower = keyword.map(|s| s.to_lowercase());
    let mut printed_ids: HashSet<String> = HashSet::new();

    for path in get_appstream_paths() {
        parse_and_print_streaming(&path, keyword_lower.as_deref(), &mut printed_ids)?;
    }

    if printed_ids.is_empty() {
        eprintln!("No local matches, fetching from remote...");
        fallback_to_remote_ls(keyword)?;
    }

    Ok(())
}

/// Parse appstream file and print matching apps incrementally
fn parse_and_print_streaming(
    path: &PathBuf,
    keyword: Option<&str>,
    printed_ids: &mut HashSet<String>,
) -> Result<()> {
    let output = Command::new("zcat").arg(path).output()?;

    if !output.status.success() {
        return Ok(());
    }

    let content = String::from_utf8_lossy(&output.stdout);

    let mut current_id = String::new();
    let mut current_name = String::new();
    let mut current_summary = String::new();
    let mut in_desktop_component = false;

    for line in content.lines() {
        let line = line.trim();

        if line.contains("<component type=\"desktop-application\"") {
            if in_desktop_component && !current_id.is_empty() && !current_name.is_empty() {
                print_app_if_matches(
                    &current_id,
                    &current_name,
                    &current_summary,
                    keyword,
                    printed_ids,
                );
            }
            in_desktop_component = true;
            current_id.clear();
            current_name.clear();
            current_summary.clear();
            continue;
        }

        if !in_desktop_component {
            continue;
        }

        if line.contains("</component>") {
            let has_nested_element = line.contains("<developer>")
                || line.contains("<project>")
                || line.contains("<provides>");

            if !has_nested_element {
                if current_id.is_empty()
                    && let Some(id) = extract_tag_content(line, "id")
                {
                    current_id = id;
                }
                if current_name.is_empty()
                    && let Some(name) = extract_tag_content(line, "name")
                {
                    current_name = name;
                }
                if current_summary.is_empty()
                    && let Some(summary) = extract_tag_content(line, "summary")
                {
                    current_summary = summary;
                }
            }

            if !current_id.is_empty() && !current_name.is_empty() {
                print_app_if_matches(
                    &current_id,
                    &current_name,
                    &current_summary,
                    keyword,
                    printed_ids,
                );
            }
            in_desktop_component = false;
            continue;
        }

        if current_id.is_empty()
            && let Some(id) = extract_tag_content(line, "id")
        {
            current_id = id;
            continue;
        }

        if current_name.is_empty()
            && let Some(name) = extract_tag_content(line, "name")
        {
            current_name = name;
            continue;
        }

        if current_summary.is_empty()
            && let Some(summary) = extract_tag_content(line, "summary")
        {
            current_summary = summary;
            continue;
        }
    }

    if in_desktop_component && !current_id.is_empty() && !current_name.is_empty() {
        print_app_if_matches(
            &current_id,
            &current_name,
            &current_summary,
            keyword,
            printed_ids,
        );
    }

    Ok(())
}

/// Print app if it matches the keyword filter and hasn't been printed yet
fn print_app_if_matches(
    id: &str,
    name: &str,
    summary: &str,
    keyword: Option<&str>,
    printed_ids: &mut HashSet<String>,
) {
    if printed_ids.contains(id) {
        return;
    }

    if let Some(kw) = keyword {
        let kw_lower = kw.to_lowercase();
        let id_matches = id.to_lowercase().contains(&kw_lower);
        let name_matches = name.to_lowercase().contains(&kw_lower);
        let summary_matches = summary.to_lowercase().contains(&kw_lower);

        if !id_matches && !name_matches && !summary_matches {
            return;
        }
    }

    printed_ids.insert(id.to_string());
    println!("{}\t{}\t{}", id, name, summary);
}

/// Extract text content between XML tags
fn extract_tag_content(line: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);

    if let Some(start) = line.find(&open_tag)
        && let Some(end) = line.find(&close_tag)
    {
        let content = &line[start + open_tag.len()..end];
        if content.contains("xml:lang") {
            return None;
        }
        let trimmed = content.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('<') {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Get all appstream.xml.gz paths from both user and system installations
fn get_appstream_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(data_dir) = dirs::data_dir() {
        let user_appstream = data_dir.join("flatpak/appstream");
        paths.extend(find_appstream_files(&user_appstream));
    }

    paths.extend(find_appstream_files(&PathBuf::from(
        "/var/lib/flatpak/appstream",
    )));

    paths
}

/// Find appstream.xml.gz files in the given directory
fn find_appstream_files(base_dir: &PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if !base_dir.exists() {
        return files;
    }

    if let Ok(entries) = fs::read_dir(base_dir) {
        for entry in entries.flatten() {
            let remote_dir = entry.path();
            if remote_dir.is_dir() {
                let appstream_path = remote_dir.join("x86_64/active/appstream.xml.gz");
                if appstream_path.exists() {
                    files.push(appstream_path);
                }
            }
        }
    }

    files
}

/// Fallback to flatpak remote-ls command
fn fallback_to_remote_ls(keyword: Option<&str>) -> Result<()> {
    let output = Command::new("flatpak")
        .args(["remote-ls", "--app"])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let keyword_lower = keyword.map(|s| s.to_lowercase());

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            let id = parts.get(0).map(|s| *s).unwrap_or("");
            let name = parts.get(1).map(|s| *s).unwrap_or("");
            let summary = parts.get(2).map(|s| *s).unwrap_or("");

            if let Some(kw) = keyword_lower.as_ref() {
                let id_matches = id.to_lowercase().contains(kw);
                let name_matches = name.to_lowercase().contains(kw);
                let summary_matches = summary.to_lowercase().contains(kw);

                if !id_matches && !name_matches && !summary_matches {
                    continue;
                }
            }

            println!("{}\t{}\t{}", id, name, summary);
        }
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "flatpak remote-ls failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
