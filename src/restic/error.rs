use thiserror::Error;

#[derive(Error, Debug)]
pub enum ResticError {
    #[error("Restic command failed: {0}")]
    CommandFailed(String),

    #[error("Failed to parse JSON output: {0}")]
    JsonParseError(#[from] serde_json::Error),

    #[error("Repository does not exist")]
    RepositoryNotFound,

    #[error("Invalid password")]
    InvalidPassword,

    #[error("Failed to lock repository")]
    RepositoryLocked,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
}

impl ResticError {
    pub fn from_exit_code(code: i32, stderr: &str) -> Self {
        match code {
            10 => ResticError::RepositoryNotFound,
            11 => ResticError::RepositoryLocked,
            12 => ResticError::InvalidPassword,
            _ => ResticError::CommandFailed(format!("Exit code {}: {}", code, stderr)),
        }
    }
}