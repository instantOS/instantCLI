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

/// A parsed mirrorlist: the text as it stands, and the active `Server =` lines
/// within it, carrying the line indices they occupy.
///
/// The servers stay attached to the content they were parsed from, so every
/// operation on this type indexes lines of *this* text and a [`MirrorEntry`]
/// belonging to a different mirrorlist is rejected rather than applied here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorList {
    content: String,
    servers: Vec<MirrorEntry>,
}

impl MirrorList {
    /// Parse a mirrorlist, keeping only active (uncommented) servers in pacman's
    /// configured order.
    pub fn parse(content: &str) -> Result<Self> {
        let servers = content
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
            .collect();
        Ok(Self {
            content: content.to_string(),
            servers,
        })
    }

    pub fn servers(&self) -> &[MirrorEntry] {
        &self.servers
    }

    /// The mirror pacman would try first.
    pub fn primary(&self) -> Result<&MirrorEntry> {
        self.servers
            .first()
            .ok_or_else(|| anyhow!("Mirrorlist contains no active Server entries"))
    }

    /// Whether any server in this list is fetched over the network, as opposed
    /// to a local `file://` source.
    pub fn has_network_mirrors(&self) -> bool {
        self.servers.iter().any(MirrorEntry::is_network)
    }

    /// Probe the configured mirrors in order and return the first healthy one,
    /// with how many were probed to find it.
    pub async fn first_healthy(
        &self,
        client: &reqwest::Client,
        limit: usize,
    ) -> Result<(MirrorProbe, usize)> {
        let candidates = self.servers.iter().take(limit.max(1));
        let mut attempts = 0;
        let mut failures = Vec::new();

        for mirror in candidates {
            attempts += 1;
            match mirror.probe(client).await {
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

    /// The text as it stands, exactly as it was parsed.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// This mirrorlist with `server` swapped into the first server position, or
    /// `None` when `server` is already first and the text would not change.
    ///
    /// Only line contents are swapped, so comments, blank lines, and the file's
    /// original newline style remain intact. Nothing else is reordered, no
    /// mirror is removed, and the list is not otherwise tidied — this promotes
    /// one mirror and nothing more.
    ///
    /// The servers and their line indices stay attached to the content they
    /// were parsed from, so the result can be promoted again or re-probed
    /// without re-parsing.
    ///
    /// `server` must come from this list: a server parsed out of a different
    /// mirrorlist is rejected rather than used to index this one's lines.
    pub fn promote(&self, server: &MirrorEntry) -> Result<Option<Self>> {
        let Some(entry) = self
            .servers
            .iter()
            .find(|candidate| candidate.line_index == server.line_index)
        else {
            return Err(anyhow!(
                "Selected mirror line {} is not an active Server entry",
                server.line_index
            ));
        };
        if entry.template != server.template {
            return Err(anyhow!(
                "Selected mirror line {} ({}) does not belong to this mirrorlist",
                server.line_index,
                server.template
            ));
        }

        let first = self.primary()?;
        if first.line_index == entry.line_index {
            return Ok(None);
        }

        let mut lines: Vec<LinePart<'_>> = split_lines_preserving_endings(&self.content).collect();
        let first_content = lines[first.line_index].content.to_string();
        let selected_content = lines[entry.line_index].content.to_string();
        lines[first.line_index].replacement = Some(selected_content);
        lines[entry.line_index].replacement = Some(first_content);

        let mut output = String::with_capacity(self.content.len());
        for line in lines {
            output.push_str(line.replacement.as_deref().unwrap_or(line.content));
            output.push_str(line.ending);
        }
        Ok(Some(Self::parse(&output)?))
    }
}

impl MirrorEntry {
    /// Whether pacman fetches this mirror over the network, as opposed to a
    /// local `file://` source.
    ///
    /// The comparison is case-insensitive because the URL parser lowercases
    /// schemes, so `HTTP://` probes successfully and is a network mirror here.
    pub fn is_network(&self) -> bool {
        let scheme_end = self.template.find("://");
        let scheme = match scheme_end {
            Some(end) => &self.template[..end],
            None => return false,
        };
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    }

    /// The URL to request when checking this mirror: the `core` database, with
    /// pacman's template variables expanded.
    pub fn probe_url(&self) -> Result<String> {
        let expanded = self
            .template
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

    /// Ask this mirror whether it is serving real repository data.
    pub async fn probe(&self, client: &reqwest::Client) -> Result<MirrorProbe> {
        let requested_url = self.probe_url()?;
        let started = Instant::now();
        let response = client
            .get(&requested_url)
            .header(RANGE, "bytes=0-1023")
            .send()
            .await
            .with_context(|| format!("Could not reach {}", self.template))?;
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
            mirror: self.clone(),
            latency,
        })
    }
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

        let mirrors = MirrorList::parse(content).unwrap();
        assert_eq!(mirrors.servers().len(), 2);
        assert_eq!(mirrors.servers()[0].line_index, 1);
        assert_eq!(
            mirrors.servers()[0].template,
            "https://one.example/$repo/os/$arch"
        );
        assert_eq!(mirrors.servers()[1].line_index, 2);
    }

    #[test]
    fn creates_repository_database_probe_url() {
        let entry = MirrorEntry {
            line_index: 0,
            template: "https://mirror.example/${repo}/os/${arch}/".to_string(),
        };
        assert_eq!(
            entry.probe_url().unwrap(),
            format!(
                "https://mirror.example/core/os/{}/core.db",
                std::env::consts::ARCH
            )
        );
    }

    #[test]
    fn rejects_unknown_template_variables() {
        let entry = MirrorEntry {
            line_index: 0,
            template: "https://mirror.example/$repo/$unknown".to_string(),
        };
        let error = entry.probe_url().unwrap_err();
        assert!(error.to_string().contains("unsupported variables"));
    }

    #[test]
    fn network_mirrors_are_http_and_https_and_nothing_else() {
        let list = MirrorList::parse(
            "\
#Server = https://commented.example/$repo/os/$arch
Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch
",
        )
        .unwrap();
        // The commented entry does not count, so this list is bundle-only.
        assert!(!list.has_network_mirrors());

        let networked = MirrorList::parse("Server = https://one.example/$repo/os/$arch\n").unwrap();
        assert!(networked.has_network_mirrors());

        // The URL parser lowercases schemes, so an uppercase one probes fine
        // and must not read as a non-network mirror here.
        let shouty = MirrorList::parse("Server = HTTP://one.example/$repo/os/$arch\n").unwrap();
        assert!(shouty.has_network_mirrors());
        assert!(shouty.servers()[0].probe_url().is_ok());
    }

    #[test]
    fn promotion_preserves_formatting_and_newlines() {
        let content = "## First\r\nServer = https://one/$repo/os/$arch\r\n\r\n## Second\r\nServer = https://two/$repo/os/$arch\r\n";
        let list = MirrorList::parse(content).unwrap();
        let second = list.servers()[1].clone();
        let promoted = list
            .promote(&second)
            .unwrap()
            .expect("second is not the primary");
        assert_eq!(
            promoted.content(),
            "## First\r\nServer = https://two/$repo/os/$arch\r\n\r\n## Second\r\nServer = https://one/$repo/os/$arch\r\n"
        );
        // The servers describe the text this result now holds.
        assert_eq!(
            promoted.primary().unwrap().template,
            "https://two/$repo/os/$arch"
        );
    }

    #[test]
    fn promoting_the_primary_mirror_reports_no_change() {
        let list = MirrorList::parse(
            "Server = https://one/$repo/os/$arch\nServer = https://two/$repo/os/$arch\n",
        )
        .unwrap();
        let primary = list.servers()[0].clone();
        assert!(list.promote(&primary).unwrap().is_none());
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

        let probe = mirror.probe(&http_client().unwrap()).await.unwrap();
        assert_eq!(probe.mirror, mirror);
    }

    #[tokio::test]
    async fn probe_rejects_http_errors_and_html_success_pages() {
        let not_found = MirrorEntry {
            line_index: 0,
            template: serve_once("404 Not Found", "text/html", b"<html>missing</html>").await,
        };
        let error = not_found.probe(&http_client().unwrap()).await.unwrap_err();
        assert!(error.to_string().contains("HTTP 404"));

        let disguised_html = MirrorEntry {
            line_index: 0,
            template: serve_once("200 OK", "application/octet-stream", b"<!doctype html>oops")
                .await,
        };
        let error = disguised_html
            .probe(&http_client().unwrap())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("HTML page"));
    }

