use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ui::nerd_font::NerdFont;
use colored::Colorize;
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
    Kernel,
    UseEncryption,
    EncryptionPassword,
    UsePlymouth,
    LogUpload,
    ConfirmInstall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum BootMode {
    UEFI64,
    UEFI32,
    #[default]
    BIOS,
}

impl std::fmt::Display for BootMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootMode::UEFI64 => write!(f, "UEFI64"),
            BootMode::UEFI32 => write!(f, "UEFI32"),
            BootMode::BIOS => write!(f, "BIOS"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GpuKind {
    Nvidia,
    Amd,
    Intel,
    Other(String),
}

impl std::fmt::Display for GpuKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuKind::Nvidia => write!(f, "NVIDIA"),
            GpuKind::Amd => write!(f, "AMD"),
            GpuKind::Intel => write!(f, "Intel"),
            GpuKind::Other(name) => write!(f, "{}", name),
        }
    }
}

impl GpuKind {
    pub fn to_colored_string(&self) -> colored::ColoredString {
        match self {
            GpuKind::Nvidia => self.to_string().bright_green(),
            GpuKind::Amd => self.to_string().bright_red(),
            GpuKind::Intel => self.to_string().bright_blue(),
            GpuKind::Other(_) => self.to_string().normal(),
        }
    }

    pub fn get_driver_packages(&self) -> Vec<&'static str> {
        match self {
            GpuKind::Nvidia => vec!["nvidia", "nvidia-utils", "nvidia-settings"],
            GpuKind::Amd => vec![
                "vulkan-radeon",
                "lib32-vulkan-radeon",
                "libva-mesa-driver",
                "lib32-libva-mesa-driver",
            ],
            GpuKind::Intel => vec!["vulkan-intel", "lib32-vulkan-intel", "intel-media-driver"],
            GpuKind::Other(_) => vec!["mesa", "lib32-mesa"],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemInfo {
    pub boot_mode: BootMode,
    pub has_amd_cpu: bool,
    pub has_intel_cpu: bool,
    pub gpus: Vec<GpuKind>,
    pub vm_type: Option<String>,
    pub internet_connected: bool,
}

impl SystemInfo {
    pub fn detect() -> Self {
        let mut info = SystemInfo::default();

        // Internet check
        info.internet_connected = crate::common::network::check_internet();

        // Boot mode check
        if std::path::Path::new("/sys/firmware/efi/fw_platform_size").exists() {
            let content =
                std::fs::read_to_string("/sys/firmware/efi/fw_platform_size").unwrap_or_default();
            if content.trim() == "64" {
                info.boot_mode = BootMode::UEFI64;
            } else if content.trim() == "32" {
                info.boot_mode = BootMode::UEFI32;
            }
        } else if std::path::Path::new("/sys/firmware/efi").exists() {
            // Fallback if fw_platform_size doesn't exist but efi does
            info.boot_mode = BootMode::UEFI64;
        }

        // CPU check
        if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
            info.has_amd_cpu = cpuinfo.contains("AuthenticAMD");
            info.has_intel_cpu = cpuinfo.contains("GenuineIntel");
        }

        // GPU check using /sys/class/drm/ approach
        let mut found_gpus = false;
        if let Ok(drm_entries) = std::fs::read_dir("/sys/class/drm") {
            let mut detected_gpus = std::collections::HashSet::new();

            for entry in drm_entries.flatten() {
                if let Ok(path) = entry.path().join("device").read_link() {
                    if let Some(path_str) = path.to_str() {
                        let path_lower = path_str.to_lowercase();
                        if path_lower.contains("nvidia") {
                            detected_gpus.insert(GpuKind::Nvidia);
                            found_gpus = true;
                        } else if path_lower.contains("amd") || path_lower.contains("radeon") {
                            detected_gpus.insert(GpuKind::Amd);
                            found_gpus = true;
                        } else if path_lower.contains("intel") {
                            detected_gpus.insert(GpuKind::Intel);
                            found_gpus = true;
                        }
                    }
                }
            }

            if found_gpus {
                info.gpus = detected_gpus.into_iter().collect();
            }
        }

        // Fallback to lspci if drm detection didn't find anything
        if !found_gpus {
            if let Ok(lspci) = std::process::Command::new("lspci").output() {
                let output = String::from_utf8_lossy(&lspci.stdout);
                let mut detected_gpus = std::collections::HashSet::new();

                if output.to_lowercase().contains("nvidia") {
                    detected_gpus.insert(GpuKind::Nvidia);
                }
                if output.to_lowercase().contains("amd")
                    || output.to_lowercase().contains("radeon")
                    || output.to_lowercase().contains("advanced micro devices")
                {
                    detected_gpus.insert(GpuKind::Amd);
                }
                if output.to_lowercase().contains("intel")
                    || output.to_lowercase().contains("integrated graphics")
                    || output.to_lowercase().contains("hd graphics")
                    || output.to_lowercase().contains("iris")
                {
                    detected_gpus.insert(GpuKind::Intel);
                }

                info.gpus = detected_gpus.into_iter().collect();
            }
        }

        // VM check
        if let Ok(virt) = std::process::Command::new("systemd-detect-virt").output() {
            if virt.status.success() {
                info.vm_type = Some(String::from_utf8_lossy(&virt.stdout).trim().to_string());
            }
        }

        info
    }
}

use std::any::Any;

/// Trait for defining type-safe keys for the data map
pub trait DataKey: Send + Sync + 'static {
    type Value: Send + Sync + Clone + 'static;
    const KEY: &'static str;
}

