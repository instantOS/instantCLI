use anyhow::Result;
use std::fs;
use std::path::Path;

pub struct TimezoneProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for TimezoneProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let timezones = fetch_timezones()?;

        let mut data = context.data.lock().unwrap();
        data.insert("timezones".to_string(), timezones.join("\n"));

        Ok(())
    }
}

fn fetch_timezones() -> Result<Vec<String>> {
    let zoneinfo_path = Path::new("/usr/share/zoneinfo");
    let mut timezones = Vec::new();

    // Files/directories to skip
    let skip_names = [
        "posixrules",
        "tzdata.zi",
        "leapseconds",
        "iso3166.tab",
        "zone.tab",
        "zone1970.tab",
        "+VERSION",
    ];

    collect_timezones(zoneinfo_path, zoneinfo_path, &mut timezones, &skip_names)?;

    // Sort for better UX
    timezones.sort();

    Ok(timezones)
}

fn collect_timezones(
    base_path: &Path,
    current_path: &Path,
    timezones: &mut Vec<String>,
    skip_names: &[&str],
) -> Result<()> {
    if !current_path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(current_path)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Skip special files and uppercase directories (like Etc, SystemV, etc.)
        if skip_names.contains(&name_str.as_ref()) {
            continue;
        }

        if path.is_dir() {
            // Recursively collect from subdirectories
            collect_timezones(base_path, &path, timezones, skip_names)?;
        } else if path.is_file() {
            // Get the relative path from zoneinfo root
            if let Ok(relative) = path.strip_prefix(base_path) {
                if let Some(tz) = relative.to_str() {
                    // Only include valid timezone format (Region/City or Region/Subregion/City)
                    if tz.contains('/') && !tz.starts_with('.') {
                        timezones.push(tz.to_string());
                    }
                }
            }
        }
    }

    Ok(())
}
