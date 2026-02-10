#[cfg(test)]
mod tests {
    use crate::dot::config::{Config, Repo};
    use crate::dot::dotfilerepo::DotfileRepo;
    use crate::dot::types::RepoMetaData;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_local_repo_uses_config_metadata() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        fs::create_dir_all(&repo_path).unwrap();

        // Create a dummy file to simulate a repo
        fs::write(repo_path.join("README.md"), "# Test Repo").unwrap();

        // Initialize git repo
        let _ = std::process::Command::new("git")
            .args(["init", repo_path.to_str().unwrap()])
            .output()
            .expect("Failed to init git repo");

        let mut config = Config::default();
        config.repos_dir = crate::common::TildePath::new(dir.path().to_path_buf());

        let metadata = RepoMetaData {
            name: "test-repo".to_string(),
            author: None,
            description: None,
            read_only: None,
            dots_dirs: vec![".".to_string()],
            default_active_subdirs: None,
            units: vec![],
        };

        let repo_config = Repo {
            url: "https://example.com/repo.git".to_string(),
            name: "repo".to_string(), // Folder name
            branch: None,
            active_subdirectories: None,
            enabled: true,
            read_only: false,
            metadata: Some(metadata.clone()),
        };

        config.repos.push(repo_config);

        let dotfile_repo = DotfileRepo::new(&config, "repo".to_string()).unwrap();

        assert_eq!(dotfile_repo.meta, metadata);
        assert_eq!(dotfile_repo.dotfile_dirs.len(), 1);
        assert_eq!(dotfile_repo.dotfile_dirs[0].path, repo_path.join("."));
    }

    #[test]
    fn test_local_repo_fallback_to_file() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        fs::create_dir_all(&repo_path).unwrap();

        // Create instantdots.toml
        let toml_content = r#"
            name = "file-repo"
            dots_dirs = ["dots"]
        "#;
        fs::write(repo_path.join("instantdots.toml"), toml_content).unwrap();
        fs::create_dir_all(repo_path.join("dots")).unwrap();

        let mut config = Config::default();
        config.repos_dir = crate::common::TildePath::new(dir.path().to_path_buf());

        let repo_config = Repo {
            url: "https://example.com/repo.git".to_string(),
            name: "repo".to_string(),
            branch: None,
            active_subdirectories: None,
            enabled: true,
            read_only: false,
            metadata: None,
        };

        config.repos.push(repo_config);

        let dotfile_repo = DotfileRepo::new(&config, "repo".to_string()).unwrap();

        assert_eq!(dotfile_repo.meta.name, "file-repo");
        assert_eq!(dotfile_repo.dotfile_dirs.len(), 1);
        assert_eq!(dotfile_repo.dotfile_dirs[0].path, repo_path.join("dots"));
    }

    #[test]
    fn test_repo_with_empty_metadata_is_disabled() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        fs::create_dir_all(&repo_path).unwrap();

        let toml_content = r#"
            name = "empty-repo"
            dots_dirs = []
            default_active_subdirs = []
        "#;
        fs::write(repo_path.join("instantdots.toml"), toml_content).unwrap();
        fs::create_dir_all(repo_path.join("dots")).unwrap();

        let mut config = Config::default();
        config.repos_dir = crate::common::TildePath::new(dir.path().to_path_buf());

        let repo_config = Repo {
            url: "https://example.com/repo.git".to_string(),
            name: "repo".to_string(),
            branch: None,
            active_subdirectories: None,
            enabled: true,
            read_only: false,
            metadata: None,
        };

        config.repos.push(repo_config);

        let dotfile_repo = DotfileRepo::new(&config, "repo".to_string()).unwrap();

        assert_eq!(dotfile_repo.meta.name, "empty-repo");
        assert!(dotfile_repo.dotfile_dirs.is_empty());
    }
}
