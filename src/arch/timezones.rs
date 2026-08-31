use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::arch::engine::DataKey;

/// Non-timezone entries: metadata files, plus the `posix` and `right`
/// compatibility trees, which duplicate every zone under a different root
/// (identical copies / leap-second variants) and are never valid answers.
const NON_TIMEZONE_ENTRIES: &[&str] = &[
    "posix",
    "right",
    "posixrules",
    "tzdata.zi",
    "leapseconds",
    "iso3166.tab",
    "zone.tab",
    "zone1970.tab",
    "+VERSION",
];

pub struct TimezonesKey;

impl DataKey for TimezonesKey {
    type Value = Vec<String>;
    const KEY: &'static str = "timezones";
}

pub struct TimezoneProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for TimezoneProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let timezones = fetch_timezones()?;
        context.set::<TimezonesKey>(timezones);
        Ok(())
    }
}

fn fetch_timezones() -> Result<Vec<String>> {
    let zoneinfo_path = Path::new("/usr/share/zoneinfo");
    let mut timezones = Vec::new();

    collect_timezones(
        zoneinfo_path,
        zoneinfo_path,
        &mut timezones,
        NON_TIMEZONE_ENTRIES,
    )?;

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

        // Skip known non-timezone entries
        if skip_names.contains(&name_str.as_ref()) {
            continue;
        }

        if path.is_dir() {
            // Recursively collect from subdirectories
            collect_timezones(base_path, &path, timezones, skip_names)?;
        } else if path.is_file() {
            // Get the relative path from zoneinfo root
            if let Ok(relative) = path.strip_prefix(base_path)
                && let Some(tz) = relative.to_str()
            {
                // Only include valid timezone format (Region/City or Region/Subregion/City)
                if tz.contains('/') && !tz.starts_with('.') {
                    timezones.push(tz.to_string());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_skips_compatibility_trees_and_metadata() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let zoneinfo = dir.path();

        for relative in [
            "America/New_York",
            "Europe/Berlin",
            "posix/America/New_York",
            "right/Europe/Berlin",
        ] {
            let path = zoneinfo.join(relative);
            fs::create_dir_all(path.parent().expect("parent exists"))?;
            fs::write(path, "")?;
        }
        for metadata in ["zone.tab", "localtime", "tzdata.zi"] {
            fs::write(zoneinfo.join(metadata), "")?;
        }

        let mut timezones = Vec::new();
        collect_timezones(zoneinfo, zoneinfo, &mut timezones, NON_TIMEZONE_ENTRIES)?;
        timezones.sort();

        assert_eq!(timezones, vec!["America/New_York", "Europe/Berlin"]);
        Ok(())
    }
}
