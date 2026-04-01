use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use super::{PreviewContext, PreviewId};

const PREVIEW_CACHE_VERSION: u32 = 1;

pub(super) fn get_or_render<F>(id: PreviewId, ctx: &PreviewContext, render: F) -> Result<String>
where
    F: FnOnce() -> Result<String>,
{
    let Some(ttl) = cache_ttl(id) else {
        return render();
    };

    let Some(cache_path) = cache_path(id, ctx)? else {
        return render();
    };

    if let Some(cached) = read_fresh_cache(&cache_path, ttl)? {
        return Ok(cached);
    }

    let rendered = render()?;
    let _ = write_cache(&cache_path, &rendered);
    Ok(rendered)
}

pub(super) fn get_cached(id: PreviewId, ctx: &PreviewContext) -> Result<Option<String>> {
    let Some(ttl) = cache_ttl(id) else {
        return Ok(None);
    };
    let Some(cache_path) = cache_path(id, ctx)? else {
        return Ok(None);
    };
    read_fresh_cache(&cache_path, ttl)
}

pub(super) fn store(id: PreviewId, ctx: &PreviewContext, text: &str) -> Result<()> {
    let Some(_ttl) = cache_ttl(id) else {
        return Ok(());
    };
    let Some(cache_path) = cache_path(id, ctx)? else {
        return Ok(());
    };
    write_cache(&cache_path, text)
}

fn cache_ttl(id: PreviewId) -> Option<Duration> {
    match id {
        PreviewId::Package
        | PreviewId::InstalledPackage
        | PreviewId::Apt
        | PreviewId::Dnf
        | PreviewId::Zypper
        | PreviewId::Pacman
        | PreviewId::Snap
        | PreviewId::Pkg
        | PreviewId::Flatpak
        | PreviewId::Aur
        | PreviewId::Cargo => Some(Duration::from_secs(120)),
        _ => None,
    }
}

fn cache_path(id: PreviewId, ctx: &PreviewContext) -> Result<Option<PathBuf>> {
    let Some(key) = ctx.key() else {
        return Ok(None);
    };
    let mut hasher = Sha256::new();
    hasher.update(PREVIEW_CACHE_VERSION.to_le_bytes());
    hasher.update(id.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(key.as_bytes());
    hasher.update([0]);
    hasher.update(ctx.columns.unwrap_or_default().to_le_bytes());
    hasher.update(ctx.lines.unwrap_or_default().to_le_bytes());
    let digest = format!("{:x}", hasher.finalize());

    Ok(Some(preview_cache_dir()?.join(format!("{digest}.txt"))))
}

fn preview_cache_dir() -> Result<PathBuf> {
    let root = std::env::var_os("INS_PREVIEW_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join("ins-preview-cache");
    fs::create_dir_all(&dir).context("Failed to create preview cache directory")?;
    Ok(dir)
}

fn read_fresh_cache(path: &PathBuf, ttl: Duration) -> Result<Option<String>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("Failed to stat preview cache file"),
    };

    let modified = metadata
        .modified()
        .context("Failed to read preview cache mtime")?;
    if is_stale(modified, ttl) {
        let _ = fs::remove_file(path);
        return Ok(None);
    }

    Ok(Some(
        fs::read_to_string(path).context("Failed to read preview cache file")?,
    ))
}

fn write_cache(path: &PathBuf, text: &str) -> Result<()> {
    fs::write(path, text).context("Failed to write preview cache file")
}

fn is_stale(modified: SystemTime, ttl: Duration) -> bool {
    match SystemTime::now().duration_since(modified) {
        Ok(age) => age > ttl,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip_uses_tmp_dir() {
        let temp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("INS_PREVIEW_CACHE_DIR", temp.path());
        }

        let ctx = PreviewContext {
            key: Some("abc".to_string()),
            columns: Some(80),
            lines: Some(24),
        };

        let rendered = get_or_render(PreviewId::Flatpak, &ctx, || {
            Ok::<_, anyhow::Error>("value".into())
        })
        .unwrap();
        assert_eq!(rendered, "value");

        let cached = get_or_render(PreviewId::Flatpak, &ctx, || {
            Ok::<_, anyhow::Error>("other".into())
        })
        .unwrap();
        assert_eq!(cached, "value");

        unsafe {
            std::env::remove_var("INS_PREVIEW_CACHE_DIR");
        }
    }
}
