//! Shared discovered save record for Wine prefix providers.
use super::DiscoveredGame;
use crate::common::TildePath;
use crate::menu::protocol::FzfPreview;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum PrefixSource {
    Wine,
    Faugus,
}
impl PrefixSource {
    fn id(self) -> &'static str {
        match self {
            Self::Wine => "wine",
            Self::Faugus => "faugus",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Wine => "Wine Prefix",
            Self::Faugus => "Faugus",
        }
    }
    fn short(self) -> &'static str {
        match self {
            Self::Wine => "Wine",
            Self::Faugus => "Faugus",
        }
    }
}
#[derive(Debug, Clone)]
pub struct PrefixDiscoveredGame {
    pub source: PrefixSource,
    pub display_name: String,
    pub prefix_path: PathBuf,
    pub save_path: PathBuf,
    pub tracked_name: Option<String>,
}

impl PrefixDiscoveredGame {
    pub fn new(
        source: PrefixSource,
        display_name: String,
        prefix_path: PathBuf,
        save_path: PathBuf,
    ) -> Self {
        Self {
            source,
            display_name,
            prefix_path,
            save_path,
            tracked_name: None,
        }
    }
}

impl DiscoveredGame for PrefixDiscoveredGame {
    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn save_path(&self) -> &PathBuf {
        &self.save_path
    }

    fn game_path(&self) -> Option<&PathBuf> {
        None
    }

    fn prefix_path(&self) -> Option<&PathBuf> {
        Some(&self.prefix_path)
    }

    fn platform_name(&self) -> &'static str {
        self.source.name()
    }

    fn platform_short(&self) -> &'static str {
        self.source.short()
    }

    fn unique_key(&self) -> String {
        format!(
            "{}:{}|{}",
            self.source.id(),
            self.prefix_path.to_string_lossy(),
            self.save_path.to_string_lossy()
        )
    }

    fn is_existing(&self) -> bool {
        self.tracked_name.is_some()
    }

    fn tracked_name(&self) -> Option<&str> {
        self.tracked_name.as_deref()
    }

    fn set_existing(&mut self, tracked_name: String) {
        self.tracked_name = Some(tracked_name);
    }

    fn build_preview(&self) -> FzfPreview {
        let prefix_display = TildePath::new(self.prefix_path.clone()).display_string();
        let save_display = TildePath::new(self.save_path.clone()).display_string();
        let header_name = self.tracked_name.as_deref().unwrap_or(&self.display_name);

        let mut builder = PreviewBuilder::new()
            .header(
                if self.is_existing() {
                    NerdFont::Check
                } else {
                    NerdFont::Wine
                },
                header_name,
            )
            .field("Platform", self.platform_name())
            .blank()
            .separator()
            .blank()
            .text("Prefix:")
            .bullet(&prefix_display)
            .blank()
            .text("Save path:")
            .bullet(&save_display)
            .blank()
            .separator()
            .blank();

        if self.is_existing() {
            builder = builder.subtext("Already tracked — press Enter to open game menu");
        } else {
            builder = builder.subtext(&format!("Auto-discovered from {}", self.platform_name()));
        }

        builder.build()
    }

    fn clone_box(&self) -> Box<dyn DiscoveredGame> {
        Box::new(self.clone())
    }
}
