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
        Some(Box::new(crate::arch::annotations::LocaleAnnotationProvider))
    }
}
