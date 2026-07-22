//! Shared parsing and health checks for pacman mirrorlists.
//!
//! A mirror is considered usable when a small request for the `core` repository
//! database succeeds and does not look like an HTML error page.  Both the Arch
//! installer and `ins doctor` use this module so they agree on mirror health.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_TYPE, RANGE};

pub const DEFAULT_PROBE_LIMIT: usize = 8;
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_REPOSITORY: &str = "core";
const PROBE_DATABASE: &str = "core.db";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorEntry {
    pub line_index: usize,
    pub template: String,
}

#[derive(Debug, Clone)]
pub struct MirrorProbe {
    pub mirror: MirrorEntry,
    pub latency: Duration,
}

#[derive(Debug)]
pub struct PreparedMirrorlist {
    pub content: String,
    pub selected: MirrorProbe,
    pub attempts: usize,
}

pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(format!("ins/{}/mirror-check", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to create mirror health-check HTTP client")
}

/// Return active (uncommented) servers in pacman's configured order.
pub fn active_mirrors(content: &str) -> Vec<MirrorEntry> {
    content
        .lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let (key, value) = line.trim().split_once('=')?;
            if key.trim() != "Server" || value.trim().is_empty() {
                return None;
            }
            Some(MirrorEntry {
                line_index,
                template: value.trim().to_string(),
            })
        })
        .collect()
}

pub fn probe_url(template: &str) -> Result<String> {
    let expanded = template
        .replace("${repo}", PROBE_REPOSITORY)
        .replace("$repo", PROBE_REPOSITORY)
        .replace("${arch}", std::env::consts::ARCH)
        .replace("$arch", std::env::consts::ARCH);

    if expanded.contains('$') {
        return Err(anyhow!(
            "mirror URL contains unsupported variables after expansion: {expanded}"
        ));
    }

    let mut url = reqwest::Url::parse(&expanded)
        .with_context(|| format!("Invalid mirror URL: {expanded}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(anyhow!("Unsupported mirror URL scheme: {}", url.scheme()));
    }

    let path = format!("{}/{}", url.path().trim_end_matches('/'), PROBE_DATABASE);
    url.set_path(&path);
    Ok(url.to_string())
}

pub async fn probe_mirror(client: &reqwest::Client, mirror: &MirrorEntry) -> Result<MirrorProbe> {
    let requested_url = probe_url(&mirror.template)?;
    let started = Instant::now();
    let response = client
        .get(&requested_url)
        .header(RANGE, "bytes=0-1023")
        .send()
        .await
        .with_context(|| format!("Could not reach {}", mirror.template))?;
    let latency = started.elapsed();
    let status = response.status();
    let final_url = response.url().to_string();

    if !status.is_success() {
        return Err(anyhow!("HTTP {status} from {final_url}"));
    }

    if response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(content_type_is_html)
    {
        return Err(anyhow!("Mirror returned HTML instead of repository data"));
    }

    let mut stream = response.bytes_stream();
    let first_chunk = stream
        .next()
        .await
        .transpose()
        .context("Failed to read mirror response")?
        .ok_or_else(|| anyhow!("Mirror returned an empty response"))?;

    if body_looks_like_html(&first_chunk) {
        return Err(anyhow!(
            "Mirror returned an HTML page instead of repository data"
        ));
    }

    Ok(MirrorProbe {
        mirror: mirror.clone(),
        latency,
    })
}

/// Probe mirrors in their configured order and return the first healthy one.
pub async fn first_healthy_mirror(
    client: &reqwest::Client,
    mirrors: &[MirrorEntry],
    limit: usize,
) -> Result<(MirrorProbe, usize)> {
    let candidates = mirrors.iter().take(limit.max(1));
    let mut attempts = 0;
    let mut failures = Vec::new();

    for mirror in candidates {
        attempts += 1;
        match probe_mirror(client, mirror).await {
            Ok(probe) => return Ok((probe, attempts)),
            Err(error) => failures.push(format!("{}: {error:#}", mirror.template)),
        }
    }

    if attempts == 0 {
        return Err(anyhow!("Mirrorlist contains no active Server entries"));
    }

    Err(anyhow!(
        "No healthy mirror found in the first {attempts} candidate(s): {}",
        failures.join("; ")
    ))
}

/// Promote a mirror by swapping it with the first active server line.
///
/// Only line contents are swapped, so comments, blank lines, and the file's
/// original newline style remain intact.
pub fn promote_mirror(content: &str, selected_line_index: usize) -> Result<String> {
    let mirrors = active_mirrors(content);
    let first = mirrors
        .first()
        .ok_or_else(|| anyhow!("Mirrorlist contains no active Server entries"))?;
    if !mirrors
        .iter()
        .any(|mirror| mirror.line_index == selected_line_index)
    {
        return Err(anyhow!(
            "Selected mirror line {selected_line_index} is not an active Server entry"
        ));
    }
    if first.line_index == selected_line_index {
        return Ok(content.to_string());
    }

    let mut lines: Vec<LinePart<'_>> = split_lines_preserving_endings(content).collect();
    let first_content = lines[first.line_index].content.to_string();
    let selected_content = lines[selected_line_index].content.to_string();
    lines[first.line_index].replacement = Some(selected_content);
    lines[selected_line_index].replacement = Some(first_content);

    let mut output = String::with_capacity(content.len());
    for line in lines {
        output.push_str(line.replacement.as_deref().unwrap_or(line.content));
        output.push_str(line.ending);
    }
    Ok(output)
}

pub async fn prepare_mirrorlist(content: &str, limit: usize) -> Result<PreparedMirrorlist> {
    let mirrors = active_mirrors(content);
    let client = http_client()?;
    let (selected, attempts) = first_healthy_mirror(&client, &mirrors, limit).await?;
    let promoted = promote_mirror(content, selected.mirror.line_index)?;
    Ok(PreparedMirrorlist {
        content: promoted,
        selected,
        attempts,
    })
}

/// Rewrite a mirrorlist without changing the existing file's permissions.
pub fn write_mirrorlist(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("Failed to open {} for writing", path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("Failed to sync {}", path.display()))?;
    Ok(())
}

fn content_type_is_html(value: &str) -> bool {
    let media_type = value.split(';').next().unwrap_or(value).trim();
    media_type.eq_ignore_ascii_case("text/html")
        || media_type.eq_ignore_ascii_case("application/xhtml+xml")
}

fn body_looks_like_html(body: &[u8]) -> bool {
    let sample_len = body.len().min(512);
    let sample = String::from_utf8_lossy(&body[..sample_len]);
    let trimmed = sample.trim_start().to_ascii_lowercase();
    trimmed.starts_with("<!doctype html")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<head")
        || trimmed.starts_with("<body")
}

#[derive(Debug)]
struct LinePart<'a> {
    content: &'a str,
    ending: &'a str,
    replacement: Option<String>,
}

