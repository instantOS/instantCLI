//! Typed screen region shared by every area-selection consumer.
//!
//! Converts slurp's `X,Y WxH` and slop's `WxH+X+Y` formats at tool boundaries.

use anyhow::{Context, Result};

/// A screen region in layout (logical) pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Geometry {
    /// Parse the `X,Y WxH` format emitted by `slurp`, and also accepted by
    /// `grim -g` and `wf-recorder -g`.
    pub fn parse_slurp(raw: &str) -> Result<Self> {
        let (pos, size) = raw
            .trim()
            .split_once(char::is_whitespace)
            .with_context(|| format!("Invalid slurp geometry: {raw:?}"))?;

        let (x, y) = pos
            .split_once(',')
            .with_context(|| format!("Invalid slurp geometry: {raw:?}"))?;

        Self::from_parts(x, y, size, raw)
    }

    /// Parse the `WxH+X+Y` format emitted by `slop`'s `%g`.
    pub fn parse_x11(raw: &str) -> Result<Self> {
        let raw = raw.trim();

        // Slice rather than `split_once`: splitting on `+`/`-` would consume the
        // sign of the first offset, turning `-100-200` into `[100, -200]` and
        // silently capturing the wrong region of the screen.
        let offset_start = raw
            .find(['+', '-'])
            .with_context(|| format!("Invalid X11 geometry: {raw:?}"))?;

        let coords = split_signed(&raw[offset_start..]);
        anyhow::ensure!(
            coords.len() == 2,
            "Invalid X11 geometry: {raw:?} (expected two signed offsets)"
        );

        Self::from_parts(
            &coords[0].to_string(),
            &coords[1].to_string(),
            &raw[..offset_start],
            raw,
        )
    }

    /// Render as `X,Y WxH`: what `grim -g` and `wf-recorder -g` expect.
    pub fn to_slurp(self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.width, self.height)
    }

    /// Render as `WxH+X+Y` for `feh --geometry` and ImageMagick `import -crop`.
    pub fn to_x11(self) -> String {
        // `{:+}` emits the sign itself, so the separators must not add another.
        format!("{}x{}{:+}{:+}", self.width, self.height, self.x, self.y)
    }

    fn from_parts(x: &str, y: &str, size: &str, raw: &str) -> Result<Self> {
        let (width, height) = size
            .split_once('x')
            .with_context(|| format!("Invalid geometry size in {raw:?}"))?;

        let geometry = Self {
            x: x.trim()
                .parse()
                .with_context(|| format!("Invalid x offset in {raw:?}"))?,
            y: y.trim()
                .parse()
                .with_context(|| format!("Invalid y offset in {raw:?}"))?,
            width: width
                .trim()
                .parse()
                .with_context(|| format!("Invalid width in {raw:?}"))?,
            height: height
                .trim()
                .parse()
                .with_context(|| format!("Invalid height in {raw:?}"))?,
        };

        anyhow::ensure!(
            geometry.width > 0 && geometry.height > 0,
            "Selected area must be at least 1x1, got {}x{}",
            geometry.width,
            geometry.height
        );

        Ok(geometry)
    }
}

