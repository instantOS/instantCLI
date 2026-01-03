use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;

pub struct LowRamWarning;

#[async_trait::async_trait]
impl Question for LowRamWarning {
    fn id(&self) -> QuestionId {
        QuestionId::LowRamWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        // Show warning if RAM is detected and less than 1GB
        context.system_info.total_ram_gb.is_some_and(|ram| ram < 1)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let ram_gb = context.system_info.total_ram_gb.unwrap_or(0);
        FzfWrapper::message(&format!(
            "{} Low Memory Warning\n\n\
             System has {} GB of RAM (less than 1 GB).\n\
             Installation may be slow or fail.\n\n\
             Installation will proceed, but consider upgrading your RAM for a better experience.",
            NerdFont::Warning,
            ram_gb
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}

pub struct VirtualBoxWarning;

#[async_trait::async_trait]
impl Question for VirtualBoxWarning {
    fn id(&self) -> QuestionId {
        QuestionId::VirtualBoxWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if let Some(vm_type) = &context.system_info.vm_type {
            let vm = vm_type.to_lowercase();
            vm.contains("oracle") || vm.contains("virtualbox")
        } else {
            false
        }
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        FzfWrapper::message(&format!(
            "{} VirtualBox Detected!\n\n\
             Wayland does not work properly in VirtualBox.\n\
             Please use X11 or another hypervisor for the best experience.",
            NerdFont::Warning
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}

pub struct WeakPasswordWarning;

#[async_trait::async_trait]
impl Question for WeakPasswordWarning {
    fn id(&self) -> QuestionId {
        QuestionId::WeakPasswordWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if !context.get_answer_bool(QuestionId::UseEncryption) {
            return false;
        }
        if let Some(pass) = context.get_answer(&QuestionId::EncryptionPassword) {
            pass.len() < 4
        } else {
            false
        }
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        FzfWrapper::message(&format!(
            "{} Weak Password Warning\n\n\
             The encryption password is shorter than 4 characters.\n\
             This is considered insecure.",
            NerdFont::Warning
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}
