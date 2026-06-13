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
//! );
//! ```
//!
//! On initial file creation, fields with default values are commented out.
//! Runtime saves use [`DocumentedConfig::save_documented_pretty_toml`] so
//! structured fields stay readable while users can still discover all fields.

use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::fs;
use std::path::Path;

/// Collapse any newlines (and surrounding whitespace) into single spaces so the
/// result fits on a single `# ...` comment line. Prevents multi-line strings
/// from emitting uncommented trailing lines that would be parsed as TOML.
fn single_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn documented_field_line(field: &ConfigFieldMeta) -> String {
    let description = single_line(field.description);
    if field.secret {
        return format!("# {}  # {}\n", field.name, description);
    }

    match field.default_value.as_deref() {
        Some(default_value) if is_compact_doc_value(default_value) => {
            format!("# {} = {}  # {}\n", field.name, default_value, description)
        }
        _ => format!("# {}  # {}\n", field.name, description),
    }
}

fn is_compact_doc_value(value: &str) -> bool {
    value.len() <= 80 && !value.contains('{') && !value.contains('\n')
}

fn write_config_contents(
    path: impl AsRef<Path>,
    header_comment: Option<&str>,
    field_docs: Option<&str>,
    toml: &str,
) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating config directory {}", parent.display()))?;
    }

    let mut contents = String::new();
    if let Some(comment) = header_comment {
        contents.push_str(&format!("# {}\n", single_line(comment)));
    }
    if let Some(docs) = field_docs {
        contents.push_str(docs);
        contents.push('\n');
    }
    contents.push_str(toml);

    fs::write(path, contents).with_context(|| format!("writing config to {}", path.display()))?;
    Ok(())
}

/// Serialize a value to TOML string representation
fn to_toml_string<T: Serialize>(value: &T) -> Option<String> {
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
}

/// Trait fallback for all non-Option types
pub trait DocValueFallback {
    fn to_toml(&self) -> Option<String>;
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
    /// When true, the default value is treated as sensitive (password, API
    /// key, etc.) and must never be written into generated documentation. The
    /// field name and description are still documented. This does not redact
    /// or encrypt actual configured values when saving the TOML body.
    pub secret: bool,
}

/// Trait for configs with documented defaults
///
/// This trait is automatically implemented by the `documented_config!` macro.
pub trait DocumentedConfig: Sized + Default + Serialize {
    /// Get metadata for all configuration fields
    fn field_metadata() -> Vec<ConfigFieldMeta>;

    /// Get the TOML-serialized value for a specific field
    fn get_field_value(&self, field_name: &str) -> Option<String>;

    /// Save a documented config using pretty TOML plus a commented field
    /// reference. Keeps nested values readable while still showing every known
    /// field, including fields that are currently unset or skipped by serde
    /// defaults. Secret fields (see [`ConfigFieldMeta::secret`]) are documented
    /// without their default values.
    fn save_documented_pretty_toml(
        &self,
        path: impl AsRef<Path>,
        header_comment: Option<&str>,
    ) -> Result<()> {
        let mut field_docs = String::new();
        field_docs.push_str("# Available fields:\n");
        for field in Self::field_metadata() {
            field_docs.push_str(&documented_field_line(&field));
        }

        let toml = toml::to_string_pretty(self).context("serializing config to toml")?;
        write_config_contents(path, header_comment, Some(&field_docs), &toml)
    }

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
                output.push_str(&documented_field_line(field));
            } else if let Some(value) = current_value {
                output.push_str(&format!(
                    "{} = {}  # {}\n",
                    field.name,
                    value,
                    single_line(field.description)
                ));
            }
        }

        fs::write(path, output).with_context(|| format!("writing config to {}", path.display()))?;
        Ok(())
    }

    /// Load config from string
    fn load_from_str_documented(contents: &str, path: &Path) -> Result<Self>
    where
        Self: DeserializeOwned,
    {
        toml::from_str(contents).with_context(|| format!("parsing config {}", path.display()))
    }

    /// Load config from path, creating initial file if it doesn't exist
    fn load_from_path_documented(path: impl AsRef<Path>) -> Result<Self>
    where
        Self: DeserializeOwned,
    {
        let path = path.as_ref();
        if !path.exists() {
            let config = Self::default();
            config.create_initial_config(path)?;
            return Ok(config);
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading config from {}", path.display()))?;
        Self::load_from_str_documented(&contents, path)
    }
}

/// Macro to generate DocumentedConfig trait implementation
///
/// # Syntax
///
/// ```ignore
/// documented_config!(VideoConfig,
///     music_volume, "Music volume (0.0-1.0)",
///     preprocessor, "Audio preprocessor",
///     auphonic_api_key, "Auphonic API key", secret,
/// );
/// ```
///
/// Append `, secret` after a field's description to mark its default value as
/// sensitive (e.g. passwords, API keys); the field will still be documented,
/// but its default value will not be written to generated documentation. This
/// does not redact or encrypt actual configured values when saving the TOML body.
///
/// Option<T> fields are automatically detected - the inner type's default
/// is used for documentation (e.g., `Option<Vec<String>>` shows `[]`).
///
/// On initial creation, fields with default values are commented out.
#[macro_export]
macro_rules! documented_config {
    // Entry point: tt-munch the field list into accumulated tuples.
    (
        $config_name:ident,
        $($rest:tt)*
    ) => {
        $crate::documented_config!(
            @munch $config_name,
            []
            $($rest)*
        );
    };

    // Munch: secret field. Accumulate ($field, $desc, true).
    // Arm must come before the plain-field arm so the literal `secret` token
    // is consumed here instead of leaking into $rest.
    (@munch
        $config_name:ident,
        [$($acc:tt)*]
        $field:ident, $desc:expr, secret, $($rest:tt)*
    ) => {
        $crate::documented_config!(
            @munch $config_name,
            [$($acc)* ($field, $desc, true)]
            $($rest)*
        );
    };

    // Munch: plain field followed by more. Accumulate ($field, $desc, false).
    (@munch
        $config_name:ident,
        [$($acc:tt)*]
        $field:ident, $desc:expr, $($rest:tt)*
    ) => {
        $crate::documented_config!(
            @munch $config_name,
            [$($acc)* ($field, $desc, false)]
            $($rest)*
        );
    };

    // Munch: terminator reached — emit the impl.
    (@munch
        $config_name:ident,
        [$(($field:ident, $desc:expr, $secret:expr))*]
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
                            secret: $secret,
                        }
                    ),*
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

        }
    };
}
