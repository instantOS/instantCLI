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
        if line.is_empty() || line.starts_with('#') || line.starts_with('%') {
            continue;
        }
        // Format is usually: en_US.UTF-8 UTF-8
        // We just want the first part
        if let Some(locale) = line.split_whitespace().next() {
            locales.push(locale.to_string());
        }
    }
    // If empty (e.g. all commented out), maybe fallback or return empty?
    // Let's also include commented ones but maybe mark them?
    // Actually, usually we want to select from ALL available locales to generate.
    // The previous code in settings/language/locale_gen.rs parsed ALL lines, even commented ones.
    // Let's do that too, so user can select any locale supported by the system.

    if locales.is_empty() {
        // Re-parse including commented lines
        for line in contents.lines() {
            let line = line.trim();
            // Skip empty or comments that are not locales (e.g. headers)
            // But how to distinguish?
            // The settings code had a robust parser. Let's try to be simple but effective.
            // Look for lines that contain "UTF-8" or "ISO" maybe?

            // Let's just strip leading # and whitespace
            let clean_line = line.trim_start_matches('#').trim();
            if let Some(locale) = clean_line.split_whitespace().next() {
                if !locale.is_empty()
                    && (clean_line.contains("UTF-8") || clean_line.contains("ISO"))
                {
                    locales.push(locale.to_string());
                }
            }
        }
    }

    // Sort and deduplicate
    locales.sort();
    locales.dedup();

    locales
}
