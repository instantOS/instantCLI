use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub path: PathBuf,
    pub description: Option<String>,
}

impl Package {
    /// Create a Package from a directory containing a PKGBUILD file
    pub fn from_directory(dir: &Path) -> Option<Self> {
        let pkgbuild_path = dir.join("PKGBUILD");
        if !pkgbuild_path.exists() {
            return None;
        }

        // Extract package name from directory name
        let name = dir.file_name()?.to_str()?.to_string();

        // Try to extract description from PKGBUILD
        let description = Self::extract_description(&pkgbuild_path);

        Some(Package {
            name,
            path: dir.to_path_buf(),
            description,
        })
    }

    /// Extract package description from PKGBUILD file
    fn extract_description(pkgbuild_path: &Path) -> Option<String> {
        if let Ok(content) = fs::read_to_string(pkgbuild_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("pkgdesc=") {
                    let desc = line["pkgdesc=".len()..].trim_matches(&['\'', '"'][..]);
                    return Some(desc.to_string());
                }
            }
        }
        None
    }

    /// Discover all packages in a repository directory
    pub fn discover_packages(repo_path: &Path) -> Result<Vec<Package>> {
        let mut packages = Vec::new();

        if !repo_path.exists() {
            return Err(anyhow::anyhow!(
                "Repository path does not exist: {:?}",
                repo_path
            ));
        }

        for entry in WalkDir::new(repo_path)
            .max_depth(2)
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
        {
            let entry = entry?;

            if entry.file_type().is_dir()
                && let Some(package) = Package::from_directory(entry.path())
            {
                // Filter out invalid entries (dots, single chars)
                if package.name.len() > 1 && !package.name.starts_with('.') {
                    packages.push(package);
                }
            }
        }

        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(packages)
    }
}

/// Helper function to check if a directory entry is hidden
fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}
