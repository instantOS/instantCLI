use crate::arch::engine::{InstallContext, QuestionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEnvironment {
    Sway,
    Niri,
    InstantWM,
    Hyprland,
    Tty,
}

impl DesktopEnvironment {
    pub const DEFAULT: Self = Self::Sway;

    pub fn from_answer(answer: &str) -> Self {
        match answer {
            "sway" => Self::Sway,
            "niri" => Self::Niri,
            "instantwm" => Self::InstantWM,
            "hyprland" => Self::Hyprland,
            "none/tty" => Self::Tty,
            _ => Self::DEFAULT,
        }
    }

    pub fn from_context(context: &InstallContext) -> Self {
        context
            .get_answer(&QuestionId::DesktopEnvironment)
            .map(|answer| Self::from_answer(answer))
            .unwrap_or(Self::DEFAULT)
    }

    pub fn answer_value(&self) -> &'static str {
        match self {
            Self::Sway => "sway",
            Self::Niri => "niri",
            Self::InstantWM => "instantwm",
            Self::Hyprland => "hyprland",
            Self::Tty => "none/tty",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sway => "Sway",
            Self::Niri => "niri",
            Self::InstantWM => "instantWM",
            Self::Hyprland => "Hyprland",
            Self::Tty => "None / TTY",
        }
    }

    pub fn session_name(&self) -> Option<&'static str> {
        match self {
            Self::Sway => Some("sway"),
            Self::Niri => Some("niri"),
            Self::InstantWM => Some("instantwm"),
            Self::Hyprland => Some("hyprland"),
            Self::Tty => None,
        }
    }

    pub fn package_names(&self) -> &'static [&'static str] {
        match self {
            Self::Sway => &["sway"],
            Self::Niri => &["niri"],
            Self::InstantWM => &[],
            Self::Hyprland => &["hyprland"],
            Self::Tty => &[],
        }
    }

    pub fn requires_display_manager(&self) -> bool {
        !matches!(self, Self::Tty)
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopEnvironment;

    #[test]
    fn parses_desktop_environment_answers() {
        assert_eq!(
            DesktopEnvironment::from_answer("instantwm"),
            DesktopEnvironment::InstantWM
        );
        assert_eq!(
            DesktopEnvironment::from_answer("none/tty"),
            DesktopEnvironment::Tty
        );
        assert_eq!(
            DesktopEnvironment::from_answer("unknown"),
            DesktopEnvironment::Sway
        );
    }
}
