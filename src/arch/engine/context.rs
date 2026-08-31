use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use super::types::{StepId, SystemInfo};

/// Trait for defining type-safe keys for the data map
pub trait DataKey: Send + Sync + 'static {
    type Value: Send + Sync + Clone + 'static;
    const KEY: &'static str;
}

/// Key to store whether ESP needs to be formatted
/// False when reusing existing ESP in dual boot mode
pub struct EspNeedsFormat;

impl DataKey for EspNeedsFormat {
    type Value = bool;
    const KEY: &'static str = "esp_needs_format";
}

/// Key to store dual boot partition paths (root, boot, swap)
/// Used to pass partition paths from prepare_dualboot_disk to format_and_mount_partitions
pub struct DualBootPartitions;

/// Partition paths for dual boot installation
#[derive(Clone, Debug)]
pub struct DualBootPartitionPaths {
    pub root: String,
    pub boot: String,
    pub swap: String,
}

impl DataKey for DualBootPartitions {
    type Value = DualBootPartitionPaths;
    const KEY: &'static str = "dualboot_partitions";
}

/// Holds the state of the installation wizard
#[derive(Default, Clone)]
pub struct InstallContext {
    pub(super) answers: HashMap<StepId, String>,
    /// Steps which completed without producing configuration data.
    pub(super) completed_steps: BTreeSet<StepId>,
    /// Fingerprint of each step's dependency state when it completed.
    pub(super) step_dependency_fingerprints: HashMap<StepId, String>,
    pub system_info: SystemInfo,
    // We use Arc<Mutex> for interior mutability across threads
    pub data: Arc<Mutex<HashMap<String, Box<dyn Any + Send + Sync>>>>,
}

// Custom Serialize implementation to skip the data field
impl Serialize for InstallContext {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("InstallContext", 4)?;
        state.serialize_field("answers", &self.answers)?;
        state.serialize_field("completed_steps", &self.completed_steps)?;
        state.serialize_field(
            "step_dependency_fingerprints",
            &self.step_dependency_fingerprints,
        )?;
        state.serialize_field("system_info", &self.system_info)?;
        state.end()
    }
}

// Custom Deserialize implementation
impl<'de> Deserialize<'de> for InstallContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            answers: HashMap<StepId, String>,
            completed_steps: BTreeSet<StepId>,
            step_dependency_fingerprints: HashMap<StepId, String>,
            system_info: SystemInfo,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(InstallContext {
            answers: helper.answers,
            completed_steps: helper.completed_steps,
            step_dependency_fingerprints: helper.step_dependency_fingerprints,
            system_info: helper.system_info,
            data: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

impl InstallContext {
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let context: Self = toml::from_str(&content)?;
        Ok(context)
    }

    pub fn new() -> Self {
        Self {
            answers: HashMap::new(),
            completed_steps: BTreeSet::new(),
            step_dependency_fingerprints: HashMap::new(),
            system_info: SystemInfo::default(),
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_answer(&mut self, id: StepId, answer: String) {
        self.answers.insert(id, answer);
        self.completed_steps.remove(&id);
        // Direct callers do not have the step graph needed to establish
        // provenance. The engine must record dependency provenance before a
        // dependent answer can be considered current.
        self.step_dependency_fingerprints.remove(&id);
    }

    pub fn answers(&self) -> impl Iterator<Item = (&StepId, &String)> {
        self.answers.iter()
    }

    pub fn answer_count(&self) -> usize {
        self.answers.len()
    }

    pub fn has_answers(&self) -> bool {
        !self.answers.is_empty()
    }

    pub fn get_answer(&self, id: &StepId) -> Option<&String> {
        self.answers.get(id)
    }

    pub fn is_step_completed(&self, id: StepId) -> bool {
        self.answers.contains_key(&id) || self.completed_steps.contains(&id)
    }

    pub fn get_answer_bool(&self, id: StepId) -> bool {
        self.answers
            .get(&id)
            .map(|s| s == "true" || s == "yes")
            .unwrap_or(false)
    }

    /// Set a value in the data map using a strongly-typed key
    pub fn set<K: DataKey>(&self, value: K::Value) {
        let mut data = self.data.lock().unwrap();
        data.insert(K::KEY.to_string(), Box::new(value));
    }

    /// Get a value from the data map using a strongly-typed key
    pub fn get<K: DataKey>(&self) -> Option<K::Value> {
        let data = self.data.lock().unwrap();
        data.get(K::KEY)
            .and_then(|boxed| boxed.downcast_ref::<K::Value>())
            .cloned()
    }

    /// Create an InstallContext for setup mode by detecting current system settings.
    /// This allows `ins arch setup` to reuse the same code as `ins arch exec` without
    /// requiring a questions file.
    pub fn for_setup(username: Option<String>) -> Self {
        let mut ctx = Self::new();
        ctx.system_info = SystemInfo::detect();

        // Set username if provided
        if let Some(user) = username {
            ctx.set_answer(StepId::Username, user);
        }

        // Auto-detect locale from /etc/locale.conf
        if let Some(locale) = detect_system_locale() {
            ctx.set_answer(StepId::Locale, locale);
        }

        // Auto-detect timezone from /etc/localtime symlink
        if let Some(tz) = detect_system_timezone() {
            ctx.set_answer(StepId::Timezone, tz);
        }

        // Auto-detect keymap from /etc/vconsole.conf
        if let Some(keymap) = detect_system_keymap() {
            ctx.set_answer(StepId::Keymap, keymap);
        }

        // Read hostname from /etc/hostname
        if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
            let hostname = hostname.trim().to_string();
            if !hostname.is_empty() {
                ctx.set_answer(StepId::Hostname, hostname);
            }
        }

        ctx
    }
}

/// Detect system locale from /etc/locale.conf
fn detect_system_locale() -> Option<String> {
    std::fs::read_to_string("/etc/locale.conf")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.starts_with("LANG="))
                .map(|l| l.trim_start_matches("LANG=").trim().to_string())
        })
}

