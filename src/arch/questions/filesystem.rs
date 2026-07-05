use crate::arch::config::{BtrfsCompression, RootFilesystem};
use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::Result;

#[derive(Clone)]
struct RootFilesystemOption(RootFilesystem);

impl RootFilesystemOption {
    fn preview(&self) -> FzfPreview {
        match self.0 {
            RootFilesystem::Btrfs => PreviewBuilder::new()
                .header(NerdFont::HardDrive, "btrfs (recommended)")
                .subtext("Modern copy-on-write filesystem with snapshots and compression.")
                .blank()
                .line(colors::TEAL, None, "Layout")
                .bullets([
                    "Subvolumes @ (root) and @home",
                    "Transparent zstd compression by default",
                ])
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets([
                    "Snapshots and rollback (Snapper/Timeshift)",
                    "Saving disk space with compression",
                ])
                .build(),
            RootFilesystem::Ext4 => PreviewBuilder::new()
                .header(NerdFont::HardDrive, "ext4")
                .subtext("The traditional, rock-solid Linux filesystem.")
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets([
                    "Maximum stability and simplicity",
                    "Users who do not need snapshots",
                ])
                .blank()
                .line(colors::YELLOW, None, "Notes")
                .bullet("No built-in snapshots or compression")
                .build(),
        }
    }
}

impl FzfSelectable for RootFilesystemOption {
    fn fzf_display_text(&self) -> String {
        self.0.label().to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }

    fn fzf_key(&self) -> String {
        self.0.answer_value().to_string()
    }
}

pub struct RootFilesystemQuestion;

#[async_trait::async_trait]
impl Question for RootFilesystemQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::RootFilesystem
    }

    fn description(&self) -> Option<&str> {
        Some("Choose the root filesystem (btrfs or ext4)")
    }

    fn is_optional(&self) -> bool {
        true
    }

    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        Some(RootFilesystem::DEFAULT.answer_value().to_string())
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec![
            RootFilesystemOption(RootFilesystem::Btrfs),
            RootFilesystemOption(RootFilesystem::Ext4),
        ];

        let result = FzfWrapper::builder()
            .header(format!("{} Select Root Filesystem", NerdFont::HardDrive))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(option) => {
                Ok(QuestionResult::Answer(option.0.answer_value().to_string()))
            }
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        match answer {
            "btrfs" | "ext4" => Ok(()),
            _ => Err("You must select a root filesystem.".to_string()),
        }
    }
}

#[derive(Clone)]
struct BtrfsCompressionOption(BtrfsCompression);

impl BtrfsCompressionOption {
    fn preview(&self) -> FzfPreview {
        let builder = PreviewBuilder::new()
            .header(NerdFont::Sliders, "btrfs Compression")
            .subtext("Transparent compression applied to the root filesystem.");
        match self.0 {
            BtrfsCompression::None => builder
                .blank()
                .line(colors::TEAL, None, "None")
                .bullet("No compression; fastest writes, most disk usage"),
            BtrfsCompression::Zstd => builder
                .blank()
                .line(colors::TEAL, None, "zstd (recommended)")
                .bullets([
                    "Great ratio with low CPU cost",
                    "The sane default for most systems",
                ]),
            BtrfsCompression::Lzo => builder
                .blank()
                .line(colors::TEAL, None, "lzo")
                .bullet("Fastest compression, lower ratio"),
            BtrfsCompression::Zlib => builder
                .blank()
                .line(colors::TEAL, None, "zlib")
                .bullet("Highest ratio, slower than zstd"),
        }
        .build()
    }
}

impl FzfSelectable for BtrfsCompressionOption {
    fn fzf_display_text(&self) -> String {
        self.0.label().to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }

    fn fzf_key(&self) -> String {
        self.0.answer_value().to_string()
    }
}

pub struct BtrfsCompressionQuestion;

#[async_trait::async_trait]
impl Question for BtrfsCompressionQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::BtrfsCompression
    }

    fn description(&self) -> Option<&str> {
        Some("Choose btrfs compression algorithm")
    }

    fn is_optional(&self) -> bool {
        true
    }

    /// Only relevant when the root filesystem is btrfs.
    fn should_ask(&self, context: &InstallContext) -> bool {
        RootFilesystem::from_context(context).is_btrfs()
    }

    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        Some(BtrfsCompression::DEFAULT.answer_value().to_string())
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec![
            BtrfsCompressionOption(BtrfsCompression::Zstd),
            BtrfsCompressionOption(BtrfsCompression::Lzo),
            BtrfsCompressionOption(BtrfsCompression::Zlib),
            BtrfsCompressionOption(BtrfsCompression::None),
        ];

        let result = FzfWrapper::builder()
            .header(format!("{} Select btrfs Compression", NerdFont::Sliders))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(option) => {
                Ok(QuestionResult::Answer(option.0.answer_value().to_string()))
            }
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        match answer {
            "none" | "zstd" | "lzo" | "zlib" => Ok(()),
            _ => Err("You must select a compression option.".to_string()),
        }
    }
}