/// Holds the state of the installation wizard
#[derive(Default, Clone)]
pub struct InstallContext {
    pub answers: HashMap<QuestionId, String>,
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
        let mut state = serializer.serialize_struct("InstallContext", 2)?;
        state.serialize_field("answers", &self.answers)?;
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
            answers: HashMap<QuestionId, String>,
            system_info: SystemInfo,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(InstallContext {
            answers: helper.answers,
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

    pub fn is_answered(&self, id: QuestionId) -> bool {
        self.answers.contains_key(&id)
    }

    pub fn get_answer_bool(&self, id: QuestionId) -> bool {
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

    /// Check if a key exists
    pub fn contains_key(&self, key: &str) -> bool {
        let data = self.data.lock().unwrap();
        data.contains_key(key)
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
}

pub struct QuestionEngine {
    questions: Vec<Box<dyn Question>>,
    pub context: InstallContext,
    is_tty: bool,
}

impl QuestionEngine {
    pub fn new(questions: Vec<Box<dyn Question>>) -> Self {
        Self {
            questions,
            context: InstallContext::new(),
            is_tty: is_tty_environment(),
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

        let continue_opt = format!("{} Continue with installation", NerdFont::ArrowRight);
        review_items.push(continue_opt.clone());

        for q in self.questions.iter().take(current_index) {
            if let Some(ans) = self.context.get_answer(&q.id()) {
                let display_ans = if q.is_sensitive() {
                    "******"
                } else {
                    ans.as_str()
                };
                review_items.push(format!("{} {:?}: {}", NerdFont::Check, q.id(), display_ans));
            }
        }

        if review_items.len() == 1 {
            crate::menu_utils::FzfWrapper::message(&format!(
                "{} No answers to review yet.",
                NerdFont::Info
            ))?;
            return Ok(None);
        }

        let review = crate::menu_utils::FzfWrapper::builder()
            .header("Select a question to modify")
            .select(review_items)?;

        if let crate::menu_utils::FzfResult::Selected(selection) = review {
            if selection == continue_opt {
                return Ok(None);
            }

            // Format: "ICON QuestionId: Answer"
            let parts: Vec<&str> = selection.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                let id_str = parts[1].trim_end_matches(':');
                if let Some(new_index) = self
                    .questions
                    .iter()
                    .position(|q| format!("{:?}", q.id()) == id_str)
                {
                    return Ok(Some(new_index));
                }
            }
        }
        Ok(None)
    }

    fn handle_go_back(&self, mut index: usize) -> usize {
        if index > 0 {
            index -= 1;
            while index > 0 && !self.questions[index].should_ask(&self.context) {
                index -= 1;
            }
        }
        index
    }

    pub async fn run(mut self) -> Result<InstallContext> {
        loop {
            match self.find_next_question_index() {
                Some(idx) => {
                    while !self.questions[idx].is_ready(&self.context) {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }

                    loop {
                        // Clear screen if running in TTY to avoid artifacts
                        if self.is_tty {
                            print!("\x1B[2J\x1B[1;1H");
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                        }

                        let result = self.questions[idx].ask(&self.context).await?;
                        match result {
                            QuestionResult::Answer(answer) => {
                                match self.questions[idx].validate(&self.context, &answer) {
                                    Ok(()) => {
                                        let id = self.questions[idx].id();
                                        self.context.answers.insert(id, answer);
                                        break;
                                    }
                                    Err(msg) => {
                                        crate::menu_utils::FzfWrapper::message(&format!(
                                            "{} {}",
                                            NerdFont::Warning,
                                            msg
                                        ))?;
                                    }
                                }
                            }

                            QuestionResult::Cancelled => {
                                if self.handle_navigation_menu(idx).await? {
                                    break;
                                }
                            }
                        }
                    }
                }
                None => {
                    if self.handle_final_review().await? {
                        break;
                    }
                }
            }
        }

        Ok(self.context.clone())
    }

    fn find_next_question_index(&mut self) -> Option<usize> {
        for (i, q) in self.questions.iter().enumerate() {
            if !q.should_ask(&self.context) {
                continue;
            }

            // Skip optional questions in the main flow
            if q.is_optional() {
                continue;
            }

            if let Some(ans) = self.context.get_answer(&q.id()) {
                if q.validate(&self.context, ans).is_err() {
                    self.context.answers.remove(&q.id());
                    return Some(i);
                }
            } else {
                return Some(i);
            }
        }
        None
    }

    async fn handle_navigation_menu(&mut self, current_idx: usize) -> Result<bool> {
        let options = vec![
            format!("{} Resume", NerdFont::Play),
            format!("{} Review Answers", NerdFont::List),
            format!("{} Go Back", NerdFont::ArrowLeft),
            format!("{} Abort Installation", NerdFont::Cross),
        ];
        let nav = crate::menu_utils::FzfWrapper::builder()
            .header("Installation Paused")
            .select(options)?;

        match nav {
            crate::menu_utils::FzfResult::Selected(opt) => {
                if opt.contains("Resume") {
                    Ok(false)
                } else if opt.contains("Review Answers") {
                    loop {
                        if let Some(review_idx) = self.handle_review(current_idx)? {
                            self.force_ask_question(review_idx).await?;
                        } else {
                            break;
                        }
                    }
                    Ok(false)
                } else if opt.contains("Go Back") {
                    let prev_idx = self.handle_go_back(current_idx);
                    if prev_idx != current_idx {
                        let q_id = self.questions[prev_idx].id();
                        self.context.answers.remove(&q_id);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                } else if opt.contains("Abort Installation") {
                    if let Ok(crate::menu_utils::ConfirmResult::Yes) =
                        crate::menu_utils::FzfWrapper::confirm("Are you sure you want to abort?")
                    {
                        std::process::exit(0);
                    }
                    Ok(false)
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }

    async fn handle_final_review(&mut self) -> Result<bool> {
        let options = vec![
            format!("{} Install", NerdFont::Download),
            format!("{} Review Answers", NerdFont::List),
            format!("{} Advanced Options", NerdFont::Gear),
            format!("{} Abort Installation", NerdFont::Cross),
        ];
        let nav = crate::menu_utils::FzfWrapper::builder()
            .header("Installation Configuration Complete")
            .select(options)?;

        match nav {
            crate::menu_utils::FzfResult::Selected(opt) => {
                if opt.contains("Install") {
                    Ok(true)
                } else if opt.contains("Review Answers") {
                    loop {
                        if let Some(review_idx) = self.handle_review(self.questions.len())? {
                            self.force_ask_question(review_idx).await?;
                        } else {
                            break;
                        }
                    }
                    Ok(false)
                } else if opt.contains("Advanced Options") {
                    if let Some(adv_idx) = self.handle_advanced_options()? {
                        // Force ask the selected optional question
                        self.force_ask_question(adv_idx).await?;
                    }
                    Ok(false)
                } else if opt.contains("Abort Installation") {
                    if let Ok(crate::menu_utils::ConfirmResult::Yes) =
                        crate::menu_utils::FzfWrapper::confirm("Are you sure you want to abort?")
                    {
                        std::process::exit(0);
                    }
                    Ok(false)
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }

    fn handle_advanced_options(&self) -> Result<Option<usize>> {
        let mut options = Vec::new();
        let back_opt = format!("{} Back", NerdFont::ArrowLeft);
        options.push(back_opt.clone());

        for (i, q) in self.questions.iter().enumerate() {
            if q.is_optional() && q.should_ask(&self.context) {
                let status = if self.context.is_answered(q.id()) {
                    let ans = self.context.get_answer(&q.id()).unwrap();
                    format!("{:?} (Current: {})", q.id(), ans)
                } else {
                    format!("{:?}", q.id())
                };
                options.push(format!("{} {}", NerdFont::Gear, status));
            }
        }

        let result = crate::menu_utils::FzfWrapper::builder()
            .header("Advanced Options")
            .select(options)?;

        if let crate::menu_utils::FzfResult::Selected(selection) = result {
            if selection == back_opt {
                return Ok(None);
            }

            // Parse selection to find question index
            // Format: "ICON QuestionId (Current: ...)" or "ICON QuestionId"
            // We can iterate and check which question ID matches the string
            for (i, q) in self.questions.iter().enumerate() {
                if q.is_optional() {
                    let id_str = format!("{:?}", q.id());
                    if selection.contains(&id_str) {
                        return Ok(Some(i));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn force_ask_question(&mut self, idx: usize) -> Result<()> {
        loop {
            let result = self.questions[idx].ask(&self.context).await?;
            match result {
                QuestionResult::Answer(answer) => {
                    match self.questions[idx].validate(&self.context, &answer) {
                        Ok(()) => {
                            let id = self.questions[idx].id();
                            self.context.answers.insert(id, answer);
                            break;
                        }
                        Err(msg) => {
                            crate::menu_utils::FzfWrapper::message(&format!(
                                "{} {}",
                                NerdFont::Warning,
                                msg
                            ))?;
                        }
                    }
                }
                QuestionResult::Cancelled => break,
            }
        }
        Ok(())
    }
}

fn is_tty_environment() -> bool {
    std::env::var("TERM").map(|t| t == "linux").unwrap_or(false)
        || (std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestKey;
    impl DataKey for TestKey {
        type Value = String;
        const KEY: &'static str = "test_key";
    }

    struct IntKey;
    impl DataKey for IntKey {
        type Value = i32;
        const KEY: &'static str = "int_key";
    }

    #[test]
    fn test_install_context_typemap() {
        let context = InstallContext::new();

        context.set::<TestKey>("hello".to_string());
        context.set::<IntKey>(42);

        assert_eq!(context.get::<TestKey>(), Some("hello".to_string()));
        assert_eq!(context.get::<IntKey>(), Some(42));

        // Test missing key
        struct MissingKey;
        impl DataKey for MissingKey {
            type Value = bool;
            const KEY: &'static str = "missing";
        }
        assert_eq!(context.get::<MissingKey>(), None);
    }
}