/// Detect system timezone from /etc/localtime symlink
fn detect_system_timezone() -> Option<String> {
    std::fs::read_link("/etc/localtime").ok().and_then(|path| {
        path.to_string_lossy()
            .strip_prefix("/usr/share/zoneinfo/")
            .map(|s| s.to_string())
    })
}

/// Detect system keymap from /etc/vconsole.conf
fn detect_system_keymap() -> Option<String> {
    std::fs::read_to_string("/etc/vconsole.conf")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.starts_with("KEYMAP="))
                .map(|l| l.trim_start_matches("KEYMAP=").trim().to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::{DataKey, InstallContext};
    use crate::arch::engine::StepId;

    struct StringKey;

    impl DataKey for StringKey {
        type Value = String;
        const KEY: &'static str = "string_key";
    }

    struct IntKey;

    impl DataKey for IntKey {
        type Value = i32;
        const KEY: &'static str = "int_key";
    }

    #[test]
    fn typed_data_round_trips_and_missing_keys_return_none() {
        let context = InstallContext::new();
        context.set::<StringKey>("hello".to_string());
        context.set::<IntKey>(42);

        assert_eq!(context.get::<StringKey>(), Some("hello".to_string()));
        assert_eq!(context.get::<IntKey>(), Some(42));

        struct MissingKey;
        impl DataKey for MissingKey {
            type Value = bool;
            const KEY: &'static str = "missing";
        }
        assert_eq!(context.get::<MissingKey>(), None);
    }

    #[test]
    fn contexts_without_wizard_state_are_rejected() {
        let context = InstallContext::new();
        let mut serialized = toml::Value::try_from(&context).unwrap();
        let table = serialized.as_table_mut().unwrap();
        table.remove("completed_steps");
        table.remove("step_dependency_fingerprints");

        assert!(toml::from_str::<InstallContext>(&serialized.to_string()).is_err());
    }

    #[test]
    fn completion_state_round_trips_separately_from_answers() {
        let mut context = InstallContext::new();
        context.completed_steps.insert(StepId::LowRamWarning);
        context
            .step_dependency_fingerprints
            .insert(StepId::LowRamWarning, "fingerprint".to_string());

        let restored: InstallContext = toml::from_str(&context.to_toml().unwrap()).unwrap();

        assert!(restored.is_step_completed(StepId::LowRamWarning));
        assert!(restored.get_answer(&StepId::LowRamWarning).is_none());
        assert_eq!(
            restored
                .step_dependency_fingerprints
                .get(&StepId::LowRamWarning)
                .map(String::as_str),
            Some("fingerprint")
        );
    }
}
