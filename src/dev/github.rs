use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone)]
pub struct GitHubRepo {
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub clone_url: String,
    pub default_branch: String,
    pub html_url: String,
    pub language: Option<String>,
    pub stargazers_count: Option<u32>,
    pub forks_count: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GitHubError {
    pub message: String,
    pub documentation_url: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum GitHubErrorKind {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("API error: {message}")]
    ApiError {
        message: String,
        documentation_url: Option<String>,
    },

    #[error("JSON parsing error: {0}")]
    JsonError(String),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,
}

pub async fn fetch_instantos_repos() -> Result<Vec<GitHubRepo>, GitHubErrorKind> {
    let client = reqwest::Client::builder()
        .user_agent("instantCLI/0.1.1")
        .build()
        .map_err(|e| GitHubErrorKind::NetworkError(e.to_string()))?;

    let url = "https://api.github.com/orgs/instantOS/repos";

    let response = client
        .get(url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| GitHubErrorKind::NetworkError(e.to_string()))?;

    if response.status() == 403 {
        if let Some(remaining) = response.headers().get("X-RateLimit-Remaining") {
            if remaining == "0" {
                return Err(GitHubErrorKind::RateLimitExceeded);
            }
        }
    }

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        let api_error: Result<GitHubError, _> = serde_json::from_str(&error_text);

        match api_error {
            Ok(err) => Err(GitHubErrorKind::ApiError {
                message: err.message,
                documentation_url: err.documentation_url,
            }),
            Err(_) => Err(GitHubErrorKind::HttpError(format!(
                "HTTP {}: {}",
                status, error_text
            ))),
        }
    } else {
        let repos = response
            .json::<Vec<GitHubRepo>>()
            .await
            .map_err(|e| GitHubErrorKind::JsonError(e.to_string()))?;

        Ok(repos)
    }
}

pub fn format_repo_for_display(repo: &GitHubRepo) -> String {
    let stars = repo.stargazers_count.unwrap_or(0);
    let forks = repo.forks_count.unwrap_or(0);
    let lang = repo.language.as_deref().unwrap_or("Unknown");
    let desc = repo.description.as_deref().unwrap_or("No description");

    format!("⭐ {}  🍴 {}  {}  - {}", stars, forks, lang, desc)
}
