//! Best-effort geographic detection for install-time suggestions.
//!
//! The Arch live environment carries stock defaults (`UTC`, `en_US.UTF-8`,
//! `us`), so the per-module `detect_current_*` helpers cannot tell us where
//! the user actually is. This module asks a public IP-geolocation service for
//! a country/timezone hint and derives the location-dependent preselections
//! (timezone, locale, console keymap, mirror region) from it.
//!
//! Detection is strictly a nicety: every failure path yields an empty
//! [`GeoLocation`], and the questions fall back to their previous behaviour.

use std::time::Duration;

use serde::Deserialize;

use crate::arch::annotations::AnnotatedValue;
use crate::arch::engine::{AsyncDataProvider, DataKey, InstallContext};
use crate::arch::keymaps::KeymapsKey;
use crate::arch::locales::LocalesKey;
use crate::arch::mirrors::MirrorRegionCodesKey;
use crate::arch::timezones::TimezonesKey;

/// Public services tried in order. Each returns JSON with a country code and
/// (usually) an IANA timezone, without requiring an API key.
const GEO_ENDPOINTS: &[&str] = &["https://ipinfo.io/json", "https://ifconfig.co/json"];

/// Per-endpoint timeout. This is a nicety and never worth delaying the wizard.
const GEO_TIMEOUT: Duration = Duration::from_secs(3);

/// A best-effort guess at where the machine is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeoLocation {
    /// ISO 3166-1 alpha-2 country code, e.g. `DE`.
    pub country_code: Option<String>,
    /// IANA timezone, e.g. `Europe/Berlin`.
    pub timezone: Option<String>,
}

impl GeoLocation {
    fn is_empty(&self) -> bool {
        self.country_code.is_none() && self.timezone.is_none()
    }
}

pub struct GeoLocationKey;

impl DataKey for GeoLocationKey {
    type Value = GeoLocation;
    const KEY: &'static str = "geo_location";
}

/// Shares one lookup across every question that wants a suggestion.
static GEO_CACHE: tokio::sync::OnceCell<GeoLocation> = tokio::sync::OnceCell::const_new();

/// Data provider for [`GeoLocationKey`].
///
/// Never fails: a failed lookup stores an empty location so downstream
/// suggestions simply do nothing.
pub struct GeoLocationProvider;

#[async_trait::async_trait]
impl AsyncDataProvider for GeoLocationProvider {
    async fn provide(&self, context: &InstallContext) -> anyhow::Result<()> {
        context.set::<GeoLocationKey>(detect_location().await);
        Ok(())
    }
}

/// Fetches the location hint once per process.
async fn detect_location() -> GeoLocation {
    GEO_CACHE
        .get_or_init(|| async {
            for endpoint in GEO_ENDPOINTS {
                match tokio::time::timeout(GEO_TIMEOUT, fetch_geo_location(endpoint)).await {
                    Ok(Ok(location)) if !location.is_empty() => return location,
                    _ => continue,
                }
            }
            GeoLocation::default()
        })
        .await
        .clone()
}

/// Response shape covering the field names used by the supported services.
///
/// `country` is an alpha-2 code on ipinfo but a full name on some services, so
/// [`normalize_country_code`] rejects non-code values.
#[derive(Debug, Deserialize)]
struct GeoResponse {
    country: Option<String>,
    country_iso: Option<String>,
    country_code: Option<String>,
    timezone: Option<String>,
    time_zone: Option<String>,
    timezone_id: Option<String>,
}

async fn fetch_geo_location(endpoint: &str) -> anyhow::Result<GeoLocation> {
    let body = reqwest::get(endpoint)
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_geo_response(&body)
}

