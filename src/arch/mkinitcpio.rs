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
