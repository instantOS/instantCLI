mod common;
mod utils;

use anyhow::Result;
use common::TestEnvironment;

#[tokio::test]
async fn test_clone_and_apply_basic_repo() -> Result<()> {
    let env = TestEnvironment::new()?;
    
    // Create a test repository with basic dotfiles
    let repo_path = utils::create_test_repo(
        &env,
        "test-basic",
        &[(".config/test-app/config.txt", "test content")],
        &["dots"],
    )?;
    
    // Add the repository to instant config
    let output = utils::run_instant_command(
        &env,
        &["dot", "clone", repo_path.to_str().unwrap()],
    )?;
    
    assert_eq!(output.exit_code, 0, "Clone command failed: {}", output.stderr);
    
    // Apply dotfiles
    let output = utils::run_instant_command(&env, &["dot", "apply"])?;
    assert_eq!(output.exit_code, 0, "Apply command failed: {}", output.stderr);
    
    // Verify the file was created in the fake home directory
    let target_file = env.fake_home().join(".config/test-app/config.txt");
    assert!(utils::file_exists(&target_file), "Target file was not created");
    
    let content = utils::read_file(&target_file)?;
    assert_eq!(content, "test content");
    
    Ok(())
}

#[tokio::test]
async fn test_repository_removal() -> Result<()> {
    let env = TestEnvironment::new()?;
    
    // Create a test repository
    let repo_path = utils::create_test_repo(
        &env,
        "test-remove",
        &[(".config/remove-test/config.txt", "remove me")],
        &["dots"],
    )?;
    
    // Add the repository
    let output = utils::run_instant_command(
        &env,
        &["dot", "clone", repo_path.to_str().unwrap()],
    )?;
    assert_eq!(output.exit_code, 0);
    
    // Remove the repository (without files)
    let output = utils::run_instant_command(&env, &["dot", "remove", "test-remove"])?;
    assert_eq!(output.exit_code, 0);
    
    // Verify the repository is no longer in config
    let output = utils::run_instant_command(&env, &["dot", "status"])?;
    assert!(!output.stdout.contains("test-remove"));
    
    Ok(())
}

#[tokio::test]
async fn test_multiple_repositories_priority() -> Result<()> {
    let env = TestEnvironment::new()?;
    
    // Create two repositories with overlapping files
    let repo1_path = utils::create_test_repo(
        &env,
        "test-repo-1",
        &[(".config/overlap/config.txt", "repo1 content")],
        &["dots"],
    )?;
    
    let repo2_path = utils::create_test_repo(
        &env,
        "test-repo-2",
        &[(".config/overlap/config.txt", "repo2 content")],
        &["dots"],
    )?;
    
    // Add both repositories
    utils::run_instant_command(&env, &["dot", "clone", repo1_path.to_str().unwrap()])?;
    utils::run_instant_command(&env, &["dot", "clone", repo2_path.to_str().unwrap()])?;
    
    // Apply dotfiles
    utils::run_instant_command(&env, &["dot", "apply"])?;
    
    // Verify that repo2 content takes precedence (added later)
    let target_file = env.fake_home().join(".config/overlap/config.txt");
    let content = utils::read_file(&target_file)?;
    assert_eq!(content, "repo2 content");
    
    Ok(())
}

#[tokio::test]
async fn test_user_modification_detection() -> Result<()> {
    let env = TestEnvironment::new()?;
    
    // Create a test repository
    let repo_path = utils::create_test_repo(
        &env,
        "test-modify",
        &[(".config/modify-test/config.txt", "original content")],
        &["dots"],
    )?;
    
    // Add repository and apply
    utils::run_instant_command(&env, &["dot", "clone", repo_path.to_str().unwrap()])?;
    utils::run_instant_command(&env, &["dot", "apply"])?;
    
    // Modify the file
    let target_file = env.fake_home().join(".config/modify-test/config.txt");
    utils::write_file(&target_file, "modified content")?;
    
    // Check status - should detect modification
    let output = utils::run_instant_command(&env, &["dot", "status"])?;
    assert!(output.stdout.contains("modified"), "Status should detect modification");
    
    Ok(())
}

#[tokio::test]
async fn test_fetch_modified_files() -> Result<()> {
    let env = TestEnvironment::new()?;
    
    // Create a test repository
    let repo_path = utils::create_test_repo(
        &env,
        "test-fetch",
        &[(".config/fetch-test/config.txt", "original content")],
        &["dots"],
    )?;
    
    // Add repository and apply
    utils::run_instant_command(&env, &["dot", "clone", repo_path.to_str().unwrap()])?;
    utils::run_instant_command(&env, &["dot", "apply"])?;
    
    // Modify the file
    let target_file = env.fake_home().join(".config/fetch-test/config.txt");
    utils::write_file(&target_file, "modified content")?;
    
    // Fetch the modification
    let output = utils::run_instant_command(&env, &["dot", "fetch"])?;
    assert_eq!(output.exit_code, 0, "Fetch command failed: {}", output.stderr);
    
    // Verify the modification was fetched (this would require checking the repo)
    // For now, just verify the command succeeded
    assert!(output.stdout.contains("Fetching") || output.stdout.contains("complete"));
    
    Ok(())
}

#[tokio::test]
async fn test_multiple_subdirectories() -> Result<()> {
    let env = TestEnvironment::new()?;
    
    // Create a repository with multiple subdirectories
    let repo_path = utils::create_test_repo(
        &env,
        "test-multi",
        &[
            (".config/multi-app1/config.txt", "app1 content"),
            (".config/multi-app2/config.txt", "app2 content"),
        ],
        &["app1", "app2"],
    )?;
    
    // Add repository
    utils::run_instant_command(&env, &["dot", "clone", repo_path.to_str().unwrap()])?;
    
    // List subdirectories
    let output = utils::run_instant_command(&env, &["dot", "list-subdirs", "test-multi"])?;
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout.contains("app1"));
    assert!(output.stdout.contains("app2"));
    
    // Set active subdirectories
    let output = utils::run_instant_command(&env, &["dot", "set-subdirs", "test-multi", "app1", "app2"])?;
    assert_eq!(output.exit_code, 0);
    
    // Apply dotfiles
    utils::run_instant_command(&env, &["dot", "apply"])?;
    
    // Verify both files were created
    let file1 = env.fake_home().join(".config/multi-app1/config.txt");
    let file2 = env.fake_home().join(".config/multi-app2/config.txt");
    
    assert!(utils::file_exists(&file1));
    assert!(utils::file_exists(&file2));
    
    assert_eq!(utils::read_file(&file1)?, "app1 content");
    assert_eq!(utils::read_file(&file2)?, "app2 content");
    
    Ok(())
}

#[tokio::test]
async fn test_invalid_repository_url() -> Result<()> {
    let env = TestEnvironment::new()?;
    
    // Try to clone an invalid repository
    let output = utils::run_instant_command(&env, &["dot", "clone", "invalid-url"])?;
    
    // Should fail with appropriate error
    assert_ne!(output.exit_code, 0);
    assert!(output.stderr.contains("error") || output.stderr.contains("Error"));
    
    Ok(())
}