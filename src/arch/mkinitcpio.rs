use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct MkinitcpioConfig {
    original_content: String,
    hooks: Vec<String>,
    hooks_line_idx: Option<usize>,
    quote_char: Option<char>,
}

impl MkinitcpioConfig {
    pub fn parse(content: &str) -> Result<Self> {
        let mut hooks = Vec::new();
        let mut hooks_line_idx = None;
        let mut quote_char = None;

        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("HOOKS=") {
                hooks_line_idx = Some(idx);

                // Extract content inside quotes/parentheses
                if let Some(start) = line
                    .find('(')
                    .or_else(|| line.find('"'))
                    .or_else(|| line.find('\''))
                {
                    let end_char = match line.chars().nth(start).unwrap() {
                        '(' => ')',
                        '"' => '"',
                        '\'' => '\'',
                        _ => unreachable!(),
                    };

                    if let Some(end) = line.rfind(end_char) {
                        if end > start {
                            let hooks_str = &line[start + 1..end];
                            hooks = hooks_str.split_whitespace().map(String::from).collect();
                            if line.chars().nth(start).unwrap() != '(' {
                                quote_char = Some(line.chars().nth(start).unwrap());
                            }
                        }
                    }
                }
                break;
            }
        }

        Ok(Self {
            original_content: content.to_string(),
            hooks,
            hooks_line_idx,
            quote_char,
        })
    }

    pub fn hooks(&self) -> &[String] {
        &self.hooks
    }

    pub fn hooks_mut(&mut self) -> &mut Vec<String> {
        &mut self.hooks
    }

    pub fn ensure_hook(&mut self, hook: &str) {
        if !self.contains_hook(hook) {
            self.hooks.push(hook.to_string());
        }
    }

    pub fn remove_hook(&mut self, hook: &str) {
        self.hooks.retain(|h| h != hook);
    }

    pub fn replace_hook(&mut self, old: &str, new: &str) {
        for h in self.hooks.iter_mut() {
            if h == old {
                *h = new.to_string();
            }
        }
    }

    pub fn ensure_order(&mut self, before: &str, after: &str) {
        if let (Some(before_idx), Some(after_idx)) = (
            self.hooks.iter().position(|h| h == before),
            self.hooks.iter().position(|h| h == after),
        ) {
            if before_idx > after_idx {
                let removed = self.hooks.remove(before_idx);
                self.hooks.insert(after_idx, removed);
            }
        }
    }

    pub fn insert_after(&mut self, hook: &str, after: &str) {
        if self.contains_hook(hook) {
            self.remove_hook(hook);
        }

        if let Some(idx) = self.hooks.iter().position(|h| h == after) {
            self.hooks.insert(idx + 1, hook.to_string());
        } else {
            self.hooks.push(hook.to_string());
        }
    }

    pub fn contains_hook(&self, hook: &str) -> bool {
        self.hooks.iter().any(|h| h == hook)
    }

    pub fn to_string(&self) -> String {
        let mut lines: Vec<String> = self.original_content.lines().map(String::from).collect();

        if let Some(idx) = self.hooks_line_idx {
            let hooks_str = self.hooks.join(" ");
            let quote = self.quote_char.unwrap_or('"'); // Default to quotes if not parentheses
            // Arch defaults usually use parentheses HOOKS=(...), but sometimes quotes HOOKS="..."
            // We try to preserve what was there, or default to parentheses if it was parentheses (quote_char is None)

            let new_line = if self.quote_char.is_some() {
                format!("HOOKS={}{}{}", quote, hooks_str, quote)
            } else {
                format!("HOOKS=({})", hooks_str)
            };

            lines[idx] = new_line;
        } else {
            // If no HOOKS line found, append one
            lines.push(format!("HOOKS=({})", self.hooks.join(" ")));
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hooks() {
        let content = r#"
# vim:set ft=sh
MODULES=()
BINARIES=()
FILES=()
HOOKS=(base udev autodetect modconf block filesystems keyboard fsck)
"#;
        let config = MkinitcpioConfig::parse(content).unwrap();
        assert_eq!(
            config.hooks(),
            &[
                "base",
                "udev",
                "autodetect",
                "modconf",
                "block",
                "filesystems",
                "keyboard",
                "fsck"
            ]
        );
    }

    #[test]
    fn test_modify_hooks() {
        let content = "HOOKS=(base udev)";
        let mut config = MkinitcpioConfig::parse(content).unwrap();

        config.ensure_hook("plymouth");
        assert!(config.contains_hook("plymouth"));

        config.remove_hook("udev");
        assert!(!config.contains_hook("udev"));

        config.replace_hook("base", "base-systemd");
        assert!(config.contains_hook("base-systemd"));
        assert!(!config.contains_hook("base"));
    }

    #[test]
    fn test_ensure_order() {
        let content = "HOOKS=(c a b)";
        let mut config = MkinitcpioConfig::parse(content).unwrap();

        config.ensure_order("a", "b"); // a is already before b
        assert_eq!(config.hooks(), &["c", "a", "b"]);

        config.ensure_order("b", "c"); // b is after c, should move b before c? No, ensure_order(before, after) means 'before' should be before 'after'.
        // Wait, my implementation:
        // if before_idx > after_idx { remove before, insert at after }
        // c (0), a (1), b (2)
        // ensure_order("b", "c") -> before=b(2), after=c(0). 2 > 0. remove b. insert at 0. -> b, c, a

        config.ensure_order("b", "c");
        assert_eq!(config.hooks(), &["b", "c", "a"]);
    }

    #[test]
    fn test_insert_after() {
        let content = "HOOKS=(base udev)";
        let mut config = MkinitcpioConfig::parse(content).unwrap();

        config.insert_after("plymouth", "base");
        assert_eq!(config.hooks(), &["base", "plymouth", "udev"]);

        config.insert_after("systemd", "base");
        assert_eq!(config.hooks(), &["base", "systemd", "plymouth", "udev"]);
    }

    #[test]
    fn test_serialization() {
        let content = "HOOKS=(base udev)";
        let mut config = MkinitcpioConfig::parse(content).unwrap();
        config.ensure_hook("test");
        assert_eq!(config.to_string(), "HOOKS=(base udev test)");

        let content_quotes = "HOOKS=\"base udev\"";
        let mut config_quotes = MkinitcpioConfig::parse(content_quotes).unwrap();
        config_quotes.ensure_hook("test");
        assert_eq!(config_quotes.to_string(), "HOOKS=\"base udev test\"");
    }
}
