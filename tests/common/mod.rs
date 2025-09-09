use anyhow::Result;
use shellexpand;
use std::path::PathBuf;
use tempfile::TempDir;

pub struct TestEnvironment {
    temp_dir: TempDir,
    test_id: String,
}

impl TestEnvironment {
    pub fn new() -> Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let test_id = format!("instant-test-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs());
        
        Ok(Self { temp_dir, test_id })
    }
    
    /// Get a unique test path within the real home directory
    pub fn test_path(&self, relative_path: &str) -> PathBuf {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        home.join(".config").join("instantdottest").join(&self.test_id).join(relative_path)
    }
    
    /// Get the real home directory
    pub fn real_home(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde("~").to_string())
    }
    
    /// Get the temp directory for storing test repositories
    pub fn temp_dir(&self) -> &std::path::Path {
        self.temp_dir.path()
    }
    
    /// Get the temp directory path for storing test repositories
    pub fn path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }
    
    /// Clean up test files from the real home directory
    pub fn cleanup(&self) -> Result<()> {
        let test_dir = self.real_home().join(".config").join("instantdottest").join(&self.test_id);
        if test_dir.exists() {
            std::fs::remove_dir_all(&test_dir)?;
        }
        Ok(())
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        // Clean up test files
        if let Err(e) = self.cleanup() {
            eprintln!("Warning: failed to clean up test files: {}", e);
        }
        // temp_dir will be cleaned up when dropped
    }
}