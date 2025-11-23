use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Represents a unique identifier for a question
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
pub enum QuestionId {
    Hostname,
    Username,
    Password,
    Keymap,
    Disk,
    MirrorRegion,
    Timezone,
    Locale,
    ConfirmInstall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum BootMode {
    UEFI64,
    UEFI32,
    #[default]
    BIOS,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemInfo {
    pub boot_mode: BootMode,
    pub has_amd_cpu: bool,
    pub has_intel_cpu: bool,
    pub has_nvidia_gpu: bool,
    pub internet_connected: bool,
}

/// Holds the state of the installation wizard
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct InstallContext {
    pub answers: HashMap<QuestionId, String>,
    pub system_info: SystemInfo,
    #[serde(skip)]
    pub data: Arc<Mutex<HashMap<String, String>>>, // For async data like mirror lists
}

impl InstallContext {
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }
    pub fn new() -> Self {
        Self {
            answers: HashMap::new(),
            system_info: SystemInfo::default(),
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_answer(&mut self, id: QuestionId, answer: String) {
        self.answers.insert(id, answer);
    }

    pub fn get_answer(&self, id: &QuestionId) -> Option<&String> {
        self.answers.get(id)
    }
}

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

    /// Returns true if this question should be skipped based on previous answers
    fn should_skip(&self, _context: &InstallContext) -> bool {
        false
    }

    /// Validate the answer. Returns Ok(()) if valid, or Err(message) if invalid.
    fn validate(&self, _answer: &str) -> Result<(), String> {
        Ok(())
    }

    /// Returns a list of data providers required by this question
    fn data_providers(&self) -> Vec<Box<dyn AsyncDataProvider>> {
        vec![]
    }
}

pub struct QuestionEngine {
    questions: Vec<Box<dyn Question>>,
    pub context: InstallContext,
}

impl QuestionEngine {
    pub fn new(questions: Vec<Box<dyn Question>>) -> Self {
        Self {
            questions,
            context: InstallContext::new(),
        }
    }

    pub fn initialize_providers(&self) {
        for question in &self.questions {
            for provider in question.data_providers() {
                let context = self.context.clone();
                tokio::spawn(async move {
                    if let Err(e) = provider.provide(&context).await {
                        eprintln!("Data provider failed: {}", e);
                    }
                });
            }
        }
    }

    fn handle_review(&self, current_index: usize) -> Result<Option<usize>> {
        let mut review_items = Vec::new();
        for q in self.questions.iter().take(current_index) {
            if let Some(ans) = self.context.get_answer(&q.id()) {
                review_items.push(format!("{:?}: {}", q.id(), ans));
            }
        }

        if review_items.is_empty() {
            crate::menu_utils::FzfWrapper::message("No answers to review yet.")?;
            return Ok(None);
        }

        let review = crate::menu_utils::FzfWrapper::builder()
            .header("Select a question to modify")
            .select(review_items)?;

        match review {
            crate::menu_utils::FzfResult::Selected(selection) => {
                let parts: Vec<&str> = selection.splitn(2, ": ").collect();
                if let Some(id_str) = parts.first() {
                    if let Some(new_index) = self
                        .questions
                        .iter()
                        .position(|q| format!("{:?}", q.id()) == *id_str)
                    {
                        return Ok(Some(new_index));
                    }
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn handle_go_back(&self, mut index: usize) -> usize {
        if index > 0 {
            index -= 1;
            while index > 0 && self.questions[index].should_skip(&self.context) {
                index -= 1;
            }
        }
        index
    }

    pub async fn run(mut self) -> Result<InstallContext> {
        let mut index = 0;
        while index < self.questions.len() {
            let question = &self.questions[index];

            // Wait until question is ready (dependencies met)
            while !question.is_ready(&self.context) {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            if question.should_skip(&self.context) {
                index += 1;
                continue;
            }

            loop {
                let result = question.ask(&self.context).await?;
                match result {
                    QuestionResult::Answer(answer) => match question.validate(&answer) {
                        Ok(()) => {
                            self.context.answers.insert(question.id(), answer);
                            index += 1;
                            break;
                        }
                        Err(msg) => {
                            crate::menu_utils::FzfWrapper::message(&msg)?;
                        }
                    },
                    QuestionResult::Cancelled => {
                        // Show Navigation Menu
                        let options =
                            vec!["Resume", "Review Answers", "Go Back", "Abort Installation"];
                        let nav = crate::menu_utils::FzfWrapper::builder()
                            .header("Installation Paused")
                            .select(options)?;

                        match nav {
                            crate::menu_utils::FzfResult::Selected(opt) => match opt {
                                "Resume" => continue, // Retry question
                                "Review Answers" => {
                                    if let Some(new_index) = self.handle_review(index)? {
                                        index = new_index;
                                        break;
                                    }
                                    continue;
                                }
                                "Go Back" => {
                                    index = self.handle_go_back(index);
                                    break; // Break inner loop, go to outer loop with new index
                                }
                                "Abort Installation" => {
                                    std::process::exit(0);
                                }
                                _ => continue,
                            },
                            _ => continue, // Cancelled menu -> Resume
                        }
                    }
                }
            }
        }

        Ok(self.context.clone())
    }
}