    #[tokio::test]
    async fn selecting_a_healthy_mirror_and_promoting_it_are_separate_steps() {
        let broken = serve_once("404 Not Found", "text/html", b"missing").await;
        let healthy = serve_once(
            "206 Partial Content",
            "application/octet-stream",
            b"repo-data",
        )
        .await;
        let content = format!("Server = {broken}\nServer = {healthy}\n");

        let list = MirrorList::parse(&content).unwrap();
        let client = http_client().unwrap();
        let (selected, attempts) = list.first_healthy(&client, 8).await.unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(selected.mirror.template, healthy);

        // Selecting does not touch the text; promoting does.
        assert_eq!(list.primary().unwrap().template, broken);
        let promoted = list
            .promote(&selected.mirror)
            .unwrap()
            .expect("selected mirror is not the primary");
        assert!(
            promoted
                .content()
                .starts_with(&format!("Server = {healthy}\n"))
        );
    }

    #[test]
    fn promoting_rejects_a_server_belonging_to_another_mirrorlist() {
        // A server from another mirrorlist is refused even when its line index
        // happens to exist here, because that index may point at a different
        // server: only the template tells them apart.
        let one = MirrorList::parse(
            "Server = https://one/$repo/os/$arch\nServer = https://two/$repo/os/$arch\n",
        )
        .unwrap();
        let elsewhere = MirrorList::parse(
            "Server = https://one/$repo/os/$arch\nServer = https://elsewhere/$repo/os/$arch\n",
        )
        .unwrap();

        // Same line index, different mirror: refused, not silently used.
        let error = one.promote(&elsewhere.servers()[1]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not belong to this mirrorlist"),
            "{error}"
        );

        // An index this list does not have at all is a different mistake.
        let short = MirrorList::parse("Server = https://one/$repo/os/$arch\n").unwrap();
        let error = short.promote(&elsewhere.servers()[1]).unwrap_err();
        assert!(
            error.to_string().contains("is not an active Server entry"),
            "{error}"
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
