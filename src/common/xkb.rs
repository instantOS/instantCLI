//! Pure XKB layout/variant helpers shared by settings, previews, and
//! compositor backends.
//!
//! Stored layout codes use XKB's `layout(variant)` spelling, e.g.
//! `de(nodeadkeys)`. This is the format `instantwmctl keyboard set` already
//! parses, and it survives the comma-joined settings values unchanged.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Path of the XKB rules index listing layouts, variants, and options.
pub const XKB_RULES_LIST: &str = "/usr/share/X11/xkb/rules/evdev.lst";

/// Resolve the path to the XKB rules list (`evdev.lst`).
///
/// Respects `$XKB_CONFIG_ROOT` if set, otherwise checks standard system paths
/// and NixOS fallback locations before defaulting to [`XKB_RULES_LIST`].
pub fn xkb_rules_path() -> PathBuf {
    if let Some(root) = std::env::var_os("XKB_CONFIG_ROOT") {
        let path = Path::new(&root).join("rules/evdev.lst");
        if path.exists() {
            return path;
        }
    }

    let default = Path::new(XKB_RULES_LIST);
    if default.exists() {
        return default.to_path_buf();
    }

    for candidate in [
        "/run/current-system/sw/share/X11/xkb/rules/evdev.lst",
        "/etc/xkb/rules/evdev.lst",
        "/usr/local/share/X11/xkb/rules/evdev.lst",
    ] {
        let path = Path::new(candidate);
        if path.exists() {
            return path.to_path_buf();
        }
    }

    default.to_path_buf()
}

/// Split a stored layout code into its base layout and optional variant.
///
/// `de(nodeadkeys)` splits into `("de", Some("nodeadkeys"))`; a bare `de`
/// into `("de", None)`. Malformed variants (empty halves) are treated as
/// "no variant" so they never reach XKB tooling.
pub fn split_layout_variant(code: &str) -> (&str, Option<&str>) {
    let Some(inner) = code.strip_suffix(')') else {
        return (code, None);
    };
    match inner.rsplit_once('(') {
        Some((layout, variant)) if !layout.is_empty() && !variant.is_empty() => {
            (layout, Some(variant))
        }
        _ => (code, None),
    }
}

/// The base layout of a stored code, ignoring any variant.
pub fn base_layout(code: &str) -> &str {
    split_layout_variant(code).0
}

