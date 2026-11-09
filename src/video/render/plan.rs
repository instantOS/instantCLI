use std::path::Path;

use anyhow::{Result, bail};

use crate::ui::prelude::Level;
use crate::video::document::VideoSource;
use crate::video::planning::{TimelinePlan, align_plan_with_subtitles};
use crate::video::render::envelopes::load_source_envelopes;
use crate::video::render::logging::log_event;

pub(crate) fn build_timeline_plan(
    document: &crate::video::document::VideoDocument,
    cues: &[crate::video::support::transcript::TranscriptCue],
    sources: &[VideoSource],
    markdown_path: &Path,
) -> Result<TimelinePlan> {
    log_event(
        Level::Info,
        "video.render.plan",
        "Planning timeline (selecting clips, overlays, cards)",
    );
    let mut plan = document.plan_timeline()?;

    if plan.items.is_empty() {
        bail!(
            "No renderable blocks found in {}. Ensure the markdown contains timestamp code spans or headings.",
            markdown_path.display()
        );
    }

    log_event(
        Level::Info,
        "video.render.plan.align",
        "Aligning planned segments with transcript timing",
    );
    let envelopes = load_source_envelopes(sources, &plan);
    align_plan_with_subtitles(&mut plan, cues, &envelopes)?;

    Ok(plan)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use super::*;
    use crate::video::document::parse_video_document;
    use crate::video::planning::TimelinePlanItem;
    use crate::video::render::transcripts::apply_markdown_edits;
    use crate::video::support::transcript::{TranscriptCue, WordTiming};

    #[test]
    fn text_corrections_preserve_original_acoustic_source_windows() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let audio = temp.path().join("voice.wav");
        // A shallow dip fits the original 40 ms edge-word bounds; the deeper
        // dip beyond them becomes reachable if corrected words are used first.
        let output = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "aevalsrc='0.5*sin(2*PI*200*t)*if(between(t,0.505,0.525),0.01,if(between(t,0.535,0.565),0,1))':s=16000:d=1",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&audio)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let sources = [VideoSource {
            id: "a".into(),
            name: None,
            source: temp.path().join("source.mp4"),
            transcript: temp.path().join("transcript.json"),
            audio,
            hash: None,
        }];
        let cues = [
            (7, [("hello", 200, 460), ("a", 460, 500)]),
            (11, [("b", 500, 540), ("world", 540, 900)]),
        ]
        .into_iter()
        .map(|(cue_id, words)| TranscriptCue {
            cue_id,
            start: Duration::from_millis(words[0].1),
            end: Duration::from_millis(words[1].2),
            text: format!("{} {}", words[0].0, words[1].0),
            words: words
                .into_iter()
                .map(|(word, start, end)| WordTiming {
                    word: word.into(),
                    start: Duration::from_millis(start),
                    end: Duration::from_millis(end),
                })
                .collect(),
            source_id: "a".into(),
        })
        .collect::<Vec<_>>();
        let markdown_path = temp.path().join("edit.md");
        let document = |first: &str, second: &str| {
            parse_video_document(
                &format!(
                    "`a#7@00:00:00.2-00:00:00.5` {first}\n\
                     `a#11@00:00:00.5-00:00:00.9` {second}\n"
                ),
                &markdown_path,
            )
        };
        let original = document("hello a", "b world")?;
        let corrected = document("greetings", "everyone")?;
        let windows = |plan: &TimelinePlan| {
            plan.items
                .iter()
                .map(|item| {
                    let TimelinePlanItem::Clip(clip) = item else {
                        panic!("expected dialogue clip");
                    };
                    assert_eq!(clip.source_id, "a");
                    (clip.cue_id, clip.time_window.start, clip.time_window.end)
                })
                .collect::<Vec<_>>()
        };
        let original_plan = build_timeline_plan(&original, &cues, &sources, &markdown_path)?;
        let corrected_plan = build_timeline_plan(&corrected, &cues, &sources, &markdown_path)?;
        let planned_windows = windows(&original_plan);
        assert_eq!(windows(&corrected_plan), planned_windows);
        assert_eq!(planned_windows[0].0, Some(7));
        assert_eq!(planned_windows[1].0, Some(11));
        let cut = planned_windows[0].2;
        assert_eq!(cut, planned_windows[1].1);
        assert!(
            (0.505..=0.52).contains(&cut),
            "cut {cut} must snap into the dip, not remain at the plain 0.5 s boundary"
        );

        let mut edited_cues = cues.clone();
        apply_markdown_edits(&mut edited_cues, &corrected);
        for (original, edited) in cues.iter().zip(&edited_cues) {
            assert_eq!(edited.cue_id, original.cue_id);
            assert_eq!(original.words.len(), 2);
            assert_eq!(edited.words.len(), 1);
            assert_ne!(edited.words[0].end, original.words[0].end);
        }
        assert_eq!(edited_cues[0].text, "greetings");
        assert_eq!(edited_cues[1].text, "everyone");
        assert_eq!(windows(&original_plan), planned_windows);
        assert_eq!(windows(&corrected_plan), planned_windows);

        // Ensure this fixture detects the incorrect edit-before-plan ordering.
        let replanned = build_timeline_plan(&corrected, &edited_cues, &sources, &markdown_path)?;
        assert_ne!(windows(&replanned), planned_windows);
        Ok(())
    }
}
