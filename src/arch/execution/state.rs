use super::step::InstallStep;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::paths;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstallState {
    pub completed_steps: HashSet<InstallStep>,
}

impl InstallState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load() -> Result<Self> {
        if Path::new(paths::STATE_FILE).exists() {
            let content = fs::read_to_string(paths::STATE_FILE)?;
            let state: InstallState = toml::from_str(&content)?;
            Ok(state)
        } else {
            Ok(Self::new())
        }
    }

    pub fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = Path::new(paths::STATE_FILE).parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
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
