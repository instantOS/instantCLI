use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dirs::cache_dir;
use sha2::{Digest, Sha256};

const TITLE_CARD_DURATION_SECONDS: f64 = 3.0;
const CSS_VERSION_TOKEN: &str = "1";

pub struct TitleCardGenerator {
    cache_dir: PathBuf,
    width: u32,
    height: u32,
}

pub struct TitleCardAsset {
    pub video_path: PathBuf,
    pub duration: f64,
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

    pub fn generate(&self, level: u32, text: &str) -> Result<TitleCardAsset> {
        let cache_key = self.build_cache_key(level, text);
        let card_dir = self.cache_dir.join(&cache_key);
        let markdown_path = card_dir.join("input.md");
        let css_path = card_dir.join("title.css");
        let html_path = card_dir.join("title.html");
        let image_path = card_dir.join("title.jpg");
        let video_path = card_dir.join("title.mp4");

        if video_path.exists() {
            return Ok(TitleCardAsset {
                video_path,
                duration: TITLE_CARD_DURATION_SECONDS,
            });
        }

        fs::create_dir_all(&card_dir).with_context(|| {
            format!(
                "Failed to create title card cache entry at {}",
                card_dir.display()
            )
        })?;

        self.write_markdown(&markdown_path, level, text)?;
        self.write_css(&css_path)?;
        self.run_pandoc(&markdown_path, &html_path, &css_path)?;
        self.capture_screenshot(&html_path, &image_path)?;
        self.render_video(&image_path, &video_path)?;

        Ok(TitleCardAsset {
            video_path,
            duration: TITLE_CARD_DURATION_SECONDS,
        })
    }

    fn build_cache_key(&self, level: u32, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(level.to_le_bytes());
        hasher.update(self.width.to_le_bytes());
        hasher.update(self.height.to_le_bytes());
        hasher.update(TITLE_CARD_DURATION_SECONDS.to_le_bytes());
        hasher.update(CSS_VERSION_TOKEN.as_bytes());
        hasher.update(text.as_bytes());
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
        file.write_all(Self::default_css().as_bytes())
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

    fn render_video(&self, image: &Path, video: &Path) -> Result<()> {
        let duration = format!("{:.3}", TITLE_CARD_DURATION_SECONDS);
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

    fn default_css() -> String {
        format!(
            r#":root {{
  color-scheme: dark;
}}

body {{
  margin: 0;
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: radial-gradient(circle at center, #1f1f1f, #0b0b0b);
  color: #f4f4f5;
  font-family: 'Inter', 'Segoe UI', Helvetica, Arial, sans-serif;
  text-align: center;
}}

h1, h2, h3, h4, h5, h6 {{
  margin: 0;
  padding: 0 4rem;
  line-height: 1.25;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  text-shadow: 0 4px 30px rgba(0, 0, 0, 0.45);
}}

h1 {{ font-size: min(8vw, 6rem); }}
h2 {{ font-size: min(7vw, 5.4rem); }}
h3 {{ font-size: min(6vw, 4.8rem); }}
h4 {{ font-size: min(5vw, 4.0rem); }}
h5 {{ font-size: min(4vw, 3.2rem); }}
h6 {{ font-size: min(3vw, 2.6rem); }}

body::after {{
  content: '';
  position: absolute;
  inset: 0;
  background: radial-gradient(circle at center, rgba(255, 255, 255, 0.15), transparent 60%);
  pointer-events: none;
}}
"#
        )
    }
}
