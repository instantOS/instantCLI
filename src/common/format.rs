//! Formatting utilities for displaying system data

/// Format bytes as human-readable size (KB, MB, GB, TB)
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1000), "1000 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 10), "10.0 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(260 * 1024 * 1024), "260.0 MB");
        assert_eq!(format_size(500 * 1024 * 1024 + 512 * 1024), "500.5 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_size(10 * 1024 * 1024 * 1024), "10.0 GB");
        assert_eq!(format_size(256 * 1024 * 1024 * 1024), "256.0 GB");
    }

    #[test]
    fn test_format_size_tb() {
        assert_eq!(format_size(1024u64 * 1024 * 1024 * 1024), "1.0 TB");
        assert_eq!(format_size(2 * 1024u64 * 1024 * 1024 * 1024), "2.0 TB");
    }
}
