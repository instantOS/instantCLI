use anyhow::Result;
use crate::arch::engine::{Question, QuestionId, InstallContext};
use crate::menu_utils::FzfWrapper;

pub struct HostnameQuestion;

#[async_trait::async_trait]
impl Question for HostnameQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Hostname
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        FzfWrapper::input("Please enter the hostname for the new system")
    }
}

pub struct UsernameQuestion;

#[async_trait::async_trait]
impl Question for UsernameQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Username
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        FzfWrapper::input("Please enter the username for the new user")
    }
}

pub struct MirrorRegionQuestion;

#[async_trait::async_trait]
impl Question for MirrorRegionQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::MirrorRegion
    }

    fn is_ready(&self, context: &InstallContext) -> bool {
        let data = context.data.lock().unwrap();
        data.contains_key("mirror_regions")
    }

    async fn ask(&self, context: &InstallContext) -> Result<String> {
        let data = context.data.lock().unwrap();
        let regions_str = data.get("mirror_regions").unwrap();
        let regions: Vec<String> = regions_str.split(',').map(|s| s.to_string()).collect();
        
        let result = FzfWrapper::builder()
            .header("Select Mirror Region")
            .select(regions)?;
            
        match result {
            crate::menu_utils::FzfResult::Selected(region) => Ok(region),
            _ => Ok("".to_string()), // Handle cancellation better?
        }
    }
}

pub struct TimezoneQuestion;

#[async_trait::async_trait]
impl Question for TimezoneQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Timezone
    }

    fn is_ready(&self, context: &InstallContext) -> bool {
        let data = context.data.lock().unwrap();
        data.contains_key("timezones")
    }

    async fn ask(&self, context: &InstallContext) -> Result<String> {
        let data = context.data.lock().unwrap();
        let timezones_str = data.get("timezones").unwrap();
        let timezones: Vec<String> = timezones_str.lines().map(|s| s.to_string()).collect();
        
        let result = FzfWrapper::builder()
            .header("Select Timezone")
            .select(timezones)?;
            
        match result {
            crate::menu_utils::FzfResult::Selected(tz) => Ok(tz),
            _ => Ok("".to_string()),
        }
    }
}

pub struct DiskQuestion;

#[async_trait::async_trait]
impl Question for DiskQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Disk
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        // Mock disks for now
        let disks = vec![
            "/dev/sda (500GB)".to_string(),
            "/dev/nvme0n1 (1TB)".to_string(),
        ];
        
        let result = FzfWrapper::builder()
            .header("Select Disk to Install To")
            .select(disks)?;
            
        match result {
            crate::menu_utils::FzfResult::Selected(disk) => Ok(disk),
            _ => Ok("".to_string()),
        }
    }
}

pub struct KeymapQuestion;

#[async_trait::async_trait]
impl Question for KeymapQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Keymap
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        // Mock keymaps
        let keymaps = vec![
            "us".to_string(),
            "de-latin1".to_string(),
            "uk".to_string(),
        ];
        
        let result = FzfWrapper::builder()
            .header("Select Keymap")
            .select(keymaps)?;
            
        match result {
            crate::menu_utils::FzfResult::Selected(km) => Ok(km),
            _ => Ok("".to_string()),
        }
    }
}

pub struct LocaleQuestion;

#[async_trait::async_trait]
impl Question for LocaleQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Locale
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        FzfWrapper::input("Please enter the system locale (e.g., en_US.UTF-8)")
    }
}

pub struct PasswordQuestion;

#[async_trait::async_trait]
impl Question for PasswordQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Password
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        FzfWrapper::password("Please enter the password for the new user (and root)")
    }
}
