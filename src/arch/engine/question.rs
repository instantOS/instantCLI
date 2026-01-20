use anyhow::Result;

use super::context::{DataKey, InstallContext};
use super::types::QuestionId;

/// Result of asking a question
pub enum QuestionResult {
    Answer(String),
    Cancelled,
}

/// Trait for providing async data to the install context
#[async_trait::async_trait]
pub trait AsyncDataProvider: Send + Sync {
    /// Fetches data and updates the context
    async fn provide(&self, context: &InstallContext) -> Result<()>;

    /// Returns an optional annotation provider for this data provider
    fn annotation_provider(&self) -> Option<Box<dyn crate::arch::annotations::AnnotationProvider>> {
        None
    }

    /// Helper to annotate and save a list of items to the context
    fn save_list<K, T>(&self, context: &InstallContext, items: Vec<T>)
    where
        T: crate::menu_utils::FzfSelectable + Clone + Send + Sync + Ord + 'static,
        K: DataKey<Value = Vec<crate::arch::annotations::AnnotatedValue<T>>>,
        Self: Sized,
    {
        let provider = self.annotation_provider();
        let annotated = crate::arch::annotations::annotate_list(provider.as_deref(), items);
        context.set::<K>(annotated);
    }
}

/// Trait that every question must implement
#[async_trait::async_trait]
pub trait Question: Send + Sync {
    fn id(&self) -> QuestionId;

    /// Returns a list of keys that must exist in context.data before this question is ready
    fn required_data_keys(&self) -> Vec<String> {
        vec![]
    }

    /// Returns true if the question is ready to be asked (dependencies met)
    fn is_ready(&self, context: &InstallContext) -> bool {
        let keys = self.required_data_keys();
        if keys.is_empty() {
            return true;
        }
        let data = context.data.lock().unwrap();
        keys.iter().all(|k| data.contains_key(k))
    }

    /// Asks the question and returns the answer or cancellation
    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult>;

    /// Returns true if the question is relevant/active given the current context
    fn should_ask(&self, _context: &InstallContext) -> bool {
        true
    }

    /// Returns true if the answer should be masked in the review UI
    fn is_sensitive(&self) -> bool {
        false
    }

    /// Returns true if the question is optional and should be skipped in the main flow
    fn is_optional(&self) -> bool {
        false
    }

    /// Validate the answer. Returns Ok(()) if valid, or Err(message) if invalid.
    fn validate(&self, _context: &InstallContext, _answer: &str) -> Result<(), String> {
        Ok(())
    }

    /// Returns a list of data providers required by this question
    fn data_providers(&self) -> Vec<Box<dyn AsyncDataProvider>> {
        vec![]
    }

    /// Returns the default value for this question if one exists
    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        None
    }

    /// Returns a fatal error message if this question cannot proceed due to a required
    /// data provider failure. Override this for questions where provider failure is fatal
    /// (e.g., disk selection). Return None for questions that handle failures gracefully
    /// (e.g., mirror regions with fallback).
    fn fatal_error_message(&self, _context: &InstallContext) -> Option<String> {
        None
    }
}
