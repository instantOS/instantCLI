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
//! - Powerline Extra: e0a3, e0b4 - e0c8, e0ca, e0cc - e0d7
//! - Font Awesome Extension: e200 - e2a9
//! - Weather Icons: e300 - e3e3
//! - Seti-UI + Custom: e5fa - e6b7
//! - Devicons: e700 - e8ef
//! - Codicons: ea60 - ec1e
//! - Font Awesome: ed00 - f2ff
//! - Font Logos: f300 - f381
//! - Octicons: f400 - f533
//! - Material Design: f500 - fd46

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;

/// Nerd Font icon set ranges for comprehensive coverage testing.
/// Each entry: (name, start_codepoint, end_codepoint, sample_count)
/// We sample a few characters from each range to test coverage without being excessive.
const NERD_FONT_RANGES: &[(&str, u32, u32, usize)] = &[
    // Pomicons (e000 - e00a) - 11 glyphs total
    ("Pomicons", 0xe000, 0xe00a, 3),
    // Powerline (e0a0 - e0a2, e0b0 - e0b3) - core symbols
    ("Powerline", 0xe0a0, 0xe0a2, 3),
    ("Powerline Arrows", 0xe0b0, 0xe0b3, 4),
    // Powerline Extra (e0b4 - e0c8)
    ("Powerline Extra", 0xe0b4, 0xe0c8, 4),
    // Font Awesome Extension (e200 - e2a9)
    ("FA Extension", 0xe200, 0xe2a9, 5),
    // Weather Icons (e300 - e3e3)
    ("Weather", 0xe300, 0xe3e3, 5),
    // Seti-UI + Custom (e5fa - e6b7)
    ("Seti-UI", 0xe5fa, 0xe6b7, 5),
    // Devicons (e700 - e8ef)
    ("Devicons", 0xe700, 0xe8ef, 5),
    // Codicons (ea60 - ec1e)
    ("Codicons", 0xea60, 0xec1e, 5),
    // Font Awesome (ed00 - f2ff) - large range
    ("Font Awesome", 0xed00, 0xf2ff, 8),
    // Font Logos (f300 - f381)
    ("Font Logos", 0xf300, 0xf381, 4),
    // Octicons (f400 - f533)
    ("Octicons", 0xf400, 0xf533, 5),
    // Material Design Icons (f500 - fd46) - very large range
    ("Material Design", 0xf500, 0xfd46, 10),
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
        // Check if fontconfig tools are available
        if !self.fc_match_available() {
            return CheckStatus::Fail {
                message: String::from(
                    "fontconfig tools not available. Install fontconfig package.",
                ),
                fixable: false,
            };
        }

        let current_font = self
            .get_current_monospace_font()
            .unwrap_or_else(|| "system default".to_string());

        let coverage = self.check_comprehensive_coverage();

        if coverage.total_checked == 0 {
            return CheckStatus::Fail {
                message: String::from("Could not test any Nerd Font symbols"),
                fixable: true,
            };
        }

        let pass_rate = coverage.nerd_font_count as f64 / coverage.total_checked as f64;
        let fail_rate = coverage.non_nerd_count as f64 / coverage.total_checked as f64;

        // Build a summary of which icon sets have issues
        let problem_sets: Vec<&str> = coverage
            .range_results
            .iter()
            .filter(|(_, nerd, non_nerd)| *non_nerd > *nerd)
            .map(|(name, _, _)| *name)
            .collect();

        if pass_rate >= 0.95 {
            // 95%+ coverage is excellent
            CheckStatus::Pass(format!(
                "Nerd Font symbols rendering correctly ({}/{} symbols, font: '{}')",
                coverage.nerd_font_count, coverage.total_checked, current_font
            ))
        } else if pass_rate >= 0.7 {
            // 70-95% - mostly working but some issues
            let bad_fonts_str = if coverage.bad_fonts.is_empty() {
                "unknown fonts".to_string()
            } else {
                coverage.bad_fonts.join(", ")
            };

            CheckStatus::Warning {
                message: format!(
                    "Partial Nerd Font coverage: {}/{} symbols OK. {} symbols falling back to: {}. \
                     Problem icon sets: {}",
                    coverage.nerd_font_count,
                    coverage.total_checked,
                    coverage.non_nerd_count,
                    bad_fonts_str,
                    if problem_sets.is_empty() {
                        "various".to_string()
                    } else {
                        problem_sets.join(", ")
                    }
                ),
                fixable: true,
            }
        } else if fail_rate > 0.5 {
            // More than 50% failing - likely no nerd font or wrong priority
            let bad_fonts_str = if coverage.bad_fonts.is_empty() {
                "system fallback fonts".to_string()
            } else {
                coverage.bad_fonts.join(", ")
            };

            CheckStatus::Fail {
                message: format!(
                    "Nerd Font symbols not rendering correctly. Only {}/{} symbols use a Nerd Font. \
                     Symbols are falling back to: {} (may appear as boxes, Chinese, or Arabic characters). \
                     Current monospace font: '{}'",
                    coverage.nerd_font_count, coverage.total_checked, bad_fonts_str, current_font
                ),
                fixable: true,
            }
        } else {
            // Low pass rate but not catastrophic
            CheckStatus::Warning {
                message: format!(
                    "Nerd Font symbols partially working ({}/{} OK). Some symbols may display incorrectly. \
                     Consider installing or prioritizing a Nerd Font.",
                    coverage.nerd_font_count, coverage.total_checked
                ),
                fixable: true,
            }
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Automatically install CaskaydiaCove Nerd Font to ~/.local/share/fonts/".to_string())
    }

    async fn fix(&self) -> Result<()> {
        println!("Attempting to install CaskaydiaCove Nerd Font...");

        let home = dirs::home_dir().context("Could not determine home directory")?;

        let fonts_dir = home.join(".local/share/fonts");

        if !fonts_dir.exists() {
            println!("Creating fonts directory: {:?}", fonts_dir);

            tokio::fs::create_dir_all(&fonts_dir)
                .await
                .context("Failed to create fonts directory")?;
        }

        let zip_url =
            "https://github.com/ryanoasis/nerd-fonts/releases/download/v3.3.0/CascadiaCode.zip";

        let zip_name = "CascadiaCode.zip";

        let zip_path = std::env::temp_dir().join(zip_name);

        println!("Downloading {}...", zip_url);

        let client = reqwest::Client::new();

        let response = client
            .get(zip_url)
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            anyhow::bail!("Download failed: {}", response.status());
        }

        let content = response
            .bytes()
            .await
            .context("Failed to get response bytes")?;

        tokio::fs::write(&zip_path, content)
            .await
            .context("Failed to write zip file")?;

        println!("Extracting to {:?}...", fonts_dir);

        // Use system unzip command

        let status = Command::new("unzip")
            .arg("-o") // overwrite
            .arg("-q") // quiet
            .arg(&zip_path)
            .arg("-d")
            .arg(&fonts_dir)
            .output()
            .await
            .context("Failed to execute unzip command. Is 'unzip' installed?")?;

        if !status.status.success() {
            anyhow::bail!("Unzip failed: {}", String::from_utf8_lossy(&status.stderr));
        }

        // Clean up zip

        let _ = tokio::fs::remove_file(&zip_path).await;

        // Configure fontconfig priority

        let config_dir = home.join(".config/fontconfig/conf.d");

        if !config_dir.exists() {
            println!("Creating fontconfig directory: {:?}", config_dir);

            tokio::fs::create_dir_all(&config_dir)
                .await
                .context("Failed to create fontconfig directory")?;
        }

        let config_file = config_dir.join("10-nerd-font-priority.conf");

        let config_content = r#"<?xml version="1.0"?>

        <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">

        <fontconfig>

          <alias>

            <family>monospace</family>

            <prefer>

              <family>CaskaydiaCove Nerd Font</family>

            </prefer>

          </alias>

        </fontconfig>

        "#;

        println!("Writing fontconfig priority file to {:?}...", config_file);

        tokio::fs::write(&config_file, config_content)
            .await
            .context("Failed to write fontconfig file")?;

        println!("Updating font cache...");

        let status = Command::new("fc-cache")
            .arg("-f")
            .output()
            .await
            .context("Failed to execute fc-cache")?;

        if !status.status.success() {
            println!(
                "Warning: fc-cache returned error: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        }

        println!("Successfully installed CaskaydiaCove Nerd Font and updated priority!");

        println!("Please restart your terminal for changes to take effect.");

        Ok(())
    }
}

impl NerdFontCheck {
    /// Check if fc-match is available
    fn fc_match_available(&self) -> bool {
        use std::process::Command;
        Command::new("fc-match")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn get_current_monospace_font(&self) -> Option<String> {
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

    /// Generate sample codepoints from a range, evenly distributed
    fn sample_codepoints(start: u32, end: u32, count: usize) -> Vec<u32> {
        let range_size = end.saturating_sub(start) + 1;
        if count == 0 || range_size == 0 {
            return vec![];
        }

        let actual_count = count.min(range_size as usize);

        if actual_count == range_size as usize {
            // Take all
            (start..=end).collect()
        } else {
            // Evenly distribute samples across the range
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

                if let Some(font) = self.find_font_for_codepoint(cp) {
                    if self.is_non_nerd_font(&font) {
                        non_nerd_count += 1;
                        range_non_nerd += 1;
                        bad_fonts.insert(font);
                    } else {
                        // Font is not in the known non-nerd list, likely a nerd font
                        nerd_font_count += 1;
                        range_nerd += 1;
                    }
                } else {
                    // No font found - counts as non-nerd (will render as box)
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

    fn find_font_for_codepoint(&self, codepoint: u32) -> Option<String> {
        use std::process::Command;

        // Use fc-match to find the font effectively used for this codepoint
        // syntax: fc-match -f "%{family}" "monospace:charset=XXXX"
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
    fn is_non_nerd_font(&self, font: &str) -> bool {
        let font_lower = font.to_lowercase();

        // Check against known non-nerd fonts
        for pattern in NON_NERD_FONTS {
            if font_lower.contains(pattern) {
                return true;
            }
        }

        false
    }
}

struct ComprehensiveCoverageResult {
    total_checked: usize,
    nerd_font_count: usize,
    non_nerd_count: usize,
    bad_fonts: Vec<String>,
    /// Per-range results: (range_name, nerd_count, non_nerd_count)
    range_results: Vec<(&'static str, usize, usize)>,
}