fn split_lines_preserving_endings(content: &str) -> impl Iterator<Item = LinePart<'_>> {
    content.split_inclusive('\n').map(|line| {
        let (content, ending) = if let Some(content) = line.strip_suffix("\r\n") {
            (content, "\r\n")
        } else if let Some(content) = line.strip_suffix('\n') {
            (content, "\n")
        } else {
            (line, "")
        };
        LinePart {
            content,
            ending,
            replacement: None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    #[test]
    fn parses_only_active_server_lines() {
        let content = "#Server = https://disabled/$repo/os/$arch\n\
Server = https://one.example/$repo/os/$arch\n\
  Server = https://two.example/$repo/os/$arch  \n\
CacheServer = https://cache.example/$repo/os/$arch\n";

        let mirrors = active_mirrors(content);
        assert_eq!(mirrors.len(), 2);
        assert_eq!(mirrors[0].line_index, 1);
        assert_eq!(mirrors[0].template, "https://one.example/$repo/os/$arch");
        assert_eq!(mirrors[1].line_index, 2);
    }

    #[test]
    fn creates_repository_database_probe_url() {
        assert_eq!(
            probe_url("https://mirror.example/${repo}/os/${arch}/").unwrap(),
            format!(
                "https://mirror.example/core/os/{}/core.db",
                std::env::consts::ARCH
            )
        );
    }

    #[test]
    fn rejects_unknown_template_variables() {
        let error = probe_url("https://mirror.example/$repo/$unknown").unwrap_err();
        assert!(error.to_string().contains("unsupported variables"));
    }

    #[test]
    fn promotion_preserves_formatting_and_newlines() {
        let content = "## First\r\nServer = https://one/$repo/os/$arch\r\n\r\n## Second\r\nServer = https://two/$repo/os/$arch\r\n";
        let promoted = promote_mirror(content, 4).unwrap();
        assert_eq!(
            promoted,
            "## First\r\nServer = https://two/$repo/os/$arch\r\n\r\n## Second\r\nServer = https://one/$repo/os/$arch\r\n"
        );
    }

    #[test]
    fn html_detection_is_narrow_and_case_insensitive() {
        assert!(content_type_is_html("text/html; charset=utf-8"));
        assert!(body_looks_like_html(b"  <!DOCTYPE HTML><title>404</title>"));
        assert!(!body_looks_like_html(&[0x1f, 0x8b, 0x08, 0x00]));
    }

    #[tokio::test]
    async fn probe_accepts_repository_data() {
        let template = serve_once(
            "206 Partial Content",
            "application/octet-stream",
            b"repo-data",
        )
        .await;
        let mirror = MirrorEntry {
            line_index: 0,
            template,
        };

        let probe = probe_mirror(&http_client().unwrap(), &mirror)
            .await
            .unwrap();
        assert_eq!(probe.mirror, mirror);
    }

    #[tokio::test]
    async fn probe_rejects_http_errors_and_html_success_pages() {
        let not_found = MirrorEntry {
            line_index: 0,
            template: serve_once("404 Not Found", "text/html", b"<html>missing</html>").await,
        };
        let error = probe_mirror(&http_client().unwrap(), &not_found)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("HTTP 404"));

        let disguised_html = MirrorEntry {
            line_index: 0,
            template: serve_once("200 OK", "application/octet-stream", b"<!doctype html>oops")
                .await,
        };
        let error = probe_mirror(&http_client().unwrap(), &disguised_html)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("HTML page"));
    }

    #[tokio::test]
    async fn preparation_promotes_first_healthy_fallback() {
        let broken = serve_once("404 Not Found", "text/html", b"missing").await;
        let healthy = serve_once(
            "206 Partial Content",
            "application/octet-stream",
            b"repo-data",
        )
        .await;
        let content = format!("Server = {broken}\nServer = {healthy}\n");

        let prepared = prepare_mirrorlist(&content, 8).await.unwrap();
        assert_eq!(prepared.attempts, 2);
        assert_eq!(prepared.selected.mirror.template, healthy);
        assert!(
            prepared
                .content
                .starts_with(&format!("Server = {healthy}\n"))
        );
    }

    async fn serve_once(status: &str, content_type: &str, body: &[u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let content_type = content_type.to_string();
        let body = body.to_vec();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
        });

        format!("http://{address}/$repo/os/$arch")
    }
}
