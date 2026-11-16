use std::collections::HashSet;
use std::process::Command;

use anyhow::{Context, Result};

use crate::assist::utils::show_notification;
use crate::common::audio::{
    AudioSourceInfo, default_source_names, list_audio_source_names, list_audio_sources_short,
    pactl_defaults,
};
use crate::settings::store::{
    SCREEN_RECORD_AUDIO_SOURCES_KEY, SettingsStore, is_audio_sources_default,
    parse_audio_source_selection,
};

#[derive(Debug)]
pub(super) struct AudioTarget {
    pub(super) name: String,
    modules: AudioModules,
}

impl AudioTarget {
    pub(super) fn module_ids(&self) -> &[u32] {
        &self.modules.ids
    }

    /// Leave modules running after this CLI exits. The caller must save their
    /// IDs for cleanup, or report them if recorder rollback also fails.
    pub(super) fn retain_modules(&mut self) {
        self.modules.ids.clear();
    }
}

#[derive(Debug)]
pub(super) enum AudioSelectionMode {
    Defaults,
    Explicit(Vec<String>),
}

pub(super) fn audio_selection_mode(store: Option<&SettingsStore>) -> AudioSelectionMode {
    match store {
        Some(store) => {
            let raw = store.optional_string(SCREEN_RECORD_AUDIO_SOURCES_KEY);
            if is_audio_sources_default(&raw) {
                AudioSelectionMode::Defaults
            } else {
                AudioSelectionMode::Explicit(parse_audio_source_selection(raw))
            }
        }
        None => AudioSelectionMode::Explicit(Vec::new()),
    }
}

fn notify_audio_issue(message: &str) {
    let _ = show_notification("Screen Recording", message);
}

fn load_available_audio_sources() -> Option<Vec<AudioSourceInfo>> {
    match list_audio_sources_short() {
        Ok(sources) => Some(sources),
        Err(_) => {
            notify_audio_issue("Unable to list audio sources; recording video only.");
            None
        }
    }
}

fn select_audio_sources(
    selection_mode: AudioSelectionMode,
    source_info: &[AudioSourceInfo],
) -> Option<Vec<String>> {
    match selection_mode {
        AudioSelectionMode::Defaults => match pactl_defaults() {
            Ok(defaults) => Some(default_source_names(&defaults, source_info)),
            Err(_) => {
                notify_audio_issue("Unable to detect default audio sources; recording video only.");
                None
            }
        },
        AudioSelectionMode::Explicit(list) => Some(list),
    }
}

fn build_available_source_set(source_info: &[AudioSourceInfo]) -> HashSet<String> {
    source_info
        .iter()
        .map(|source| source.name.clone())
        .collect()
}

fn filter_unavailable_sources(
    mut sources: Vec<String>,
    available_set: &HashSet<String>,
) -> Vec<String> {
    let mut missing = false;
    sources.retain(|source| {
        if available_set.contains(source) {
            true
        } else {
            missing = true;
            false
        }
    });

    if missing {
        notify_audio_issue(
            "Some selected audio sources were unavailable; recording available sources only.",
        );
    }

    sources
}

fn primary_audio_target(name: &str) -> AudioTarget {
    AudioTarget {
        name: name.to_string(),
        modules: AudioModules::default(),
    }
}

fn build_audio_target_from_sources(sources: Vec<String>) -> AudioTarget {
    if sources.len() == 1 {
        return primary_audio_target(&sources[0]);
    }

    match create_recording_mix(&sources) {
        Ok(target) => target,
        Err(err) => {
            eprintln!("Unable to combine recording audio sources: {err:#}");
            notify_audio_issue("Unable to combine audio sources; recording first source only.");
            primary_audio_target(&sources[0])
        }
    }
}

pub(super) fn resolve_audio_target(selection_mode: AudioSelectionMode) -> Option<AudioTarget> {
    let source_info = load_available_audio_sources()?;
    let available_set = build_available_source_set(&source_info);

    let sources = dedup_audio_sources(select_audio_sources(selection_mode, &source_info)?);
    if sources.is_empty() {
        return None;
    }

    let sources = filter_unavailable_sources(sources, &available_set);

    if sources.is_empty() {
        notify_audio_issue("No selected audio sources available; recording video only.");
        return None;
    }

    Some(build_audio_target_from_sources(sources))
}

fn dedup_audio_sources(sources: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    sources
        .into_iter()
        .filter(|source| seen.insert(source.clone()))
        .collect()
}

