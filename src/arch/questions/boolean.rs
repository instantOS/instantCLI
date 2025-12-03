use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;

pub struct BooleanQuestion {
    pub id: QuestionId,
    pub prompt: String,
    pub icon: NerdFont,
    pub is_optional: bool,
    pub default_yes: bool,
    pub dynamic_default: Option<Box<dyn Fn(&InstallContext) -> bool + Send + Sync>>,
    pub should_ask_predicate: Option<Box<dyn Fn(&InstallContext) -> bool + Send + Sync>>,
}

impl BooleanQuestion {
    pub fn new(id: QuestionId, prompt: impl Into<String>, icon: NerdFont) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            icon,
            is_optional: false,
            default_yes: false,
            dynamic_default: None,
            should_ask_predicate: None,
        }
    }

    pub fn optional(mut self) -> Self {
        self.is_optional = true;
        self
    }

    pub fn default_yes(mut self) -> Self {
        self.default_yes = true;
        self
    }

    pub fn dynamic_default<F>(mut self, func: F) -> Self
    where
        F: Fn(&InstallContext) -> bool + 'static + Send + Sync,
    {
        self.dynamic_default = Some(Box::new(func));
        self
    }

    pub fn should_ask<F>(mut self, func: F) -> Self
    where
        F: Fn(&InstallContext) -> bool + 'static + Send + Sync,
    {
        self.should_ask_predicate = Some(Box::new(func));
        self
    }
}

#[async_trait::async_trait]
impl Question for BooleanQuestion {
    fn id(&self) -> QuestionId {
        self.id.clone()
    }

    fn is_optional(&self) -> bool {
        self.is_optional
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if let Some(predicate) = &self.should_ask_predicate {
            predicate(context)
        } else {
            true
        }
    }

    fn get_default(&self, context: &InstallContext) -> Option<String> {
        let effective_default = if let Some(dynamic_func) = &self.dynamic_default {
            dynamic_func(context)
        } else {
            self.default_yes
        };
        Some(if effective_default {
            "yes".to_string()
        } else {
            "no".to_string()
        })
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        // Always present Yes/No in a consistent order, ignoring the default for UI purposes
        let options = vec!["yes".to_string(), "no".to_string()];

        let result = FzfWrapper::builder()
            .header(format!("{} {}", self.icon, self.prompt))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(ans) => Ok(QuestionResult::Answer(ans)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}
