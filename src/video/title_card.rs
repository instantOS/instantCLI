use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dirs::cache_dir;
use sha2::{Digest, Sha256};

const CSS_VERSION_TOKEN: &str = "3";
const DEFAULT_CSS: &str = include_str!("title_card.css");

pub struct TitleCardGenerator {
    cache_dir: PathBuf,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone)]
pub struct TitleCardAsset {
    card_dir: PathBuf,
    pub image_path: PathBuf,
}

impl TitleCardGenerator {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let cache_root = cache_dir()
            .context("Unable to determine cache directory for title cards")?
            .join("instant")
            .join("video")
            .join("title_cards");

        fs::create_dir_all(&cache_root).with_context(|| {
            format!(
                "Failed to create title card cache directory at {}",
                cache_root.display()
            )
        })?;

        Ok(Self {
            cache_dir: cache_root,
            width,
            height,
        })
    }

    pub fn heading_card(&self, level: u32, text: &str) -> Result<TitleCardAsset> {
        let cache_key = self.build_cache_key(level, text);
        let card_dir = self.cache_dir.join(&cache_key);
        let markdown_path = card_dir.join("input.md");
        let css_path = card_dir.join("title.css");
        let html_path = card_dir.join("title.html");
        let image_path = card_dir.join("title.jpg");

        self.ensure_card_dir(&card_dir)?;
        if !image_path.exists() {
            self.write_markdown(&markdown_path, level, text)?;
            self.write_css(&css_path)?;
            self.run_pandoc(&markdown_path, &html_path, &css_path)?;
            self.capture_screenshot(&html_path, &image_path)?;
        }

        Ok(TitleCardAsset {
            card_dir,
            image_path,
        })
    }

    pub fn markdown_card(&self, markdown_content: &str) -> Result<TitleCardAsset> {
        let cache_key = self.build_markdown_cache_key(markdown_content);
        let card_dir = self.cache_dir.join(&cache_key);
        let markdown_path = card_dir.join("input.md");
        let css_path = card_dir.join("title.css");
        let html_path = card_dir.join("title.html");
        let image_path = card_dir.join("title.jpg");

        self.ensure_card_dir(&card_dir)?;
        if !image_path.exists() {
            fs::write(&markdown_path, markdown_content.as_bytes()).with_context(|| {
                format!(
                    "Failed to write title card markdown to {}",
                    markdown_path.display()
                )
            })?;
            self.write_css(&css_path)?;
            self.run_pandoc(&markdown_path, &html_path, &css_path)?;
            self.capture_screenshot(&html_path, &image_path)?;
        }

        Ok(TitleCardAsset {
            card_dir,
            image_path,
        })
    }

    pub fn generate_image(&self, level: u32, text: &str, output_path: &Path) -> Result<()> {
        let asset = self.heading_card(level, text)?;
        self.copy_image(&asset.image_path, output_path)
    }

    pub fn generate_image_from_markdown(
        &self,
        markdown_content: &str,
        output_path: &Path,
    ) -> Result<()> {
        let asset = self.markdown_card(markdown_content)?;
        self.copy_image(&asset.image_path, output_path)
    }

    pub fn ensure_video_for_duration(
        &self,
        asset: &TitleCardAsset,
        duration: f64,
    ) -> Result<PathBuf> {
        let sanitized = (duration * 1000.0).round() as u64;
        let video_path = asset.card_dir.join(format!("title_{sanitized}.mp4"));
        if video_path.exists() {
            return Ok(video_path);
        }

        self.render_video(&asset.image_path, &video_path, duration)?;
        Ok(video_path)
    }

    fn ensure_card_dir(&self, card_dir: &Path) -> Result<()> {
        fs::create_dir_all(card_dir).with_context(|| {
            format!(
                "Failed to create title card cache entry at {}",
                card_dir.display()
            )
        })
    }

    fn copy_image(&self, source: &Path, dest: &Path) -> Result<()> {
        fs::copy(source, dest).with_context(|| {
            format!(
                "Failed to copy title card image from {} to {}",
                source.display(),
                dest.display()
            )
        })?;
        Ok(())
    }

    fn build_cache_key(&self, level: u32, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(level.to_le_bytes());
        hasher.update(self.width.to_le_bytes());
        hasher.update(self.height.to_le_bytes());
        hasher.update(CSS_VERSION_TOKEN.as_bytes());
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn build_markdown_cache_key(&self, markdown_content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.width.to_le_bytes());
        hasher.update(self.height.to_le_bytes());
        hasher.update(CSS_VERSION_TOKEN.as_bytes());
        hasher.update(markdown_content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn write_markdown(&self, path: &Path, level: u32, text: &str) -> Result<()> {
        let heading_level = level.max(1);
        let hashes = "#".repeat(heading_level as usize);
        let content = format!("{hashes} {}\n", text.trim());
        fs::write(path, content.as_bytes())
            .with_context(|| format!("Failed to write title card markdown to {}", path.display()))
    }

    fn write_css(&self, path: &Path) -> Result<()> {
        let mut file = fs::File::create(path)
            .with_context(|| format!("Failed to create CSS file at {}", path.display()))?;
        file.write_all(DEFAULT_CSS.as_bytes())
            .with_context(|| format!("Failed to write CSS to {}", path.display()))
    }

    fn run_pandoc(&self, markdown: &Path, html: &Path, css: &Path) -> Result<()> {
        let status = Command::new("pandoc")
            .arg(markdown)
            .arg("-o")
            .arg(html)
            .arg("--standalone")
            .arg("--katex")
            .arg("--css")
            .arg(css)
            .status()
            .with_context(|| "Failed to spawn pandoc for title card rendering")?;

        if !status.success() {
            anyhow::bail!("pandoc exited with status {:?}", status.code());
        }

        Ok(())
    }

    fn capture_screenshot(&self, html: &Path, image: &Path) -> Result<()> {
        let file_url = format!("file://{}", html.display());
        let window_arg = format!("--window-size={},{}", self.width, self.height);
        let screenshot_arg = format!("--screenshot={}", image.display());

        let status = Command::new("chromium")
            .arg("--headless")
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg(window_arg)
            .arg(screenshot_arg)
            .arg(file_url)
            .status()
            .with_context(|| "Failed to spawn chromium for title card screenshot")?;

        if !status.success() {
            anyhow::bail!("chromium exited with status {:?}", status.code());
        }

        Ok(())
    }

    fn render_video(&self, image: &Path, video: &Path, duration_secs: f64) -> Result<()> {
        let duration = format!("{:.3}", duration_secs);
        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-loop")
            .arg("1")
            .arg("-i")
            .arg(image)
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("anullsrc=r=48000:cl=stereo")
            .arg("-shortest")
            .arg("-t")
            .arg(&duration)
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("medium")
            .arg("-crf")
            .arg("18")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("192k")
            .arg(video)
            .status()
            .with_context(|| "Failed to spawn ffmpeg for title card video generation")?;

        if !status.success() {
            anyhow::bail!("ffmpeg exited with status {:?}", status.code());
        }

        Ok(())
    }
}
