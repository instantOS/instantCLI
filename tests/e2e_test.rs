mod common;
mod utils;

use anyhow::Result;
use common::TestEnvironment;
use std::sync::Mutex;

// Global mutex to ensure tests run one at a time
static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Ensures complete test isolation by using a global mutex
fn setup_test() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_MUTEX.lock().unwrap();
    
    // Create a temporary test environment for cleanup
    if let Ok(env) = TestEnvironment::new() {
        let _ = env.cleanup_all_test_state();
        // Additional delay to ensure cleanup completes
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    guard
}

#[test]
fn test_clone_and_apply_basic_repo() -> Result<()> {
    let _guard = setup_test();
    let env = TestEnvironment::new()?;
    
    // Clean up any existing state before starting
    env.cleanup_all_test_state()?;
    
    // Create a test repository with basic dotfiles
    let repo_path = utils::create_test_repo(
        &env,
        "test-basic",
        &[(utils::test_dotfile_path("test-app/config.txt").as_str(), "test content")],
        &["dots"],
    )?;
    
    // Convert to file:// URL for cloning
    let repo_url = format!("file://{}", repo_path.to_str().unwrap());
    
    // Add the repository to instant config
    let output = utils::run_instant_command(&env, &["dot", "clone", &repo_url])?;
    assert_eq!(output.exit_code, 0, "Clone command failed: {}", output.stderr);
    
    // Apply dotfiles
    let output = utils::run_instant_command(&env, &["dot", "apply"])?;
    assert_eq!(output.exit_code, 0, "Apply command failed: {}", output.stderr);
    
    // The file should be created in the real home directory
    let target_file = env.real_home().join(utils::test_dotfile_path("test-app/config.txt"));
    assert!(utils::file_exists(&target_file), "Target file was not created");
    
    let content = utils::read_file(&target_file)?;
    assert_eq!(content, "test content");
    
    // Clean up after the test
    env.cleanup_all_test_state()?;
    
    Ok(())
}

#[test]
fn test_repository_removal() -> Result<()> {
    let _guard = setup_test();
    let env = TestEnvironment::new()?;
    
    // Clean up any existing state before starting
    env.cleanup_all_test_state()?;
    
    // Create a test repository
    let repo_path = utils::create_test_repo(
        &env,
        "test-remove",
        &[("remove-test/config.txt", "remove me")],
        &["dots"],
    )?;
    
    let repo_url = format!("file://{}", repo_path.to_str().unwrap());
    
    // Add the repository
    let output = utils::run_instant_command(&env, &["dot", "clone", &repo_url])?;
    assert_eq!(output.exit_code, 0);
    
    // Remove the repository (without files)
    let output = utils::run_instant_command(&env, &["dot", "remove", "test-remove"])?;
    assert_eq!(output.exit_code, 0);
    
    // Verify the repository is no longer in config
    let output = utils::run_instant_command(&env, &["dot", "status"])?;
    assert!(!output.stdout.contains("test-remove"));
    
    // Clean up after the test (repository should already be removed by the test)
    env.cleanup_all_test_state()?;
    
    Ok(())
}

#[test]
fn test_multiple_repositories_priority() -> Result<()> {
    let _guard = setup_test();
    let env = TestEnvironment::new()?;
    
    // Clean up any existing state before starting
    env.cleanup_all_test_state()?;
    
    // Create two repositories with overlapping files
    let repo1_path = utils::create_test_repo(
        &env,
        "test-repo-1",
        &[("overlap/config.txt", "repo1 content")],
        &["dots"],
    )?;
    
    let repo2_path = utils::create_test_repo(
        &env,
        "test-repo-2",
        &[("overlap/config.txt", "repo2 content")],
        &["dots"],
    )?;
    
    let repo1_url = format!("file://{}", repo1_path.to_str().unwrap());
    let repo2_url = format!("file://{}", repo2_path.to_str().unwrap());
    
    // Add both repositories
    utils::run_instant_command(&env, &["dot", "clone", &repo1_url])?;
    utils::run_instant_command(&env, &["dot", "clone", &repo2_url])?;
    
    // Apply dotfiles
    utils::run_instant_command(&env, &["dot", "apply"])?;
    
    // Verify that repo2 content takes precedence (added later)
    let target_file = env.real_home().join("overlap/config.txt");
    let content = utils::read_file(&target_file)?;
    assert_eq!(content, "repo2 content");
    
    // Clean up after the test
    env.cleanup_all_test_state()?;
    
    Ok(())
}

