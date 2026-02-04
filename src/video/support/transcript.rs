use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

/// A single word with its timing information.
#[derive(Debug, Clone)]
pub struct WordTiming {
    pub word: String,
    pub start: Duration,
    pub end: Duration,
}

#[derive(Debug, Clone)]
pub struct TranscriptCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
    /// Individual word timings for karaoke-style highlighting.
    /// If empty, the cue text is displayed without word-level highlighting.
    pub words: Vec<WordTiming>,
    pub source_id: String,
}

#[derive(Debug, Deserialize)]
struct WhisperOutput {
    segments: Vec<WhisperSegment>,
}

#[derive(Debug, Deserialize)]
struct WhisperSegment {
    #[serde(default)]
    words: Vec<WhisperWord>,
    // Fallback if words are missing (e.g. no alignment)
    #[serde(default)]
    start: f64,
    #[serde(default)]
    end: f64,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct WhisperWord {
    word: String,
    start: f64,
    end: f64,
    #[serde(default)]
    #[allow(dead_code)]
    score: f64,
}

const MAX_CLUSTER_SIZE: usize = 10;
const PAUSE_THRESHOLD_SECONDS: f64 = 0.6;

pub fn parse_whisper_json(json_str: &str) -> Result<Vec<TranscriptCue>> {
    let output: WhisperOutput =
        serde_json::from_str(json_str).context("Failed to parse WhisperX JSON output")?;

    let mut cues = Vec::new();
    let mut current_cluster: Vec<WhisperWord> = Vec::new();

    // Flatten all words from all segments
    // If a segment has no words, we treat the whole segment as a "word" for fallback
    let mut all_words = Vec::new();
    for segment in output.segments {
        if !segment.words.is_empty() {
            all_words.extend(segment.words);
        } else if !segment.text.trim().is_empty() {
            // Fallback for segments without word alignment
            all_words.push(WhisperWord {
                word: segment.text.trim().to_string(),
                start: segment.start,
                end: segment.end,
                score: 0.0,
            });
        }
    }

    for word in all_words {
        let mut flush = false;

        if let Some(last) = current_cluster.last() {
            let pause = word.start - last.end;
            if pause > PAUSE_THRESHOLD_SECONDS || current_cluster.len() >= MAX_CLUSTER_SIZE {
                flush = true;
            }
        }

        if flush && !current_cluster.is_empty() {
            cues.push(create_cue_from_cluster(&current_cluster));
            current_cluster.clear();
        }

        current_cluster.push(word);
    }

    if !current_cluster.is_empty() {
        cues.push(create_cue_from_cluster(&current_cluster));
    }

    cues.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(cues)
}

fn create_cue_from_cluster(cluster: &[WhisperWord]) -> TranscriptCue {
    let start = cluster.first().map(|w| w.start).unwrap_or(0.0);
    let end = cluster.last().map(|w| w.end).unwrap_or(0.0);

    let text = cluster
        .iter()
        .map(|w| w.word.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    let words = cluster
        .iter()
        .map(|w| WordTiming {
            word: w.word.clone(),
            start: Duration::from_secs_f64(w.start),
            end: Duration::from_secs_f64(w.end),
        })
        .collect();

    TranscriptCue {
        start: Duration::from_secs_f64(start),
        end: Duration::from_secs_f64(end),
        text,
        words,
        source_id: "".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_whisper_json_clustering() {
        let json = r#"
        {
            "segments": [
                {
                    "start": 0.0,
                    "end": 2.0,
                    "text": "Hello world.",
                    "words": [
                        {"word": "Hello", "start": 0.0, "end": 0.5, "score": 0.9},
                        {"word": "world", "start": 0.6, "end": 1.0, "score": 0.8}
                    ]
                },
                {
                    "start": 2.0,
                    "end": 4.0,
                    "text": " Next phrase.",
                    "words": [
                        {"word": "Next", "start": 2.5, "end": 3.0, "score": 0.7},
                        {"word": "phrase", "start": 3.1, "end": 3.5, "score": 0.6}
                    ]
                }
            ]
        }
        "#;

        let cues = parse_whisper_json(json).expect("parse json");

        // With default threshold (0.6s) and max cluster size (10):
        // "Hello" (0.0-0.5)
        // "world" (0.6-1.0) -> gap 0.1s < 0.6s -> same cluster
        // "Next" (2.5-3.0) -> gap 1.5s > 0.6s -> new cluster
        // "phrase" (3.1-3.5) -> gap 0.1s < 0.6s -> same cluster

        assert_eq!(cues.len(), 2);

        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[0].start.as_secs_f64(), 0.0);
        assert_eq!(cues[0].end.as_secs_f64(), 1.0);

        assert_eq!(cues[1].text, "Next phrase");
        assert_eq!(cues[1].start.as_secs_f64(), 2.5);
        assert_eq!(cues[1].end.as_secs_f64(), 3.5);
    }

    #[test]
    fn test_long_pause_splitting() {
        let json = r#"
        {
            "segments": [{
                "words": [
                    {"word": "Word1", "start": 0.0, "end": 0.5},
                    {"word": "Word2", "start": 2.0, "end": 2.5}, 
                    {"word": "Word3", "start": 3.0, "end": 3.5}
                ]
            }]
        }
        "#;
        // Word1 end=0.5, Word2 start=2.0 -> gap 1.5 > 0.6 -> Split (new cluster starts at Word2)
        // Cluster 1: Word1

        // Word2 end=2.5, Word3 start=3.0 -> gap 0.5 < 0.6 -> Cluster
        // Cluster 2: Word2 Word3

        let cues = parse_whisper_json(json).expect("parse json");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Word1");
        assert_eq!(cues[1].text, "Word2 Word3");
        assert_eq!(cues[1].start.as_secs_f64(), 2.0);
        assert_eq!(cues[1].end.as_secs_f64(), 3.5);
    }

    #[test]
    fn test_max_cluster_size() {
        // Create 12 words with short gaps
        let mut words = Vec::new();
        for i in 0..12 {
            words.push(format!(
                r#"{{"word": "w{}", "start": {}, "end": {}}}"#,
                i,
                i as f64,
                i as f64 + 0.5
            ));
        }
        let json = format!(
            r#"{{ "segments": [ {{ "words": [{}] }} ] }}"#,
            words.join(",")
        );

        let cues = parse_whisper_json(&json).expect("parse json");
        // Should split after 10 words
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text.matches(' ').count() + 1, 10); // 10 words
        assert_eq!(cues[1].text.matches(' ').count() + 1, 2); // 2 words
    }
}
