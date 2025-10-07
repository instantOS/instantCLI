use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dirs::cache_dir;
use sha2::{Digest, Sha256};

const TITLE_CARD_DURATION_SECONDS: f64 = 3.0;
const CSS_VERSION_TOKEN: &str = "2";

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

    pub fn generate_image(&self, level: u32, text: &str, output_path: &Path) -> Result<()> {
        let cache_key = self.build_cache_key(level, text);
        let card_dir = self.cache_dir.join(&cache_key);
        let markdown_path = card_dir.join("input.md");
        let css_path = card_dir.join("title.css");
        let html_path = card_dir.join("title.html");
        let image_path = card_dir.join("title.jpg");

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

        // Copy the generated image to the output path
        fs::copy(&image_path, output_path).with_context(|| {
            format!(
                "Failed to copy title card image from {} to {}",
                image_path.display(),
                output_path.display()
            )
        })?;

        Ok(())
    }

    pub fn generate_from_markdown(&self, markdown_content: &str) -> Result<TitleCardAsset> {
        let cache_key = self.build_markdown_cache_key(markdown_content);
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

        fs::write(&markdown_path, markdown_content.as_bytes())
            .with_context(|| format!("Failed to write title card markdown to {}", markdown_path.display()))?;
        self.write_css(&css_path)?;
        self.run_pandoc(&markdown_path, &html_path, &css_path)?;
        self.capture_screenshot(&html_path, &image_path)?;
        self.render_video(&image_path, &video_path)?;

        Ok(TitleCardAsset {
            video_path,
            duration: TITLE_CARD_DURATION_SECONDS,
        })
    }

    pub fn generate_image_from_markdown(&self, markdown_content: &str, output_path: &Path) -> Result<()> {
        let cache_key = self.build_markdown_cache_key(markdown_content);
        let card_dir = self.cache_dir.join(&cache_key);
        let markdown_path = card_dir.join("input.md");
        let css_path = card_dir.join("title.css");
        let html_path = card_dir.join("title.html");
        let image_path = card_dir.join("title.jpg");

        fs::create_dir_all(&card_dir).with_context(|| {
            format!(
                "Failed to create title card cache entry at {}",
                card_dir.display()
            )
        })?;

        fs::write(&markdown_path, markdown_content.as_bytes())
            .with_context(|| format!("Failed to write title card markdown to {}", markdown_path.display()))?;
        self.write_css(&css_path)?;
        self.run_pandoc(&markdown_path, &html_path, &css_path)?;
        self.capture_screenshot(&html_path, &image_path)?;

        // Copy the generated image to the output path
        fs::copy(&image_path, output_path).with_context(|| {
            format!(
                "Failed to copy title card image from {} to {}",
                image_path.display(),
                output_path.display()
            )
        })?;

        Ok(())
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

    fn build_markdown_cache_key(&self, markdown_content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.width.to_le_bytes());
        hasher.update(self.height.to_le_bytes());
        hasher.update(TITLE_CARD_DURATION_SECONDS.to_le_bytes());
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

* {{
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}}

body {{
  margin: 0;
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #0a0a0a 0%, #1a1a1a 50%, #0a0a0a 100%);
  color: #f8f8f8;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Inter', Roboto, Oxygen, Ubuntu, sans-serif;
  line-height: 1.6;
  padding: 6rem;
  overflow: hidden;
}}

body::before {{
  content: '';
  position: absolute;
  top: -50%;
  left: -50%;
  width: 200%;
  height: 200%;
  background: radial-gradient(circle at center, rgba(59, 130, 246, 0.08) 0%, transparent 60%);
  animation: subtle-pulse 8s ease-in-out infinite;
  pointer-events: none;
}}

@keyframes subtle-pulse {{
  0%, 100% {{ opacity: 0.4; }}
  50% {{ opacity: 0.6; }}
}}

.content {{
  position: relative;
  z-index: 1;
  max-width: 90%;
  text-align: center;
}}

h1, h2, h3, h4, h5, h6 {{
  margin: 0 0 1.5rem 0;
  padding: 0;
  line-height: 1.2;
  font-weight: 700;
  letter-spacing: -0.02em;
  color: #ffffff;
}}

h1 {{ font-size: clamp(3rem, 8vw, 7rem); }}
h2 {{ font-size: clamp(2.5rem, 6.5vw, 5.5rem); }}
h3 {{ font-size: clamp(2rem, 5.5vw, 4.5rem); }}
h4 {{ font-size: clamp(1.75rem, 4.5vw, 3.5rem); }}
h5 {{ font-size: clamp(1.5rem, 4vw, 3rem); }}
h6 {{ font-size: clamp(1.25rem, 3.5vw, 2.5rem); }}

p {{
  font-size: clamp(1.5rem, 3.5vw, 3rem);
  line-height: 1.5;
  margin: 0 0 1.5rem 0;
  color: #e5e5e5;
  max-width: 85%;
  margin-left: auto;
  margin-right: auto;
}}

blockquote {{
  border-left: 4px solid #3b82f6;
  padding: 1.5rem 2rem;
  margin: 2rem auto;
  font-size: clamp(1.75rem, 4vw, 3.5rem);
  font-style: italic;
  color: #d1d5db;
  background: rgba(59, 130, 246, 0.05);
  border-radius: 0 8px 8px 0;
  max-width: 80%;
}}

blockquote p {{
  margin: 0;
  color: inherit;
  max-width: 100%;
}}

code {{
  background: rgba(255, 255, 255, 0.1);
  padding: 0.2em 0.6em;
  border-radius: 4px;
  font-family: 'JetBrains Mono', 'Fira Code', 'Consolas', monospace;
  font-size: 0.9em;
  color: #60a5fa;
}}

pre {{
  background: rgba(0, 0, 0, 0.4);
  padding: 2rem;
  border-radius: 8px;
  overflow-x: auto;
  margin: 2rem auto;
  max-width: 85%;
}}

pre code {{
  background: none;
  padding: 0;
  font-size: clamp(1.25rem, 2.5vw, 2rem);
  color: #93c5fd;
}}

ul, ol {{
  text-align: left;
  font-size: clamp(1.5rem, 3vw, 2.5rem);
  line-height: 1.6;
  margin: 2rem auto;
  max-width: 70%;
  color: #e5e5e5;
}}

li {{
  margin: 1rem 0;
  padding-left: 0.5rem;
}}

ul li::marker {{
  color: #3b82f6;
}}

ol li::marker {{
  color: #3b82f6;
  font-weight: 600;
}}

strong, b {{
  font-weight: 700;
  color: #ffffff;
}}

em, i {{
  font-style: italic;
  color: #d1d5db;
}}

a {{
  color: #60a5fa;
  text-decoration: none;
  border-bottom: 2px solid #3b82f6;
  transition: color 0.3s;
}}

a:hover {{
  color: #93c5fd;
}}

hr {{
  border: none;
  height: 2px;
  background: linear-gradient(90deg, transparent, #3b82f6, transparent);
  margin: 3rem auto;
  width: 60%;
}}

table {{
  margin: 2rem auto;
  border-collapse: collapse;
  font-size: clamp(1.25rem, 2.5vw, 2rem);
  max-width: 85%;
}}

th, td {{
  padding: 1rem 1.5rem;
  text-align: left;
  border: 1px solid rgba(255, 255, 255, 0.1);
}}

th {{
  background: rgba(59, 130, 246, 0.2);
  color: #ffffff;
  font-weight: 600;
}}

td {{
  background: rgba(0, 0, 0, 0.2);
  color: #e5e5e5;
}}
"#
        )
    }
}
