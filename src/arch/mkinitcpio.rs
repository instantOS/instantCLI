use anyhow::Result;

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

                    if let Some(end) = line.rfind(end_char)
                        && end > start
                    {
                        let hooks_str = &line[start + 1..end];
                        hooks = hooks_str.split_whitespace().map(String::from).collect();
                        if line.chars().nth(start).unwrap() != '(' {
                            quote_char = Some(line.chars().nth(start).unwrap());
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

    pub fn contains_hook(&self, hook: &str) -> bool {
        self.hooks.iter().any(|h| h == hook)
    }

    fn hook_index(&self, hook: &str) -> Option<usize> {
        self.hooks.iter().position(|h| h == hook)
    }

    fn max_index_of(&self, hooks: &[&str]) -> Option<usize> {
        hooks
            .iter()
            .filter_map(|hook| self.hooks.iter().position(|h| h == *hook))
            .max()
    }

    fn min_index_of(&self, hooks: &[&str]) -> Option<usize> {
        hooks
            .iter()
            .filter_map(|hook| self.hooks.iter().position(|h| h == *hook))
            .min()
    }

    fn insertion_bounds(&self, after: &[&str], before: &[&str]) -> (usize, usize) {
        let min_idx = after
            .iter()
            .filter_map(|hook| self.hooks.iter().position(|h| h == *hook))
            .map(|idx| idx + 1)
            .max()
            .unwrap_or(0);

        let max_idx = before
            .iter()
            .filter_map(|hook| self.hooks.iter().position(|h| h == *hook))
            .min()
            .unwrap_or(self.hooks.len());

        (min_idx, max_idx)
    }

    fn target_for_after_violation(&self, after: &[&str], before: &[&str]) -> usize {
        let (min_idx, max_idx) = self.insertion_bounds(after, before);
        if min_idx > max_idx { min_idx } else { max_idx }
    }

    fn target_for_before_violation(&self, after: &[&str], before: &[&str]) -> usize {
        let (min_idx, max_idx) = self.insertion_bounds(after, before);
        if max_idx < min_idx { max_idx } else { min_idx }
    }

    pub fn ensure_hook_position(&mut self, hook: &str, after: &[&str], before: &[&str]) {
        self.ensure_hook(hook);

        let mut changed = true;
        for _ in 0..10 {
            if !changed {
                break;
            }
            changed = false;

            let Some(current_idx) = self.hook_index(hook) else {
                break;
            };

            if let Some(max_after_idx) = self.max_index_of(after)
                && max_after_idx >= current_idx
            {
                let removed = self.hooks.remove(current_idx);
                let target = self.target_for_after_violation(after, before);
                self.hooks.insert(target, removed);
                changed = true;
                continue;
            }

            if let Some(min_before_idx) = self.min_index_of(before)
                && min_before_idx <= current_idx
            {
                let removed = self.hooks.remove(current_idx);
                let target = self.target_for_before_violation(after, before);
                self.hooks.insert(target, removed);
                changed = true;
            }
        }
    }

    pub fn to_string(&self) -> String {
        let mut lines: Vec<String> = self.original_content.lines().map(String::from).collect();

        if let Some(idx) = self.hooks_line_idx {
            let hooks_str = self.hooks.join(" ");

            // Preserve original quote style: parentheses HOOKS=(...) or quotes HOOKS="..."
            let new_line = if let Some(quote) = self.quote_char {
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

    // ── Parse ───────────────────────────────────────────────────────────

    #[test]
    fn test_parse_parentheses() {
        let config = MkinitcpioConfig::parse("HOOKS=(base udev)").unwrap();
        assert_eq!(config.hooks, vec!["base", "udev"]);
    }

    #[test]
    fn test_parse_double_quotes() {
        let config = MkinitcpioConfig::parse("HOOKS=\"base udev\"").unwrap();
        assert_eq!(config.hooks, vec!["base", "udev"]);
    }

    #[test]
    fn test_parse_single_quotes() {
        let config = MkinitcpioConfig::parse("HOOKS='base udev'").unwrap();
        assert_eq!(config.hooks, vec!["base", "udev"]);
    }

    #[test]
    fn test_parse_no_hooks_line() {
        let config = MkinitcpioConfig::parse("# no hooks here").unwrap();
        assert!(config.hooks.is_empty());
        assert!(config.hooks_line_idx.is_none());
    }

    #[test]
    fn test_parse_multiline_content() {
        let content =
            "MODULES=()\nBINARIES=()\nFILES=()\nHOOKS=(base udev block encrypt filesystems)\n";
        let config = MkinitcpioConfig::parse(content).unwrap();
        assert_eq!(
            config.hooks,
            vec!["base", "udev", "block", "encrypt", "filesystems"]
        );
        assert_eq!(config.hooks_line_idx, Some(3));
    }

    // ── Serialization ───────────────────────────────────────────────────

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

    #[test]
    fn test_to_string_preserves_surrounding_lines() {
        let content = "MODULES=()\nHOOKS=(base udev)\nCOMPRESSION=zstd";
        let mut config = MkinitcpioConfig::parse(content).unwrap();
        config.ensure_hook("keyboard");
        let result = config.to_string();
        assert!(result.contains("MODULES=()"));
        assert!(result.contains("HOOKS=(base udev keyboard)"));
        assert!(result.contains("COMPRESSION=zstd"));
    }

    #[test]
    fn test_to_string_appends_when_no_hooks_line() {
        let content = "MODULES=()";
        let mut config = MkinitcpioConfig::parse(content).unwrap();
        config.ensure_hook("base");
        let result = config.to_string();
        assert!(result.contains("HOOKS=(base)"));
    }

    #[test]
    fn test_preserves_quote_style() {
        let content = "HOOKS='base udev'";
        let mut config = MkinitcpioConfig::parse(content).unwrap();
        config.ensure_hook("keyboard");
        assert_eq!(config.to_string(), "HOOKS='base udev keyboard'");
    }

    // ── ensure_hook ─────────────────────────────────────────────────────

    #[test]
    fn test_ensure_hook_adds_new() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base)").unwrap();
        config.ensure_hook("udev");
        assert_eq!(config.hooks, vec!["base", "udev"]);
    }

    #[test]
    fn test_ensure_hook_idempotent() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev)").unwrap();
        config.ensure_hook("udev");
        assert_eq!(config.hooks, vec!["base", "udev"]);
    }

    // ── remove_hook ─────────────────────────────────────────────────────

    #[test]
    fn test_remove_hook_existing() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev block)").unwrap();
        config.remove_hook("udev");
        assert_eq!(config.hooks, vec!["base", "block"]);
    }

    #[test]
    fn test_remove_hook_nonexistent_is_noop() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev)").unwrap();
        config.remove_hook("filesystems");
        assert_eq!(config.hooks, vec!["base", "udev"]);
    }

    // ── replace_hook ────────────────────────────────────────────────────

    #[test]
    fn test_replace_hook_existing() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev)").unwrap();
        config.replace_hook("udev", "systemd");
        assert_eq!(config.hooks, vec!["base", "systemd"]);
    }

    #[test]
    fn test_replace_hook_nonexistent_is_noop() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base)").unwrap();
        config.replace_hook("udev", "systemd");
        assert_eq!(config.hooks, vec!["base"]);
    }

    #[test]
    fn test_replace_hook_replaces_all_occurrences() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev base)").unwrap();
        config.replace_hook("base", "core");
        assert_eq!(config.hooks, vec!["core", "udev", "core"]);
    }

    // ── contains_hook ───────────────────────────────────────────────────

    #[test]
    fn test_contains_hook_true() {
        let config = MkinitcpioConfig::parse("HOOKS=(base udev)").unwrap();
        assert!(config.contains_hook("base"));
        assert!(config.contains_hook("udev"));
    }

    #[test]
    fn test_contains_hook_false() {
        let config = MkinitcpioConfig::parse("HOOKS=(base udev)").unwrap();
        assert!(!config.contains_hook("filesystems"));
    }

    // ── ensure_hook_position ────────────────────────────────────────────

    #[test]
    fn test_ensure_hook_position_already_correct() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev block filesystems)").unwrap();
        config.ensure_hook_position("block", &["udev"], &["filesystems"]);
        assert_eq!(config.hooks, vec!["base", "udev", "block", "filesystems"]);
    }

    #[test]
    fn test_ensure_hook_position_moves_after_anchor() {
        // block is before udev, should be moved after it
        let mut config = MkinitcpioConfig::parse("HOOKS=(block base udev filesystems)").unwrap();
        config.ensure_hook_position("block", &["udev"], &["filesystems"]);
        let idx_block = config.hooks.iter().position(|h| h == "block").unwrap();
        let idx_udev = config.hooks.iter().position(|h| h == "udev").unwrap();
        assert!(idx_block > idx_udev, "block should be after udev");
    }

    #[test]
    fn test_ensure_hook_position_moves_before_anchor() {
        // filesystems is after block, but should be before it
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev filesystems block)").unwrap();
        config.ensure_hook_position("filesystems", &["udev"], &["block"]);
        let idx_fs = config
            .hooks
            .iter()
            .position(|h| h == "filesystems")
            .unwrap();
        let idx_block = config.hooks.iter().position(|h| h == "block").unwrap();
        assert!(idx_fs < idx_block, "filesystems should be before block");
    }

    #[test]
    fn test_ensure_hook_position_adds_if_missing() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev filesystems)").unwrap();
        config.ensure_hook_position("block", &["udev"], &["filesystems"]);
        assert!(config.contains_hook("block"));
        let idx_block = config.hooks.iter().position(|h| h == "block").unwrap();
        let idx_udev = config.hooks.iter().position(|h| h == "udev").unwrap();
        let idx_fs = config
            .hooks
            .iter()
            .position(|h| h == "filesystems")
            .unwrap();
        assert!(idx_block > idx_udev);
        assert!(idx_block < idx_fs);
    }

    #[test]
    fn test_ensure_hook_position_no_anchors_inserts_at_start() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(filesystems)").unwrap();
        config.ensure_hook_position("base", &[], &["filesystems"]);
        assert_eq!(config.hooks[0], "base");
    }

    #[test]
    fn test_ensure_hook_position_no_before_anchor_inserts_at_end() {
        let mut config = MkinitcpioConfig::parse("HOOKS=(base udev)").unwrap();
        config.ensure_hook_position("filesystems", &["udev"], &[]);
        assert_eq!(config.hooks[config.hooks.len() - 1], "filesystems");
    }

    // ── insertion_bounds / target helpers (via ensure_hook_position) ────

    #[test]
    fn test_ensure_hook_position_encryption_typical() {
        // Real-world case: encrypt hook must be after block, before filesystems
        let mut config =
            MkinitcpioConfig::parse("HOOKS=(base udev block filesystems keyboard)").unwrap();
        config.ensure_hook_position("encrypt", &["block"], &["filesystems"]);
        let hooks = &config.hooks;
        let idx_encrypt = hooks.iter().position(|h| h == "encrypt").unwrap();
        let idx_block = hooks.iter().position(|h| h == "block").unwrap();
        let idx_fs = hooks.iter().position(|h| h == "filesystems").unwrap();
        assert!(idx_encrypt > idx_block);
        assert!(idx_encrypt < idx_fs);
    }
}