fn parse_geo_response(body: &str) -> anyhow::Result<GeoLocation> {
    let response: GeoResponse = serde_json::from_str(body)?;

    let country_code = [
        response.country_iso.as_deref(),
        response.country_code.as_deref(),
        response.country.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(normalize_country_code);

    let timezone = [
        response.timezone.as_deref(),
        response.time_zone.as_deref(),
        response.timezone_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .map(str::to_string);

    Ok(GeoLocation {
        country_code,
        timezone,
    })
}

/// Accepts only a plausible ISO 3166-1 alpha-2 code, so a full country name
/// (`"Germany"`) coming through a `country` field is ignored.
fn normalize_country_code(value: &str) -> Option<String> {
    let value = value.trim();
    (value.len() == 2 && value.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| value.to_ascii_uppercase())
}

// ============================================================================
// Derived suggestions
// ============================================================================

/// Suggested timezone for the timezone question, if it exists in the list.
pub fn timezone_suggestion(context: &InstallContext) -> Option<String> {
    let location = context.get::<GeoLocationKey>()?;
    let timezones = context.get::<TimezonesKey>()?;
    let country_timezone = location.country_code.as_deref().and_then(country_timezone);
    first_present(&timezones, [location.timezone.as_deref(), country_timezone])
}

/// Suggested locale for the locale question, if it exists in the list.
pub fn locale_suggestion(context: &InstallContext) -> Option<String> {
    let location = context.get::<GeoLocationKey>()?;
    let country = location.country_code.as_deref()?;
    let locales = context.get::<LocalesKey>()?;
    locale_for_country(country, &locales)
}

/// Suggested console keymap for the keymap question, if it exists in the list.
pub fn keymap_suggestion(context: &InstallContext) -> Option<String> {
    let location = context.get::<GeoLocationKey>()?;
    let country = location.country_code.as_deref()?;

    let locale = context
        .get::<LocalesKey>()
        .as_deref()
        .and_then(|locales| locale_for_country(country, locales));
    let language = locale.as_deref().and_then(language_of);

    let candidate = country_keymap(country).or_else(|| language.and_then(language_keymap));
    let keymaps = context.get::<KeymapsKey>()?;
    first_present(&keymaps, [candidate])
}

/// Suggested mirror region name for the mirror-region question.
pub fn mirror_region_suggestion(context: &InstallContext) -> Option<String> {
    let location = context.get::<GeoLocationKey>()?;
    let country = location.country_code.as_deref()?;
    let regions = context.get::<MirrorRegionCodesKey>()?;
    regions
        .iter()
        .find(|(_, code)| code.eq_ignore_ascii_case(country))
        .map(|(name, _)| name.clone())
}

/// Returns the first candidate that is actually present in `list`, so a
/// suggestion never points at an option the user cannot select.
fn first_present<'a>(
    list: &[AnnotatedValue<String>],
    candidates: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<String> {
    for candidate in candidates.into_iter().flatten() {
        if list.iter().any(|item| item.value == candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// The language part of a locale, e.g. `de` for `de_DE.UTF-8`.
fn language_of(locale: &str) -> Option<&str> {
    let base = locale.split('.').next().unwrap_or(locale);
    let (language, _) = base.split_once('_')?;
    (!language.is_empty()).then_some(language)
}

/// The `(language, territory)` parts of a locale, e.g. `de_DE.UTF-8`.
fn territory_of(locale: &str) -> Option<(&str, &str)> {
    let base = locale.split('.').next().unwrap_or(locale);
    let (language, territory) = base.split_once('_')?;
    (!language.is_empty() && !territory.is_empty()).then_some((language, territory))
}

/// Picks an available locale whose territory matches the country, preferring
/// the country's primary language where one is known.
fn locale_for_country(country: &str, available: &[AnnotatedValue<String>]) -> Option<String> {
    let preferred = country_language(country);
    let mut fallback = None;

    for item in available {
        let Some((language, territory)) = territory_of(&item.value) else {
            continue;
        };
        if !territory.eq_ignore_ascii_case(country) {
            continue;
        }
        if preferred == Some(language) {
            return Some(item.value.clone());
        }
        fallback.get_or_insert_with(|| item.value.clone());
    }

    fallback
}

fn country_language(country: &str) -> Option<&'static str> {
    lookup(COUNTRY_LANGUAGE, country)
}

fn country_keymap(country: &str) -> Option<&'static str> {
    lookup(COUNTRY_KEYMAP, country)
}

fn country_timezone(country: &str) -> Option<&'static str> {
    lookup(COUNTRY_TIMEZONE, country)
}

fn language_keymap(language: &str) -> Option<&'static str> {
    lookup(LANGUAGE_KEYMAP, language)
}

fn lookup<'a>(table: &[(&str, &'a str)], key: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| *value)
}

