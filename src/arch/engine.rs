use anyhow::Result;
use std::collections::HashMap;
use std::any::Any;
use std::sync::{Arc, Mutex};

use serde::{Serialize, Deserialize};

/// Represents a unique identifier for a question
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BootMode {
    UEFI64,
    UEFI32,
    BIOS,
}

impl Default for BootMode {
    fn default() -> Self {
        Self::BIOS
    }
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

/// Trait that every question must implement
#[async_trait::async_trait]
pub trait Question: Send + Sync {
    fn id(&self) -> QuestionId;
    
    /// Returns true if the question is ready to be asked (dependencies met)
    fn is_ready(&self, context: &InstallContext) -> bool;
    
    /// Asks the question and returns the answer as a string
    async fn ask(&self, context: &InstallContext) -> Result<String>;
    
    /// Returns true if this question should be skipped based on previous answers
    fn should_skip(&self, _context: &InstallContext) -> bool {
        false
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

    pub async fn run(&mut self) -> Result<InstallContext> {
        // Simple linear execution for now, but designed to be expandable for async dependencies
        // In a real async dependency graph, we'd loop and check `is_ready`
        
        for question in &self.questions {
            if question.should_skip(&self.context) {
                continue;
            }

            // Wait until ready (in a real async scenario this would be more complex)
            while !question.is_ready(&self.context) {
                // For now, we assume questions are ordered correctly or don't block indefinitely
                // In the future, we can have a background task fetching data
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            let answer = question.ask(&self.context).await?;
            self.context.set_answer(question.id(), answer);
        }

        Ok(self.context.clone())
    }
}
