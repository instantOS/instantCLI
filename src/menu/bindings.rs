//! Shell-facing accept actions. Key names are validated by the same type as
//! application menus; labels never become executable fzf expressions.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};

use super::{
    MenuBackend, ResolvedBackend,
    client::HostedMenuClient,
    frecency,
    protocol::{ChoiceOptions, STREAM_ITEM_BUFFER_CAPACITY, SerializableMenuItem},
};
use crate::menu_utils::{DialogOutcome, FzfWrapper, MenuKey, MenuKeybind, MenuSelection};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub key: String,
    pub label: String,
}
impl std::str::FromStr for Binding {
    type Err = String;
    fn from_str(spec: &str) -> std::result::Result<Self, Self::Err> {
        let (key, label) = spec
            .split_once(':')
            .ok_or("expected KEY:LABEL, for example ctrl-e:Edit")?;
        let binding = Self {
            key: key.into(),
            label: label.into(),
        };
        binding.typed().map_err(|error| error.to_string())?;
        Ok(binding)
    }
}
impl Binding {
    fn typed(&self) -> Result<MenuKeybind<String>> {
        if self.label.trim().is_empty() || self.label.chars().any(char::is_control) {
            bail!("binding labels must be nonempty and contain no control characters");
        }
        Ok(MenuKeybind::new(
            MenuKey::new(&self.key)?,
            &self.label,
            self.key.clone(),
        ))
    }
}
pub(super) fn validate(bindings: &[Binding]) -> Result<Vec<MenuKeybind<String>>> {
    let mut keys = std::collections::HashSet::new();
    bindings
        .iter()
        .map(|binding| {
            let typed = binding.typed()?;
            if !keys.insert(&binding.key) {
                bail!("duplicate menu keybind: {}", binding.key);
            }
            Ok(typed)
        })
        .collect()
}

pub(super) fn select(
    prompt: &str,
    items: Vec<SerializableMenuItem>,
    multi: bool,
    namespace: Option<&str>,
    bindings: &[Binding],
) -> Result<DialogOutcome<MenuSelection<SerializableMenuItem, String>>> {
    let typed = validate(bindings)?;
    let mut state = namespace.map(frecency::MenuFrecency::open).transpose()?;
    let mut items = items;
    if let Some(state) = state.as_mut() {
        items = state.prepare(items);
    }
    let outcome = FzfWrapper::builder()
        .prompt(prompt.to_string())
        .multi_select(multi)
        .select_with_keybinds(items, &typed)?;
    if let DialogOutcome::Submitted(selection) = &outcome
        && let Some(state) = state.as_mut()
        && let Err(error) = state.record_all(&selection.items)
    {
        eprintln!("Warning: {error:#}");
    }
    Ok(outcome)
}

