//! Best-effort geographic detection for install-time suggestions.
//!
//! The Arch live environment carries stock defaults (`UTC`, `en_US.UTF-8`,
//! `us`), so the per-module `detect_current_*` helpers cannot tell us where
//! the user actually is. This module asks a public IP-geolocation service for
//! a country/timezone hint and derives the location-dependent preselections
//! (timezone and mirror region) from it. Language and keyboard layout are
//! personal preferences, not geographic properties, so they deliberately use
//! the existing local-system defaults instead.
//!
//! Detection is strictly a nicety: every failure path yields an empty
//! [`GeoLocation`], and the questions fall back to their previous behaviour.

use std::time::Duration;

use serde::Deserialize;

use crate::arch::engine::{AsyncDataProvider, DataKey, InstallContext, StepId};
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
pub struct GeoLocationProvider {
    question: StepId,
}

impl GeoLocationProvider {
    pub fn for_question(question: StepId) -> Self {
        Self { question }
    }
}

#[async_trait::async_trait]
impl AsyncDataProvider for GeoLocationProvider {
    async fn provide(&self, context: &InstallContext) -> anyhow::Result<()> {
        // Do not disclose the public IP when a saved or previous answer already
        // determines where the cursor should start.
        if context.previous_answer(&self.question).is_some() {
            return Ok(());
        }
        // Offline installs never call out: store the empty location a failed
        // lookup would produce so preselection falls back to stock defaults.
        if crate::arch::offline::mode().is_offline() {
            context.set::<GeoLocationKey>(GeoLocation::default());
            return Ok(());
        }
        context.set::<GeoLocationKey>(detect_location().await);
        Ok(())
    }
}

/// Fetches the location hint once per process.
async fn detect_location() -> GeoLocation {
    GEO_CACHE
        .get_or_init(|| async {
            let mut partial = GeoLocation::default();
            for endpoint in GEO_ENDPOINTS {
                match tokio::time::timeout(GEO_TIMEOUT, fetch_geo_location(endpoint)).await {
                    Ok(Ok(location)) if !location.is_empty() => {
                        if location.country_code.is_some() && location.timezone.is_some() {
                            return location;
                        }
                        if partial.country_code.is_none() {
                            partial.country_code = location.country_code;
                        }
                        if partial.timezone.is_none() {
                            partial.timezone = location.timezone;
                        }
                    }
                    _ => continue,
                }
            }
            partial
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
    let timezone = location.timezone?;
    timezones
        .iter()
        .any(|item| item.value == timezone)
        .then_some(timezone)
}

/// Fall back from an earlier keyboard choice when IP detection has no usable
/// timezone. The language-to-country defaults are intentionally small and
/// conservative; missing entries simply produce no suggestion.
pub fn timezone_suggestion_from_keymap(context: &InstallContext) -> Option<String> {
    let keymap = context.get_answer(&StepId::Keymap)?;
    let (_, country) = keymap_profile(keymap)?;
    let timezone = crate::arch::timezones::timezone_for_country(context, country)?;
    let timezones = context.get::<TimezonesKey>()?;
    timezones
        .iter()
        .any(|item| item.value == timezone)
        .then_some(timezone)
}

/// Suggest a locale only when earlier choices provide both a language and a
/// territory. This avoids guessing the first locale listed for multilingual
/// countries.
pub fn locale_suggestion(context: &InstallContext) -> Option<String> {
    let keymap = context.get_answer(&StepId::Keymap)?;
    let (language, _) = keymap_profile(keymap)?;
    let country = context
        .get_answer(&StepId::Timezone)
        .and_then(|timezone| crate::arch::timezones::country_for_timezone(context, timezone))
        .or_else(|| context.get::<GeoLocationKey>()?.country_code)?;
    let locales = context.get::<LocalesKey>()?;

    locales.into_iter().find_map(|item| {
        let (candidate_language, candidate_country) = locale_parts(&item.value)?;
        (candidate_language.eq_ignore_ascii_case(language)
            && candidate_country.eq_ignore_ascii_case(&country))
        .then_some(item.value)
    })
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

fn locale_parts(locale: &str) -> Option<(&str, &str)> {
    let base = locale.split(['.', '@']).next()?;
    let (language, country) = base.split_once('_')?;
    (!language.is_empty() && !country.is_empty()).then_some((language, country))
}

fn keymap_profile(keymap: &str) -> Option<(&'static str, &'static str)> {
    let base = keymap.split(['-', '_']).next()?;
    KEYMAP_PROFILES
        .iter()
        .find(|(candidate, _, _)| candidate.eq_ignore_ascii_case(base))
        .map(|(_, language, country)| (*language, *country))
}

/// Common console-keymap families and their best-effort language/territory.
const KEYMAP_PROFILES: &[(&str, &str, &str)] = &[
    ("us", "en", "US"),
    ("uk", "en", "GB"),
    ("de", "de", "DE"),
    ("sg", "de", "CH"),
    ("fr", "fr", "FR"),
    ("be", "nl", "BE"),
    ("es", "es", "ES"),
    ("la", "es", "MX"),
    ("it", "it", "IT"),
    ("pt", "pt", "PT"),
    ("br", "pt", "BR"),
    ("nl", "nl", "NL"),
    ("sv", "sv", "SE"),
    ("no", "nb", "NO"),
    ("dk", "da", "DK"),
    ("fi", "fi", "FI"),
    ("pl", "pl", "PL"),
    ("cz", "cs", "CZ"),
    ("sk", "sk", "SK"),
    ("hu", "hu", "HU"),
    ("ro", "ro", "RO"),
    ("ru", "ru", "RU"),
    ("ua", "uk", "UA"),
    ("tr", "tr", "TR"),
    ("gr", "el", "GR"),
    ("il", "he", "IL"),
    ("jp106", "ja", "JP"),
    ("kr", "ko", "KR"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::arch::annotations::AnnotatedValue;

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

    #[tokio::test]
    async fn provider_skips_lookup_when_question_has_a_previous_answer() {
        let mut context = InstallContext::new();
        context.set_answer(StepId::Timezone, "Europe/Berlin".to_string());

        GeoLocationProvider::for_question(StepId::Timezone)
            .provide(&context)
            .await
            .expect("provider should succeed without a lookup");

        assert_eq!(context.get::<GeoLocationKey>(), None);
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
    fn timezone_suggestion_needs_an_exact_timezone() {
        let context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: None,
        });
        context.set::<TimezonesKey>(annotated(&["Europe/Berlin"]));

        assert_eq!(timezone_suggestion(&context), None);
    }

    #[test]
    fn locale_suggestion_combines_keymap_language_with_geo_country() {
        let mut context = context_with(GeoLocation {
            country_code: Some("DE".to_string()),
            timezone: Some("Europe/Berlin".to_string()),
        });
        context.set_answer(StepId::Keymap, "de-latin1".to_string());
        context.set::<LocalesKey>(annotated(&["de_AT.UTF-8", "hsb_DE.UTF-8", "de_DE.UTF-8"]));

        assert_eq!(locale_suggestion(&context).as_deref(), Some("de_DE.UTF-8"));
    }

    #[test]
    fn keymap_profiles_keep_language_and_region_distinct() {
        assert_eq!(keymap_profile("de-latin1"), Some(("de", "DE")));
        assert_eq!(keymap_profile("uk"), Some(("en", "GB")));
        assert_eq!(keymap_profile("br-abnt2"), Some(("pt", "BR")));
        assert_eq!(keymap_profile("unknown"), None);
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

        assert_eq!(timezone_suggestion(&context), None);
        assert_eq!(mirror_region_suggestion(&context), None);
    }
}