/// Split a run of signed integers such as `+100-200` into `100` and `-200`.
fn split_signed(raw: &str) -> Vec<i32> {
    let chars: Vec<char> = raw.chars().collect();
    let mut values = Vec::new();
    let mut idx = 0;

    while idx < chars.len() {
        let mut token = String::new();

        if chars[idx] == '+' || chars[idx] == '-' {
            token.push(chars[idx]);
            idx += 1;
        }

        while idx < chars.len() && chars[idx].is_ascii_digit() {
            token.push(chars[idx]);
            idx += 1;
        }

        match token.parse::<i32>() {
            Ok(value) => values.push(value),
            Err(_) => return Vec::new(),
        }
    }

    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slurp_geometry() {
        let geom = Geometry::parse_slurp("100,200 300x400").unwrap();
        assert_eq!(
            geom,
            Geometry {
                x: 100,
                y: 200,
                width: 300,
                height: 400
            }
        );
    }

    #[test]
    fn parses_slurp_geometry_with_odd_dimensions() {
        // This is the shape that used to produce a 0-byte mp4.
        let geom = Geometry::parse_slurp("0,0 1256x817").unwrap();
        assert_eq!(geom.width, 1256);
        assert_eq!(geom.height, 817);
    }

    #[test]
    fn parses_x11_geometry() {
        let geom = Geometry::parse_x11("300x400+100+200").unwrap();
        assert_eq!(
            geom,
            Geometry {
                x: 100,
                y: 200,
                width: 300,
                height: 400
            }
        );
    }

    #[test]
    fn parses_x11_geometry_with_negative_offsets() {
        let geom = Geometry::parse_x11("300x400-100-200").unwrap();
        assert_eq!(
            geom,
            Geometry {
                x: -100,
                y: -200,
                width: 300,
                height: 400
            }
        );

        let mixed = Geometry::parse_x11("64x64+1280-64").unwrap();
        assert_eq!(
            mixed,
            Geometry {
                x: 1280,
                y: -64,
                width: 64,
                height: 64
            }
        );
    }

    #[test]
    fn round_trips_through_both_formats() {
        let original = Geometry {
            x: 7,
            y: 13,
            width: 301,
            height: 403,
        };

        let via_slurp = Geometry::parse_slurp(&original.to_slurp()).unwrap();
        let via_x11 = Geometry::parse_x11(&original.to_x11()).unwrap();

        assert_eq!(via_slurp, original);
        assert_eq!(via_x11, original);
    }

    #[test]
    fn renders_slurp_format() {
        let geom = Geometry {
            x: 100,
            y: 200,
            width: 300,
            height: 400,
        };
        assert_eq!(geom.to_slurp(), "100,200 300x400");
    }

    #[test]
    fn renders_x11_format_with_explicit_signs() {
        let geom = Geometry {
            x: -100,
            y: 200,
            width: 300,
            height: 400,
        };
        assert_eq!(geom.to_x11(), "300x400-100+200");
    }

    #[test]
    fn rejects_empty_regions() {
        assert!(Geometry::parse_slurp("0,0 0x400").is_err());
        assert!(Geometry::parse_slurp("0,0 400x0").is_err());
        assert!(Geometry::parse_x11("0x400+0+0").is_err());
    }

    #[test]
    fn rejects_malformed_geometry() {
        assert!(Geometry::parse_slurp("").is_err());
        assert!(Geometry::parse_slurp("100,200").is_err());
        assert!(Geometry::parse_slurp("100;200 300x400").is_err());
        assert!(Geometry::parse_slurp("a,b 300x400").is_err());
        assert!(Geometry::parse_x11("300x400").is_err());
        assert!(Geometry::parse_x11("300x400+100").is_err());
        assert!(Geometry::parse_x11("300x400+100+200garbage").is_err());
        assert!(Geometry::parse_x11("wide x tall +1+2").is_err());
    }

    #[test]
    fn split_signed_handles_mixed_signs() {
        assert_eq!(split_signed("+100+200"), vec![100, 200]);
        assert_eq!(split_signed("-100-200"), vec![-100, -200]);
        assert_eq!(split_signed("+100-200"), vec![100, -200]);
        assert_eq!(split_signed("-100+200"), vec![-100, 200]);
        assert_eq!(split_signed("0+0"), vec![0, 0]);
        assert_eq!(split_signed(""), Vec::<i32>::new());
    }

    #[test]
    fn negative_offsets_keep_their_sign() {
        // Regression: splitting on the first `+`/`-` used to consume the sign of
        // the x offset, so a selection at negative coordinates resolved to the
        // mirrored, positive region instead.
        for (raw, expected) in [
            (
                "300x400-100-200",
                Geometry {
                    x: -100,
                    y: -200,
                    width: 300,
                    height: 400,
                },
            ),
            (
                "300x400-100+200",
                Geometry {
                    x: -100,
                    y: 200,
                    width: 300,
                    height: 400,
                },
            ),
            (
                "300x400+100-200",
                Geometry {
                    x: 100,
                    y: -200,
                    width: 300,
                    height: 400,
                },
            ),
        ] {
            assert_eq!(Geometry::parse_x11(raw).unwrap(), expected, "for {raw:?}");
        }
    }

    #[test]
    fn x11_geometry_round_trips_with_negative_offsets() {
        for geom in [
            Geometry {
                x: -100,
                y: -200,
                width: 300,
                height: 400,
            },
            Geometry {
                x: -1,
                y: 1,
                width: 2,
                height: 2,
            },
        ] {
            assert_eq!(Geometry::parse_x11(&geom.to_x11()).unwrap(), geom);
        }
    }
}