/// Comma-joined XKB variant list positionally matching `codes`, or `None`
/// when no code carries a variant.
///
/// Missing variants become empty segments, matching how setxkbmap, sway, and
/// niri expect positional variant lists: `[us, de(nodeadkeys)]` becomes
/// `,nodeadkeys`.
pub fn positional_variants(codes: &[String]) -> Option<String> {
    let variants: Vec<Option<&str>> = codes
        .iter()
        .map(|code| split_layout_variant(code).1)
        .collect();
    if variants.iter().all(Option::is_none) {
        return None;
    }
    Some(
        variants
            .iter()
            .map(|variant| variant.unwrap_or_default())
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// Merge XKB layout and variant lists into stored `layout(variant)` codes.
///
/// Both inputs are positional comma lists as reported by `setxkbmap -query`
/// or niri/sway configuration; a missing or empty variant entry leaves the
/// layout bare.
pub fn merge_layout_variants(layouts: &[String], variants: &[String]) -> Vec<String> {
    let mut codes = Vec::new();
    for (index, layout) in layouts.iter().enumerate() {
        if layout.is_empty() {
            continue;
        }
        match variants.get(index).map(String::as_str) {
            Some(variant) if !variant.is_empty() => codes.push(format!("{layout}({variant})")),
            _ => codes.push(layout.clone()),
        }
    }
    codes
}

/// One row of the `! variant` section of `evdev.lst`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XkbVariant {
    pub layout: String,
    pub code: String,
    pub name: String,
}

/// Parse the `! variant` section of an `evdev.lst`-style rules index into
/// variants grouped by layout code.
///
/// Variant rows look like `nodeadkeys de: German (nodeadkeys)` — the variant
/// code, the layout it belongs to, and a human-readable description. Rows
/// before the section and in later `!` sections are ignored.
pub fn parse_xkb_variant_lines<'a, I>(lines: I) -> BTreeMap<String, Vec<XkbVariant>>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut variants: BTreeMap<String, Vec<XkbVariant>> = BTreeMap::new();
    let mut in_variant_section = false;

    for line in lines {
        let trimmed = line.trim();

        if trimmed == "! variant" {
            in_variant_section = true;
            continue;
        }

        if in_variant_section && trimmed.starts_with('!') {
            break;
        }

        if !in_variant_section || trimmed.is_empty() {
            continue;
        }

        let Some((code, remainder)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let Some((layout, description)) = remainder.trim().split_once(':') else {
            continue;
        };
        let layout = layout.trim();
        if layout.is_empty() {
            continue;
        }
        let name = description.trim();
        let name = if name.is_empty() {
            code.to_string()
        } else {
            name.to_string()
        };

        variants
            .entry(layout.to_string())
            .or_default()
            .push(XkbVariant {
                layout: layout.to_string(),
                code: code.to_string(),
                name,
            });
    }

    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_layout_variant_handles_all_shapes() {
        assert_eq!(split_layout_variant("us"), ("us", None));
        assert_eq!(
            split_layout_variant("de(nodeadkeys)"),
            ("de", Some("nodeadkeys"))
        );
        assert_eq!(split_layout_variant(""), ("", None));
        // Malformed halves fall back to the bare code.
        assert_eq!(split_layout_variant("de()"), ("de()", None));
        assert_eq!(split_layout_variant("(nodeadkeys)"), ("(nodeadkeys)", None));
    }

    #[test]
    fn positional_variants_only_render_when_a_variant_exists() {
        let none = vec!["us".to_string(), "de".to_string()];
        assert_eq!(positional_variants(&none), None);

        let mixed = vec!["us".to_string(), "de(nodeadkeys)".to_string()];
        assert_eq!(positional_variants(&mixed).as_deref(), Some(",nodeadkeys"));

        let all = vec!["fr(azerty)".to_string(), "de(nodeadkeys)".to_string()];
        assert_eq!(
            positional_variants(&all).as_deref(),
            Some("azerty,nodeadkeys")
        );
    }

    #[test]
    fn merge_layout_variants_is_positional_and_skips_empty_layouts() {
        let layouts = vec![
            "us".to_string(),
            "de".to_string(),
            String::new(),
            "fr".to_string(),
        ];
        let variants = vec![
            String::new(),
            "nodeadkeys".to_string(),
            "ignored".to_string(),
        ];
        assert_eq!(
            merge_layout_variants(&layouts, &variants),
            vec![
                "us".to_string(),
                "de(nodeadkeys)".to_string(),
                "fr".to_string(),
            ]
        );
    }

    #[test]
    fn variant_lines_group_by_layout_and_stop_at_next_section() {
        let lines = [
            "! model",
            "  pc105 pc105",
            "! layout",
            "  de German",
            "! variant",
            "  nodeadkeys de: German (nodeadkeys)",
            "  T3 de: German (T3)",
            "  azerty fr: French (Azerty)",
            "",
            "  brokenline",
            "! option",
            "  grp:alt_shift_toggle: Alt+Shift",
        ];
        let variants = parse_xkb_variant_lines(lines);

        let german = variants.get("de").expect("german variants");
        assert_eq!(german.len(), 2);
        assert_eq!(german[0].code, "nodeadkeys");
        assert_eq!(german[0].name, "German (nodeadkeys)");
        assert_eq!(german[1].code, "T3");

        let french = variants.get("fr").expect("french variants");
        assert_eq!(french.len(), 1);

        assert!(!variants.contains_key("alt_shift_toggle"));
    }

    #[test]
    fn variant_rows_without_description_fall_back_to_the_code() {
        let lines = ["! variant", "  nodeadkeys de:"];
        let variants = parse_xkb_variant_lines(lines);
        assert_eq!(variants["de"][0].name, "nodeadkeys");
    }
}
