use crate::ui::prelude::{Level, emit};

#[derive(Debug, Clone)]
pub(crate) struct ReportLine {
    pub(crate) level: Level,
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl ReportLine {
    pub(crate) fn new(level: Level, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            level,
            code,
            message: message.into(),
        }
    }
}

pub(crate) fn emit_report(lines: &[ReportLine]) {
    for line in lines {
        emit(line.level, line.code, &line.message, None);
    }
}

pub(crate) fn format_report_lines(lines: &[ReportLine]) -> Vec<String> {
    lines.iter().flat_map(format_report_line).collect()
}

fn format_report_line(line: &ReportLine) -> Vec<String> {
    let prefix = format!("[{}] ", level_label(line.level));
    let mut message_lines = line.message.lines();
    let Some(first) = message_lines.next() else {
        return vec![prefix.trim_end().to_string()];
    };

    let mut formatted = Vec::new();
    formatted.push(format!("{prefix}{first}"));

    let indent = " ".repeat(prefix.len());
    for rest in message_lines {
        formatted.push(format!("{indent}{rest}"));
    }

    formatted
}

fn level_label(level: Level) -> &'static str {
    match level {
        Level::Info => "INFO",
        Level::Success => "OK",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
        Level::Debug => "DEBUG",
    }
}
