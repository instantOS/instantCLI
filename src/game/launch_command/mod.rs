mod detect;
mod parse;
mod render;
mod tests;
mod types;

use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use types::*;

impl LaunchCommand {
    pub fn manual(command: impl Into<String>) -> Self {
        Self {
            wrappers: LaunchWrappers::default(),
            kind: LaunchCommandKind::Manual {
                command: command.into(),
            },
        }
    }

    pub fn from_shell_or_manual(command: impl Into<String>) -> Self {
        let command = command.into();
        Self::from_str(&command).unwrap_or_else(|_| Self::manual(command))
    }

    pub fn to_shell_command(&self) -> String {
        render::to_shell_command(self)
    }
}

impl Display for LaunchCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_shell_command())
    }
}

impl Serialize for LaunchCommand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_shell_command())
    }
}

impl<'de> Deserialize<'de> for LaunchCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_shell_or_manual(raw))
    }
}

impl FromStr for LaunchCommand {
    type Err = shell_words::ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let tokens = shell_words::split(input)?;
        Ok(parse::parse_launch_command(input, tokens))
    }
}
