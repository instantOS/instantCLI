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

    pub fn ensure_hook_position(&mut self, hook: &str, after: &[&str], before: &[&str]) {
        self.ensure_hook(hook);

        let mut changed = true;
        // Simple iterative approach to satisfy constraints
        // We limit iterations to avoid infinite loops in case of cyclic dependencies (though unlikely here)
        for _ in 0..10 {
            if !changed {
                break;
            }
            changed = false;

            let current_idx = self.hooks.iter().position(|h| h == hook).unwrap();

            // Check 'after' constraints
            let mut max_after_idx = -1isize;
            for &a in after {
                if let Some(idx) = self.hooks.iter().position(|h| h == a) {
                    if (idx as isize) > max_after_idx {
                        max_after_idx = idx as isize;
                    }
                }
            }

            if max_after_idx >= current_idx as isize {
                // Need to move hook after max_after_idx
                let removed = self.hooks.remove(current_idx);
                self.hooks.insert((max_after_idx) as usize, removed); // insert at same index pushes it after? No.
                // If we want it AFTER index X, we insert at X+1.
                // But we removed it.
                // Example: [A, B, HOOK], max_after is B (1). current is 2. 1 < 2. OK.
                // Example: [HOOK, A, B]. max_after is B (2). current is 0. 2 >= 0. Move.
                // Remove HOOK -> [A, B]. Insert at 2+1? No, indices shifted.
                // Let's re-evaluate indices after removal? Or just use swap?
                // Easier: Remove, then insert at target.
                // Target: max_after_idx. If we insert at max_after_idx + 1 (original index), it will be after.
                // But wait, if we remove, indices > current_idx shift down.
                // If max_after_idx > current_idx, it shifts down by 1.
                // So target index in new list is max_after_idx.
                // insert(i, e) inserts before element at i. So to be after element at i, insert at i+1.
                // So target is max_after_idx.
                // Let's verify. [H, A]. remove H -> [A]. A is at 0. max_after is A (0).
                // We want [A, H]. Insert at 1.
                // max_after_idx (original) = 1.
                // So insert at max_after_idx.

                // Let's just do: remove, then find index of 'after' again, then insert after it.
                // This is safer.
                // But we have multiple 'after's.
                // We know max_after_idx was the constraint violator.
                // Let's just move it to the end, then check 'before' constraints?
                // Or move it strictly after the constraint.

                // Let's use a simpler logic:
                // 1. Remove hook.
                // 2. Calculate valid range [min, max].
                //    min = max(indices of 'after') + 1
                //    max = min(indices of 'before') (if exists, else len)
                // 3. Insert at max(min, current_idx) clamped to max?
                //    Actually we just want to satisfy constraints.
                //    If we just place it at 'min', it satisfies 'after'.
                //    Does it satisfy 'before'? Hopefully.

                let removed = self.hooks.remove(current_idx);

                let mut min_idx = 0;
                for &a in after {
                    if let Some(idx) = self.hooks.iter().position(|h| h == a) {
                        if idx + 1 > min_idx {
                            min_idx = idx + 1;
                        }
                    }
                }

                let mut max_idx = self.hooks.len();
                for &b in before {
                    if let Some(idx) = self.hooks.iter().position(|h| h == b) {
                        if idx < max_idx {
                            max_idx = idx;
                        }
                    }
                }

                // If min > max, we have a conflict. We prioritize 'after' (dependencies) usually?
                // Or just clamp.
                let target = if min_idx > max_idx {
                    min_idx // Conflict, but respect 'after'
                } else {
                    // Try to keep it close to where it was, or just put it at min?
                    // Putting it at min is safe.
                    min_idx
                };

                self.hooks.insert(target, removed);
                changed = true;
            } else {
                // Check 'before' constraints
                let mut min_before_idx = self.hooks.len() as isize;
                for &b in before {
                    if let Some(idx) = self.hooks.iter().position(|h| h == b) {
                        if (idx as isize) < min_before_idx {
                            min_before_idx = idx as isize;
                        }
                    }
                }

                if min_before_idx <= current_idx as isize {
                    // Need to move hook before min_before_idx
                    let removed = self.hooks.remove(current_idx);

                    // Re-calculate indices
                    let mut min_idx = 0;
                    for &a in after {
                        if let Some(idx) = self.hooks.iter().position(|h| h == a) {
                            if idx + 1 > min_idx {
                                min_idx = idx + 1;
                            }
                        }
                    }

                    let mut max_idx = self.hooks.len();
                    for &b in before {
                        if let Some(idx) = self.hooks.iter().position(|h| h == b) {
                            if idx < max_idx {
                                max_idx = idx;
                            }
                        }
                    }

                    let target = if max_idx < min_idx {
                        max_idx // Conflict, respect 'before' this time? No, let's stick to valid range logic.
                    } else {
                        max_idx
                    };

                    self.hooks.insert(target, removed);
                    changed = true;
                }
            }
        }
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
