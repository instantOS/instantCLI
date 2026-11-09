use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Independent speech analysis of the full, original recording, not a transcript.
/// An empty `speech` list means analysis found no speech; missing analysis is `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechActivity {
    pub version: u32,
    pub duration_seconds: f64,
    pub speech: Vec<SpeechInterval>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechInterval {
    pub start: f64,
    pub end: f64,
}

impl SpeechActivity {
    /// Loads a version-1 sidecar without repairing or extending its coverage.
    pub fn from_json(json: &str) -> Result<Self> {
        let activity: Self =
            serde_json::from_str(json).context("Failed to parse speech activity JSON")?;
        activity.validate()?;
        Ok(activity)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported speech activity version");
        ensure!(
            self.duration_seconds.is_finite() && self.duration_seconds > 0.0,
            "Speech activity duration must be finite and positive"
        );

        let mut previous_end = 0.0;
        for (index, interval) in self.speech.iter().enumerate() {
            ensure!(
                interval.start.is_finite()
                    && interval.end.is_finite()
                    && interval.start >= previous_end
                    && interval.start < interval.end
                    && interval.end <= self.duration_seconds,
                "Invalid speech activity interval {index}: expected sorted, non-overlapping intervals within the recording"
            );
            previous_end = interval.end;
        }
        Ok(())
    }

    /// Suggests review only for fully analyzed cues of at least half a second
    /// with at most 10% detected speech. This never changes transcript timing.
    pub fn is_suspected_non_speech(&self, start: Duration, end: Duration) -> bool {
        // Public fields also allow direct construction; invalid analysis must
        // never turn into evidence against a transcript cue.
        if self.validate().is_err() || end <= start || end - start < Duration::from_millis(500) {
            return false;
        }

        let span = (end - start).as_secs_f64();
        let start = start.as_secs_f64();
        let end = end.as_secs_f64();
        if end > self.duration_seconds || end <= start {
            return false;
        }

        let overlap: f64 = self
            .speech
            .iter()
            .take_while(|interval| interval.start < end)
            .map(|interval| (interval.end.min(end) - interval.start.max(start)).max(0.0))
            .sum();
        overlap <= span * 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suspected(activity: &SpeechActivity, start_ms: u64, end_ms: u64) -> bool {
        activity.is_suspected_non_speech(
            Duration::from_millis(start_ms),
            Duration::from_millis(end_ms),
        )
    }

    #[test]
    fn no_detected_speech_flags_silence_or_noise() -> Result<()> {
        let activity =
            SpeechActivity::from_json(r#"{"version":1,"duration_seconds":10.0,"speech":[]}"#)?;
        assert!(suspected(&activity, 0, 500));
        assert!(suspected(&activity, 0, 10_000));
        assert!(suspected(&activity, 9_500, 10_000));
        Ok(())
    }

    #[test]
    fn detected_speech_protects_cues() -> Result<()> {
        let activity = SpeechActivity::from_json(
            r#"{"version":1,"duration_seconds":10.0,"speech":[{"start":0.0,"end":10.0}]}"#,
        )?;
        assert!(!suspected(&activity, 0, 500));
        assert!(!suspected(&activity, 0, 10_000));
        assert!(!suspected(&activity, 9_500, 10_000));
        Ok(())
    }

    #[test]
    fn mixed_audio_sums_and_clips_speech_overlap() -> Result<()> {
        let activity = SpeechActivity::from_json(
            r#"{"version":1,"duration_seconds":10.0,"speech":[
                {"start":0.0,"end":1.0},
                {"start":2.0,"end":2.5},
                {"start":3.0,"end":3.5},
                {"start":8.0,"end":10.0}
            ]}"#,
        )?;
        assert!(suspected(&activity, 3_500, 8_000));
        assert!(!suspected(&activity, 2_000, 4_000));
        assert!(!suspected(&activity, 500, 2_500));
        assert!(!suspected(&activity, 7_000, 9_000));
        Ok(())
    }

    #[test]
    fn overlap_threshold_is_inclusive_and_uses_total_speech() -> Result<()> {
        let activity = SpeechActivity::from_json(
            r#"{"version":1,"duration_seconds":10.0,"speech":[
                {"start":0.0,"end":0.5},
                {"start":2.0,"end":2.5}
            ]}"#,
        )?;
        assert!(suspected(&activity, 0, 10_000)); // Exactly 10% speech.
        assert!(!suspected(&activity, 0, 9_999));
        assert!(!suspected(&activity, 0, 500));
        Ok(())
    }

    #[test]
    fn short_reversed_and_out_of_range_cues_are_not_flagged() -> Result<()> {
        let activity =
            SpeechActivity::from_json(r#"{"version":1,"duration_seconds":10.0,"speech":[]}"#)?;
        for (start, end) in [
            (0, 499),
            (5_000, 5_499),
            (1_000, 1_000),
            (2_000, 1_000),
            (9_500, 10_001),
            (10_000, 11_000),
            (11_000, 12_000),
        ] {
            assert!(!suspected(&activity, start, end), "{start}..{end}");
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_cached_data() {
        for json in [
            "not JSON",
            "null",
            "{}",
            r#"{"version":2,"duration_seconds":10,"speech":[]}"#,
            r#"{"version":1,"duration_seconds":0,"speech":[]}"#,
            r#"{"version":1,"duration_seconds":-1,"speech":[]}"#,
            r#"{"version":1,"duration_seconds":1e999,"speech":[]}"#,
            r#"{"version":1,"duration_seconds":10}"#,
            r#"{"version":1,"duration_seconds":10,"speech":null}"#,
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":-1,"end":1}]}"#,
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":1,"end":1}]}"#,
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":2,"end":1}]}"#,
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":9,"end":11}]}"#,
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":1,"end":1e999}]}"#,
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":2,"end":3},{"start":1,"end":2}]}"#,
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":1,"end":3},{"start":2,"end":4}]}"#,
        ] {
            assert!(SpeechActivity::from_json(json).is_err(), "{json}");
        }
    }

    #[test]
    fn invalid_directly_constructed_activity_fails_safe() {
        let mut activity = SpeechActivity {
            version: 1,
            duration_seconds: 10.0,
            speech: vec![],
        };
        for duration in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            activity.duration_seconds = duration;
            assert!(!suspected(&activity, 0, 1_000));
        }
        activity.duration_seconds = 10.0;
        for (start, end) in [
            (f64::NAN, 1.0),
            (0.0, f64::NAN),
            (0.0, f64::INFINITY),
            (f64::NEG_INFINITY, 1.0),
        ] {
            activity.speech = vec![SpeechInterval { start, end }];
            assert!(!suspected(&activity, 0, 1_000));
        }
        activity.speech.clear();
        activity.version = 2;
        assert!(!suspected(&activity, 0, 1_000));
    }

    #[test]
    fn valid_activity_serialization_round_trips() -> Result<()> {
        let activity = SpeechActivity::from_json(
            r#"{"version":1,"duration_seconds":10,"speech":[{"start":0,"end":1},{"start":1,"end":10}]}"#,
        )?;
        let restored = SpeechActivity::from_json(&serde_json::to_string(&activity)?)?;
        assert_eq!(restored, activity);
        Ok(())
    }
}
