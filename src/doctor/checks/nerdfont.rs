//! Nerd Font check - ensures Nerd Font symbols can be rendered correctly
//!
//! Nerd Fonts extend regular fonts with thousands of glyphs in the Private Use Area.
//! Common Nerd Font PUA ranges:
//! - U+F000 - U+F0FF: Font Awesome
//! - U+E000 - U+E0FF: Devicons
//! - U+E200 - U+E2FF: Octicons
//! - U+E300 - U+E3FF: Font Awesome Extension
//! - U+E500 - U+E5FF: Seti-UI
//! - U+E600 - U+E6FF: Powerline Symbols
//! - U+E700 - U+E7FF: Font Awesome 4
//! - U+EB00 - U+EBFF: Powerline Extra
//! - U+EC00 - U+ECFF: Code Icons
//!
//! Without a Nerd Font, these PUA symbols render as boxes or wrong characters
//! (often Chinese or Arabic glyphs from font fallback chains).

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;

const COMMON_SYMBOLS: &[char] = &[
    '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '',
    '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '',
];

const NERD_FONT_PATTERNS: &[&[&str]] = &[
    &[
        "caskaydia",
        "caskaydiacove",
        "caskaydia cove",
        "caskaydia mono",
    ],
    &["jetbrainsmono", "jetbrains mono"],
    &["firacode", "fira code"],
    &["hacknerdfont", "hack nerd", "hack"],
    &["iosevka"],
    &["maple mono", "maple mono nf"],
    &["meslo"],
    &["sauce code"],
    &["sourcecodepro", "source code pro"],
    &["ubuntumono", "ubuntu mono"],
    &["notomono", "noto mono", "noto sans mono"],
    &["anonymouspro", "anonymous pro"],
    &["agave"],
    &["bigbluemono", "big blue mono"],
    &["commitmono"],
    &["envy code"],
    &["fira mono"],
    &["go mono"],
    &["hasklig"],
    &["ibm plex mono"],
    &["inconsolata"],
    &["ia writer mono"],
    &["karmilla"],
    &["lekton"],
    &["libertinus mono"],
    &["menlo"],
    &["monoid"],
    &["mononoki"],
    &["terminus"],
    &["victor mono"],
    &["space mono"],
    &["sharetechmono"],
    &["roboto mono"],
    &["pt mono", "pt sans mono"],
    &["overpass mono"],
    &["droid sans mono"],
    &["racket mono"],
    &["scientifica"],
    &["zed mono"],
    &["zeity mono"],
    &["tinos"],
    &["chroma mono"],
    &["cmu mono", "computer modern mono"],
    &["boston mono"],
    &["blexblanco"],
    &["dustin mono"],
    &["grold mono"],
    &["heavy mono"],
    &["lumberjack mono"],
    &["martian mono"],
    &["recco mono"],
    &["spot mono"],
    &["stacksans mono"],
    &["write mono"],
    &["vim mono"],
    &["wine mono"],
    &["prestige elite"],
    &["proggy", "pro font"],
    &["quali"],
    &["shannons mono"],
];

