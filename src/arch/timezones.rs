use anyhow::Result;
use std::collections::HashMap;
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

#[derive(Clone, Default)]
struct TimezoneCountries {
    country_by_timezone: HashMap<String, String>,
    timezone_by_country: HashMap<String, String>,
}

struct TimezoneCountriesKey;

impl DataKey for TimezoneCountriesKey {
    type Value = TimezoneCountries;
    const KEY: &'static str = "timezone_countries";
}

/// The timezone of the current system, derived from the `/etc/localtime`
/// symlink, if it points into the zoneinfo database.
pub(crate) fn detect_current_timezone() -> Option<String> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    localtime_target_to_timezone(&target.to_string_lossy())
}

/// Find the ISO country code associated with an IANA timezone in zone.tab.
pub(crate) fn country_for_timezone(
    context: &crate::arch::engine::InstallContext,
    timezone: &str,
) -> Option<String> {
    context
        .get::<TimezoneCountriesKey>()?
        .country_by_timezone
        .get(timezone)
        .cloned()
}

/// Pick zone.tab's representative timezone for a country.
pub(crate) fn timezone_for_country(
    context: &crate::arch::engine::InstallContext,
    country: &str,
) -> Option<String> {
    context
        .get::<TimezoneCountriesKey>()?
        .timezone_by_country
        .get(&country.to_ascii_uppercase())
        .cloned()
}

fn parse_zone_tab(contents: &str) -> impl Iterator<Item = (&str, &str)> {
    contents.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let country = fields.next()?;
        if country.starts_with('#') {
            return None;
        }
        let _coordinates = fields.next()?;
        let timezone = fields.next()?;
        Some((country, timezone))
    })
}

fn timezone_countries(contents: &str) -> TimezoneCountries {
    let mut countries = TimezoneCountries::default();
    for (country, timezone) in parse_zone_tab(contents) {
        countries
            .country_by_timezone
            .insert(timezone.to_string(), country.to_string());
        countries
            .timezone_by_country
            .entry(country.to_string())
            .or_insert_with(|| timezone.to_string());
    }
    countries
}

fn localtime_target_to_timezone(target: &str) -> Option<String> {
    target
        .strip_prefix("/usr/share/zoneinfo/")
        .filter(|tz| !tz.is_empty())
        .map(str::to_string)
}

pub struct TimezoneProvider;

pub struct TimezoneCountriesProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for TimezoneCountriesProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let countries = fs::read_to_string("/usr/share/zoneinfo/zone.tab")
            .map(|contents| timezone_countries(&contents))
            .unwrap_or_default();
        context.set::<TimezoneCountriesKey>(countries);
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for TimezoneProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let timezones = fetch_timezones()?;

        self.save_list::<TimezonesKey, _>(context, timezones);
        TimezoneCountriesProvider.provide(context).await?;

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

    #[test]
    fn zone_tab_maps_in_both_directions() {
        let contents =
            "# comment\nDE\t+5230+01322\tEurope/Berlin\nUS\t+404251-0740023\tAmerica/New_York\n";
        let entries: Vec<_> = parse_zone_tab(contents).collect();

        assert_eq!(
            entries,
            vec![("DE", "Europe/Berlin"), ("US", "America/New_York")]
        );

        let countries = timezone_countries(contents);
        assert_eq!(
            countries.country_by_timezone.get("Europe/Berlin"),
            Some(&"DE".to_string())
        );
        assert_eq!(
            countries.timezone_by_country.get("US"),
            Some(&"America/New_York".to_string())
        );
    }

    #[test]
    fn selected_timezone_and_keymap_can_suggest_a_locale() {
        use crate::arch::annotations::AnnotatedValue;
        use crate::arch::engine::{InstallContext, StepId};
        use crate::arch::locales::LocalesKey;

        let mut context = InstallContext::new();
        context.set_answer(StepId::Keymap, "de-latin1".to_string());
        context.set_answer(StepId::Timezone, "Europe/Berlin".to_string());
        context.set::<LocalesKey>(vec![
            AnnotatedValue::new("de_AT.UTF-8".to_string(), None),
            AnnotatedValue::new("de_DE.UTF-8".to_string(), None),
        ]);
        context.set::<TimezoneCountriesKey>(timezone_countries("DE\t+5230+01322\tEurope/Berlin\n"));

        assert_eq!(
            crate::arch::geo::locale_suggestion(&context).as_deref(),
            Some("de_DE.UTF-8")
        );
    }
}
