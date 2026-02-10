//! Reusable documented configuration pattern
//!
//! This module provides a macro-based approach for creating configuration files
//! with inline documentation.
//!
//! # Example
//!
//! ```ignore
//! documented_config!(VideoConfig,
//!     music_volume, "Music volume (0.0-1.0)",
//!     preprocessor, "Audio preprocessor",
//!     auphonic_api_key, "Auphonic API key",
//!     => paths::instant_config_dir()?.join("video.toml")
//! );
//! ```
//!
//! On initial file creation, fields with default values are commented out.
//! On subsequent saves, all values are written uncommented to preserve user intent.

use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Serialize a value to TOML string representation
pub fn to_toml_string<T: Serialize>(value: &T) -> Option<String> {
    toml::Value::try_from(value).map(|v| v.to_string()).ok()
}

/// Helper for automatic Option<T> detection using method resolution priority.
/// Inherent methods (for Option<T>) take priority over trait methods (fallback).
pub struct DocValue<'a, T>(pub &'a T);

/// Inherent impl for Option<T> - uses inner type's default for documentation
impl<T: Serialize + Default + Clone> DocValue<'_, Option<T>> {
    pub fn to_toml(&self) -> Option<String> {
        to_toml_string(&self.0.clone().unwrap_or_default())
    }

    /// Returns true if this Option is None (field is unset)
    pub fn is_unset(&self) -> bool {
        self.0.is_none()
    }
}

/// Trait fallback for all non-Option types
pub trait DocValueFallback {
    fn to_toml(&self) -> Option<String>;
    /// Non-Option types are never "unset"
    fn is_unset(&self) -> bool {
        false
    }
}

impl<T: Serialize> DocValueFallback for DocValue<'_, T> {
    fn to_toml(&self) -> Option<String> {
        to_toml_string(self.0)
    }
}

/// Metadata about a configuration field
#[derive(Debug, Clone)]
pub struct ConfigFieldMeta {
    pub name: &'static str,
    /// TOML-serialized default value, or None if serialization failed
    pub default_value: Option<String>,
    pub description: &'static str,
}

/// Trait for configs with documented defaults
///
/// This trait is automatically implemented by the `documented_config!` macro.
pub trait DocumentedConfig: Sized + Default {
    /// Get metadata for all configuration fields
    fn field_metadata() -> Vec<ConfigFieldMeta>;

    /// Get the TOML-serialized value for a specific field
    fn get_field_value(&self, field_name: &str) -> Option<String>;

    /// Check if a field is unset (Option<T> that is None)
    fn is_field_unset(&self, field_name: &str) -> bool;

    /// Create initial config file with defaults commented out
    /// Use this only for first-time file creation
    fn create_initial_config(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating config directory {}", parent.display()))?;
        }

        let mut output = String::new();
        let metadata = Self::field_metadata();

        for field in &metadata {
            let current_value = self.get_field_value(field.name);

            // Compare current value to default
            let is_default = match (&current_value, &field.default_value) {
                (Some(current), Some(default)) => current == default,
                (None, None) => true,
                _ => false,
            };

            if is_default {
                // Comment out fields with default values on initial creation
                let Some(default_val) = &field.default_value else {
                    continue;
                };
                output.push_str(&format!(
                    "# {} = {}  # {}\n",
                    field.name, default_val, field.description
                ));
            } else if let Some(value) = current_value {
                output.push_str(&format!(
                    "{} = {}  # {}\n",
                    field.name, value, field.description
                ));
            }
        }

        fs::write(path, output).with_context(|| format!("writing config to {}", path.display()))?;
        Ok(())
    }

    /// Save config - Option<T> fields that are None are commented out,
    /// all other values are written uncommented (preserves user intent)
    fn save_with_documentation(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating config directory {}", parent.display()))?;
        }

        let mut output = String::new();
        let metadata = Self::field_metadata();

        for field in &metadata {
            let Some(value) = self.get_field_value(field.name) else {
                continue;
            };

            if self.is_field_unset(field.name) {
                // Option<T> that is None - comment out with inner default
                output.push_str(&format!(
                    "# {} = {}  # {}\n",
                    field.name, value, field.description
                ));
            } else {
                output.push_str(&format!(
                    "{} = {}  # {}\n",
                    field.name, value, field.description
                ));
            }
        }

        fs::write(path, output).with_context(|| format!("writing config to {}", path.display()))?;
        Ok(())
    }

    /// Load config from string
    fn load_from_str_documented(contents: &str, _path: &Path) -> Result<Self>
    where
        for<'de> Self: serde::de::Deserialize<'de>,
    {
        toml::from_str(contents).context("parsing config")
    }

    /// Load config from path, creating initial file if it doesn't exist
    fn load_from_path_documented(path: PathBuf) -> Result<Self>;
}

/// Macro to generate DocumentedConfig trait implementation
///
/// # Syntax
///
/// ```ignore
/// documented_config!(VideoConfig,
///     music_volume, "Music volume (0.0-1.0)",
///     preprocessor, "Audio preprocessor",
///     auphonic_api_key, "Auphonic API key",
///     => paths::instant_config_dir()?.join("video.toml")
/// );
/// ```
///
/// Option<T> fields are automatically detected - the inner type's default
/// is used for documentation (e.g., `Option<Vec<String>>` shows `[]`).
///
/// On initial creation, fields with default values are commented out.
/// On save, all values are written uncommented.
#[macro_export]
macro_rules! documented_config {
    (
        $config_name:ident,
        $($field:ident, $desc:expr,)*
        => $path:expr
    ) => {
        impl $crate::common::config::DocumentedConfig for $config_name {
            fn field_metadata() -> Vec<$crate::common::config::ConfigFieldMeta> {
                use $crate::common::config::DocValueFallback;
                let default_config = Self::default();
                vec![
                    $(
                        $crate::common::config::ConfigFieldMeta {
                            name: stringify!($field),
                            default_value: $crate::common::config::DocValue(&default_config.$field).to_toml(),
                            description: $desc,
                        },
                    )*
                ]
            }

            fn get_field_value(&self, field_name: &str) -> Option<String> {
                use $crate::common::config::DocValueFallback;
                match field_name {
                    $(
                        stringify!($field) => $crate::common::config::DocValue(&self.$field).to_toml(),
                    )*
                    _ => None,
                }
            }

            fn is_field_unset(&self, field_name: &str) -> bool {
                use $crate::common::config::DocValueFallback;
                match field_name {
                    $(
                        stringify!($field) => $crate::common::config::DocValue(&self.$field).is_unset(),
                    )*
                    _ => false,
                }
            }

            fn load_from_path_documented(path: PathBuf) -> Result<Self> {
                if !path.exists() {
                    let config = Self::default();
                    config.create_initial_config(&path)?;
                    return Ok(config);
                }
                let contents = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading config from {}", path.display()))?;
                Self::load_from_str_documented(&contents, &path)
            }
        }
    };
}
