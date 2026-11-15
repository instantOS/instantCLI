//! Shared desktop application metadata, MIME associations and defaults.

pub(crate) mod app_info;
pub(crate) mod desktop;
pub(crate) mod discovery;
pub(crate) mod mime_cache;
pub(crate) mod system;
pub(crate) mod types;

/// Application data roots in precedence order, including exported Flatpak apps.
pub(crate) fn data_dirs() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::data_dir() {
        roots.push(home.clone());
        roots.push(home.join("flatpak/exports/share"));
    }
    roots.push("/var/lib/flatpak/exports/share".into());
    let system = std::env::var_os("XDG_DATA_DIRS").filter(|value| !value.is_empty());
    match system {
        Some(value) => {
            roots.extend(std::env::split_paths(&value).filter(|path| !path.as_os_str().is_empty()))
        }
        None => roots.extend(["/usr/local/share".into(), "/usr/share".into()]),
    }
    let mut seen = std::collections::HashSet::new();
    roots.retain(|path| seen.insert(path.clone()));
    roots
}