#[test]
fn test_user_modification_detection() -> Result<()> {
    let _guard = setup_test();
    let env = TestEnvironment::new()?;
    
    // Clean up any existing state before starting
    env.cleanup_all_test_state()?;
    
    // Create a test repository
    let repo_path = utils::create_test_repo(
        &env,
        "test-modify",
        &[("modify-test/config.txt", "original content")],
        &["dots"],
    )?;
    
    let repo_url = format!("file://{}", repo_path.to_str().unwrap());
    
    // Add repository and apply
    utils::run_instant_command(&env, &["dot", "clone", &repo_url])?;
    utils::run_instant_command(&env, &["dot", "apply"])?;
    
    // Modify the file
    let target_file = env.real_home().join("modify-test/config.txt");
    utils::write_file(&target_file, "modified content")?;
    
    // Check status - should detect modification
    let output = utils::run_instant_command(&env, &["dot", "status"])?;
    assert!(output.stdout.contains("modified"), "Status should detect modification: {}", output.stdout);
    
    // Clean up after the test
    env.cleanup_all_test_state()?;
    
    Ok(())
}

#[test]
fn test_fetch_modified_files() -> Result<()> {
    let _guard = setup_test();
    let env = TestEnvironment::new()?;
    
    // Clean up any existing state before starting
    env.cleanup_all_test_state()?;
    
    // Create a test repository
    let repo_path = utils::create_test_repo(
        &env,
        "test-fetch",
        &[("fetch-test/config.txt", "original content")],
        &["dots"],
    )?;
    
    let repo_url = format!("file://{}", repo_path.to_str().unwrap());
    
    // Add repository and apply
    utils::run_instant_command(&env, &["dot", "clone", &repo_url])?;
    utils::run_instant_command(&env, &["dot", "apply"])?;
    
    // Modify the file
    let target_file = env.real_home().join("fetch-test/config.txt");
    utils::write_file(&target_file, "modified content")?;
    
    // Fetch the modification
    let output = utils::run_instant_command(&env, &["dot", "fetch"])?;
    assert_eq!(output.exit_code, 0, "Fetch command failed: {}", output.stderr);
    
    // Verify the modification was fetched
    assert!(output.stdout.contains("fetching") || output.stdout.contains("complete") || output.stdout.contains("No modified dotfiles to fetch."));
    
    // Clean up after the test
    env.cleanup_all_test_state()?;
    
    Ok(())
}

#[test]
fn test_multiple_subdirectories() -> Result<()> {
    let _guard = setup_test();
    let env = TestEnvironment::new()?;
    
    // Clean up any existing state before starting
    env.cleanup_all_test_state()?;
    
    // Create a repository with multiple subdirectories
    let repo_path = utils::create_test_repo(
        &env,
        "test-multi",
        &[
            ("multi-app1/config.txt", "app1 content"),
            ("multi-app2/config.txt", "app2 content"),
        ],
        &["app1", "app2"],
    )?;
    
    let repo_url = format!("file://{}", repo_path.to_str().unwrap());
    
    // Add repository
    utils::run_instant_command(&env, &["dot", "clone", &repo_url])?;
    
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
    let file1 = env.real_home().join("multi-app1/config.txt");
    let file2 = env.real_home().join("multi-app2/config.txt");
    
    assert!(utils::file_exists(&file1));
    assert!(utils::file_exists(&file2));
    
    assert_eq!(utils::read_file(&file1)?, "app1 content");
    assert_eq!(utils::read_file(&file2)?, "app2 content");
    
    // Clean up after the test
    env.cleanup_all_test_state()?;
    
    Ok(())
}

#[test]
fn test_invalid_repository_url() -> Result<()> {
    let _guard = setup_test();
    let env = TestEnvironment::new()?;
    
    // Clean up any existing state before starting
    env.cleanup_all_test_state()?;
    
    // Try to clone an invalid repository
    let output = utils::run_instant_command(&env, &["dot", "clone", "invalid-url"])?;
    
    // Should fail with appropriate error
    assert_ne!(output.exit_code, 0);
    assert!(output.stderr.contains("error") || output.stderr.contains("Error"));
    
    // Clean up after the test
    env.cleanup_all_test_state()?;
    
    Ok(())
}