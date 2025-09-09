use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};
use shellexpand;

/// A PathBuf that automatically handles tilde expansion/compression
#[derive(Debug, Clone, PartialEq)]
pub struct TildePath(PathBuf);

impl TildePath {
    pub fn new(path: PathBuf) -> Self {
        TildePath(path)
    }
    
    pub fn as_path(&self) -> &Path {
        &self.0
    }
    
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
    
    /// Create from a string with tilde expansion
    pub fn from_str(s: &str) -> Result<Self> {
        let expanded = shellexpand::tilde(s).to_string();
        let path = PathBuf::from(expanded);
        Ok(TildePath(path))
    }
    
    /// Convert to string with tilde compression (replace home dir with ~)
    pub fn to_tilde_string(&self) -> Result<String> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        
        if let Ok(relative) = self.0.strip_prefix(&home_dir) {
            if relative.as_os_str().is_empty() {
                return Ok("~".to_string());
            }
            return Ok(format!("~/{}", relative.display()));
        }
        
        Ok(self.0.to_string_lossy().to_string())
    }
}

impl From<TildePath> for PathBuf {
    fn from(tilde_path: TildePath) -> Self {
        tilde_path.0
    }
}

impl AsRef<Path> for TildePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Serialize for TildePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let tilde_string = self.to_tilde_string()
            .map_err(serde::ser::Error::custom)?;
        tilde_string.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TildePath {
    fn deserialize<D>(deserializer: D) -> Result<TildePath, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TildePath::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Default for TildePath {
    fn default() -> Self {
        TildePath(PathBuf::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
        
    #[test]
    fn test_tilde_expansion() {
        let tilde_path = TildePath::from_str("~/test/path").unwrap();
        let expanded = tilde_path.as_path();
        assert!(expanded.to_string_lossy().contains(&std::env::var("HOME").unwrap()));
    }
    
    #[test]
    fn test_tilde_compression() {
        let home = dirs::home_dir().unwrap();
        let test_path = home.join("test").join("path");
        let tilde_path = TildePath::new(test_path);
        let compressed = tilde_path.to_tilde_string().unwrap();
        assert_eq!(compressed, "~/test/path");
    }
    
    #[test]
    fn test_serialization_roundtrip() {
        let original = TildePath::from_str("~/test/path").unwrap();
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: TildePath = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original.as_path(), deserialized.as_path());
    }
}