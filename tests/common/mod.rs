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
    
    /// Get the real home directory
    pub fn real_home(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde("~").to_string())
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
    
    /// Clean up a repository by name
    pub fn cleanup_repo(&self, repo_name: &str) -> Result<()> {
        let repo_path = self.real_home().join(".local").join("share").join("instantos").join("dots").join(repo_name);
        if repo_path.exists() {
            std::fs::remove_dir_all(&repo_path)?;
        }
        Ok(())
    }
    
    /// Clean up all test repositories
    pub fn cleanup_all_repos(&self) -> Result<()> {
        let repos_dir = self.real_home().join(".local").join("share").join("instantos").join("dots");
        if repos_dir.exists() {
            for entry in std::fs::read_dir(&repos_dir)? {
                let entry = entry?;
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                
                // Only remove test repositories (those starting with "test-")
                if file_name_str.starts_with("test-") {
                    let repo_path = repos_dir.join(&file_name);
                    if repo_path.is_dir() {
                        std::fs::remove_dir_all(&repo_path)?;
                    }
                }
            }
        }
        Ok(())
    }
    
    /// Clean up files from home directory
    pub fn cleanup_home_files(&self, paths: &[&str]) -> Result<()> {
        for path in paths {
            let file_path = self.real_home().join(path);
            if file_path.exists() {
                if file_path.is_dir() {
                    std::fs::remove_dir_all(&file_path)?;
                } else {
                    std::fs::remove_file(&file_path)?;
                }
            }
        }
        Ok(())
    }
    
    /// Clean up instant config
    pub fn cleanup_config(&self) -> Result<()> {
        let config_path = self.real_home().join(".config").join("instant").join("instant.toml");
        if config_path.exists() {
            std::fs::remove_file(&config_path)?;
        }
        Ok(())
    }
    
    /// Clean up the database file
    pub fn cleanup_database(&self) -> Result<()> {
        let db_path = self.real_home().join(".local").join("share").join("instantos").join("instant.db");
        if db_path.exists() {
            std::fs::remove_file(&db_path)?;
        }
        Ok(())
    }
    
    /// Clean up all test state (comprehensive cleanup)
    pub fn cleanup_all_test_state(&self) -> Result<()> {
        // Clean up all repositories (not just test ones)
        let repos_dir = self.real_home().join(".local").join("share").join("instantos").join("dots");
        if repos_dir.exists() {
            std::fs::remove_dir_all(&repos_dir)?;
        }
        
        // Clean up config directory
        let config_dir = self.real_home().join(".config").join("instant");
        if config_dir.exists() {
            std::fs::remove_dir_all(&config_dir)?;
        }
        
        // Clean up database
        self.cleanup_database()?;
        
        // Clean up ALL possible test directories from home
        let test_dirs = [
            "test-app", "modify-test", "fetch-test", "overlap", 
            "multi-app1", "multi-app2", "remove-test",
            "test-basic", "test-remove", "test-priority1", "test-priority2",
            "test-modify", "test-fetch", "test-sub"
        ];
        self.cleanup_home_files(&test_dirs)?;
        
        // Also clean up any .instantos directories
        let instantos_dir = self.real_home().join(".instantos");
        if instantos_dir.exists() {
            std::fs::remove_dir_all(&instantos_dir)?;
        }
        
        // Longer delay to ensure filesystem operations complete
        std::thread::sleep(std::time::Duration::from_millis(50));
        
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