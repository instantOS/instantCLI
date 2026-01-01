//! Nerd Font check - ensures Nerd Font symbols can be rendered correctly
//!
//! Nerd Fonts extend regular fonts with thousands of glyphs in the Private Use Area (PUA).
//! This check samples symbols from ALL major Nerd Font icon sets to ensure comprehensive
//! coverage. Without a Nerd Font, these PUA symbols render as boxes or wrong characters
//! (often Chinese or Arabic glyphs from font fallback chains).
//!
//! Nerd Font PUA ranges (v3.x):
//! - Pomicons: e000 - e00a
//! - Powerline: e0a0 - e0a2, e0b0 - e0b3
//! - Powerline Extra: e0b4 - e0c8
//! - Font Awesome Extension: e200 - e2a9
//! - Weather Icons: e300 - e3e3
//! - Seti-UI + Custom: e5fa - e6b5
//! - Devicons: e700 - e7c5
//! - Codicons: ea60 - ebeb
//! - Font Awesome: f000 - f2e0
//! - Font Logos: f300 - f375
//! - Octicons: f400 - f532
//! - Material Design (v3.0+): f0001 - f1af0 (Supplementary PUA-A)

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Nerd Font icon set ranges for comprehensive coverage testing (v3.x codepoints).
/// Each entry: (name, start_codepoint, end_codepoint, sample_count)
/// We sample a few characters from each range to test coverage without being excessive.
///
/// Note: Nerd Fonts v3.0+ relocated Material Design icons to Supplementary PUA-A (f0001+).
/// The old range (f500-fd46) was deprecated and removed.
const NERD_FONT_RANGES: &[(&str, u32, u32, usize)] = &[
    // Basic Multilingual Plane PUA (e000 - f8ff)
    ("Pomicons", 0xe000, 0xe00a, 3),
    ("Powerline", 0xe0a0, 0xe0a2, 3),
    ("Powerline Arrows", 0xe0b0, 0xe0b3, 4),
    ("Powerline Extra", 0xe0b4, 0xe0c8, 4),
    ("FA Extension", 0xe200, 0xe2a9, 5),
    ("Weather", 0xe300, 0xe3e3, 5),
    ("Seti-UI", 0xe5fa, 0xe6b5, 5),
    ("Devicons", 0xe700, 0xe7c5, 5),
    ("Codicons", 0xea60, 0xebeb, 5),
    ("Font Awesome", 0xf000, 0xf2e0, 8),
    ("Font Logos", 0xf300, 0xf375, 4),
    ("Octicons", 0xf400, 0xf532, 5),
    // Supplementary PUA-A (f0000 - fffff) - Material Design moved here in v3.0+
    ("Material Design", 0xf0001, 0xf1af0, 10),
    // Known gap range where fallback commonly occurs (e.g. lazygit/starship issues)
    ("Gap Range (e900)", 0xe900, 0xe905, 5),
];

/// Fonts that are known to NOT be Nerd Fonts - these provide fallback glyphs
/// that appear as boxes, Arabic, Chinese, or other wrong characters.
const NON_NERD_FONTS: &[&str] = &[
    // System UI fonts
    "dejavu",
    "liberation",
    "freesans",
    "freemono",
    "freeserif",
    // Noto fonts (regular, not nerd)
    "noto sans",
    "noto serif",
    "noto mono",
    "noto color emoji",
    "noto sans cjk",
    "noto sans arabic",
    "noto sans hebrew",
    "noto sans symbols",
    // CJK fallback fonts
    "ipagothic",
    "ipaexgothic",
    "ipamincho",
    "wqy",
    "wenquanyi",
    "arphic",
    "uming",
    "ukai",
    "droid sans fallback",
    "source han",
    "nanum",
    // Arabic fallback fonts
    "droid arabic",
    "scheherazade",
    "amiri",
    "lateef",
    "harmattan",
    // Symbol fallback fonts
    "symbola",
    "unifont",
    "lastresort",
    "babelstone",
    // Generic system fonts
    "sans",
    "serif",
    "monospace",
    "courier",
    "courier new",
    "arial",
    "helvetica",
    "times",
    "times new roman",
    "georgia",
    "verdana",
    "tahoma",
    // Linux system fonts
    "ubuntu",
    "cantarell",
    "droid sans",
    "roboto",
    "lato",
    "open sans",
    // Other common non-nerd fonts
    "latin modern",
    "computer modern",
    "urw",
    "nimbus",
    "bitstream",
    "tex gyre",
];

