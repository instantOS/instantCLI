//! Console font choices shared by the wizard and validated install plan.
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleFont {
    pub name: &'static str,
    pub label: &'static str,
    pub package: &'static str,
}

impl ConsoleFont {
    pub const DEFAULT: Self = Self {
        name: "default8x16",
        label: "Default console font (8x16)",
        package: "kbd",
    };

    pub const ALL: &'static [Self] = &[
        Self::DEFAULT,
        Self {
            name: "Lat2-Terminus16",
            label: "Terminus (8x16)",
            package: "kbd",
        },
        Self {
            name: "sun12x22",
            label: "Sun (12x22) - large",
            package: "kbd",
        },
        Self {
            name: "latarcyrheb-sun32",
            label: "LatArCyrHeb (16x32) - extra large",
            package: "kbd",
        },
        Self {
            name: "ter-v20n",
            label: "Terminus (10x20)",
            package: "terminus-font",
        },
        Self {
            name: "ter-v24n",
            label: "Terminus (12x24) - large",
            package: "terminus-font",
        },
        Self {
            name: "ter-v28n",
            label: "Terminus (14x28) - larger",
            package: "terminus-font",
        },
        Self {
            name: "ter-v32n",
            label: "Terminus (16x32) - extra large",
            package: "terminus-font",
        },
    ];

    pub fn parse(name: &str) -> Result<Self> {
        match Self::ALL.iter().find(|font| font.name == name) {
            Some(font) => Ok(*font),
            None => bail!("Unknown console font {name:?}"),
        }
    }

    pub fn file_in(self, directory: &Path) -> Option<PathBuf> {
        ["psfu.gz", "psf.gz", "psfu", "psf"]
            .into_iter()
            .map(|extension| directory.join(format!("{}.{}", self.name, extension)))
            .find(|path| path.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_supported_fonts_can_enter_an_install_plan() {
        for font in ConsoleFont::ALL {
            assert_eq!(ConsoleFont::parse(font.name).unwrap(), *font);
        }
        for invalid in ["../font", "-R", "font\nKEYMAP=bad", "unknown"] {
            assert!(ConsoleFont::parse(invalid).is_err());
        }
    }

    #[test]
    fn discovery_requires_a_real_font_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ConsoleFont::DEFAULT.file_in(dir.path()).is_none());
        let file = dir.path().join("default8x16.psfu.gz");
        std::fs::write(&file, b"font fixture").unwrap();
        assert_eq!(ConsoleFont::DEFAULT.file_in(dir.path()), Some(file));
    }
}
