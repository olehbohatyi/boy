//! `boy u [name]`: refresh the registry, upgrade packages, clear old versions.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::defs::Def;
use crate::home::Home;
use crate::install;
use crate::lock::Lock;
use crate::registry;
use crate::source::Source;

pub fn update(home: &Home, only: Option<&str>) -> Result<()> {
    update_with(home, only, &mut std::io::stdin().lock())
}

/// `update` with the answer to the cleanup question read from `input`.
fn update_with(home: &Home, only: Option<&str>, input: &mut impl BufRead) -> Result<()> {
    home.ensure()?;
    let lock = Lock::load(&home.lock())?;
    if let Some(name) = only
        && !lock.pkgs.contains_key(name)
    {
        bail!("`{name}` is not installed");
    }

    // 1. Refresh the registry. Offline, the local copy still lets boy check versions.
    eprintln!("updating the registry");
    if let Err(e) = registry::refresh(home) {
        eprintln!("warning: {e:#}; using the local copy");
    }

    // 2. Upgrade. `boy u` skips pinned packages; `boy u <name>` updates and unpins.
    let mut updated = Vec::new();
    let mut failed = Vec::new();
    for (name, entry) in &lock.pkgs {
        let wanted = match only {
            Some(only) => only == name,
            None => !entry.pinned,
        };
        if !wanted {
            continue;
        }
        match upgrade(home, name, &entry.version, entry.pinned) {
            Ok(Some(new)) => updated.push(format!("{name} {} -> {new}", entry.version)),
            Ok(None) => {}
            Err(e) => {
                eprintln!("warning: cannot update {name}: {e:#}");
                failed.push(name.clone());
            }
        }
    }
    if updated.is_empty() {
        println!("everything is up to date");
    } else {
        println!("updated: {}", updated.join(", "));
    }

    // 3. Clear old versions, including ones kept by an earlier `n`.
    let stale = stale_versions(home, &Lock::load(&home.lock())?)?;
    if !stale.is_empty() {
        let list: Vec<String> = stale.iter().map(|(n, v)| format!("{n} {v}")).collect();
        let question = format!("remove old versions ({})? [Y/n] ", list.join(", "));
        if ask(&question, input) {
            for (name, version) in &stale {
                drop_version(home, name, version);
            }
        }
    }

    if !failed.is_empty() {
        bail!("could not update {}", failed.join(", "));
    }
    Ok(())
}

/// Install the latest release next to the current one and switch to it.
/// Returns the new version, or `None` when there is nothing newer.
fn upgrade(home: &Home, name: &str, current: &str, pinned: bool) -> Result<Option<String>> {
    let def = Def::load(&home.defs(), name)?;
    let (tag, version) = Source::of(&def)?.latest(&def)?;
    if version == current {
        if pinned {
            // `boy u <name>` on a pinned package that is already the latest: just unpin.
            let mut lock = Lock::load(&home.lock())?;
            if let Some(entry) = lock.pkgs.get_mut(name) {
                entry.pinned = false;
                lock.save(&home.lock())?;
            }
        }
        return Ok(None);
    }
    install::put_version(home, &def, &tag, &version, false)?;
    Ok(Some(version))
}

/// Version folders in `pkgs/` that are not the installed version of their package.
fn stale_versions(home: &Home, lock: &Lock) -> Result<Vec<(String, String)>> {
    let mut stale = Vec::new();
    for (name, entry) in &lock.pkgs {
        let Ok(dirs) = std::fs::read_dir(home.pkgs().join(name)) else {
            continue;
        };
        for dir in dirs {
            let dir = dir?;
            let version = dir.file_name().to_string_lossy().into_owned();
            if dir.file_type()?.is_dir() && version != entry.version {
                stale.push((name.clone(), version));
            }
        }
    }
    stale.sort();
    Ok(stale)
}

