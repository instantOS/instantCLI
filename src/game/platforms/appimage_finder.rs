//! Shared AppImage locations; emulator identity depends on the filename, not
//! the directory it happens to be installed in.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::game::launch_command::EmulatorPlatform;

pub(super) const SEARCH_DIRECTORIES: &[&str] = &[
    "Applications",
    "AppImages",
    ".local/bin",
    ".local/share/applications",
    "Emulation/tools",
    "emulation/tools",
];

pub(super) fn find_appimages(platform: EmulatorPlatform) -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|home| find_in_home(platform, &home))
        .unwrap_or_default()
}

fn find_in_home(platform: EmulatorPlatform, home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for directory in SEARCH_DIRECTORIES {
        let root = home.join(directory);
        if !root.is_dir() {
            continue;
        }
        // Include launcher and version subdirectories without walking the rest
        // of the home directory or following directory symlinks.
        for entry in WalkDir::new(&root).max_depth(3).sort_by_file_name() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    if crate::ui::is_debug_enabled() {
                        eprintln!(
                            "Could not scan AppImage location {}: {error}",
                            root.display()
                        );
                    }
                    continue;
                }
            };
            let path = entry.path();
            if platform.matches_appimage(path) && path.is_file() {
                let identity = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                if seen.insert(identity) {
                    found.push(path.to_path_buf());
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emulator_is_found_in_every_shared_location() {
        let home = tempfile::tempdir().unwrap();
        for directory in SEARCH_DIRECTORIES {
            let root = home.path().join(directory).join("launchers");
            std::fs::create_dir_all(&root).unwrap();
            for platform in EmulatorPlatform::ALL {
                std::fs::write(
                    root.join(format!("{}-v1-x86_64.ApPiMaGe", platform.id())),
                    b"app",
                )
                .unwrap();
            }
        }
        for platform in EmulatorPlatform::ALL {
            let found = find_in_home(platform, home.path());
            assert_eq!(found.len(), SEARCH_DIRECTORIES.len(), "{}", platform.name());
            assert!(found.iter().all(|path| platform.matches_appimage(path)));
        }
    }

    #[test]
    fn ignores_other_files_and_deduplicates_symlink_aliases() {
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join("Applications");
        let images = home.path().join("AppImages");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::create_dir_all(&images).unwrap();
        let eden = apps.join("Eden.AppImage");
        std::fs::write(&eden, b"app").unwrap();
        std::os::unix::fs::symlink(&eden, images.join("eden-current.AppImage")).unwrap();
        std::fs::write(apps.join("eden.txt"), b"text").unwrap();
        std::fs::create_dir(apps.join("eden-directory.AppImage")).unwrap();
        assert_eq!(
            find_in_home(EmulatorPlatform::Eden, home.path()),
            vec![eden]
        );
    }
}
