use std::{process::Command, path::PathBuf};

struct DotFile {
    source: DotFileSource,
    target: DotFiletarget
}

impl DotFile {
    pub fn is_outdated() -> bool {
        true
    }
}

struct DotFileSource {
    path: PathBuf
    hash: Option<String>
}

struct DotFileTarget {
    path: PathBuf
    hash: Option<String>
}