/// Font to install and its configuration
const NERD_FONT_NAME: &str = "CaskaydiaCove Nerd Font";
const NERD_FONT_ZIP_URL: &str =
    "https://github.com/ryanoasis/nerd-fonts/releases/download/v3.3.0/CascadiaCode.zip";
const FONTCONFIG_PRIORITY_FILENAME: &str = "99-nerd-font-priority.conf";

#[derive(Default)]
pub struct NerdFontCheck;

#[async_trait]
impl DoctorCheck for NerdFontCheck {
    fn name(&self) -> &'static str {
        "Nerd Font Symbols"
    }

    fn id(&self) -> &'static str {
        "nerd-font"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        if !Self::fc_match_available() {
            return CheckStatus::Fail {
                message: String::from(
                    "fontconfig tools not available. Install fontconfig package.",
                ),
                fixable: false,
            };
        }

        let current_font =
            Self::get_current_monospace_font().unwrap_or_else(|| "system default".to_string());

        let coverage = self.check_comprehensive_coverage();

        if coverage.total_checked == 0 {
            return CheckStatus::Fail {
                message: String::from("Could not test any Nerd Font symbols"),
                fixable: true,
            };
        }

        Self::evaluate_coverage(&coverage, &current_font)
    }

    fn fix_message(&self) -> Option<String> {
        Some(format!(
            "Install {} to ~/.local/share/fonts/ and configure fontconfig priority",
            NERD_FONT_NAME
        ))
    }

    async fn fix(&self) -> Result<()> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let fonts_dir = home.join(".local/share/fonts");
        let fontconfig_dir = home.join(".config/fontconfig/conf.d");

        // Check current state for idempotency
        let state = self.check_fix_state(&fonts_dir, &fontconfig_dir).await;

        if state.is_fully_configured() {
            println!(
                "✓ {} is already installed and configured correctly.",
                NERD_FONT_NAME
            );
            println!("  Font location: {:?}", fonts_dir);
            println!(
                "  Config file: {:?}",
                fontconfig_dir.join(FONTCONFIG_PRIORITY_FILENAME)
            );
            return Ok(());
        }

        // Install font if needed
        if !state.font_installed {
            self.install_font(&fonts_dir).await?;
        } else {
            println!("✓ {} already installed, skipping download.", NERD_FONT_NAME);
        }

        // Configure fontconfig if needed
        if !state.fontconfig_configured {
            self.configure_fontconfig(&fontconfig_dir).await?;
        } else {
            println!("✓ Fontconfig priority already configured, skipping.");
        }

        // Always refresh font cache to ensure changes are picked up
        self.refresh_font_cache().await?;

        println!();
        println!("✓ {} setup complete!", NERD_FONT_NAME);
        println!("  Please restart your terminal for changes to take effect.");

        Ok(())
    }
}

// =============================================================================
// Fix State and Idempotency
// =============================================================================

/// Represents the current state of the nerd font fix
struct FixState {
    font_installed: bool,
    fontconfig_configured: bool,
}

impl FixState {
    fn is_fully_configured(&self) -> bool {
        self.font_installed && self.fontconfig_configured
    }
}

impl NerdFontCheck {
    /// Check the current state of the fix for idempotency
    async fn check_fix_state(&self, fonts_dir: &Path, fontconfig_dir: &Path) -> FixState {
        FixState {
            font_installed: Self::is_font_installed(fonts_dir).await,
            fontconfig_configured: Self::is_fontconfig_configured(fontconfig_dir).await,
        }
    }

