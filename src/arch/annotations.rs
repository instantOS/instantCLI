use crate::menu_utils::{FzfPreview, FzfSelectable};
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

impl<T: FzfSelectable> FzfSelectable for AnnotatedValue<T> {
    fn fzf_display_text(&self) -> String {
        match &self.annotation {
            Some(ann) => format!("{} - {}", ann, self.value.fzf_display_text()),
            None => self.value.fzf_display_text(),
        }
    }

    fn fzf_key(&self) -> String {
        self.value.fzf_key()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.value.fzf_preview()
    }
}

pub trait AnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String>;
}

pub fn annotate_list<T: FzfSelectable + Clone>(
    provider: Option<&dyn AnnotationProvider>,
    items: Vec<T>,
) -> Vec<AnnotatedValue<T>> {
    items
        .into_iter()
        .map(|item| {
            let annotation = if let Some(p) = provider {
                let key = item.fzf_key();
                p.annotate(&key)
            } else {
                None
            };
            AnnotatedValue::new(item, annotation)
        })
        .collect()
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
    fn test_annotate_list() {
        let provider = LocaleAnnotationProvider;
        let items = vec!["de_DE.UTF-8", "unknown"];
        let annotated = annotate_list(Some(&provider), items);

        assert_eq!(annotated.len(), 2);
        assert_eq!(
            annotated[0].fzf_display_text(),
            "German (Germany) - de_DE.UTF-8"
        );
        assert_eq!(annotated[1].fzf_display_text(), "unknown");
    }

    #[test]
    fn test_annotate_list_no_provider() {
        let items = vec!["de_DE.UTF-8", "unknown"];
        let annotated = annotate_list(None, items);

        assert_eq!(annotated.len(), 2);
        assert_eq!(annotated[0].fzf_display_text(), "de_DE.UTF-8");
        assert_eq!(annotated[1].fzf_display_text(), "unknown");
    }
}