/// Countries where more than one available locale shares the territory code;
/// the preferred language wins the `_CC.UTF-8` scan.
const COUNTRY_LANGUAGE: &[(&str, &str)] = &[
    ("CA", "en"),
    ("CH", "de"),
    ("BE", "nl"),
    ("LU", "fr"),
    ("IN", "en"),
    ("ZA", "en"),
    ("SG", "en"),
    ("PH", "en"),
    ("MY", "en"),
];

/// Console keymaps for countries whose layout is not implied by the locale's
/// language (notably `en_GB` -> `uk`). Validated against `KeymapsKey`, so a
/// name that is missing from the system just yields no suggestion.
const COUNTRY_KEYMAP: &[(&str, &str)] = &[
    ("US", "us"),
    ("GB", "uk"),
    ("IE", "uk"),
    ("AU", "us"),
    ("NZ", "us"),
    ("CA", "ca"),
    ("DE", "de-latin1"),
    ("AT", "de-latin1"),
    ("CH", "sg-latin1"),
    ("LI", "de-latin1"),
    ("FR", "fr"),
    ("BE", "be-latin1"),
    ("LU", "fr"),
    ("MC", "fr"),
    ("ES", "es"),
    ("IT", "it"),
    ("PT", "pt-latin1"),
    ("BR", "br-abnt2"),
    ("NL", "nl"),
    ("SE", "sv-latin1"),
    ("NO", "no-latin1"),
    ("DK", "dk-latin1"),
    ("FI", "fi"),
    ("PL", "pl"),
    ("CZ", "cz"),
    ("SK", "sk-qwertz"),
    ("HU", "hu101"),
    ("RO", "ro"),
    ("RU", "ru"),
    ("UA", "ua-utf"),
    ("TR", "tr_q-latin5"),
    ("GR", "gr"),
    ("IL", "il"),
    ("JP", "jp106"),
    ("KR", "kr"),
    ("CN", "us"),
    ("TW", "us"),
    ("HK", "us"),
    ("MX", "la-latin1"),
    ("AR", "la-latin1"),
    ("CL", "la-latin1"),
    ("CO", "la-latin1"),
];

/// Fallback for countries without a specific keymap above.
const LANGUAGE_KEYMAP: &[(&str, &str)] = &[
    ("en", "us"),
    ("de", "de-latin1"),
    ("fr", "fr"),
    ("es", "es"),
    ("it", "it"),
    ("pt", "pt-latin1"),
    ("nl", "nl"),
    ("sv", "sv-latin1"),
    ("nb", "no-latin1"),
    ("nn", "no-latin1"),
    ("da", "dk-latin1"),
    ("fi", "fi"),
    ("pl", "pl"),
    ("cs", "cz"),
    ("sk", "sk-qwertz"),
    ("hu", "hu101"),
    ("ro", "ro"),
    ("ru", "ru"),
    ("uk", "ua-utf"),
    ("tr", "tr_q-latin5"),
    ("el", "gr"),
    ("he", "il"),
    ("ja", "jp106"),
    ("ko", "kr"),
    ("zh", "us"),
];

