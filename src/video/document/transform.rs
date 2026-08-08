//! Transform parameters parsed from markdown b-roll references.

use anyhow::{Context, Result, bail};

/// Semantic screen position for an overlay or b-roll clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Center,
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

/// Transform specification parsed from the `|` parameters after a timestamp.
///
/// Carried through the document and planning layers, then converted to a
/// render-layer [`Transform`](crate::video::render::timeline::Transform) at
/// timeline-build time.
#[derive(Debug, Clone)]
pub struct TransformSpec {
    pub scale: Option<f32>,
    pub position: Option<Position>,
}

impl TransformSpec {
    pub fn empty() -> Self {
        Self {
            scale: None,
            position: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.scale.is_none() && self.position.is_none()
    }
}

/// Named presets covering the most common overlay arrangements.
struct Preset {
    name: &'static str,
    scale: Option<f32>,
    position: Option<Position>,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "full",
        scale: Some(1.0),
        position: Some(Position::Center),
    },
    Preset {
        name: "pip",
        scale: Some(0.3),
        position: Some(Position::TopRight),
    },
    Preset {
        name: "side",
        scale: Some(0.5),
        position: Some(Position::Right),
    },
];

/// Parse `|`-delimited transform parameters from the suffix of a timestamp
/// reference (everything after the first `|`).
///
/// Parameters are applied left to right. A bare word is treated as a preset
/// name; `key=value` pairs override individual fields.
pub fn parse_transform_params(params: &str) -> Result<TransformSpec> {
    let mut spec = TransformSpec::empty();

    for raw in params.split('|') {
        let param = raw.trim();
        if param.is_empty() {
            continue;
        }

        // Preset lookup (bare word).
        if !param.contains('=') {
            let preset = PRESETS
                .iter()
                .find(|p| p.name == param)
                .ok_or_else(|| anyhow::anyhow!("Unknown transform preset: '{param}'"))?;
            spec.scale = preset.scale.or(spec.scale);
            spec.position = preset.position.or(spec.position);
            continue;
        }

        // key=value pair.
        let (key, value) = param
            .split_once('=')
            .with_context(|| format!("Invalid transform parameter: '{param}'"))?;

        match key.trim() {
            "scale" => {
                spec.scale = Some(
                    value
                        .trim()
                        .parse::<f32>()
                        .with_context(|| format!("Invalid scale value: '{value}'"))?,
                );
            }
            "pos" | "position" => {
                spec.position = Some(parse_position(value.trim())?);
            }
            _ => bail!("Unknown transform parameter: '{key}'"),
        }
    }

    Ok(spec)
}

fn parse_position(s: &str) -> Result<Position> {
    match s.to_lowercase().as_str() {
        "center" | "middle" => Ok(Position::Center),
        "top-left" | "topleft" => Ok(Position::TopLeft),
        "top" => Ok(Position::Top),
        "top-right" | "topright" => Ok(Position::TopRight),
        "right" => Ok(Position::Right),
        "bottom-right" | "bottomright" => Ok(Position::BottomRight),
        "bottom" => Ok(Position::Bottom),
        "bottom-left" | "bottomleft" => Ok(Position::BottomLeft),
        "left" => Ok(Position::Left),
        _ => bail!("Unknown position: '{s}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_preset() {
        let spec = parse_transform_params("pip").unwrap();
        assert_eq!(spec.scale, Some(0.3));
        assert_eq!(spec.position, Some(Position::TopRight));
    }

    #[test]
    fn parses_explicit_values() {
        let spec = parse_transform_params("scale=0.6|pos=bottom-right").unwrap();
        assert_eq!(spec.scale, Some(0.6));
        assert_eq!(spec.position, Some(Position::BottomRight));
    }

    #[test]
    fn preset_then_override() {
        let spec = parse_transform_params("pip|scale=0.5").unwrap();
        assert_eq!(spec.scale, Some(0.5));
        assert_eq!(spec.position, Some(Position::TopRight));
    }

    #[test]
    fn empty_string_is_empty_spec() {
        let spec = parse_transform_params("").unwrap();
        assert!(spec.is_empty());
    }

    #[test]
    fn unknown_preset_errors() {
        assert!(parse_transform_params("banana").is_err());
    }

    #[test]
    fn unknown_key_errors() {
        assert!(parse_transform_params("blur=5").is_err());
    }
}
