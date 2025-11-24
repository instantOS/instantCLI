use crate::menu_utils::FzfSelectable;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AnnotatedValue<T> {
    pub value: T,
    pub annotation: Option<String>,
}

impl<T> AnnotatedValue<T> {
    pub fn new(value: T, annotation: Option<String>) -> Self {
        Self { value, annotation }
    }
}

impl<T: ToString> FzfSelectable for AnnotatedValue<T> {
    fn fzf_display_text(&self) -> String {
        match &self.annotation {
            Some(ann) => format!("{} - {}", ann, self.value.to_string()),
            None => self.value.to_string(),
        }
    }

    fn fzf_key(&self) -> String {
        self.value.to_string()
    }
}

pub trait AnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String>;
}

pub struct LocaleAnnotationProvider;

impl AnnotationProvider for LocaleAnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String> {
        let map: HashMap<&str, &str> = [
            ("en_US.UTF-8", "English (United States)"),
            ("en_GB.UTF-8", "English (United Kingdom)"),
            ("de_DE.UTF-8", "German (Germany)"),
            ("fr_FR.UTF-8", "French (France)"),
            ("es_ES.UTF-8", "Spanish (Spain)"),
            ("it_IT.UTF-8", "Italian (Italy)"),
            ("pt_BR.UTF-8", "Portuguese (Brazil)"),
            ("ru_RU.UTF-8", "Russian (Russia)"),
            ("ja_JP.UTF-8", "Japanese (Japan)"),
            ("zh_CN.UTF-8", "Chinese (China)"),
        ]
        .iter()
        .cloned()
        .collect();

        map.get(value).map(|s| s.to_string())
    }
}

pub struct KeymapAnnotationProvider;

impl AnnotationProvider for KeymapAnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String> {
        let map: HashMap<&str, &str> = [
            ("us", "English (US)"),
            ("de-latin1", "German"),
            ("uk", "English (UK)"),
            ("fr", "French"),
            ("es", "Spanish"),
            ("it", "Italian"),
            ("pt-latin1", "Portuguese"),
            ("ru", "Russian"),
            ("jp106", "Japanese"),
        ]
        .iter()
        .cloned()
        .collect();

        map.get(value).map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annotated_value_display() {
        let val = AnnotatedValue::new("de_DE.UTF-8", Some("German (Germany)".to_string()));
        assert_eq!(val.fzf_display_text(), "German (Germany) - de_DE.UTF-8");

        let val_no_ann = AnnotatedValue::new("unknown", None);
        assert_eq!(val_no_ann.fzf_display_text(), "unknown");
    }

    #[test]
    fn test_locale_annotation() {
        let provider = LocaleAnnotationProvider;
        assert_eq!(
            provider.annotate("de_DE.UTF-8"),
            Some("German (Germany)".to_string())
        );
        assert_eq!(provider.annotate("unknown"), None);
    }

    #[test]
    fn test_keymap_annotation() {
        let provider = KeymapAnnotationProvider;
        assert_eq!(provider.annotate("us"), Some("English (US)".to_string()));
        assert_eq!(provider.annotate("unknown"), None);
    }
}
