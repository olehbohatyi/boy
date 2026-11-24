//! The boy home folder and its layout.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct Home {
    root: PathBuf,
}

impl Home {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `BOY_HOME`, else the folder boy itself is installed in, else `~/.boy`.
    pub fn discover() -> Result<Self> {
        if let Some(dir) = std::env::var_os("BOY_HOME").filter(|v| !v.is_empty()) {
            return Ok(Self::at(dir));
        }
        if let Some(root) = std::env::current_exe()
            .ok()
            .and_then(|exe| root_from_exe(&exe))
        {
            return Ok(Self::at(root));
        }
        let user = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(Self::at(Path::new(&user).join(".boy")))
    }

    pub fn bins(&self) -> PathBuf {
        self.root.join("bins")
    }
    pub fn pkgs(&self) -> PathBuf {
        self.root.join("pkgs")
    }
    pub fn defs(&self) -> PathBuf {
        self.root.join("defs")
    }
    pub fn temp(&self) -> PathBuf {
        self.root.join("temp")
    }
    pub fn lock(&self) -> PathBuf {
        self.root.join("lock.toml")
    }

    /// Create the folders if they don't exist yet.
    pub fn ensure(&self) -> Result<()> {
        for dir in [self.bins(), self.pkgs(), self.defs(), self.temp()] {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        Ok(())
    }
}

/// Find the home from where the boy binary lives:
/// `<home>/bins/boy` (the symlink) or `<home>/pkgs/boy/<version>/boy` (its target).
fn root_from_exe(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    if dir.file_name()? == "bins" {
        return dir.parent().map(Path::to_path_buf);
    }
    let pkg = dir.parent()?;
    let pkgs = pkg.parent()?;
    if pkg.file_name()? == "boy" && pkgs.file_name()? == "pkgs" {
        return pkgs.parent().map(Path::to_path_buf);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_root_from_the_symlink_in_bins() {
        assert_eq!(
            root_from_exe(Path::new("/x/.boy/bins/boy")),
            Some(PathBuf::from("/x/.boy"))
        );
    }

    #[test]
    fn finds_root_from_the_installed_binary() {
        assert_eq!(
            root_from_exe(Path::new("/x/.boy/pkgs/boy/0.1.0/boy")),
            Some(PathBuf::from("/x/.boy"))
        );
    }

    #[test]
    fn a_dev_build_is_not_a_home() {
        assert_eq!(root_from_exe(Path::new("/src/boy/target/debug/boy")), None);
    }
}
