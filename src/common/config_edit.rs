//! Minimal, policy-driven editing of system config files (`/etc/default/grub`,
//! `lightdm.conf`, `os-release`, INI-style files, ...).
//!
//! These files must be edited *in place* as text: comments, unknown keys and
//! formatting have to survive, and a `#key=` commented default should be
//! reactivated rather than duplicated. Serialization-based config crates
//! cannot do this, so the logic lives here — implemented once instead of
//! being re-rolled per call site.
//!
//! All functions are pure text transforms; I/O stays at the call sites so the
//! helpers work equally in the chroot installer (`std::fs`), the async
//! settings runtime (`tokio::fs`) and root-delegated writes (`install -m`).
//!
//! Matching policy (shared by [`set_keys`] and [`set_keys_in_section`]):
//! - Leading whitespace is ignored; optional whitespace around `=` is
//!   tolerated. `key` must be followed by `=`, so `ID` never matches
//!   `ID_LIKE=` and `autologin-user` never matches `autologin-user-timeout=`.
//! - Only `#` starts a comment key (`#key=`).
//! - An active `key=` line wins and is rewritten in place; any further
//!   occurrences (active or commented) are removed so the key stays
//!   unambiguous. Without an active line, the first `#key=` line is
//!   reactivated in place. If the key exists nowhere, it is appended.
//! - Values are written verbatim; callers add quotes where the target file
//!   expects them.
//! - The presence or absence of a trailing newline is preserved.

use anyhow::{Context, Result};

/// Result of a pure config-file transform.
pub struct Edit {
    /// Transformed file content.
    pub content: String,
    /// Whether `content` differs from the input, i.e. whether it needs writing.
    pub changed: bool,
}

/// Set one or more `key=value` pairs in a flat, shell-style config file
/// (`/etc/default/grub`, `/etc/os-release`, `lightdm.conf`, ...).
///
/// See the [module policy](self) for matching rules.
pub fn set_keys(content: &str, keys: &[(&str, &str)]) -> Edit {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    for (key, value) in keys {
        set_key_in_lines(&mut lines, key, value);
    }
    finish(content, lines)
}

/// Set one or more `key=value` pairs inside an INI-style `[section]`.
///
/// The first section whose header trims to exactly `[section]` is targeted;
/// keys outside it are never touched. Missing keys are appended at the end of
/// the section. If the section does not exist, it is created at the end of
/// the file (which also covers empty content).
///
/// See the [module policy](self) for matching rules.
pub fn set_keys_in_section(content: &str, section: &str, keys: &[(&str, &str)]) -> Edit {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let header = format!("[{section}]");

    match lines.iter().position(|line| line.trim() == header) {
        Some(start) => {
            let end = lines[start + 1..]
                .iter()
                .position(|line| line.trim_start().starts_with('['))
                .map_or(lines.len(), |offset| start + 1 + offset);

            let mut section_lines: Vec<String> = lines[start + 1..end].to_vec();
            for (key, value) in keys {
                set_key_in_lines(&mut section_lines, key, value);
            }
            lines.splice(start + 1..end, section_lines);
        }
        None => {
            if let Some(last) = lines.last()
                && !last.trim().is_empty()
            {
                lines.push(String::new());
            }
            lines.push(header);
            for (key, value) in keys {
                lines.push(format!("{key}={value}"));
            }
        }
    }

    finish(content, lines)
}

/// Read a text file, apply a transform and write it back only if the content
/// changed. Returns whether the file was written.
///
/// Errors if the file does not exist; call sites that treat a missing file as
/// a warning should check existence first to keep their own message.
pub fn update_file(path: &str, transform: impl FnOnce(&str) -> Edit) -> Result<bool> {
    let content = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let edit = transform(&content);
    if edit.changed {
        std::fs::write(path, &edit.content).with_context(|| format!("writing {path}"))?;
    }
    Ok(edit.changed)
}

/// Apply the key policy to `lines` in place.
fn set_key_in_lines(lines: &mut Vec<String>, key: &str, value: &str) {
    let matches: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_key_line(line, key))
        .map(|(idx, _)| idx)
        .collect();

    if matches.is_empty() {
        lines.push(format!("{key}={value}"));
        return;
    }

    let anchor = matches
        .iter()
        .copied()
        .find(|&idx| is_active_key_line(&lines[idx], key))
        .unwrap_or(matches[0]);
    lines[anchor] = format!("{key}={value}");

    for &idx in matches.iter().rev() {
        if idx != anchor {
            lines.remove(idx);
        }
    }
}

/// Whether the line is a `key=` or `#key=` line (commented or not).
fn is_key_line(line: &str, key: &str) -> bool {
    let mut trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix('#') {
        trimmed = rest.trim_start();
    }
    matches_key(trimmed, key)
}

