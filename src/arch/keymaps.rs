use crate::arch::annotations::{AnnotatedValue, AnnotationProvider};
use crate::arch::engine::DataKey;
use anyhow::Result;
use std::process::Command;

pub struct KeymapsKey;

impl DataKey for KeymapsKey {
    type Value = Vec<AnnotatedValue<String>>;
    const KEY: &'static str = "keymaps";
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
        Some(Box::new(crate::arch::annotations::KeymapAnnotationProvider))
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
    use super::*;

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
}
