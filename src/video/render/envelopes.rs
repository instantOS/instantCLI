use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};

use crate::ui::prelude::Level;
use crate::video::config::VideoDirectories;
use crate::video::document::{SegmentKind, VideoSource};
use crate::video::planning::{AudioEnvelope, SourceEnvelopes, TimelinePlan, TimelinePlanItem};
use crate::video::render::logging::log_event;
use crate::video::support::utils::compute_file_hash;
use crate::video::transcript_language::TranscriptLanguage;

/// Sample rate used only to measure loudness; speech dips are fully visible at
/// 16 kHz and decoding stays fast.
const ENVELOPE_SAMPLE_RATE: u32 = 16_000;

// Bump when decoding, filtering, hop size, or smoothing changes.
const ENVELOPE_CACHE_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct AudioIdentity {
    hash: String,
    modified: SystemTime,
}

impl AudioIdentity {
    fn read(audio: &Path) -> Result<Self> {
        Ok(Self {
            hash: compute_file_hash(audio)?,
            // The shared hash samples large files. Modification time also
            // invalidates changes outside those samples.
            modified: fs::metadata(audio)?.modified()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedEnvelope<T = AudioEnvelope> {
    version: u32,
    audio: AudioIdentity,
    envelope: T,
}

fn envelope_cache_path(source: &VideoSource, audio_hash: &str) -> Result<PathBuf> {
    let hash = if source.source == source.audio {
        audio_hash.to_string()
    } else {
        compute_file_hash(&source.source)?
    };
    let directories = VideoDirectories::new()?;
    let paths = directories.cache_paths(&hash, TranscriptLanguage::En);
    // Keep envelopes alongside transcripts/enhanced audio so project cache
    // clearing removes them too. Different rendered audio gets its own entry.
    Ok(paths.transcript_dir().join(format!(
        "envelope-v{ENVELOPE_CACHE_VERSION}-{audio_hash}.json"
    )))
}

fn load_cached_envelope(
    audio: &Path,
    cache_path: impl FnOnce(&str) -> Result<PathBuf>,
    decode: impl FnOnce(&Path) -> Result<AudioEnvelope>,
) -> Result<AudioEnvelope> {
    let identity = match AudioIdentity::read(audio) {
        Ok(identity) => identity,
        Err(err) => {
            log_cache_failure(&err);
            return decode(audio);
        }
    };
    let cache_path = match cache_path(&identity.hash) {
        Ok(path) => path,
        Err(err) => {
            log_cache_failure(&err);
            return decode(audio);
        }
    };
    match fs::read(&cache_path) {
        Ok(bytes) => match serde_json::from_slice::<CachedEnvelope>(&bytes) {
            Ok(cached) if cached.version == ENVELOPE_CACHE_VERSION && cached.audio == identity => {
                log_event(
                    Level::Info,
                    "video.render.envelope.cached",
                    format!("Using cached audio measurements ({})", audio.display()),
                );
                return Ok(cached.envelope);
            }
            Ok(_) => {}
            Err(err) => log_cache_failure(&err.into()),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log_cache_failure(&err.into()),
    }

    log_event(
        Level::Info,
        "video.render.envelope",
        format!("Measuring audio for cut refinement ({})", audio.display()),
    );
    let envelope = decode(audio)?;
    let save = || -> Result<()> {
        // Never publish measurements under an identity that changed mid-decode.
        anyhow::ensure!(
            AudioIdentity::read(audio)? == identity,
            "Audio changed during cut analysis"
        );
        let parent = cache_path
            .parent()
            .context("Envelope cache has no parent directory")?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        let mut writer = BufWriter::new(temporary.as_file_mut());
        serde_json::to_writer(
            &mut writer,
            &CachedEnvelope {
                version: ENVELOPE_CACHE_VERSION,
                audio: identity,
                envelope: &envelope,
            },
        )?;
        writer.flush()?;
        drop(writer);
        temporary.persist(cache_path)?;
        Ok(())
    };
    if let Err(err) = save() {
        log_cache_failure(&err);
    }
    Ok(envelope)
}

fn log_cache_failure(err: &anyhow::Error) {
    log_event(
        Level::Warn,
        "video.render.envelope.cache.failed",
        format!(
            "Could not reuse or save audio measurements ({err:#}); continuing without the cache"
        ),
    );
}

/// Measure the loudness of dialogue sources' rendered audio so dialogue cuts can
/// land in the quietest nearby point instead of mid-word. Measurements persist
/// in the shared video cache across planning operations and CLI invocations.
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
        let result = load_cached_envelope(
            &source.audio,
            |audio_hash| envelope_cache_path(source, audio_hash),
            decode_envelope,
        );
        match result {
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

    fn fake_envelope(_: &Path) -> Result<AudioEnvelope> {
        AudioEnvelope::from_s16le_reader(std::io::Cursor::new(vec![0; 320]), 16_000)
    }

    #[test]
    fn persistent_cache_reuses_measurements_and_invalidates_changed_audio() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let audio = temp.path().join("audio.wav");
        let cache = temp.path().join("cache/envelope.json");
        fs::write(&audio, b"first recording")?;
        let first = load_cached_envelope(&audio, |_| Ok(cache.clone()), fake_envelope)?;
        let reused = load_cached_envelope(
            &audio,
            |_| Ok(cache.clone()),
            |_| panic!("a valid disk cache must avoid decoding"),
        )?;
        assert_eq!(first.duration(), reused.duration());
        assert_eq!(
            first.quietest_point(0.005, 0.0, 0.01),
            reused.quietest_point(0.005, 0.0, 0.01)
        );

        // Same-size replacement must invalidate the old measurements too.
        fs::write(&audio, b"other recording")?;
        let decoded = std::cell::Cell::new(false);
        load_cached_envelope(
            &audio,
            |_| Ok(cache.clone()),
            |path| {
                decoded.set(true);
                fake_envelope(path)
            },
        )?;
        assert!(decoded.get());
        Ok(())
    }

    #[test]
    fn modification_time_invalidates_an_unchanged_hash() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let audio = temp.path().join("audio.wav");
        let cache = temp.path().join("envelope.json");
        fs::write(&audio, b"recording")?;
        load_cached_envelope(&audio, |_| Ok(cache.clone()), fake_envelope)?;
        let modified = fs::metadata(&audio)?.modified()? + std::time::Duration::from_secs(5);
        fs::File::open(&audio)?.set_modified(modified)?;
        let decoded = std::cell::Cell::new(false);
        load_cached_envelope(
            &audio,
            |_| Ok(cache.clone()),
            |path| {
                decoded.set(true);
                fake_envelope(path)
            },
        )?;
        assert!(decoded.get());
        Ok(())
    }

    #[test]
    fn audio_changed_during_decoding_is_not_cached() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let audio = temp.path().join("audio.wav");
        let cache = temp.path().join("envelope.json");
        fs::write(&audio, b"recording")?;
        load_cached_envelope(
            &audio,
            |_| Ok(cache.clone()),
            |path| {
                fs::write(path, b"changed recording")?;
                fake_envelope(path)
            },
        )?;
        assert!(!cache.exists());
        Ok(())
    }

    #[test]
    fn corrupt_and_old_cache_entries_are_rebuilt() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let audio = temp.path().join("audio.wav");
        let cache = temp.path().join("envelope.json");
        fs::write(&audio, b"recording")?;
        fs::write(&cache, b"truncated cache")?;
        load_cached_envelope(&audio, |_| Ok(cache.clone()), fake_envelope)?;
        let mut entry: CachedEnvelope = serde_json::from_slice(&fs::read(&cache)?)?;
        entry.version = 0;
        fs::write(&cache, serde_json::to_vec(&entry)?)?;
        let decoded = std::cell::Cell::new(false);
        load_cached_envelope(
            &audio,
            |_| Ok(cache.clone()),
            |path| {
                decoded.set(true);
                fake_envelope(path)
            },
        )?;
        assert!(decoded.get());
        let entry: CachedEnvelope = serde_json::from_slice(&fs::read(&cache)?)?;
        assert_eq!(entry.version, ENVELOPE_CACHE_VERSION);
        Ok(())
    }

    #[test]
    fn unwritable_cache_and_decoder_failures_do_not_publish_bad_entries() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let audio = temp.path().join("audio.wav");
        fs::write(&audio, b"recording")?;
        let cache = temp.path().join("envelope.json");
        let error = load_cached_envelope(&audio, |_| Ok(cache.clone()), |_| bail!("decode failed"));
        assert!(error.is_err());
        assert!(!cache.exists());
        // A file in place of the parent reliably prevents writes, even as root.
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, b"file")?;
        let envelope =
            load_cached_envelope(&audio, |_| Ok(blocked.join("envelope.json")), fake_envelope)?;
        assert_eq!(envelope.duration(), 0.01);
        Ok(())
    }

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
