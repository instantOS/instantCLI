use anyhow::Result;
use std::fs;
use crate::arch::annotations::{AnnotatedValue, AnnotationProvider};
use crate::arch::engine::DataKey;

pub struct LocalesKey;

impl DataKey for LocalesKey {
    type Value = Vec<AnnotatedValue<String>>;
    const KEY: &'static str = "locales";
}

pub struct LocaleProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for LocaleProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let contents = fs::read_to_string("/etc/locale.gen")?;
        let locales = parse_locale_gen(&contents);

        self.save_list::<LocalesKey, _>(context, locales);

        Ok(())
    }

    fn annotation_provider(&self) -> Option<Box<dyn crate::arch::annotations::AnnotationProvider>> {
        Some(Box::new(crate::arch::annotations::LocaleAnnotationProvider))
    }
}

fn parse_locale_gen(contents: &str) -> Vec<String> {
    let mut locales = Vec::new();

    for line in contents.lines() {
        let line = line.trim();

        // Skip empty lines and comment headers (lines starting with %)
        if line.is_empty() || line.starts_with('%') {
            continue;
        }

        // Strip leading # to handle both commented and uncommented lines
        let clean_line = line.trim_start_matches('#').trim();

        if clean_line.is_empty() {
            continue;
        }

        // Get the locale name (first word)
        if let Some(locale) = clean_line.split_whitespace().next() {
            // Only include locales that end with .UTF-8
            if locale.ends_with(".UTF-8") {
                locales.push(locale.to_string());
            }
        }
    }

    // Sort and deduplicate
    locales.sort();
    locales.dedup();

    locales
}