/// Whether the line is an active (uncommented) `key=` line.
fn is_active_key_line(line: &str, key: &str) -> bool {
    matches_key(line.trim_start(), key)
}

/// Whether trimmed text starts with `key` immediately followed by `=`
/// (whitespace around `=` allowed).
fn matches_key(trimmed: &str, key: &str) -> bool {
    trimmed
        .strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

fn finish(original: &str, lines: Vec<String>) -> Edit {
    let mut content = lines.join("\n");
    if original.ends_with('\n') {
        content.push('\n');
    }
    Edit {
        changed: content != original,
        content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Flat keys
    // ------------------------------------------------------------------

    #[test]
    fn reactivates_commented_defaults_in_place() {
        // Real lightdm.conf shape: commented defaults at their original spot.
        let input = "\n[Seat:*]\n#autologin-guest=false\n#autologin-user=\n#autologin-user-timeout=0\n#autologin-session=\n";
        let expected = "\n[Seat:*]\n#autologin-guest=false\nautologin-user=testuser\nautologin-user-timeout=0\nautologin-session=sway\n";

        let edit = set_keys(
            input,
            &[
                ("autologin-user", "testuser"),
                ("autologin-user-timeout", "0"),
                ("autologin-session", "sway"),
            ],
        );

        assert!(edit.changed);
        assert_eq!(edit.content, expected);
    }

    #[test]
    fn replaces_active_keys() {
        let input = "autologin-user=olduser\nautologin-user-timeout=5\nautologin-session=hyprland";
        let expected =
            "autologin-user=newuser\nautologin-user-timeout=0\nautologin-session=instantwm";

        let edit = set_keys(
            input,
            &[
                ("autologin-user", "newuser"),
                ("autologin-user-timeout", "0"),
                ("autologin-session", "instantwm"),
            ],
        );

        assert!(edit.changed);
        assert_eq!(edit.content, expected);
    }

    #[test]
    fn rewrites_every_os_release_key_without_cross_matching() {
        let input = "NAME=\"Arch Linux\"\nID=arch\nID_LIKE=arch\nPRETTY_NAME=\"Arch Linux\"\n";
        let expected =
            "NAME=\"instantOS\"\nID=\"instantos\"\nID_LIKE=\"arch\"\nPRETTY_NAME=\"instantOS\"\n";

        let edit = set_keys(
            input,
            &[
                ("NAME", "\"instantOS\""),
                ("ID", "\"instantos\""),
                ("PRETTY_NAME", "\"instantOS\""),
                ("ID_LIKE", "\"arch\""),
            ],
        );

        assert!(edit.changed);
        assert_eq!(edit.content, expected);
    }

    #[test]
    fn appends_keys_that_nowhere_exist() {
        let edit = set_keys(
            "GRUB_TIMEOUT=5\n",
            &[("GRUB_THEME", "\"/usr/share/grub/theme.txt\"")],
        );

        assert!(edit.changed);
        assert_eq!(
            edit.content,
            "GRUB_TIMEOUT=5\nGRUB_THEME=\"/usr/share/grub/theme.txt\"\n"
        );
    }

    #[test]
    fn dedupes_duplicate_occurrences() {
        let edit = set_keys("FOO=1\n#FOO=2\nFOO=3\nBAR=keep\n", &[("FOO", "bar")]);

        assert_eq!(edit.content, "FOO=bar\nBAR=keep\n");
    }

    #[test]
    fn no_op_reports_unchanged() {
        let edit = set_keys("A=1\nB=2\n", &[("A", "1")]);

        assert!(!edit.changed);
        assert_eq!(edit.content, "A=1\nB=2\n");
    }

    #[test]
    fn preserves_trailing_newline_state() {
        let with = set_keys("A=1\n", &[("B", "2")]);
        assert!(with.content.ends_with('\n'));

        let without = set_keys("A=1", &[("B", "2")]);
        assert_eq!(without.content, "A=1\nB=2");
    }

    // ------------------------------------------------------------------
    // Stock-file regressions
    // ------------------------------------------------------------------

    #[test]
    fn uncomments_stock_arch_grub_cryptodisk() {
        // Stock Arch /etc/default/grub ships `#GRUB_ENABLE_CRYPTODISK=y`
        // commented; a substring-based check used to silently no-op here.
        let input = "GRUB_DEFAULT=0\n#GRUB_ENABLE_CRYPTODISK=y\nGRUB_TIMEOUT=5\n";

        let edit = set_keys(input, &[("GRUB_ENABLE_CRYPTODISK", "y")]);

        assert!(edit.changed);
        assert_eq!(
            edit.content,
            "GRUB_DEFAULT=0\nGRUB_ENABLE_CRYPTODISK=y\nGRUB_TIMEOUT=5\n"
        );
    }

    #[test]
    fn tolerates_whitespace_around_equals() {
        let input = "#AutomaticLoginEnable = true\n#AutomaticLogin = user1\n";

        let edit = set_keys(
            input,
            &[("AutomaticLoginEnable", "true"), ("AutomaticLogin", "bob")],
        );

        assert_eq!(
            edit.content,
            "AutomaticLoginEnable=true\nAutomaticLogin=bob\n"
        );
    }

    #[test]
    fn does_not_match_longer_keys() {
        // gtk-theme must not clobber gtk-theme-name (prefix-matching bug).
        let input = "[Settings]\ngtk-theme-name=Adwaita\n";

        let edit = set_keys_in_section(input, "Settings", &[("gtk-theme", "dark")]);

        assert_eq!(
            edit.content,
            "[Settings]\ngtk-theme-name=Adwaita\ngtk-theme=dark\n"
        );
    }

    // ------------------------------------------------------------------
    // INI sections
    // ------------------------------------------------------------------

    #[test]
    fn replaces_keys_within_their_section_only() {
        let input = "\n[daemon]\n#WaylandEnable=false\n#AutomaticLoginEnable = true\n#AutomaticLogin = user1\n\n[security]\n";
        let expected = "\n[daemon]\n#WaylandEnable=false\nAutomaticLoginEnable=true\nAutomaticLogin=testuser\n\n[security]\n";

        let edit = set_keys_in_section(
            input,
            "daemon",
            &[
                ("AutomaticLoginEnable", "true"),
                ("AutomaticLogin", "testuser"),
            ],
        );

        assert!(edit.changed);
        assert_eq!(edit.content, expected);
    }

    #[test]
    fn inserts_missing_keys_at_end_of_existing_section() {
        // Real default /etc/gdm/custom.conf: active section headers, no keys.
        // Missing keys are appended at the end of the section (matches the
        // historical GDM behavior of inserting before the next header).
        let input =
            "# GDM configuration storage\n\n[daemon]\n\n[security]\n\n[debug]\n#Enable=true\n";
        let expected = "# GDM configuration storage\n\n[daemon]\n\nAutomaticLoginEnable=true\nAutomaticLogin=testuser\n[security]\n\n[debug]\n#Enable=true\n";

        let edit = set_keys_in_section(
            input,
            "daemon",
            &[
                ("AutomaticLoginEnable", "true"),
                ("AutomaticLogin", "testuser"),
            ],
        );

        assert_eq!(edit.content, expected);
    }

    #[test]
    fn creates_missing_section_in_empty_file() {
        // Fresh AccountsService user file.
        let edit = set_keys_in_section("", "User", &[("Session", "sway")]);

        assert!(edit.changed);
        assert_eq!(edit.content, "[User]\nSession=sway");
    }

    #[test]
    fn creates_missing_section_at_end_of_file() {
        let edit = set_keys_in_section("[Other]\nKey=1\n", "User", &[("Session", "sway")]);

        assert!(edit.changed);
        assert_eq!(edit.content, "[Other]\nKey=1\n\n[User]\nSession=sway\n");
    }

    #[test]
    fn appends_within_section_but_not_beyond_it() {
        let input =
            "[User]\nSystemAccount=false\nSession=old\n\n[PreExisting]\nSession=unrelated\n";

        let edit = set_keys_in_section(input, "User", &[("Session", "sway")]);

        assert_eq!(
            edit.content,
            "[User]\nSystemAccount=false\nSession=sway\n\n[PreExisting]\nSession=unrelated\n"
        );
    }

    #[test]
    fn section_keys_do_not_leak_across_sections() {
        // Setting Session in [User] must not touch Session elsewhere, and a
        // Session key before the section must stay untouched.
        let input = "[NotUser]\nSession=before\n\n[User]\nSession=inside\n";

        let edit = set_keys_in_section(input, "User", &[("Session", "new")]);

        assert_eq!(
            edit.content,
            "[NotUser]\nSession=before\n\n[User]\nSession=new\n"
        );
    }

    #[test]
    fn lightdm_defaults_only_change_the_wildcard_seat() {
        let input = "[Seat:*]\n#user-session=\n#autologin-user=\n\n[Seat:seat0]\nuser-session=custom\nautologin-user=alice\n";

        let edit = set_keys_in_section(
            input,
            "Seat:*",
            &[("user-session", "sway"), ("autologin-user", "bob")],
        );

        assert_eq!(
            edit.content,
            "[Seat:*]\nuser-session=sway\nautologin-user=bob\n\n[Seat:seat0]\nuser-session=custom\nautologin-user=alice\n"
        );
    }
}
