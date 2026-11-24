//! `lock.toml`: what is installed.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Lock {
    #[serde(default)]
    pub pkgs: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub version: String,
    #[serde(default)]
    pub pinned: bool,
    /// The release file it was installed from.
    pub file: String,
    /// Symlinks created in `bins/`.
    pub bins: Vec<String>,
}

impl Lock {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("bad {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// Write through a temporary file so a crash never leaves a half-written lock.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_lock_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            Lock::load(&dir.path().join("lock.toml"))
                .unwrap()
                .pkgs
                .is_empty()
        );
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock.toml");
        let entry = Entry {
            version: "1.8.2".into(),
            pinned: false,
            file: "jq-macos-amd64".into(),
            bins: vec!["jq".into()],
        };
        let mut lock = Lock::default();
        lock.pkgs.insert("jq".into(), entry.clone());
        lock.save(&path).unwrap();
        assert_eq!(Lock::load(&path).unwrap().pkgs["jq"], entry);
    }
}