    /// Check if the nerd font is already installed
    async fn is_font_installed(fonts_dir: &Path) -> bool {
        if !fonts_dir.exists() {
            return false;
        }

        // Check for CaskaydiaCove or CascadiaCode font files
        let patterns = ["CaskaydiaCove", "CascadiaCode"];

        if let Ok(mut entries) = tokio::fs::read_dir(fonts_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                for pattern in &patterns {
                    if name_str.contains(pattern)
                        && (name_str.ends_with(".ttf") || name_str.ends_with(".otf"))
                    {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check if fontconfig priority is already configured with strict mode
    async fn is_fontconfig_configured(fontconfig_dir: &Path) -> bool {
        let config_file = fontconfig_dir.join(FONTCONFIG_PRIORITY_FILENAME);

        if !config_file.exists() {
            return false;
        }

        // Check if the config file matches our new strict configuration
        if let Ok(content) = tokio::fs::read_to_string(&config_file).await {
            // Must contain key elements of our configuration
            return content.contains(NERD_FONT_NAME)
                && content.contains("STRICT enforcement")
                && content.contains(r#"mode="assign""#);
        }

        false
    }
}

// =============================================================================
// Font Installation
// =============================================================================

impl NerdFontCheck {
    /// Download and install the nerd font
    async fn install_font(&self, fonts_dir: &Path) -> Result<()> {
        // Ensure fonts directory exists
        Self::ensure_directory(fonts_dir).await?;

        // Download the font zip
        let zip_path = self.download_font().await?;

        // Extract the font
        Self::extract_font(&zip_path, fonts_dir).await?;

        // Clean up zip file
        let _ = tokio::fs::remove_file(&zip_path).await;

        println!("✓ Installed {} to {:?}", NERD_FONT_NAME, fonts_dir);
        Ok(())
    }

    /// Download the font zip file
    async fn download_font(&self) -> Result<PathBuf> {
        let zip_path = std::env::temp_dir().join("CascadiaCode.zip");

        println!("  Downloading {}...", NERD_FONT_NAME);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .context("Failed to create HTTP client")?;

        let response = client
            .get(NERD_FONT_ZIP_URL)
            .send()
            .await
            .context("Failed to download font (network error)")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to download font: HTTP {}", response.status());
        }

        let content = response
            .bytes()
            .await
            .context("Failed to read font download")?;

        tokio::fs::write(&zip_path, content)
            .await
            .context("Failed to save font zip file")?;

        Ok(zip_path)
    }

    /// Extract the font zip to the fonts directory
    async fn extract_font(zip_path: &Path, fonts_dir: &Path) -> Result<()> {
        println!("  Extracting fonts...");

        let output = Command::new("unzip")
            .arg("-o") // overwrite existing
            .arg("-q") // quiet
            .arg(zip_path)
            .arg("-d")
            .arg(fonts_dir)
            .output()
            .await
            .context("Failed to run unzip. Is the 'unzip' package installed?")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to extract font: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}

// =============================================================================
// Fontconfig Configuration
// =============================================================================

impl NerdFontCheck {
    /// Configure fontconfig to prioritize the nerd font
    async fn configure_fontconfig(&self, fontconfig_dir: &Path) -> Result<()> {
        Self::ensure_directory(fontconfig_dir).await?;

        let config_file = fontconfig_dir.join(FONTCONFIG_PRIORITY_FILENAME);
        let config_content = Self::generate_fontconfig_xml();

        println!("  Configuring fontconfig priority...");

        tokio::fs::write(&config_file, config_content)
            .await
            .context("Failed to write fontconfig file")?;

        println!("✓ Created fontconfig priority file: {:?}", config_file);
        Ok(())
    }

    /// Generate the fontconfig XML content
    ///
    /// This configuration specifically targets the Private Use Area (PUA) unicode ranges
    /// where Nerd Font icons live, without affecting regular text rendering (including Arabic).
    /// Used STRICT assignment to prevent fallback to Arabic/CJK fonts for these ranges.
    fn generate_fontconfig_xml() -> String {
        format!(
            r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <!--
    Nerd Font priority configuration for instantCLI.
    This ensures {} is used for Nerd Font icon codepoints
    in the Private Use Area (PUA) without affecting regular text.
  -->

  <!-- 1. General preference for monospace -->
  <alias>
    <family>monospace</family>
    <prefer>
      <family>{}</family>
    </prefer>
  </alias>

  <!--
    2. STRICT enforcement for PUA ranges.
    If a character is in the PUA ranges (where icons live), we FORCE the font family
    to be our Nerd Font (assign, not prepend). This effectively disables fallback to
    other fonts for these specific characters preventing random Arabic/CJK glyphs
    from appearing. A missing icon (box) is better than a wrong one.

    Ranges targeted:
    - BMP PUA: U+E000-U+F8FF (Powerline, Devicons, Codicons, Font Awesome, etc.)
    - Supplementary PUA-A: U+F0000-U+FFFFD (Material Design icons in v3.0+)
  -->
  <match target="pattern">
    <test name="charset" compare="contains">
      <charset>
        <range>
          <int>0xE000</int>
          <int>0xF8FF</int>
        </range>
        <range>
          <int>0xF0000</int>
          <int>0xFFFFD</int>
        </range>
      </charset>
    </test>
    <edit name="family" mode="assign" binding="strong">
      <string>{}</string>
    </edit>
  </match>
</fontconfig>
"#,
            NERD_FONT_NAME, NERD_FONT_NAME, NERD_FONT_NAME
        )
    }

    /// Refresh the font cache
    async fn refresh_font_cache(&self) -> Result<()> {
        println!("  Refreshing font cache...");

        let output = Command::new("fc-cache")
            .arg("-f")
            .output()
            .await
            .context("Failed to run fc-cache")?;

        if !output.status.success() {
            // Non-fatal warning
            eprintln!(
                "  Warning: fc-cache returned an error: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

impl NerdFontCheck {
    /// Ensure a directory exists, creating it if necessary
    async fn ensure_directory(path: &Path) -> Result<()> {
        if !path.exists() {
            tokio::fs::create_dir_all(path)
                .await
                .with_context(|| format!("Failed to create directory: {:?}", path))?;
        }
        Ok(())
    }

    /// Check if fc-match is available
    fn fc_match_available() -> bool {
        use std::process::Command;
        Command::new("fc-match")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get the current default monospace font
    fn get_current_monospace_font() -> Option<String> {
        use std::process::Command;

        let output = Command::new("fc-match").arg("monospace").output().ok()?;

        if output.status.success() {
            let font = String::from_utf8_lossy(&output.stdout);
            let font_name = font.split(':').next()?.trim().to_string();
            if !font_name.is_empty() {
                return Some(font_name);
            }
        }

        None
    }
}

// =============================================================================
// Coverage Checking
// =============================================================================

impl NerdFontCheck {
    /// Evaluate coverage results and return appropriate status
    fn evaluate_coverage(
        coverage: &ComprehensiveCoverageResult,
        current_font: &str,
    ) -> CheckStatus {
        let pass_rate = coverage.nerd_font_count as f64 / coverage.total_checked as f64;

        let problem_sets: Vec<&str> = coverage
            .range_results
            .iter()
            .filter(|(_, nerd, non_nerd)| *non_nerd > *nerd)
            .map(|(name, _, _)| *name)
            .collect();

        // Check for "dangerous" fallbacks (fonts that cause Arabic/CJK garbage in PUA)
        let dangerous_fallbacks = coverage.bad_fonts.iter().any(|f| {
            let f_lower = f.to_lowercase();
            f_lower.contains("aming")
                || f_lower.contains("uming")
                || f_lower.contains("gentium")
                || f_lower.contains("amiri")
                || f_lower.contains("kacst")
        });

        // 1. Excellent coverage, no dangerous fallbacks
        if pass_rate >= 0.95 && !dangerous_fallbacks {
            return CheckStatus::Pass(format!(
                "Nerd Font symbols rendering correctly ({}/{} symbols, font: '{}')",
                coverage.nerd_font_count, coverage.total_checked, current_font
            ));
        }

        // 2. Good coverage OR Dangerous Fallbacks present
        // If we have dangerous fallbacks, we MUST warn even if coverage is high
        let bad_fonts_str = Self::format_bad_fonts(&coverage.bad_fonts);
        let problem_sets_str = Self::format_problem_sets(&problem_sets);

        let message = format!(
            "Partial Nerd Font coverage: {}/{} symbols OK. {} symbols falling back to: {}. \
             Problem icon sets: {}",
            coverage.nerd_font_count,
            coverage.total_checked,
            coverage.non_nerd_count,
            bad_fonts_str,
            problem_sets_str
        );

        if dangerous_fallbacks {
            // High visibility warning for Arabic/CJK fallbacks
            CheckStatus::Warning {
                message: format!(
                    "{} (Detected unsafe fallback fonts likely causing incorrect glyphs)",
                    message
                ),
                fixable: true,
            }
        } else if pass_rate >= 0.7 {
            // Standard warning for moderate coverage gaps (likely boxes)
            CheckStatus::Warning {
                message,
                fixable: true,
            }
        } else {
            // Fail based on low rate
            CheckStatus::Fail {
                message: format!(
                    "Nerd Font symbols not rendering correctly. Only {}/{} symbols use a Nerd Font. \
                     Symbols are falling back to: {}. Current monospace font: '{}'",
                    coverage.nerd_font_count, coverage.total_checked, bad_fonts_str, current_font
                ),
                fixable: true,
            }
        }
    }

    fn format_bad_fonts(fonts: &[String]) -> String {
        if fonts.is_empty() {
            "unknown fonts".to_string()
        } else {
            fonts.join(", ")
        }
    }

    fn format_problem_sets(sets: &[&str]) -> String {
        if sets.is_empty() {
            "various".to_string()
        } else {
            sets.join(", ")
        }
    }

    /// Generate sample codepoints from a range, evenly distributed
    fn sample_codepoints(start: u32, end: u32, count: usize) -> Vec<u32> {
        let range_size = end.saturating_sub(start) + 1;
        if count == 0 || range_size == 0 {
            return vec![];
        }

        let actual_count = count.min(range_size as usize);

        if actual_count == range_size as usize {
            (start..=end).collect()
        } else {
            let step = range_size as f64 / actual_count as f64;
            (0..actual_count)
                .map(|i| start + (i as f64 * step) as u32)
                .collect()
        }
    }

    /// Check comprehensive coverage across all Nerd Font ranges
    fn check_comprehensive_coverage(&self) -> ComprehensiveCoverageResult {
        let mut total_checked = 0;
        let mut nerd_font_count = 0;
        let mut non_nerd_count = 0;
        let mut bad_fonts = std::collections::HashSet::new();
        let mut range_results = Vec::new();

        for (name, start, end, sample_count) in NERD_FONT_RANGES {
            let codepoints = Self::sample_codepoints(*start, *end, *sample_count);
            let mut range_nerd = 0;
            let mut range_non_nerd = 0;

            for cp in codepoints {
                total_checked += 1;

                if let Some(font) = Self::find_font_for_codepoint(cp) {
                    if Self::is_non_nerd_font(&font) {
                        non_nerd_count += 1;
                        range_non_nerd += 1;
                        bad_fonts.insert(font);
                    } else {
                        nerd_font_count += 1;
                        range_nerd += 1;
                    }
                } else {
                    non_nerd_count += 1;
                    range_non_nerd += 1;
                }
            }

            range_results.push((*name, range_nerd, range_non_nerd));
        }

        ComprehensiveCoverageResult {
            total_checked,
            nerd_font_count,
            non_nerd_count,
            bad_fonts: bad_fonts.into_iter().collect(),
            range_results,
        }
    }

    fn find_font_for_codepoint(codepoint: u32) -> Option<String> {
        use std::process::Command;

        let output = Command::new("fc-match")
            .arg("-f")
            .arg("%{family}")
            .arg(format!("monospace:charset={:x}", codepoint))
            .output()
            .ok()?;

        if output.status.success() {
            let font = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !font.is_empty() {
                return Some(font);
            }
        }

        None
    }

    /// Check if a font is a known non-nerd font (fallback font)
    fn is_non_nerd_font(font: &str) -> bool {
        let font_lower = font.to_lowercase();
        NON_NERD_FONTS
            .iter()
            .any(|pattern| font_lower.contains(pattern))
    }
}

// =============================================================================
// Types
// =============================================================================

struct ComprehensiveCoverageResult {
    total_checked: usize,
    nerd_font_count: usize,
    non_nerd_count: usize,
    bad_fonts: Vec<String>,
    range_results: Vec<(&'static str, usize, usize)>,
}