fn wait_for_audio_source(name: &str) -> bool {
    for _ in 0..10 {
        if let Ok(sources) = list_audio_source_names()
            && sources.iter().any(|source| source == name)
        {
            return true;
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    false
}

fn create_recording_mix(sources: &[String]) -> Result<AudioTarget> {
    let sink_name = format!(
        "ins_record_mix_{}",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let mut modules = AudioModules::default();

    // Create null sink
    let sink_arg = format!("sink_name={}", sink_name);
    let props_arg = format!("sink_properties=device.description={}", sink_name);

    modules.load(&["module-null-sink", &sink_arg, &props_arg])?;

    // Create loopbacks for each source
    for source in sources {
        let source_arg = format!("source={}", source);
        let sink_arg = format!("sink={}", sink_name);

        modules.load(&["module-loopback", &source_arg, &sink_arg])?;
    }

    let monitor_name = format!("{}.monitor", sink_name);
    if !wait_for_audio_source(&monitor_name) {
        anyhow::bail!("Mixed audio source was not ready");
    }

    Ok(AudioTarget {
        name: monitor_name,
        modules,
    })
}

fn unload_audio_module(module_id: u32) {
    match Command::new("pactl")
        .args(["unload-module", &module_id.to_string()])
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("Failed to unload recording audio module {module_id}: {status}"),
        Err(err) => eprintln!("Failed to unload recording audio module {module_id}: {err}"),
    }
}

pub(super) fn unload_audio_modules(module_ids: &[u32]) {
    let mut ids: Vec<u32> = module_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for module_id in ids.into_iter().rev() {
        unload_audio_module(module_id);
    }
}

/// Owns temporary PulseAudio modules until recording state takes responsibility.
#[derive(Debug, Default)]
struct AudioModules {
    ids: Vec<u32>,
}

impl AudioModules {
    fn load(&mut self, args: &[&str]) -> Result<()> {
        let output = Command::new("pactl")
            .arg("load-module")
            .args(args)
            .output()
            .with_context(|| format!("Failed to load {}", args[0]))?;
        anyhow::ensure!(
            output.status.success(),
            "Failed to load {}: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let id = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .with_context(|| format!("Failed to parse {} module id", args[0]))?;
        self.ids.push(id);
        Ok(())
    }
}

impl Drop for AudioModules {
    fn drop(&mut self) {
        unload_audio_modules(&self.ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    use std::path::Path;

    use super::super::backend::{RecordingFormat, build_wf_recorder_args};

    #[test]
    fn audio_module_lifecycle() -> Result<()> {
        const SCENARIO: &str = "INS_TEST_AUDIO_SCENARIO";
        if let Ok(scenario) = std::env::var(SCENARIO) {
            // Run in an isolated subprocess: changing PATH in the test runner
            // would race with unrelated tests and could invoke real pactl.
            match scenario.as_str() {
                "load_failure" | "invalid_id" => {
                    let error =
                        create_recording_mix(&["first".into(), "second".into()]).unwrap_err();
                    let expected = if scenario == "invalid_id" {
                        "Failed to parse module-loopback module id"
                    } else {
                        "Failed to load module-loopback"
                    };
                    assert!(error.to_string().contains(expected), "{error:#}");
                }
                "argument_failure" | "spawn_failure" | "persist" => {
                    let mut modules = AudioModules::default();
                    modules.load(&["module-null-sink", "sink_name=test"])?;
                    modules.load(&["module-loopback", "source=first"])?;
                    let mut target = AudioTarget {
                        name: "test.monitor".into(),
                        modules,
                    };
                    if scenario == "persist" {
                        let ids = target.module_ids().to_vec();
                        target.retain_modules();
                        drop(target);
                        let calls = fs::read_to_string(std::env::var("INS_TEST_AUDIO_CALLS")?)?;
                        assert!(!calls.contains("unload-module"));
                        // A later stop invocation cleans up from persisted IDs.
                        unload_audio_modules(&ids);
                    } else if scenario == "spawn_failure" {
                        assert!(
                            super::super::recorder::spawn_detached_recorder(
                                "missing-test-recorder",
                                &[]
                            )
                            .is_err()
                        );
                    } else {
                        let invalid = std::ffi::OsString::from_vec(vec![b'/', 0xff]);
                        assert!(
                            build_wf_recorder_args(
                                None,
                                RecordingFormat::Mp4,
                                Path::new(&invalid),
                                Some(&target),
                                None,
                            )
                            .is_err()
                        );
                        // Dropping the target on an argument-building failure
                        // must release all modules, without explicit cleanup.
                    }
                }
                _ => anyhow::bail!("Unknown audio test scenario"),
            }
            return Ok(());
        }

        let test_name = format!(
            "{}::audio_module_lifecycle",
            module_path!()
                .split_once("::")
                .context("Missing crate prefix")?
                .1
        );
        for scenario in [
            "load_failure",
            "invalid_id",
            "argument_failure",
            "spawn_failure",
            "persist",
        ] {
            let dir = tempfile::tempdir()?;
            let pactl = dir.path().join("pactl");
            let calls = dir.path().join("calls");
            fs::write(
                &pactl,
                r#"#!/bin/sh
printf '%s\n' "$*" >> "$INS_TEST_AUDIO_CALLS"
case "$1:$2" in
    load-module:module-null-sink) echo 11 ;;
    load-module:module-loopback)
        case "$3" in
            source=first) echo 22 ;;
            *)
                if [ "$INS_TEST_AUDIO_SCENARIO" = invalid_id ]; then
                    echo invalid
                else
                    echo 'loopback rejected' >&2
                    exit 1
                fi
                ;;
        esac
        ;;
    unload-module:*) exit 0 ;;
    *) exit 1 ;;
esac
"#,
            )?;
            fs::set_permissions(&pactl, fs::Permissions::from_mode(0o755))?;
            for program in ["setsid", "sh"] {
                std::os::unix::fs::symlink(which::which(program)?, dir.path().join(program))?;
            }
            let output = Command::new(std::env::current_exe()?)
                .args(["--exact", &test_name, "--nocapture"])
                .env("PATH", dir.path())
                .env("XDG_RUNTIME_DIR", dir.path())
                .env(SCENARIO, scenario)
                .env("INS_TEST_AUDIO_CALLS", &calls)
                .output()?;
            assert!(
                output.status.success(),
                "{scenario}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let calls = fs::read_to_string(calls)?;
            let unloaded: Vec<_> = calls
                .lines()
                .filter(|line| line.starts_with("unload-module"))
                .collect();
            assert_eq!(
                unloaded,
                ["unload-module 22", "unload-module 11"],
                "{scenario}"
            );
        }
        Ok(())
    }
}