/// Representative timezone per country, used only when the geolocation
/// service did not return a timezone of its own.
const COUNTRY_TIMEZONE: &[(&str, &str)] = &[
    ("US", "America/New_York"),
    ("CA", "America/Toronto"),
    ("MX", "America/Mexico_City"),
    ("BR", "America/Sao_Paulo"),
    ("AR", "America/Argentina/Buenos_Aires"),
    ("GB", "Europe/London"),
    ("IE", "Europe/Dublin"),
    ("FR", "Europe/Paris"),
    ("DE", "Europe/Berlin"),
    ("AT", "Europe/Vienna"),
    ("CH", "Europe/Zurich"),
    ("IT", "Europe/Rome"),
    ("ES", "Europe/Madrid"),
    ("PT", "Europe/Lisbon"),
    ("NL", "Europe/Amsterdam"),
    ("BE", "Europe/Brussels"),
    ("LU", "Europe/Luxembourg"),
    ("SE", "Europe/Stockholm"),
    ("NO", "Europe/Oslo"),
    ("DK", "Europe/Copenhagen"),
    ("FI", "Europe/Helsinki"),
    ("PL", "Europe/Warsaw"),
    ("CZ", "Europe/Prague"),
    ("SK", "Europe/Bratislava"),
    ("HU", "Europe/Budapest"),
    ("RO", "Europe/Bucharest"),
    ("RU", "Europe/Moscow"),
    ("UA", "Europe/Kyiv"),
    ("TR", "Europe/Istanbul"),
    ("GR", "Europe/Athens"),
    ("IL", "Asia/Jerusalem"),
    ("IN", "Asia/Kolkata"),
    ("CN", "Asia/Shanghai"),
    ("JP", "Asia/Tokyo"),
    ("KR", "Asia/Seoul"),
    ("AU", "Australia/Sydney"),
    ("NZ", "Pacific/Auckland"),
    ("ZA", "Africa/Johannesburg"),
    ("EG", "Africa/Cairo"),
    ("NG", "Africa/Lagos"),
    ("SG", "Asia/Singapore"),
    ("HK", "Asia/Hong_Kong"),
    ("TW", "Asia/Taipei"),
    ("AE", "Asia/Dubai"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn annotated(values: &[&str]) -> Vec<AnnotatedValue<String>> {
        values
            .iter()
            .map(|value| AnnotatedValue::new((*value).to_string(), None))
            .collect()
    }

    fn context_with(location: GeoLocation) -> InstallContext {
        let context = InstallContext::new();
        context.set::<GeoLocationKey>(location);
        context
    }

    #[test]
    fn parses_country_and_timezone_from_ipinfo_shape() {
        let location =
            parse_geo_response(r#"{"ip":"1.2.3.4","country":"DE","timezone":"Europe/Berlin"}"#)
                .expect("valid response");

        assert_eq!(location.country_code.as_deref(), Some("DE"));
        assert_eq!(location.timezone.as_deref(), Some("Europe/Berlin"));
    }

    #[test]
    fn prefers_iso_code_over_full_country_name() {
        let location = parse_geo_response(
            r#"{"country":"Germany","country_iso":"DE","time_zone":"Europe/Berlin"}"#,
        )
        .expect("valid response");

        assert_eq!(location.country_code.as_deref(), Some("DE"));
        assert_eq!(location.timezone.as_deref(), Some("Europe/Berlin"));
    }

    #[test]
    fn ignores_full_country_names() {
        let location = parse_geo_response(r#"{"country":"Germany"}"#).expect("valid response");
        assert_eq!(location.country_code, None);
    }

    #[test]
    fn malformed_response_is_an_error() {
        assert!(parse_geo_response("not json").is_err());
    }

    #[test]
    fn empty_locations_are_detected() {
        assert!(GeoLocation::default().is_empty());
        assert!(
            !GeoLocation {
                country_code: Some("DE".to_string()),
                timezone: None,
            }
            .is_empty()
        );
    }

    #[test]
    fn timezone_suggestion_only_returns_selectable_values() {
        let context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: Some("Europe/Berlin".to_string()),
        });
        context.set::<TimezonesKey>(annotated(&["UTC", "Europe/Berlin"]));

        assert_eq!(
            timezone_suggestion(&context).as_deref(),
            Some("Europe/Berlin")
        );
    }

    #[test]
    fn timezone_suggestion_skips_missing_values() {
        let context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: Some("Europe/Berlin".to_string()),
        });
        context.set::<TimezonesKey>(annotated(&["UTC"]));

        assert_eq!(timezone_suggestion(&context), None);
    }

    #[test]
    fn timezone_suggestion_falls_back_to_country() {
        let context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: None,
        });
        context.set::<TimezonesKey>(annotated(&["Europe/Berlin"]));

        assert_eq!(
            timezone_suggestion(&context).as_deref(),
            Some("Europe/Berlin")
        );
    }

    #[test]
    fn locale_suggestion_matches_the_territory() {
        let context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: None,
        });
        context.set::<LocalesKey>(annotated(&["en_US.UTF-8", "de_DE.UTF-8"]));

        assert_eq!(locale_suggestion(&context).as_deref(), Some("de_DE.UTF-8"));
    }

    #[test]
    fn locale_suggestion_prefers_the_primary_language() {
        let context = context_with(GeoLocation {
            country_code: Some("CH".to_string()),
            timezone: None,
        });
        context.set::<LocalesKey>(annotated(&["fr_CH.UTF-8", "it_CH.UTF-8", "de_CH.UTF-8"]));

        assert_eq!(locale_suggestion(&context).as_deref(), Some("de_CH.UTF-8"));
    }

    #[test]
    fn keymap_suggestion_uses_country_override() {
        let context = context_with(GeoLocation {
            country_code: Some("GB".to_string()),
            timezone: None,
        });
        context.set::<LocalesKey>(annotated(&["en_GB.UTF-8"]));
        context.set::<KeymapsKey>(annotated(&["us", "uk"]));

        assert_eq!(keymap_suggestion(&context).as_deref(), Some("uk"));
    }

    #[test]
    fn keymap_suggestion_falls_back_to_language() {
        let context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: None,
        });
        context.set::<LocalesKey>(annotated(&["de_DE.UTF-8"]));
        context.set::<KeymapsKey>(annotated(&["de-latin1"]));

        assert_eq!(keymap_suggestion(&context).as_deref(), Some("de-latin1"));
    }

    #[test]
    fn keymap_suggestion_is_none_when_absent_from_the_list() {
        let context = context_with(GeoLocation {
            country_code: Some("FR".to_string()),
            timezone: None,
        });
        context.set::<LocalesKey>(annotated(&["fr_FR.UTF-8"]));
        context.set::<KeymapsKey>(annotated(&["us"]));

        assert_eq!(keymap_suggestion(&context), None);
    }

    #[test]
    fn mirror_region_suggestion_maps_country_code_to_region_name() {
        let context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: None,
        });
        context.set::<MirrorRegionCodesKey>(HashMap::from([
            ("Germany".to_string(), "DE".to_string()),
            ("France".to_string(), "FR".to_string()),
        ]));

        assert_eq!(
            mirror_region_suggestion(&context).as_deref(),
            Some("Germany")
        );
    }

    #[test]
    fn suggestions_are_none_without_a_detected_location() {
        let context = InstallContext::new();
        context.set::<TimezonesKey>(annotated(&["Europe/Berlin"]));
        context.set::<LocalesKey>(annotated(&["de_DE.UTF-8"]));
        context.set::<KeymapsKey>(annotated(&["de-latin1"]));

        assert_eq!(timezone_suggestion(&context), None);
        assert_eq!(locale_suggestion(&context), None);
        assert_eq!(keymap_suggestion(&context), None);
        assert_eq!(mirror_region_suggestion(&context), None);
    }
}
