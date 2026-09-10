use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::arch::annotations::{AnnotatedValue, TimezoneAnnotationProvider};
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
    type Value = Vec<AnnotatedValue<String>>;
    const KEY: &'static str = "timezones";
}

/// The timezone of the current system, derived from the `/etc/localtime`
/// symlink, if it points into the zoneinfo database.
pub(crate) fn detect_current_timezone() -> Option<String> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    localtime_target_to_timezone(&target.to_string_lossy())
}

fn localtime_target_to_timezone(target: &str) -> Option<String> {
    target
        .strip_prefix("/usr/share/zoneinfo/")
        .filter(|tz| !tz.is_empty())
        .map(str::to_string)
}

pub struct TimezoneProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for TimezoneProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let timezones = fetch_timezones()?;

        self.save_list::<TimezonesKey, _>(context, timezones);

        Ok(())
    }

    fn annotation_provider(&self) -> Option<Box<dyn crate::arch::annotations::AnnotationProvider>> {
        Some(Box::new(TimezoneAnnotationProvider::new()))
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

    #[test]
    fn localtime_target_maps_to_timezone_name() {
        assert_eq!(
            localtime_target_to_timezone("/usr/share/zoneinfo/Europe/Berlin").as_deref(),
            Some("Europe/Berlin")
        );
        assert_eq!(
            localtime_target_to_timezone("/usr/share/zoneinfo/America/Argentina/Buenos_Aires")
                .as_deref(),
            Some("America/Argentina/Buenos_Aires")
        );
    }

    #[test]
    fn localtime_targets_outside_zoneinfo_are_rejected() {
        assert_eq!(localtime_target_to_timezone("/etc/localtime"), None);
        assert_eq!(localtime_target_to_timezone("/usr/share/zoneinfo/"), None);
        assert_eq!(localtime_target_to_timezone(""), None);
    }
}
