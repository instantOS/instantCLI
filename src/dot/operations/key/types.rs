use std::path::PathBuf;

#[derive(Debug)]
pub struct KeyInfo {
    pub name: String,
    pub key_type: KeyType,
    pub public_key: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Age,
    Ssh,
}

impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyType::Age => write!(f, "age"),
            KeyType::Ssh => write!(f, "ssh"),
        }
    }
}
