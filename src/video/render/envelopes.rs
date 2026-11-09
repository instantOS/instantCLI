use std::collections::HashSet;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::ui::prelude::Level;
use crate::video::document::{SegmentKind, VideoSource};
use crate::video::planning::{AudioEnvelope, SourceEnvelopes, TimelinePlan, TimelinePlanItem};
use crate::video::render::logging::log_event;

/// Sample rate used only to measure loudness; speech dips are fully visible at
/// 16 kHz and decoding stays fast.
const ENVELOPE_SAMPLE_RATE: u32 = 16_000;

/// Measure the loudness of dialogue sources' rendered audio so dialogue cuts can
/// land in the quietest nearby point instead of mid-word.
///
/// A source whose audio cannot be decoded is skipped with a warning; its cuts
/// then use plain fixed padding rather than failing the render.
pub(crate) fn load_source_envelopes(
    sources: &[VideoSource],
    plan: &TimelinePlan,
) -> SourceEnvelopes {
    let source_ids = envelope_source_ids(plan);
    let mut envelopes = SourceEnvelopes::new();
    for source in sources
        .iter()
        .filter(|source| source_ids.contains(source.id.as_str()))
    {
        log_event(
            Level::Info,
            "video.render.envelope",
            format!("Measuring audio for cut refinement ({})", source.id),
        );
        match decode_envelope(&source.audio) {
            Ok(envelope) => {
                envelopes.insert(source.id.clone(), envelope);
            }
            Err(err) => log_event(
                Level::Warn,
                "video.render.envelope.failed",
                format!(
                    "Could not analyse audio for source {} ({err:#}); cuts use fixed padding only",
                    source.id
                ),
            ),
        }
    }
    envelopes
}

fn envelope_source_ids(plan: &TimelinePlan) -> HashSet<&str> {
    plan.items
        .iter()
        .filter_map(|item| match item {
            TimelinePlanItem::Clip(clip) if clip.kind != SegmentKind::Silence => {
                Some(clip.source_id.as_str())
            }
            _ => None,
        })
        .collect()
}

fn decode_envelope(audio: &Path) -> Result<AudioEnvelope> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(audio)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(ENVELOPE_SAMPLE_RATE.to_string())
        .args(["-f", "s16le", "-"]);
    decode_envelope_with_command(&mut command, audio)
}

fn decode_envelope_with_command(command: &mut Command, audio: &Path) -> Result<AudioEnvelope> {
    // A file cannot fill up a stderr pipe while we are consuming PCM on stdout.
    let mut stderr = tempfile::tempfile().context("Failed to create ffmpeg stderr capture")?;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(
            stderr
                .try_clone()
                .context("Failed to clone ffmpeg stderr capture")?,
        )
        .spawn()
        .with_context(|| format!("Failed to spawn ffmpeg for {}", audio.display()))?;

    let envelope = child
        .stdout
        .take()
        .context("ffmpeg stdout was not captured")
        .and_then(|stdout| {
            AudioEnvelope::from_s16le_reader(BufReader::new(stdout), ENVELOPE_SAMPLE_RATE)
        });
    if envelope.is_err() {
        // The reader has dropped stdout. Kill as well in case the decoder does
        // not exit on broken pipe, and always reap it even if it already exited.
        let _ = child.kill();
    }
    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for ffmpeg on {}", audio.display()))?;
    let envelope = envelope?;
    if !status.success() {
        stderr
            .seek(SeekFrom::Start(0))
            .context("Failed to rewind ffmpeg stderr capture")?;
        let mut diagnostics = Vec::new();
        stderr
            .read_to_end(&mut diagnostics)
            .context("Failed to read ffmpeg stderr capture")?;
        bail!(
            "ffmpeg could not decode {}: {}",
            audio.display(),
            String::from_utf8_lossy(&diagnostics).trim()
        );
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::parse_video_document;

    fn plan_with_body(body: &str) -> TimelinePlan {
        let markdown = format!(
            "---\nsources:\n\
             - id: a\n  source: a.mp4\n  transcript: a.json\n\
             - id: b\n  source: b.mp4\n  transcript: b.json\n\
             - id: unused\n  source: unused.mp4\n  transcript: unused.json\n\
             default_source: a\n---\n{body}"
        );
        parse_video_document(&markdown, Path::new("test.md"))
            .unwrap()
            .plan_timeline()
            .unwrap()
    }

    #[test]
    fn excludes_unused_sources_and_deduplicates_dialogue_sources() {
        let plan =
            plan_with_body("`a@00:00:00.0-00:00:01.0` first\n`a@00:00:02.0-00:00:03.0` second\n");
        assert_eq!(envelope_source_ids(&plan), HashSet::from(["a"]));
    }

    #[test]
    fn excludes_nested_broll_sources() {
        let plan = plan_with_body(
            "> `b@00:00:00.0-00:00:01.0` broll\n\n`a@00:00:00.0-00:00:01.0` dialogue\n",
        );
        let TimelinePlanItem::Clip(clip) = &plan.items[0] else {
            panic!("expected dialogue clip");
        };
        assert_eq!(clip.broll.as_ref().unwrap().clips[0].source_id, "b");
        assert_eq!(envelope_source_ids(&plan), HashSet::from(["a"]));
    }

    #[test]
    fn excludes_silence_only_sources() {
        let plan = plan_with_body(
            "`a@00:00:00.0-00:00:01.0` dialogue\n`b@00:00:01.0-00:00:02.0` SILENCE\n",
        );
        assert_eq!(envelope_source_ids(&plan), HashSet::from(["a"]));

        let silence_only = plan_with_body("`b@00:00:00.0-00:00:01.0` SILENCE\n");
        assert!(envelope_source_ids(&silence_only).is_empty());
    }

    #[cfg(unix)]
    fn test_decoder(body: &str) -> Result<AudioEnvelope> {
        // Bound pipe-deadlock regressions without changing PATH or requiring
        // ffmpeg. The producer uses shell builtins so killing it closes stdout.
        let script = format!(
            r#"
parent=$$
(
    trap 'kill "$sleeper" 2>/dev/null; wait "$sleeper" 2>/dev/null; exit' TERM
    sleep 10 &
    sleeper=$!
    wait "$sleeper"
    kill -KILL "$parent"
) >/dev/null 2>&1 &
watchdog=$!
trap 'kill "$watchdog" 2>/dev/null; wait "$watchdog" 2>/dev/null' 0
{body}
"#
        );
        decode_envelope_with_command(
            Command::new("sh").args(["-c", &script]),
            Path::new("test-audio.wav"),
        )
    }

    #[test]
    #[cfg(unix)]
    fn decoder_failure_reports_stderr() {
        let error = test_decoder("printf 'decoder exploded\\377\\n' >&2; exit 7").unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("ffmpeg could not decode test-audio.wav"));
        assert!(message.contains("decoder exploded"));
    }

    #[test]
    #[cfg(unix)]
    fn noisy_stderr_does_not_block_pcm_decoding() {
        let envelope = test_decoder(
            r#"
i=0
while [ "$i" -lt 16384 ]; do
    printf 'decoder diagnostic padding to exceed the stderr pipe capacity repeatedly\n' >&2
    i=$((i + 1))
done
i=0
while [ "$i" -lt 80 ]; do
    printf '\000\000'
    i=$((i + 1))
done
"#,
        )
        .unwrap();
        assert!((envelope.duration() - 0.005).abs() < 1e-9);
    }
}
