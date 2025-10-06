use anyhow::{Context, Result, bail};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SrtCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

pub fn parse_srt(input: &str) -> Result<Vec<SrtCue>> {
    let mut cues = Vec::new();
    let mut lines = input.lines().peekable();

    while let Some(line) = lines.next() {
        let index_line = line.trim();
        if index_line.is_empty() {
            continue;
        }

        // Index line can sometimes be omitted; skip validation if it fails to parse
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
            bail!("SRT cue ends before it starts: {start_raw} --> {end_raw}");
        }

        let mut text_lines = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            text_lines.push(lines.next().unwrap().trim().to_string());
        }

        // Consume trailing blank lines between cues
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                lines.next();
            } else {
                break;
            }
        }

        cues.push(SrtCue {
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

    if hms.next().is_some() {
        bail!("Timestamp has more than three components: {value}");
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_srt() {
        let input = "1\n00:00:01,000 --> 00:00:03,500\nHello world!\n\n2\n00:00:04,000 --> 00:00:05,000\nNext line\n";
        let cues = parse_srt(input).expect("parse srt");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello world!");
        assert_eq!(cues[1].start.as_millis(), 4000);
    }
}