/// Delete one old version and any links still pointing into it.
fn drop_version(home: &Home, name: &str, version: &str) {
    let inside = Path::new("..").join("pkgs").join(name).join(version);
    if let Ok(links) = std::fs::read_dir(home.bins()) {
        for link in links.flatten() {
            let path: PathBuf = link.path();
            if std::fs::read_link(&path).is_ok_and(|t| t.starts_with(&inside)) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let dir = home.pkgs().join(name).join(version);
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        eprintln!("warning: cannot delete {}: {e}", dir.display());
    }
}

/// Ask a yes/no question; an empty answer means yes, no answer at all (closed input) means no.
fn ask(question: &str, input: &mut impl BufRead) -> bool {
    eprint!("{question}");
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) | Err(_) => {
            eprintln!();
            false
        }
        Ok(_) => matches!(line.trim().to_lowercase().as_str(), "" | "y" | "yes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Entry;

    #[test]
    fn answers() {
        assert!(ask("? ", &mut "\n".as_bytes()));
        assert!(ask("? ", &mut "Y\n".as_bytes()));
        assert!(ask("? ", &mut "yes\n".as_bytes()));
        assert!(!ask("? ", &mut "n\n".as_bytes()));
        assert!(!ask("? ", &mut "no\n".as_bytes()));
        assert!(
            !ask("? ", &mut "".as_bytes()),
            "closed input keeps old versions"
        );
    }

    fn home() -> (tempfile::TempDir, Home) {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        home.ensure().unwrap();
        (dir, home)
    }

    fn entry(version: &str) -> Entry {
        Entry {
            version: version.into(),
            pinned: false,
            file: String::new(),
            bins: vec![],
        }
    }

    #[test]
    fn finds_versions_other_than_the_installed_one() {
        let (_d, home) = home();
        for v in ["1.7.1", "1.8.2"] {
            std::fs::create_dir_all(home.pkgs().join("jq").join(v)).unwrap();
        }
        std::fs::create_dir_all(home.pkgs().join("just").join("1.59.0")).unwrap();
        let mut lock = Lock::default();
        lock.pkgs.insert("jq".into(), entry("1.8.2"));
        lock.pkgs.insert("just".into(), entry("1.59.0"));

        assert_eq!(
            stale_versions(&home, &lock).unwrap(),
            [("jq".to_string(), "1.7.1".to_string())]
        );
    }

    #[test]
    fn dropping_a_version_removes_only_its_links() {
        let (_d, home) = home();
        std::fs::create_dir_all(home.pkgs().join("rga").join("1.0")).unwrap();
        let link =
            |to: &str, name: &str| std::os::unix::fs::symlink(to, home.bins().join(name)).unwrap();
        link("../pkgs/rga/1.0/rga-old", "rga-old");
        link("../pkgs/rga/2.0/rga", "rga");
        link("../pkgs/rga/1.0.1/x", "x"); // a version whose name starts like 1.0

        drop_version(&home, "rga", "1.0");

        assert!(!home.pkgs().join("rga").join("1.0").exists());
        assert!(std::fs::symlink_metadata(home.bins().join("rga-old")).is_err());
        assert!(std::fs::symlink_metadata(home.bins().join("rga")).is_ok());
        assert!(std::fs::symlink_metadata(home.bins().join("x")).is_ok());
    }

    /// Hits the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn pinned_stays_and_named_update_moves_and_unpins() {
        let (_d, home) = home();
        install::install(&home, "jq", Some("1.7.1")).unwrap();

        // No answer to the cleanup question: old versions are kept.
        update_with(&home, None, &mut "".as_bytes()).unwrap();
        let lock = Lock::load(&home.lock()).unwrap();
        assert_eq!(lock.pkgs["jq"].version, "1.7.1", "pinned is left alone");

        update_with(&home, Some("jq"), &mut "n\n".as_bytes()).unwrap();
        let lock = Lock::load(&home.lock()).unwrap();
        assert_ne!(lock.pkgs["jq"].version, "1.7.1");
        assert!(!lock.pkgs["jq"].pinned);
        assert!(
            home.pkgs().join("jq").join("1.7.1").exists(),
            "kept until answered yes"
        );
        assert_eq!(stale_versions(&home, &lock).unwrap().len(), 1);

        // The next run asks again; `y` clears the old version.
        update_with(&home, None, &mut "y\n".as_bytes()).unwrap();
        assert!(!home.pkgs().join("jq").join("1.7.1").exists());
    }
}
