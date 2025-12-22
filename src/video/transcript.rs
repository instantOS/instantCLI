use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TranscriptCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
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
    score: f64,
}

const MAX_CLUSTER_SIZE: usize = 10;
const PAUSE_THRESHOLD_SECONDS: f64 = 0.6;

pub fn parse_whisper_json(json_str: &str) -> Result<Vec<TranscriptCue>> {
    let output: WhisperOutput = serde_json::from_str(json_str)
        .context("Failed to parse WhisperX JSON output")?;

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
            if pause > PAUSE_THRESHOLD_SECONDS {
                flush = true;
            } else if current_cluster.len() >= MAX_CLUSTER_SIZE {
                flush = true;
            }
        }

        if flush {
            if !current_cluster.is_empty() {
                cues.push(create_cue_from_cluster(&current_cluster));
                current_cluster.clear();
            }
        }

        current_cluster.push(word);
    }

    if !current_cluster.is_empty() {
        cues.push(create_cue_from_cluster(&current_cluster));
    }

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

    TranscriptCue {
        start: Duration::from_secs_f64(start),
        end: Duration::from_secs_f64(end),
        text,
    }
}

// Keep the SRT parser for backward compatibility or direct SRT usage
pub fn parse_srt(input: &str) -> Result<Vec<TranscriptCue>> {
    let mut cues = Vec::new();
    let mut lines = input.lines().peekable();

    while let Some(line) = lines.next() {
        let index_line = line.trim();
        if index_line.is_empty() {
            continue;
        }

        let _ = index_line.parse::<usize>();

        let times = lines
            .next()
            .map(str::trim)
            .context("SRT cue is missing a timestamp line")?;

        let (start_raw, end_raw) = times
            .split_once("-->")
            .map(|(a, b)| (a.trim(), b.trim()))
            .context("SRT cue timestamp line must contain '-->'")?;

        let start = parse_timestamp(start_raw)
            .with_context(|| format!("Failed to parse SRT start timestamp '{start_raw}'"))?;
        let end = parse_timestamp(end_raw)
            .with_context(|| format!("Failed to parse SRT end timestamp '{end_raw}'"))?;

        if end < start {
            anyhow::bail!("SRT cue ends before it starts: {start_raw} --> {end_raw}");
        }

        let mut text_lines = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            text_lines.push(lines.next().unwrap().trim().to_string());
        }

        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                lines.next();
            } else {
                break;
            }
        }

        cues.push(TranscriptCue {
            start,
            end,
            text: text_lines.join(" "),
        });
    }

    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

fn parse_timestamp(value: &str) -> Result<Duration> {
    let cleaned = value.trim().replace(',', ".");
    let mut parts = cleaned.split('.');
    let time_part = parts
        .next()
        .context("Timestamp is missing time component (HH:MM:SS)")?;
    let fractional_part = parts.next().unwrap_or("0");

    let mut hms = time_part.split(':');
    let hours = hms
        .next()
        .context("Timestamp missing hours")?
        .parse::<u64>()
        .context("Invalid hours in timestamp")?;
    let minutes = hms
        .next()
        .context("Timestamp missing minutes")?
        .parse::<u64>()
        .context("Invalid minutes in timestamp")?;
    let seconds = hms
        .next()
        .context("Timestamp missing seconds")?
        .parse::<u64>()
        .context("Invalid seconds in timestamp")?;

    let mut millis_str = fractional_part.to_string();
    if millis_str.len() < 3 {
        millis_str.push_str(&"0".repeat(3 - millis_str.len()));
    }
    let millis = millis_str
        .chars()
        .take(3)
        .collect::<String>()
        .parse::<u64>()
        .context("Invalid millisecond component in timestamp")?;

    let total_seconds = hours * 3600 + minutes * 60 + seconds;
    Ok(Duration::from_secs(total_seconds) + Duration::from_millis(millis))
}
