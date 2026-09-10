use crate::arch::annotations::AnnotatedValue;
use crate::arch::engine::DataKey;
use anyhow::Result;
use std::process::Command;

pub struct KeymapsKey;

impl DataKey for KeymapsKey {
    type Value = Vec<AnnotatedValue<String>>;
    const KEY: &'static str = "keymaps";
}

/// The keymap configured for the current system (`KEYMAP=` in
/// `/etc/vconsole.conf`), if any.
pub(crate) fn detect_current_keymap() -> Option<String> {
    let contents = std::fs::read_to_string("/etc/vconsole.conf").ok()?;
    parse_vconsole_keymap(&contents)
}

fn parse_vconsole_keymap(contents: &str) -> Option<String> {
    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("KEYMAP=")
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

pub struct KeymapProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for KeymapProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        let output = Command::new("localectl").arg("list-keymaps").output()?;

        let stdout = String::from_utf8(output.stdout)?;
        let keymaps = parse_keymaps(&stdout);

        self.save_list::<KeymapsKey, _>(context, keymaps);

        Ok(())
    }

    fn annotation_provider(&self) -> Option<Box<dyn crate::arch::annotations::AnnotationProvider>> {
        Some(Box::new(
            crate::arch::annotations::KeymapAnnotationProvider::new(),
        ))
    }
}

fn parse_keymaps(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_keymaps, parse_vconsole_keymap};

    #[test]
    fn test_parse_keymaps() {
        let input = "us\nde\nuk\n";
        let keymaps = parse_keymaps(input);
        assert_eq!(keymaps, vec!["us", "de", "uk"]);
    }

    #[test]
    fn test_parse_keymaps_empty() {
        let input = "";
        let keymaps = parse_keymaps(input);
        assert!(keymaps.is_empty());
    }

    #[test]
    fn parses_keymap_from_vconsole_conf() {
        let contents = "FONT=lat9w-16\nKEYMAP=de-latin1\n";
        assert_eq!(
            parse_vconsole_keymap(contents).as_deref(),
            Some("de-latin1")
        );
    }

    #[test]
    fn parses_quoted_keymap_and_tolerates_whitespace() {
        let contents = "  KEYMAP=\"us\"\n";
        assert_eq!(parse_vconsole_keymap(contents).as_deref(), Some("us"));
    }

    #[test]
    fn missing_or_empty_keymap_is_none() {
        assert_eq!(parse_vconsole_keymap("FONT=lat9w-16\n"), None);
        assert_eq!(parse_vconsole_keymap("KEYMAP=\n"), None);
        assert_eq!(parse_vconsole_keymap(""), None);
    }
}
