use super::step::InstallStep;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::paths;

const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallState {
    pub schema_version: u32,
    pub configuration_sha256: String,
    pub completed_steps: HashSet<InstallStep>,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
}

impl InstallState {
    pub fn new(configuration_sha256: impl Into<String>) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            configuration_sha256: configuration_sha256.into(),
            completed_steps: HashSet::new(),
            start_time: None,
        }
    }

    pub fn mark_start(&mut self) {
        if self.start_time.is_none() {
            self.start_time = Some(chrono::Utc::now());
        }
    }

    pub fn load() -> Result<Self> {
        if Path::new(paths::STATE_FILE).exists() {
            let content = fs::read_to_string(paths::STATE_FILE)?;
            let state: InstallState = toml::from_str(&content)?;
            Ok(state)
        } else {
            Ok(Self::new(String::new()))
        }
    }

    pub fn load_for_configuration(configuration_sha256: &str) -> Self {
        match Self::load() {
            Ok(state)
                if state.schema_version == STATE_SCHEMA_VERSION
                    && state.configuration_sha256 == configuration_sha256 =>
            {
                state
            }
            Ok(state) if !state.configuration_sha256.is_empty() => {
                println!(
                    "Installation configuration changed; starting with fresh execution state."
                );
                Self::new(configuration_sha256)
            }
            Ok(_) => Self::new(configuration_sha256),
            Err(error) => {
                println!("Ignoring incompatible installation state: {error}");
                Self::new(configuration_sha256)
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = Path::new(paths::STATE_FILE).parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(paths::STATE_FILE, content)?;
        Ok(())
    }

    pub fn mark_complete(&mut self, step: InstallStep) {
        self.completed_steps.insert(step);
    }

    pub fn is_complete(&self, step: InstallStep) -> bool {
        self.completed_steps.contains(&step)
    }

    pub fn check_dependencies(&self, step: InstallStep) -> Result<(), Vec<InstallStep>> {
        let deps = step.dependencies();
        let missing: Vec<InstallStep> = deps
            .into_iter()
            .filter(|dep| !self.is_complete(*dep))
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }
}
