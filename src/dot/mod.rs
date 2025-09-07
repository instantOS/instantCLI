use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf, process::Command};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Repo {
    pub url: String,
    pub name: Option<String>,
    pub branch: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Config {
    pub repos: Vec<Repo>,
}

fn config_file_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    let cfg = PathBuf::from(home).join(".config/instant/instant.toml");
    if let Some(parent) = cfg.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(cfg)
}

fn repos_base_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    let base = PathBuf::from(home).join(".local/share/instantos/dots");
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

pub fn load_repos() -> Result<Vec<Repo>, Box<dyn std::error::Error>> {
    let cfg = config_file_path()?;
    if !cfg.exists() {
        return Ok(Vec::new());
    }
    let s = std::fs::read_to_string(&cfg)?;
    let c: Config = toml::from_str(&s)?;
    Ok(c.repos)
}

pub fn save_repos(repos: &[Repo]) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = config_file_path()?;
    let c = Config { repos: repos.to_vec() };
    let toml = toml::to_string_pretty(&c)?;
    std::fs::write(cfg, toml)?;
    Ok(())
}

pub fn add_repo(repo: Repo, debug: bool) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = repos_base_dir()?;

    let repo_dir_name = match &repo.name {
        Some(n) => n.clone(),
        None => basename_from_repo(&repo.url),
    };

    let target = base.join(repo_dir_name);

    if target.exists() {
        return Err(format!("Destination '{}' already exists", target.display()).into());
    }

    let mut cmd = Command::new("git");
    cmd.arg("clone");
    if let Some(branch) = &repo.branch {
        cmd.arg("--branch").arg(branch);
    }
    cmd.arg(&repo.url).arg(&target);

    if debug {
        eprintln!("Running: {:?}", cmd);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {}", stderr).into());
    }

    // append to config
    let mut repos = load_repos()?;
    repos.push(repo);
    save_repos(&repos)?;

    Ok(target)
}

fn basename_from_repo(repo: &str) -> String {
    let s = repo.trim_end_matches(".git");
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
