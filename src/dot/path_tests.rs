#[cfg(test)]
mod tests {
    use crate::common::home_dir;
    use crate::dot::resolve_dotfile_path;

    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_resolve_dotfile_path_tilde_expansion() {
        let result = resolve_dotfile_path("~", false);
        assert!(result.is_ok());
        let home = std::env::var("HOME").unwrap();
        assert_eq!(result.unwrap().to_str().unwrap(), home);
    }

    #[test]
    fn test_resolve_dotfile_path_absolute_path() {
        let home = home_dir();
        let test_file = home.join("test_resolve_absolute.txt");

        fs::write(&test_file, "test content").unwrap();

        let result = resolve_dotfile_path(test_file.to_str().unwrap(), false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_file.canonicalize().unwrap());

        fs::remove_file(test_file).unwrap();
    }

    #[test]
    fn test_resolve_dotfile_path_outside_home() {
        let result = resolve_dotfile_path("/etc/passwd", false);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("outside the home directory"));
    }

    #[test]
    fn test_resolve_dotfile_path_nonexistent() {
        let home = home_dir();
        let nonexistent = home.join("nonexistent_file.txt");

        let result = resolve_dotfile_path(nonexistent.to_str().unwrap(), false);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("does not exist"));
    }
}
