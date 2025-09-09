use anyhow::Result;
use std::env;
use std::path::PathBuf;
use tempfile::TempDir;

pub struct TestEnvironment {
    temp_dir: TempDir,
    original_home: Option<String>,
}

impl TestEnvironment {
    pub fn new() -> Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let fake_home = temp_dir.path().join("home");
        
        // Store original HOME environment variable
        let original_home = env::var("HOME").ok();
        
        // Set fake home directory for testing
        env::set_var("INSTANT_TEST_HOME_DIR", &fake_home);
        
        Ok(Self { temp_dir, original_home })
    }
    
    pub fn fake_home(&self) -> PathBuf {
        PathBuf::from(env::var("INSTANT_TEST_HOME_DIR").unwrap())
    }
    
    pub fn path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        // Restore original HOME environment variable
        if let Some(home) = &self.original_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
        env::remove_var("INSTANT_TEST_HOME_DIR");
        // temp_dir will be cleaned up when dropped
    }
}