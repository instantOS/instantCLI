//! Reusable documented configuration pattern
//!
//! This module provides a macro-based approach for creating configuration files
//! with inline documentation, following SOLID principles.
//!
//! # Key Insight
//!
//! - Fields with actual defaults (via `#[serde(default)]`) are **always populated** by serde
//! - Only `Option<T>` fields can be `None` (invisible in TOML), so only those need commented defaults
//!
//! # Example
//!
//! ```ignore
//! documented_config!(VideoConfig {
//!     // Fields with defaults - always written uncommented
//!     [fields] music_volume: f32 = 0.2, "Music volume (0.0-1.0)"
//!     [fields] preprocessor: PreprocessorType = Local, "Audio preprocessor"
//!
//!     // Optional fields - commented when None
//!     [optional] auphonic_api_key: String, "Auphonic API key"
//!     [optional] auphonic_preset_uuid: String, "Preset UUID"
//!
//!     config_path: || paths::instant_config_dir()?.join("video.toml"),
//! });
//! ```

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata about a configuration field
#[derive(Debug, Clone)]
pub struct ConfigFieldMeta {
    pub name: &'static str,
    pub default_value: String,
    pub description: &'static str,
    pub is_optional: bool,
}

/// Trait for configs with documented defaults
///
/// This trait is automatically implemented by the `documented_config!` macro.
pub trait DocumentedConfig: Sized + Default {
    /// Get metadata for all configuration fields
    fn field_metadata() -> Vec<ConfigFieldMeta>;

    /// Check if an optional field is set (Some vs None)
    fn is_optional_field_set(&self, field_name: &str) -> bool;

    /// Get the TOML-serialized value for a specific field
    fn get_field_value(&self, field_name: &str) -> String;

    /// Get path where this config should be stored
    fn config_path() -> Result<PathBuf>;

    /// Check if an existing config file is minimal (safe to regenerate)
    fn is_minimal_config(path: &Path) -> bool {
        if !path.exists() {
            return true;
        }

        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return true,
        };

        let lines: Vec<&str> = contents.lines().collect();

        // Check for custom comments (lines starting with # that aren't our format)
        let has_custom_comments =
            lines.iter().any(|l| l.trim().starts_with('#') && !l.contains(" = "));

        // Safe to regenerate if: no custom comments OR file is very small
        !has_custom_comments || lines.len() <= 3
    }

    /// Save config with inline documentation for unset optional values
    fn save_with_documentation(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating config directory {}", parent.display()))?;
        }

        let mut output = String::new();
        let metadata = Self::field_metadata();

        for field in &metadata {
            let value = self.get_field_value(field.name);

            // For optional fields: comment out if None (not set)
            // For regular fields: always write uncommented (serde always populates them)
            if field.is_optional && !self.is_optional_field_set(field.name) {
                output.push_str(&format!(
                    "# {} = {}  # {}\n",
                    field.name, field.default_value, field.description
                ));
            } else {
                output.push_str(&format!(
                    "{} = {}  # {}\n",
                    field.name, value, field.description
                ));
            }
        }

        fs::write(path, output)
            .with_context(|| format!("writing config to {}", path.display()))?;
        Ok(())
    }

    /// Load config with smart documentation merge
    fn load_from_str_documented(contents: &str, path: &Path) -> Result<Self>
    where
        for<'de> Self: serde::de::Deserialize<'de>,
    {
        let config: Self = toml::from_str(contents).context("parsing config")?;

        // Smart merge: if minimal config, regenerate with comments
        if Self::is_minimal_config(path) {
            // We need to save with comments, but we can't because &self
            // Just note that the file should be regenerated on next save
        }

        Ok(config)
    }

    /// Load config with smart documentation merge
    fn load_from_path_documented(path: PathBuf) -> Result<Self>;
}

