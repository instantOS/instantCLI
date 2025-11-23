use anyhow::Result;
use std::fs;

pub struct LocaleProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for LocaleProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let contents = fs::read_to_string("/etc/locale.gen")?;
        let locales = parse_locale_gen(&contents);

        let mut data = context.data.lock().unwrap();
        data.insert("locales".to_string(), locales.join("\n"));

        Ok(())
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
            // Only include UTF-8 locales
            if !locale.is_empty() && clean_line.contains("UTF-8") {
                locales.push(locale.to_string());
            }
        }
    }

    // Sort and deduplicate
    locales.sort();
    locales.dedup();

    locales
}
