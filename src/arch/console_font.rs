//! Discovered console fonts and portable, validated snapshots for installation.
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

pub(crate) const FONT_DIRECTORY: &str = "/usr/share/kbd/consolefonts";
pub(crate) const DEFAULT_NAME: &str = "default8x16";
pub(crate) const INSTALLED_NAME: &str = "ins-selected";
const MAX_FONT_BYTES: usize = 1024 * 1024;
const MAX_ANSWER_BYTES: usize = MAX_FONT_BYTES * 2;

struct FontSuggestion {
    name: &'static str,
    label: &'static str,
    legacy_package: &'static str,
}

// Presentation hints, not an allowlist for discovered fonts. Package names
// support plain-name answers saved by the original picker.
const SUGGESTED: &[FontSuggestion] = &[
    FontSuggestion {
        name: DEFAULT_NAME,
        label: "Default console font (8x16)",
        legacy_package: "kbd",
    },
    FontSuggestion {
        name: "Lat2-Terminus16",
        label: "Terminus (8x16)",
        legacy_package: "kbd",
    },
    FontSuggestion {
        name: "sun12x22",
        label: "Sun (12x22) - large",
        legacy_package: "kbd",
    },
    FontSuggestion {
        name: "latarcyrheb-sun32",
        label: "LatArCyrHeb (16x32) - extra large",
        legacy_package: "kbd",
    },
    FontSuggestion {
        name: "ter-v20n",
        label: "Terminus (10x20)",
        legacy_package: "terminus-font",
    },
    FontSuggestion {
        name: "ter-v24n",
        label: "Terminus (12x24) - large",
        legacy_package: "terminus-font",
    },
    FontSuggestion {
        name: "ter-v28n",
        label: "Terminus (14x28) - larger",
        legacy_package: "terminus-font",
    },
    FontSuggestion {
        name: "ter-v32n",
        label: "Terminus (16x32) - extra large",
        legacy_package: "terminus-font",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FontInfo {
    pub width: u32,
    pub height: u32,
    pub glyphs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleFont {
    name: String,
    info: Option<FontInfo>,
    // Uncompressed PSF, including the Unicode table. Captured in the saved
    // answer so chroot re-entry and resume never depend on the live ISO's files.
    data: Option<Vec<u8>>,
    legacy_package: Option<&'static str>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedFont {
    name: String,
    psf_base64: String,
}

impl Default for ConsoleFont {
    fn default() -> Self {
        Self {
            name: DEFAULT_NAME.to_string(),
            info: None,
            data: None,
            legacy_package: Some("kbd"),
        }
    }
}

impl ConsoleFont {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn info(&self) -> Option<FontInfo> {
        self.info
    }
    pub(crate) fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }
    pub(crate) fn legacy_package(&self) -> Option<&'static str> {
        self.legacy_package
    }

    pub(crate) fn suggested_rank(&self) -> Option<usize> {
        SUGGESTED
            .iter()
            .position(|suggestion| suggestion.name == self.name)
    }

    pub(crate) fn label(&self) -> String {
        if let Some(suggestion) = SUGGESTED
            .iter()
            .find(|suggestion| suggestion.name == self.name)
        {
            return suggestion.label.to_string();
        }
        match self.info {
            Some(info) => format!("{} ({}x{})", self.name, info.width, info.height),
            None => self.name.clone(),
        }
    }

    fn from_psf(name: String, data: Vec<u8>) -> Result<Self> {
        validate_name(&name)?;
        let info = parse_psf(&data)?;
        Ok(Self {
            name,
            info: Some(info),
            data: Some(data),
            legacy_package: None,
        })
    }

    /// Validate both old plain-name answers and new self-contained snapshots.
    pub fn parse(answer: &str) -> Result<Self> {
        ensure!(
            answer.len() <= MAX_ANSWER_BYTES,
            "Console font answer is too large"
        );
        if answer.starts_with('{') {
            let saved: SavedFont =
                serde_json::from_str(answer).context("Invalid saved console font")?;
            validate_name(&saved.name)?;
            let data = STANDARD
                .decode(saved.psf_base64)
                .context("Invalid console font encoding")?;
            return Self::from_psf(saved.name, data);
        }
        // These legacy answers are fulfilled by installing their original
        // package. Unknown plain names cannot bypass snapshot validation.
        validate_name(answer)?;
        match SUGGESTED
            .iter()
            .find(|suggestion| suggestion.name == answer)
        {
            Some(suggestion) => Ok(Self {
                name: suggestion.name.to_string(),
                info: None,
                data: None,
                legacy_package: Some(suggestion.legacy_package),
            }),
            None => bail!("Unknown console font {answer:?}; select an available font again"),
        }
    }

    pub(crate) fn to_answer(&self) -> Result<String> {
        match &self.data {
            Some(data) => Ok(serde_json::to_string(&SavedFont {
                name: self.name.clone(),
                psf_base64: STANDARD.encode(data),
            })?),
            None => Ok(self.name.clone()),
        }
    }

    /// Use a private filename so font packages cannot overwrite the snapshot.
    pub(crate) fn installed_name(&self) -> &str {
        if self.data.is_some() {
            INSTALLED_NAME
        } else {
            &self.name
        }
    }

    pub(crate) fn preview_path(&self, directory: &Path, index: usize) -> Result<PathBuf> {
        match &self.data {
            Some(data) => {
                let path = directory.join(format!("font-{index}.psf"));
                std::fs::write(&path, data).context("Preparing console font preview")?;
                Ok(path)
            }
            None => Ok(PathBuf::from(&self.name)),
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 256
            && name != "."
            && name != ".."
            && !name
                .chars()
                .any(|character| character.is_control() || matches!(character, '/' | '\\')),
        "Invalid console font name"
    );
    Ok(())
}

/// Human-readable representation for reviews and reports; never show font data.
pub(crate) fn answer_label(answer: &str) -> String {
    ConsoleFont::parse(answer)
        .map(|font| font.name().to_string())
        .unwrap_or_else(|_| "Invalid console font".to_string())
}

/// Discover PSF/PSFU files, including gzip files, aliases, and subdirectories.
/// Non-font files and invalid fonts never enter the picker or install plan.
pub(crate) fn discover(directory: &Path) -> Result<Vec<ConsoleFont>> {
    match std::fs::metadata(directory) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(vec![ConsoleFont::default()]);
        }
        Err(error) => return Err(error).context("Reading console font directory"),
    }
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(directory).follow_links(false) {
        let entry = entry.context("Listing console fonts")?;
        if entry.path().is_file() {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    let mut fonts = BTreeMap::new();
    for path in paths {
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(name) = [".psfu.gz", ".psf.gz", ".psfu", ".psf"]
            .iter()
            .find_map(|suffix| filename.strip_suffix(suffix))
        else {
            continue;
        };
        let font = read_font(&path).and_then(|data| ConsoleFont::from_psf(name.to_string(), data));
        match font {
            Ok(font) => {
                fonts.entry(font.name.clone()).or_insert(font);
            }
            Err(error) => eprintln!(
                "Warning: skipping console font {}: {error:#}",
                path.display()
            ),
        }
    }
    fonts
        .entry(DEFAULT_NAME.to_string())
        .or_insert_with(ConsoleFont::default);
    let mut fonts: Vec<_> = fonts.into_values().collect();
    fonts.sort_by(|left, right| {
        left.suggested_rank()
            .unwrap_or(usize::MAX)
            .cmp(&right.suggested_rank().unwrap_or(usize::MAX))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(fonts)
}

fn read_font(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    if path.extension().is_some_and(|extension| extension == "gz") {
        // gzip is available on the supported Arch live systems. Bound the
        // decompressed output and reap the process on every read/error path.
        let mut child = Command::new("gzip")
            .arg("-cd")
            .arg("--")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Could not run gzip to read the console font")?;
        let read = child
            .stdout
            .take()
            .context("gzip did not provide font data")
            .and_then(|stdout| {
                stdout
                    .take((MAX_FONT_BYTES + 1) as u64)
                    .read_to_end(&mut data)
                    .context("Reading compressed console font")
            });
        if read.is_err() || data.len() > MAX_FONT_BYTES {
            let _ = child.kill();
            child
                .wait()
                .context("Waiting for console font decompression")?;
            read?;
            bail!("Console font exceeds the size limit");
        }
        let status = child
            .wait()
            .context("Waiting for console font decompression")?;
        ensure!(
            status.success(),
            "gzip could not decompress the console font"
        );
    } else {
        File::open(path)?
            .take((MAX_FONT_BYTES + 1) as u64)
            .read_to_end(&mut data)?;
    }
    ensure!(
        data.len() <= MAX_FONT_BYTES,
        "Console font exceeds the size limit"
    );
    Ok(data)
}

fn word(data: &[u8], offset: usize) -> Result<u32> {
    let bytes: [u8; 4] = data
        .get(offset..offset + 4)
        .context("Truncated PSF header")?
        .try_into()
        .context("Invalid PSF header")?;
    Ok(u32::from_le_bytes(bytes))
}

// Format: https://kbd-project.org/docs/font-formats/font-formats-1.html
fn parse_psf(data: &[u8]) -> Result<FontInfo> {
    ensure!(
        data.len() <= MAX_FONT_BYTES,
        "Console font exceeds the size limit"
    );
    if data.starts_with(&[0x36, 0x04]) {
        ensure!(data.len() >= 4, "Truncated PSF1 header");
        let mode = data[2];
        let height = u32::from(data[3]);
        ensure!(
            mode <= 5 && (1..=128).contains(&height),
            "Unsupported PSF1 font"
        );
        let glyphs = if mode & 1 != 0 { 512 } else { 256 };
        let bitmap_end = 4 + (glyphs * height) as usize;
        ensure!(data.len() >= bitmap_end, "Truncated PSF1 glyphs");
        let table = &data[bitmap_end..];
        if mode & 6 != 0 {
            ensure!(
                table.len().is_multiple_of(2),
                "Truncated PSF1 Unicode table"
            );
            let separators = table
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|bytes| **bytes == [0xff, 0xff])
                .count();
            ensure!(
                separators == glyphs as usize && table.ends_with(&[0xff, 0xff]),
                "Invalid PSF1 Unicode table"
            );
        } else {
            ensure!(table.is_empty(), "Unexpected data after PSF1 glyphs");
        }
        return Ok(FontInfo {
            width: 8,
            height,
            glyphs,
        });
    }
    ensure!(
        data.starts_with(&[0x72, 0xb5, 0x4a, 0x86]),
        "Not a PSF font"
    );
    ensure!(word(data, 4)? == 0, "Unsupported PSF2 version");
    let header_size = word(data, 8)? as usize;
    let flags = word(data, 12)?;
    let glyphs = word(data, 16)?;
    let bytes_per_glyph = word(data, 20)?;
    let height = word(data, 24)?;
    let width = word(data, 28)?;
    ensure!(
        header_size >= 32 && header_size <= data.len(),
        "Invalid PSF2 header size"
    );
    ensure!(
        flags <= 1
            && (1..=512).contains(&glyphs)
            && (1..=128).contains(&height)
            && (1..=64).contains(&width),
        "Unsupported PSF2 font dimensions"
    );
    ensure!(
        bytes_per_glyph == width.div_ceil(8) * height,
        "Invalid PSF2 glyph size"
    );
    let bitmap_end = header_size
        .checked_add((glyphs * bytes_per_glyph) as usize)
        .context("Invalid PSF2 font size")?;
    ensure!(data.len() >= bitmap_end, "Truncated PSF2 glyphs");
    let table = &data[bitmap_end..];
    if flags == 1 {
        ensure!(
            table.iter().filter(|byte| **byte == 0xff).count() == glyphs as usize
                && table.last() == Some(&0xff),
            "Invalid PSF2 Unicode table"
        );
        for sequence in table.split(|byte| *byte == 0xff || *byte == 0xfe) {
            std::str::from_utf8(sequence).context("Invalid UTF-8 in PSF2 Unicode table")?;
        }
    } else {
        ensure!(table.is_empty(), "Unexpected data after PSF2 glyphs");
    }
    Ok(FontInfo {
        width,
        height,
        glyphs,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn psf1() -> Vec<u8> {
        let mut data = vec![0x36, 0x04, 2, 16];
        data.resize(4 + 256 * 16, 0);
        for codepoint in 0..256u16 {
            data.extend_from_slice(&codepoint.to_le_bytes());
            data.extend_from_slice(&[0xff, 0xff]);
        }
        data
    }

    fn psf2() -> Vec<u8> {
        let mut data: Vec<_> = [0x864ab572u32, 0, 32, 1, 256, 44, 22, 12]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        data.resize(32 + 256 * 44, 0);
        for _ in 0..256 {
            data.extend_from_slice(&[b'A', 0xff]);
        }
        data
    }

    #[test]
    fn discovers_custom_fonts_compression_subdirectories_and_aliases() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let data = psf1();
        let font_path = directory.path().join("custom.psfu");
        std::fs::write(&font_path, &data).unwrap();
        let gzip = Command::new("gzip")
            .arg("-c")
            .arg(&font_path)
            .output()
            .unwrap();
        assert!(gzip.status.success());
        std::fs::write(directory.path().join("custom.psfu.gz"), &gzip.stdout).unwrap();
        std::fs::write(directory.path().join("compressed.psfu.gz"), gzip.stdout).unwrap();
        std::fs::create_dir(directory.path().join("extra")).unwrap();
        std::fs::write(directory.path().join("extra/wide.psf"), psf2()).unwrap();
        std::fs::write(directory.path().join("README"), "not a font").unwrap();
        std::fs::write(directory.path().join("broken.psf"), b"bad").unwrap();
        symlink(&font_path, directory.path().join("alias.psfu")).unwrap();
        let fonts = discover(directory.path()).unwrap();
        let names: Vec<_> = fonts.iter().map(ConsoleFont::name).collect();
        assert_eq!(
            names,
            [DEFAULT_NAME, "alias", "compressed", "custom", "wide"]
        );
        let wide = fonts.iter().find(|font| font.name() == "wide").unwrap();
        assert_eq!(
            wide.info(),
            Some(FontInfo {
                width: 12,
                height: 22,
                glyphs: 256
            })
        );
    }

    #[test]
    fn snapshot_survives_source_removal_and_rejects_invalid_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("custom.psf");
        std::fs::write(&path, psf1()).unwrap();
        let font = discover(directory.path())
            .unwrap()
            .into_iter()
            .find(|font| font.name() == "custom")
            .unwrap();
        let answer = font.to_answer().unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(ConsoleFont::parse(&answer).unwrap(), font);
        for name in ["../font", "..", "font\nKEYMAP=bad"] {
            let saved = SavedFont {
                name: name.to_string(),
                psf_base64: STANDARD.encode(psf1()),
            };
            assert!(ConsoleFont::parse(&serde_json::to_string(&saved).unwrap()).is_err());
        }
        let saved = SavedFont {
            name: "custom".to_string(),
            psf_base64: STANDARD.encode(b"not a font"),
        };
        assert!(ConsoleFont::parse(&serde_json::to_string(&saved).unwrap()).is_err());
    }

    #[test]
    fn validates_psf_headers_bitmaps_and_unicode_tables() {
        assert_eq!(
            parse_psf(&psf1()).unwrap(),
            FontInfo {
                width: 8,
                height: 16,
                glyphs: 256
            }
        );
        assert_eq!(
            parse_psf(&psf2()).unwrap(),
            FontInfo {
                width: 12,
                height: 22,
                glyphs: 256
            }
        );
        for valid in [psf1(), psf2()] {
            for length in [0, 3, 30, valid.len() - 1] {
                assert!(parse_psf(&valid[..length]).is_err());
            }
        }
        let mut invalid = psf2();
        invalid[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_psf(&invalid).is_err());
        let mut invalid = psf2();
        *invalid.last_mut().unwrap() = 0xc0;
        assert!(parse_psf(&invalid).is_err());
    }

    #[test]
    fn old_answers_and_missing_font_directories_remain_supported() {
        for suggestion in SUGGESTED {
            assert_eq!(
                ConsoleFont::parse(suggestion.name)
                    .unwrap()
                    .legacy_package(),
                Some(suggestion.legacy_package)
            );
        }
        assert!(ConsoleFont::parse("unknown").is_err());
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            discover(&directory.path().join("missing")).unwrap(),
            [ConsoleFont::default()]
        );
    }
}
