use std::env;
use std::path::PathBuf;

use freedesktop_file_parser::parse;

use crate::menu_utils::{FzfPreview, FzfSelectable};

#[derive(Debug, Clone)]
pub(crate) struct ApplicationInfo {
    pub desktop_id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub exec: Option<String>,
}

impl FzfSelectable for ApplicationInfo {
    fn fzf_display_text(&self) -> String {
        if let Some(name) = &self.name {
            format!("󰘔 {} ({})", name, self.desktop_id)
        } else {
            format!("󰘔 {}", self.desktop_id)
        }
    }

    fn fzf_key(&self) -> String {
        self.desktop_id.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut preview = String::new();

        if let Some(name) = &self.name {
            preview.push_str(&format!("Application: {}\n", name));
        } else {
            preview.push_str(&format!("Desktop ID: {}\n", self.desktop_id));
        }

        if let Some(comment) = &self.comment {
            preview.push_str(&format!("\nDescription:\n{}\n", comment));
        }

        if let Some(exec) = &self.exec {
            preview.push_str(&format!("\nCommand:\n{}\n", exec));
        }

        if let Some(icon) = &self.icon {
            preview.push_str(&format!("\nIcon: {}\n", icon));
        }

        preview.push_str(&format!("\nDesktop File:\n{}\n", self.desktop_id));

        FzfPreview::Text(preview)
    }
}

pub(crate) fn get_application_info(desktop_id: &str) -> ApplicationInfo {
    let home_dir = env::var("HOME").unwrap_or_default();
    let directories = [
        format!("{home_dir}/.local/share/applications"),
        format!("{home_dir}/.local/share/flatpak/exports/share/applications"),
        "/var/lib/flatpak/exports/share/applications".to_string(),
        "/usr/share/applications".to_string(),
    ];

    for dir in &directories {
        let path = PathBuf::from(dir).join(desktop_id);
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(desktop_file) = parse(&content)
        {
            use freedesktop_file_parser::EntryType;

            let exec = match &desktop_file.entry.entry_type {
                EntryType::Application(app) => app.exec.clone(),
                _ => None,
            };

            return ApplicationInfo {
                desktop_id: desktop_id.to_string(),
                name: Some(desktop_file.entry.name.default.clone()),
                comment: desktop_file
                    .entry
                    .comment
                    .as_ref()
                    .map(|c| c.default.clone()),
                icon: desktop_file.entry.icon.as_ref().map(|i| i.content.clone()),
                exec,
            };
        }
    }

    ApplicationInfo {
        desktop_id: desktop_id.to_string(),
        name: None,
        comment: None,
        icon: None,
        exec: None,
    }
}
