use super::{DoctorCheck, checks::*};
use std::collections::HashMap;

pub type CheckFactory = fn() -> Box<dyn DoctorCheck + Send + Sync>;

pub struct CheckRegistry {
    checks: HashMap<&'static str, CheckFactory>,
}

impl CheckRegistry {
    pub fn new() -> Self {
        let mut registry = CheckRegistry {
            checks: HashMap::new(),
        };

        // Register all checks
        registry.register::<InternetCheck>("internet");
        registry.register::<InstantRepoCheck>("instant-repo");
        registry.register::<LocaleCheck>("locale");
        registry.register::<NerdFontCheck>("nerd-font");
        registry.register::<PacmanCacheCheck>("pacman-cache");
        registry.register::<YayCacheCheck>("yay-cache");
        registry.register::<PacmanStaleDownloadsCheck>("pacman-stale-downloads");
        registry.register::<SwapCheck>("swap");
        registry.register::<PendingUpdatesCheck>("pending-updates");
        registry.register::<SmartHealthCheck>("smart-health");
        registry.register::<PacmanDbSyncCheck>("pacman-db-sync");
        registry.register::<SwayDisplayCheck>("sway-display");
        registry.register::<PolkitAgentCheck>("polkit-agent");
        registry.register::<BatCheck>("bat-cache");
        registry.register::<GitConfigCheck>("git-config");
        registry.register::<ShellCompletionCheck>("shell-completions");

        registry
    }

    fn register<T: DoctorCheck + Default + Send + Sync + 'static>(&mut self, id: &'static str) {
        self.checks.insert(id, || Box::new(T::default()));
    }

    pub fn create_check(&self, id: &str) -> Option<Box<dyn DoctorCheck + Send + Sync>> {
        self.checks.get(id).map(|factory| factory())
    }

    pub fn all_checks(&self) -> Vec<Box<dyn DoctorCheck + Send + Sync>> {
        self.checks.values().map(|factory| factory()).collect()
    }
}

// Global registry instance
lazy_static::lazy_static! {
    pub static ref REGISTRY: CheckRegistry = CheckRegistry::new();
}
