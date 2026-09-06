//! Fast, focused parser for the `[Desktop Entry]` group.
//!
//! Launch discovery only needs a small subset of desktop-entry keys. Parsing
//! actions, translations, and unrelated metadata made launcher startup scale
//! with all content in every file rather than just the fields we consume.

#[derive(Debug, Default)]
pub(super) struct DesktopEntry<'a> {
    pub(super) entry_type: Option<&'a str>,
    pub(super) name: Option<&'a str>,
    pub(super) exec: Option<&'a str>,
    pub(super) try_exec: Option<&'a str>,
    pub(super) icon: Option<&'a str>,
    pub(super) only_show_in: Option<&'a str>,
    pub(super) not_show_in: Option<&'a str>,
    pub(super) hidden: bool,
    pub(super) no_display: bool,
    pub(super) terminal: bool,
}

impl<'a> DesktopEntry<'a> {
    pub(super) fn parse(content: &'a str) -> Option<Self> {
        let mut entry = Self::default();
        let mut in_desktop_entry = false;

        for raw_line in content.lines() {
            let line = raw_line.trim_end();
            if line.is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                if in_desktop_entry {
                    break;
                }
                in_desktop_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_desktop_entry {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "Type" => entry.entry_type = Some(value),
                "Name" => entry.name = Some(value),
                "Exec" => entry.exec = Some(value),
                "TryExec" => entry.try_exec = Some(value),
                "Icon" => entry.icon = Some(value),
                "OnlyShowIn" => entry.only_show_in = Some(value),
                "NotShowIn" => entry.not_show_in = Some(value),
                "Hidden" => entry.hidden = parse_bool(value),
                "NoDisplay" => entry.no_display = parse_bool(value),
                "Terminal" => entry.terminal = parse_bool(value),
                _ => {}
            }
        }

        in_desktop_entry.then_some(entry)
    }
}

fn parse_bool(value: &str) -> bool {
    value.eq_ignore_ascii_case("true")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_main_group_and_needed_keys() {
        let content = "# comment\n[Desktop Entry]\nType=Application\nName=Example\nExec=example --flag\nNoDisplay=TRUE\nName[de]=Beispiel\n[Desktop Action New]\nName=Wrong\nExec=wrong\n";
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.entry_type, Some("Application"));
        assert_eq!(entry.name, Some("Example"));
        assert_eq!(entry.exec, Some("example --flag"));
        assert!(entry.no_display);
    }

    #[test]
    fn ignores_keys_before_the_desktop_entry_group() {
        let content = "Name=Wrong\n[Desktop Entry]\nType=Application\nName=Right\nExec=right\n";
        let entry = DesktopEntry::parse(content).unwrap();
        assert_eq!(entry.name, Some("Right"));
    }

    #[test]
    fn rejects_files_without_a_desktop_entry_group() {
        assert!(DesktopEntry::parse("[Desktop Action Foo]\nExec=foo\n").is_none());
    }
}