pub(super) fn handle(
    prompt: &str,
    items: &str,
    multi: bool,
    namespace: Option<&str>,
    backend: MenuBackend,
    bindings: &[Binding],
) -> Result<i32> {
    let typed = validate(bindings)?;
    if let Some(namespace) = namespace {
        frecency::validate_namespace(namespace)?;
    }
    if backend.resolve(true) == ResolvedBackend::Instantmenu {
        let mut cmd = Command::new("instantmenu");
        let prompt = if multi {
            format!("{prompt} (ctrl+return adds more)")
        } else {
            prompt.to_owned()
        };
        cmd.args([
            "--position",
            "center",
            "--border-width",
            "4",
            "--lines",
            "20",
            "--insensitive",
            "--prompt",
            &prompt,
        ]);
        for binding in bindings {
            cmd.arg("--bind")
                .arg(format!("{}:{}", binding.key, binding.label));
        }
        if let Some(namespace) = namespace {
            cmd.arg("--frecency-cache").arg(namespace);
        }
        if !items.is_empty() {
            cmd.stdin(Stdio::piped());
        } else if std::io::stdin().is_terminal() {
            cmd.stdin(Stdio::null());
        }
        let mut child = cmd.spawn().context("Failed to spawn instantmenu")?;
        if let Some(mut stdin) = child.stdin.take() {
            for item in items.split(' ') {
                if let Err(error) = writeln!(stdin, "{item}") {
                    if error.kind() == std::io::ErrorKind::BrokenPipe {
                        break;
                    }
                    return Err(error.into());
                }
            }
        }
        let status = child.wait()?;
        return Ok(if status.success() {
            0
        } else if status.code() == Some(1) {
            2
        } else {
            3
        });
    }
    let buffered = !items.is_empty() || namespace.is_some();
    let outcome = if buffered {
        let items = if !items.is_empty() {
            items.split(' ').map(SerializableMenuItem::plain).collect()
        } else if std::io::stdin().is_terminal() {
            Vec::new()
        } else {
            std::io::stdin()
                .lock()
                .lines()
                .map(|line| line.map(SerializableMenuItem::plain))
                .collect::<std::io::Result<Vec<_>>>()?
        };
        if backend.resolve(true) == ResolvedBackend::Scratchpad {
            let options = ChoiceOptions::new(prompt)
                .multi_select(multi)
                .with_frecency_cache(namespace.map(str::to_owned))
                .with_bindings(bindings.to_vec());
            HostedMenuClient::new().choice(options, items)?
        } else {
            select(prompt, items, multi, namespace, bindings)?
        }
    } else {
        let (tx, rx) = crossbeam_channel::bounded(STREAM_ITEM_BUFFER_CAPACITY);
        std::thread::spawn(move || {
            if std::io::stdin().is_terminal() {
                return;
            }
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(line) => {
                        if tx.send(SerializableMenuItem::plain(line)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        if backend.resolve(true) == ResolvedBackend::Scratchpad {
            let options = ChoiceOptions::new(prompt)
                .multi_select(multi)
                .with_bindings(bindings.to_vec());
            HostedMenuClient::new().choice_streaming(options, rx)?
        } else {
            FzfWrapper::builder()
                .prompt(prompt.to_string())
                .multi_select(multi)
                .select_streaming_with_keybinds(Vec::new(), rx, &typed)?
        }
    };
    match outcome {
        DialogOutcome::Cancelled => Ok(2),
        DialogOutcome::Submitted(selection) => {
            println!("{}", selection.action.as_deref().unwrap_or(""));
            for item in selection.items {
                println!("{}", item.display_text);
            }
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::MockQueue;

    #[test]
    fn validates_specs_and_duplicates_before_starting_a_backend() {
        for spec in [
            "ctrl-c:Quit",
            "enter:Submit",
            "ctrl-e",
            "ctrl-e:",
            "ctrl-e:bad\nlabel",
            "load:Oops",
            "ctrl-e,alt-s:Oops",
        ] {
            assert!(spec.parse::<Binding>().is_err(), "{spec:?}");
        }
        let bindings = vec![
            "ctrl-e:Edit".parse().unwrap(),
            "ctrl-e:Again".parse().unwrap(),
        ];
        assert!(
            validate(&bindings)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
        assert_eq!(
            "ctrl-e:Edit: details".parse::<Binding>().unwrap().label,
            "Edit: details"
        );
    }

    #[test]
    fn bound_selection_preserves_action_and_values() {
        let _guard = MockQueue::new().keybind_action("ctrl-e", vec![1]).guard();
        let bindings = vec!["ctrl-e:Edit".parse().unwrap()];
        let DialogOutcome::Submitted(selection) = select(
            "Pick",
            vec![
                SerializableMenuItem::plain("alpha"),
                SerializableMenuItem::plain("beta"),
            ],
            true,
            None,
            &bindings,
        )
        .unwrap() else {
            panic!("expected selection")
        };
        assert_eq!(selection.action.as_deref(), Some("ctrl-e"));
        assert_eq!(selection.items[0].display_text, "beta");
    }

    #[test]
    fn hosted_action_without_items_survives_wire_roundtrip() {
        use super::super::processing::RequestProcessor;
        use super::super::protocol::{MenuRequest, MenuResponse};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64},
        };
        let _guard = MockQueue::new().keybind_action("ctrl-e", vec![]).guard();
        let request = MenuRequest::Choice {
            options: ChoiceOptions::new("Pick").with_bindings(vec!["ctrl-e:Edit".parse().unwrap()]),
            items: vec![],
        };
        let request = serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
        let processor =
            RequestProcessor::new(Arc::new(AtomicBool::new(true)), Arc::new(AtomicU64::new(0)));
        let response = processor.process_internal(request).unwrap();
        let response =
            serde_json::from_str::<MenuResponse>(&serde_json::to_string(&response).unwrap())
                .unwrap();
        assert!(
            matches!(response, MenuResponse::ChoiceResult { action: Some(key), items } if key == "ctrl-e" && items.is_empty())
        );
    }

    #[test]
    fn hosted_streaming_action_keeps_selection() {
        use super::super::processing::RequestProcessor;
        use super::super::protocol::MenuResponse;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64},
        };
        let _guard = MockQueue::new().keybind_action("ctrl-e", vec![0]).guard();
        let processor =
            RequestProcessor::new(Arc::new(AtomicBool::new(true)), Arc::new(AtomicU64::new(0)));
        let (tx, rx) = crossbeam_channel::bounded(2);
        tx.send(SerializableMenuItem::plain("alpha")).unwrap();
        drop(tx);
        let response = processor
            .handle_choice_streaming(
                ChoiceOptions::new("Pick").with_bindings(vec!["ctrl-e:Edit".parse().unwrap()]),
                rx,
                || Ok(()),
            )
            .unwrap();
        assert!(
            matches!(response, MenuResponse::ChoiceResult { action: Some(key), items } if key == "ctrl-e" && items[0].display_text == "alpha")
        );
    }
}
