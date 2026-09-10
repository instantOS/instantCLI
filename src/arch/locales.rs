use crate::arch::annotations::AnnotatedValue;
use crate::arch::engine::DataKey;
use crate::common::locale_gen::available_locales;
use anyhow::{Context, Result};
use std::fs;

pub struct LocalesKey;

impl DataKey for LocalesKey {
    type Value = Vec<AnnotatedValue<String>>;
    const KEY: &'static str = "locales";
}

/// The locale configured for the current system (`LANG=` in
/// `/etc/locale.conf`), if any.
pub(crate) fn detect_current_locale() -> Option<String> {
    let contents = fs::read_to_string("/etc/locale.conf").ok()?;
    parse_locale_conf_lang(&contents)
}

fn parse_locale_conf_lang(contents: &str) -> Option<String> {
    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("LANG=")
                .map(|value| strip_surrounding_quotes(value.trim()))
        })
        .map(str::to_string)
        .find(|value| !value.is_empty())
}

fn strip_surrounding_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

pub struct LocaleProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for LocaleProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let contents = fs::read_to_string("/etc/locale.gen").context("reading /etc/locale.gen")?;
        let locales = available_locales(&contents);

        self.save_list::<LocalesKey, _>(context, locales);

        Ok(())
    }

    fn annotation_provider(&self) -> Option<Box<dyn crate::arch::annotations::AnnotationProvider>> {
        Some(Box::new(
            crate::arch::annotations::LocaleAnnotationProvider::new(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_locale_conf_lang;

    #[test]
    fn parses_lang_from_locale_conf() {
        let contents = "KEYMAP=us\nLANG=de_DE.UTF-8\nLC_COLLATE=C\n";
        assert_eq!(
            parse_locale_conf_lang(contents).as_deref(),
            Some("de_DE.UTF-8")
        );
    }

    #[test]
    fn parses_quoted_lang_and_tolerates_whitespace() {
        let contents = "  LANG=\"de_DE.UTF-8\"\n";
        assert_eq!(
            parse_locale_conf_lang(contents).as_deref(),
            Some("de_DE.UTF-8")
        );
    }

    #[test]
    fn missing_or_empty_lang_is_none() {
        assert_eq!(parse_locale_conf_lang("LC_COLLATE=C\n"), None);
        assert_eq!(parse_locale_conf_lang("LANG=\n"), None);
        assert_eq!(parse_locale_conf_lang(""), None);
    }
}
