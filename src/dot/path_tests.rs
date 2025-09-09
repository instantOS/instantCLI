#[cfg(test)]
mod tests {
    use crate::dot::resolve_dotfile_path;
    use std::fs;
    use std::path::PathBuf;
    use shellexpand;

    #[test]
    fn test_resolve_dotfile_path_tilde_expansion() {
        // Test tilde expansion - this should work since ~ always exists
        let result = resolve_dotfile_path("~");
        assert!(result.is_ok());
        let home = std::env::var("HOME").unwrap();
        assert_eq!(result.unwrap().to_str().unwrap(), home);
    }

    #[test]
    fn test_resolve_dotfile_path_absolute_path() {
        // Test absolute path within home directory
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let test_file = home.join("test_resolve_absolute.txt");
        
        // Create a test file
        fs::write(&test_file, "test content").unwrap();
        
        let result = resolve_dotfile_path(test_file.to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_file.canonicalize().unwrap());
        
        // Clean up
        fs::remove_file(test_file).unwrap();
    }

    #[test]
    fn test_resolve_dotfile_path_outside_home() {
        // Test that paths outside home directory are rejected
        let result = resolve_dotfile_path("/etc/passwd");
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("outside the home directory"));
    }

    #[test]
    fn test_resolve_dotfile_path_nonexistent() {
        // Test that nonexistent files are rejected
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let nonexistent = home.join("nonexistent_file.txt");
        
        let result = resolve_dotfile_path(nonexistent.to_str().unwrap());
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Failed to resolve path"));
    }
}