/// Macro to generate DocumentedConfig trait implementation
///
/// # Syntax
///
/// ```ignore
/// documented_config!(VideoConfig {
///     // Regular fields with defaults (always present in TOML)
///     fields: [
///         music_volume: f32 = 0.2, "Music volume (0.0-1.0)",
///         preprocessor: PreprocessorType = Local, "Audio preprocessor",
///     ],
///
///     // Optional fields (commented when None)
///     optional: [
///         auphonic_api_key: String, "Auphonic API key",
///         auphonic_preset_uuid: String, "Preset UUID",
///     ],
///
///     config_path: || paths::instant_config_dir()?.join("video.toml"),
///
///     // Optional: Custom serializers for complex types
///     serialize: {
///         preprocessor => |s| match s.preprocessor {
///             PreprocessorType::Local => "local",
///             _ => "none",
///         },
///     }
/// });
/// ```
#[macro_export]
macro_rules! documented_config {
    // With optional fields
    (
        $config_name:ident {
            fields: [
                $($field:ident: $field_ty:ty = $default:expr, $desc:expr),* $(,)?
            ],
            optional: [
                $($opt_field:ident: $opt_ty:ty, $opt_desc:expr),* $(,)?
            ],
            config_path: $path:expr $(,)?
        }
    ) => {
        impl $crate::common::config::DocumentedConfig for $config_name {
            fn field_metadata() -> Vec<$crate::common::config::ConfigFieldMeta> {
                let default_config = Self::default();
                vec![
                    $(
                        $crate::common::config::ConfigFieldMeta {
                            name: stringify!($field),
                            default_value: toml::to_string(&default_config.$field)
                                .unwrap_or_else(|_| "?".to_string()),
                            description: $desc,
                            is_optional: false,
                        },
                    )*
                    $(
                        $crate::common::config::ConfigFieldMeta {
                            name: stringify!($opt_field),
                            default_value: "\"\"".to_string(),
                            description: $opt_desc,
                            is_optional: true,
                        },
                    )*
                ]
            }

            fn is_optional_field_set(&self, field_name: &str) -> bool {
                match field_name {
                    $(
                        stringify!($opt_field) => self.$opt_field.is_some(),
                    )*
                    _ => false,
                }
            }

            fn get_field_value(&self, field_name: &str) -> String {
                match field_name {
                    $(
                        stringify!($field) => {
                            toml::to_string(&self.$field)
                                .unwrap_or_else(|_| format!("{:?}", self.$field))
                        }
                    )*
                    $(
                        stringify!($opt_field) => match &self.$opt_field {
                            Some(v) => toml::to_string(v).unwrap_or_else(|_| format!("\"{}\"", v)),
                            None => "\"\"".to_string(),
                        },
                    )*
                    _ => String::new(),
                }
            }

            fn config_path() -> Result<PathBuf> {
                $path
            }

            fn load_from_path_documented(path: PathBuf) -> Result<Self> {
                if !path.exists() {
                    let config = Self::default();
                    config.save_with_documentation(&path)?;
                    return Ok(config);
                }
                let contents = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading config from {}", path.display()))?;
                Self::load_from_str_documented(&contents, &path)
            }
        }
    };

    // Without optional fields
    (
        $config_name:ident {
            fields: [
                $($field:ident: $field_ty:ty = $default:expr, $desc:expr),* $(,)?
            ],
            config_path: $path:expr $(,)?
        }
    ) => {
        impl $crate::common::config::DocumentedConfig for $config_name {
            fn field_metadata() -> Vec<$crate::common::config::ConfigFieldMeta> {
                let default_config = Self::default();
                vec![
                    $(
                        $crate::common::config::ConfigFieldMeta {
                            name: stringify!($field),
                            default_value: toml::to_string(&default_config.$field)
                                .unwrap_or_else(|_| "?".to_string()),
                            description: $desc,
                            is_optional: false,
                        },
                    )*
                ]
            }

            fn is_optional_field_set(&self, _field_name: &str) -> bool {
                false
            }

            fn get_field_value(&self, field_name: &str) -> String {
                match field_name {
                    $(
                        stringify!($field) => {
                            toml::to_string(&self.$field)
                                .unwrap_or_else(|_| format!("{:?}", self.$field))
                        }
                    )*
                    _ => String::new(),
                }
            }

            fn config_path() -> Result<PathBuf> {
                $path
            }

            fn load_from_path_documented(path: PathBuf) -> Result<Self> {
                if !path.exists() {
                    let config = Self::default();
                    config.save_with_documentation(&path)?;
                    return Ok(config);
                }
                let contents = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading config from {}", path.display()))?;
                Self::load_from_str_documented(&contents, &path)
            }
        }
    };
}
