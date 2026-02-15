use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dirs::cache_dir;
use sha2::{Digest, Sha256};

use crate::video::support::ffmpeg::{PROFILE_SLIDE_VIDEO, run_ffmpeg_output};

pub mod cli;

const DEFAULT_CSS: &str = include_str!("assets/slide.css");
const DEFAULT_JS: &str = include_str!("assets/slide.js");

// Workaround for Chromium "new" headless mode viewport bug (grey bar artifacts).
// Set to 0 to disable the workaround (oversize rendering + cropping) once Chromium fixes the issue.
const HEADLESS_BUG_PADDING: u32 = 200;

pub struct SlideGenerator {
    cache_dir: PathBuf,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone)]
pub struct SlideAsset {
    slide_dir: PathBuf,
    pub image_path: PathBuf,
    pub was_cached: bool,
}

impl SlideGenerator {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let cache_root = cache_dir()
            .context("Unable to determine cache directory for slides")?
            .join("instant")
            .join("video")
            .join("slides");

        fs::create_dir_all(&cache_root).with_context(|| {
            format!(
                "Failed to create slide cache directory at {}",
                cache_root.display()
            )
        })?;

        Ok(Self {
            cache_dir: cache_root,
            width,
            height,
        })
    }

    pub fn markdown_slide(&self, markdown_content: &str) -> Result<SlideAsset> {
        let cache_key = self.build_markdown_cache_key(markdown_content);
        let slide_dir = self.cache_dir.join(&cache_key);
        let markdown_path = slide_dir.join("input.md");
        let css_path = slide_dir.join("slide.css");
        let html_path = slide_dir.join("slide.html");
        let image_path = slide_dir.join("slide.png");

        self.ensure_slide_dir(&slide_dir)?;
        let was_cached = image_path.exists();
        if !was_cached {
            fs::write(&markdown_path, markdown_content.as_bytes()).with_context(|| {
                format!(
                    "Failed to write slide markdown to {}",
                    markdown_path.display()
                )
            })?;
            self.write_css(&css_path)?;
            self.run_pandoc(&markdown_path, &html_path, &css_path)?;
            self.post_process_html(&html_path)?;
            self.capture_screenshot(&html_path, &image_path)?;
        }

        Ok(SlideAsset {
            slide_dir,
            image_path,
            was_cached,
        })
    }

    pub fn ensure_video_for_duration(&self, asset: &SlideAsset, duration: f64) -> Result<PathBuf> {
        let sanitized = (duration * 1000.0).round() as u64;
        let video_path = asset.slide_dir.join(format!("slide_{sanitized}.mp4"));
        if video_path.exists() {
            return Ok(video_path);
        }

        self.render_video(&asset.image_path, &video_path, duration)?;
        Ok(video_path)
    }

    fn ensure_slide_dir(&self, slide_dir: &Path) -> Result<()> {
        fs::create_dir_all(slide_dir).with_context(|| {
            format!(
                "Failed to create slide cache entry at {}",
                slide_dir.display()
            )
        })
    }

    /// Build cache key from dimensions, CSS/JS content, and markdown.
    /// Automatically invalidates when CSS or JS files change.
    fn build_markdown_cache_key(&self, markdown_content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.width.to_le_bytes());
        hasher.update(self.height.to_le_bytes());
        hasher.update(DEFAULT_CSS.as_bytes());
        hasher.update(DEFAULT_JS.as_bytes());
        hasher.update(markdown_content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn write_css(&self, path: &Path) -> Result<()> {
        let mut file = fs::File::create(path)
            .with_context(|| format!("Failed to create CSS file at {}", path.display()))?;
        file.write_all(DEFAULT_CSS.as_bytes())
            .with_context(|| format!("Failed to write CSS to {}", path.display()))?;

        // Append explicit dimensions to ensure full coverage.
        // This is part of the workaround for headless viewport issues: we force the content
        // to exactly fill the requested resolution, even if the window is padded.
        let dimensions_css = format!(
            "\nbody {{ width: {}px; height: {}px; }}\n",
            self.width, self.height
        );
        file.write_all(dimensions_css.as_bytes())
            .with_context(|| format!("Failed to append dimensions to CSS at {}", path.display()))
    }

    fn run_pandoc(&self, markdown: &Path, html: &Path, css: &Path) -> Result<()> {
        let status = Command::new("pandoc")
            .arg(markdown)
            .arg("-o")
            .arg(html)
            .arg("--standalone")
            .arg("--katex")
            .arg("--highlight-style=pygments") // Use pygments style which outputs clean CSS classes
            .arg("--css")
            .arg(css)
            .status()
            .with_context(|| "Failed to spawn pandoc for slide rendering")?;

        if !status.success() {
            anyhow::bail!("pandoc exited with status {:?}", status.code());
        }

        Ok(())
    }

    fn post_process_html(&self, html_path: &Path) -> Result<()> {
        let content = fs::read_to_string(html_path).with_context(|| {
            format!(
                "Failed to read HTML for post-processing at {}",
                html_path.display()
            )
        })?;

        // Simple string finding to inject wrapper and script
        // We assume pandoc's output structure (<body>...</body>)
        let body_start = content.find("<body>").map(|i| i + 6).unwrap_or(0);
        let body_end = content.rfind("</body>").unwrap_or(content.len());

        let before_body = &content[..body_start];
        let body_content = &content[body_start..body_end];
        let after_body = &content[body_end..];

        let script = format!("<script>{}</script>", DEFAULT_JS);

        let new_content = format!(
            "{}<div class=\"content\">{}</div>{}{}",
            before_body, body_content, script, after_body
        );

        fs::write(html_path, new_content).with_context(|| {
            format!(
                "Failed to write post-processed HTML to {}",
                html_path.display()
            )
        })
    }

    fn capture_screenshot(&self, html: &Path, image: &Path) -> Result<()> {
        let file_url = format!("file://{}", html.display());

        let (window_height, use_cropping) = if HEADLESS_BUG_PADDING > 0 {
            (self.height + HEADLESS_BUG_PADDING, true)
        } else {
            (self.height, false)
        };

        let window_arg = format!("--window-size={},{}", self.width, window_height);

        // If cropping, use a temporary path for the raw screenshot.
        // Otherwise, write directly to the final image path.
        let capture_target = if use_cropping {
            image.with_extension("raw.png")
        } else {
            image.to_path_buf()
        };

        let screenshot_arg = format!("--screenshot={}", capture_target.display());

        let status = Command::new("chromium")
            .arg("--headless")
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg("--hide-scrollbars")
            .arg("--force-device-scale-factor=1")
            .arg("--default-browser-site-isolation-level=none")
            .arg(window_arg)
            .arg(screenshot_arg)
            .arg(file_url)
            .status()
            .with_context(|| "Failed to spawn chromium for slide screenshot")?;

        if !status.success() {
            anyhow::bail!("chromium exited with status {:?}", status.code());
        }

        if use_cropping {
            let crop_filter = format!("crop={}:{}:0:0", self.width, self.height);
            run_ffmpeg_output(
                &[
                    "-y",
                    "-v",
                    "error",
                    "-i",
                    &capture_target.to_string_lossy(),
                    "-vf",
                    &crop_filter,
                    &image.to_string_lossy(),
                ],
                "for screenshot cropping",
            )?;

            if capture_target.exists() {
                let _ = fs::remove_file(capture_target);
            }
        }

        Ok(())
    }

    fn render_video(&self, image: &Path, video: &Path, duration_secs: f64) -> Result<()> {
        let duration = format!("{:.3}", duration_secs);
        let mut args = vec![
            "-y".to_string(),
            "-loop".to_string(),
            "1".to_string(),
            "-i".to_string(),
            image.to_string_lossy().into_owned(),
            "-f".to_string(),
            "lavfi".to_string(),
            "-i".to_string(),
            "anullsrc=r=48000:cl=stereo".to_string(),
            "-shortest".to_string(),
            "-t".to_string(),
            duration,
            "-vf".to_string(),
            "setsar=1".to_string(),
        ];
        PROFILE_SLIDE_VIDEO.push_to(&mut args);
        args.push(video.to_string_lossy().into_owned());
        run_ffmpeg_output(
            &args.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            &format!("for slide video generation from {}", image.display()),
        )
    }
}