const STANDARD_FONTS: &[&str] = &[
    "dejavu sans",
    "ubuntu",
    "noto sans",
    "noto sans mono",
    "arial",
    "liberation sans",
    "droid sans",
    "freesans",
    "lato",
    "roboto",
    "pt sans",
    "pt mono",
    "pt serif",
    "urw gothic",
    "go",
    "tlwg typewriter",
    "ipagothic",
    "ipaexgothic",
    "latin modern mono",
    "tinos",
    "berenis adf pro",
    "noto sans cjk",
    "noto sans egyptian",
    "noto sans glagolitic",
    "noto sans gothic",
    "noto sans gunjala",
    "noto sans inscriptional",
    "noto sans masaram",
    "noto sans mongolian",
    "noto sans tifinagh",
    "noto mono",
    "sans",
    "monospace",
    "courier",
    "courier new",
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
        let installed_fonts = match self.get_installed_fonts() {
            Ok(fonts) => fonts,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Failed to query installed fonts: {}", e),
                    fixable: false,
                };
            }
        };

        if installed_fonts.is_empty() {
            return CheckStatus::Fail {
                message: String::from("No fonts detected on the system"),
                fixable: true,
            };
        }

        let nerd_fonts = self.detect_nerd_fonts(&installed_fonts);
        let current_font = self
            .get_current_monospace_font()
            .unwrap_or_else(|| "system default".to_string());
        let coverage = self.check_coverage();

        let has_nerd_coverage = coverage.has_nerd_coverage;
        let all_symbols_render = coverage.all_symbols_render;

        let nerd_font_display = if nerd_fonts.is_empty() {
            current_font.clone()
        } else {
            nerd_fonts.join(", ")
        };

        let fc_query_working = !coverage.fc_query_failed;

        if has_nerd_coverage && all_symbols_render {
            if nerd_fonts.is_empty() {
                CheckStatus::Pass(format!(
                    "All Nerd Font symbols render correctly using '{}'",
                    current_font
                ))
            } else {
                CheckStatus::Pass(format!(
                    "Nerd Font detected and all Nerd Font symbols render correctly ({})",
                    nerd_font_display
                ))
            }
        } else if has_nerd_coverage && !all_symbols_render {
            let bad_fonts_str = if coverage.bad_fonts.is_empty() {
                "unknown system fonts".to_string()
            } else {
                coverage.bad_fonts.join(", ")
            };

            CheckStatus::Warning {
                message: format!(
                    "Nerd Font(s) detected ({}) but {} of {} symbols are not using them. They are rendering using: {}. This causes boxes or wrong characters.",
                    nerd_font_display,
                    coverage.missing_count + coverage.standard_coverage_count,
                    coverage.total_checked,
                    bad_fonts_str
                ),
                fixable: true,
            }
        } else if !nerd_fonts.is_empty() && fc_query_working {
            let bad_fonts_str = if coverage.bad_fonts.is_empty() {
                "unknown system fonts".to_string()
            } else {
                coverage.bad_fonts.join(", ")
            };

            CheckStatus::Warning {
                message: format!(
                    "Nerd Font(s) detected ({}) but symbol rendering test failed. Your system prefers other fonts for these symbols: {}. Configure your terminal or fontconfig to prioritize the Nerd Font.",
                    nerd_font_display, bad_fonts_str
                ),
                fixable: true,
            }
        } else if !nerd_fonts.is_empty() && !fc_query_working {
            CheckStatus::Warning {
                message: format!(
                    "Nerd Font(s) installed ({}) but fontconfig query is broken. Run 'fc-cache -f' and try again. Symbols may display as boxes or wrong characters.",
                    nerd_font_display
                ),
                fixable: true,
            }
        } else {
            CheckStatus::Fail {
                message: format!(
                    "No Nerd Font detected. Current font: '{}'. \
                    Nerd Font symbols will render as boxes or wrong characters (e.g. Chinese, Arabic).",
                    current_font
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

        println!("Successfully installed CaskaydiaCove Nerd Font!");

        println!("Please configure your terminal to use 'CaskaydiaCove Nerd Font'.");

        Ok(())
    }
}

impl NerdFontCheck {
    fn get_installed_fonts(&self) -> Result<Vec<String>> {
        use std::process::Command;

        let output = Command::new("fc-list").arg(":").arg("family").output()?;

        if !output.status.success() {
            anyhow::bail!("fc-list command failed");
        }

        let fonts = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(fonts)
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

    fn detect_nerd_fonts(&self, fonts: &[String]) -> Vec<String> {
        let mut detected = Vec::new();
        let fonts_lower: Vec<String> = fonts.iter().map(|s| s.to_lowercase()).collect();

        for font in &fonts_lower {
            for patterns in NERD_FONT_PATTERNS {
                for pattern in *patterns {
                    if font.contains(pattern) {
                        let original = fonts.iter().find(|f| f.to_lowercase() == *font);
                        if let Some(f) = original {
                            if !detected.contains(f) {
                                detected.push(f.clone());
                            }
                            break;
                        }
                    }
                }
            }
        }

        detected
    }

    fn check_coverage(&self) -> CoverageResult {
        let mut nerd_coverage_count = 0;
        let mut standard_coverage_count = 0;
        let mut missing_count = 0;
        let mut fc_query_failed = false;
        let mut all_render = true;
        let mut bad_fonts = std::collections::HashSet::new();

        let all_symbols: Vec<char> = COMMON_SYMBOLS.iter().copied().collect();

        for &glyph in &all_symbols {
            let codepoint = glyph as u32;
            match self.find_font_for_codepoint(codepoint) {
                Some(font) => {
                    if self.is_nerd_font(&font) {
                        nerd_coverage_count += 1;
                    } else {
                        standard_coverage_count += 1;
                        all_render = false;
                        bad_fonts.insert(font);
                    }
                }
                None => {
                    // Try to distinguish between "fc-match failed to run" and "no font found"
                    // But find_font_for_codepoint returns None on error or empty output
                    // For now, treat as missing/failure
                    fc_query_failed = true; // technically match failed or returned nothing
                    missing_count += 1;
                    all_render = false;
                }
            }
        }

        let total_checked = all_symbols.len();
        // Relaxed check: if we have significant nerd coverage, we assume it's "detected"
        // even if some symbols fall back.
        let has_nerd_coverage = nerd_coverage_count > total_checked / 3;

        CoverageResult {
            nerd_coverage_count,
            standard_coverage_count,
            missing_count,
            total_checked,
            has_nerd_coverage,
            all_symbols_render: all_render,
            fc_query_failed,
            bad_fonts: bad_fonts.into_iter().collect(),
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

    fn is_nerd_font(&self, font: &str) -> bool {
        let font_lower = font.to_lowercase();
        for patterns in NERD_FONT_PATTERNS {
            for pattern in *patterns {
                if font_lower.contains(pattern) {
                    return true;
                }
            }
        }
        false
    }

    fn is_standard_font(&self, font: &str) -> bool {
        STANDARD_FONTS
            .iter()
            .any(|sf| font.to_lowercase().contains(&sf.to_lowercase()))
    }
}

struct CoverageResult {
    nerd_coverage_count: usize,
    standard_coverage_count: usize,
    missing_count: usize,
    total_checked: usize,
    has_nerd_coverage: bool,
    all_symbols_render: bool,
    fc_query_failed: bool,
    bad_fonts: Vec<String>,
}
