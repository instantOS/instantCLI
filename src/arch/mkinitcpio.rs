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

    pub fn ensure_hook_position(&mut self, hook: &str, after: &[&str], before: &[&str]) {
        self.ensure_hook(hook);

        let mut changed = true;
        // Iteratively reposition the hook to satisfy all constraints
        for _ in 0..10 {
            if !changed {
                break;
            }
            changed = false;

            let current_idx = self.hooks.iter().position(|h| h == hook).unwrap();

            // Find the rightmost 'after' constraint (hook must come after this)
            let mut max_after_idx = -1isize;
            for &a in after {
                if let Some(idx) = self.hooks.iter().position(|h| h == a)
                    && (idx as isize) > max_after_idx
                {
                    max_after_idx = idx as isize;
                }
            }

            if max_after_idx >= current_idx as isize {
                // Violates 'after' constraint - reposition the hook
                let removed = self.hooks.remove(current_idx);

                // Calculate valid insertion range after removal
                let mut min_idx = 0;
                for &a in after {
                    if let Some(idx) = self.hooks.iter().position(|h| h == a)
                        && idx + 1 > min_idx
                    {
                        min_idx = idx + 1;
                    }
                }

                let mut max_idx = self.hooks.len();
                for &b in before {
                    if let Some(idx) = self.hooks.iter().position(|h| h == b)
                        && idx < max_idx
                    {
                        max_idx = idx;
                    }
                }

                let target = if min_idx > max_idx {
                    min_idx // Conflicting constraints - prioritize 'after' (dependencies)
                } else {
                    min_idx
                };

                self.hooks.insert(target, removed);
                changed = true;
            } else {
                // Find the leftmost 'before' constraint (hook must come before this)
                let mut min_before_idx = self.hooks.len() as isize;
                for &b in before {
                    if let Some(idx) = self.hooks.iter().position(|h| h == b)
                        && (idx as isize) < min_before_idx
                    {
                        min_before_idx = idx as isize;
                    }
                }

                if min_before_idx <= current_idx as isize {
                    // Violates 'before' constraint - reposition the hook
                    let removed = self.hooks.remove(current_idx);

                    // Calculate valid insertion range after removal
                    let mut min_idx = 0;
                    for &a in after {
                        if let Some(idx) = self.hooks.iter().position(|h| h == a)
                            && idx + 1 > min_idx
                        {
                            min_idx = idx + 1;
                        }
                    }

                    let mut max_idx = self.hooks.len();
                    for &b in before {
                        if let Some(idx) = self.hooks.iter().position(|h| h == b)
                            && idx < max_idx
                        {
                            max_idx = idx;
                        }
                    }

                    let target = if max_idx < min_idx { max_idx } else { max_idx };

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
