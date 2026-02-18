use std::path::Path;

pub fn format_time(value: f64) -> String {
    format!("{value:.6}")
}

pub fn escape_ffmpeg_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('\'', "'\\''")
        .replace(':', "\\:")
}
