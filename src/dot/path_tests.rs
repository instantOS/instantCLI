#[cfg(test)]
mod tests {
    use crate::common::home_dir;
    use crate::dot::resolve_dotfile_path;
    use crate::dot::test_util::EnvGuard;
    use serial_test::serial;

    use std::fs;

    #[test]
    #[serial]
    fn test_resolve_dotfile_path_tilde_expansion() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", temp.path());
        let result = resolve_dotfile_path("~", false, true);
        assert!(result.is_ok());
        let home = std::env::var("HOME").unwrap();
        assert_eq!(result.unwrap().to_str().unwrap(), home);
    }

    #[test]
    #[serial]
    fn test_resolve_dotfile_path_absolute_path() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", temp.path());
        let home = home_dir();
        let test_file = home.join("test_resolve_absolute.txt");

        fs::write(&test_file, "test content").unwrap();

        let result = resolve_dotfile_path(test_file.to_str().unwrap(), false, true);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_file.canonicalize().unwrap());
    }

    #[test]
    #[serial]
    fn test_resolve_dotfile_path_outside_home() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", temp.path());
        let result = resolve_dotfile_path("/etc/passwd", false, true);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("outside the home directory"));
    }

    #[test]
    #[serial]
    fn test_resolve_dotfile_path_nonexistent() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", temp.path());
        let home = home_dir();
        let nonexistent = home.join("nonexistent_file.txt");

        let result = resolve_dotfile_path(nonexistent.to_str().unwrap(), false, true);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    #[serial]
    fn test_resolve_dotfile_path_nonexistent_when_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", temp.path());
        let home = home_dir();
        let nonexistent = home.join("nonexistent_file_allowed.txt");

        let result = resolve_dotfile_path(nonexistent.to_str().unwrap(), false, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), nonexistent);
    }
}
