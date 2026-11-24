//! `boy r <name>`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::home::Home;
use crate::lock::Lock;

pub fn remove(home: &Home, name: &str) -> Result<()> {
    let mut lock = Lock::load(&home.lock())?;
    let Some(entry) = lock.pkgs.get(name).cloned() else {
        bail!("`{name}` is not installed");
    };

    // Only remove links that still point into this package; leave anything else alone.
    let ours = Path::new("..").join("pkgs").join(name);
    for bin in &entry.bins {
        let link = home.bins().join(bin);
        match std::fs::read_link(&link) {
            Ok(target) if target.starts_with(&ours) => std::fs::remove_file(&link)
                .with_context(|| format!("cannot remove {}", link.display()))?,
            Ok(_) => eprintln!(
                "warning: {} points elsewhere; left in place",
                link.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => eprintln!(
                "warning: {} is not a symlink; left in place",
                link.display()
            ),
        }
    }

    let pkg_dir: PathBuf = home.pkgs().join(name);
    match std::fs::remove_dir_all(&pkg_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("cannot remove {}", pkg_dir.display())),
    }

    // The lock changes last: if anything above fails, `boy r` can simply be run again.
    lock.pkgs.remove(name);
    lock.save(&home.lock())?;

    println!("removed {name} {}", entry.version);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Entry;

    /// A home with `name` installed the way `boy i` leaves it.
    fn installed(home: &Home, lock: &mut Lock, name: &str, version: &str) {
        let dir = home.pkgs().join(name).join(version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), "bin").unwrap();
        std::os::unix::fs::symlink(
            Path::new("..")
                .join("pkgs")
                .join(name)
                .join(version)
                .join(name),
            home.bins().join(name),
        )
        .unwrap();
        lock.pkgs.insert(
            name.into(),
            Entry {
                version: version.into(),
                pinned: false,
                file: String::new(),
                bins: vec![name.into()],
            },
        );
    }

    fn setup() -> (tempfile::TempDir, Home) {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        home.ensure().unwrap();
        (dir, home)
    }

    #[test]
    fn removes_package_links_and_lock_entry_only() {
        let (_dir, home) = setup();
        let mut lock = Lock::default();
        installed(&home, &mut lock, "jq", "1.8.2");
        installed(&home, &mut lock, "just", "1.59.0");
        lock.save(&home.lock()).unwrap();

        remove(&home, "jq").unwrap();

        assert!(!home.pkgs().join("jq").exists());
        assert!(std::fs::symlink_metadata(home.bins().join("jq")).is_err());
        let lock = Lock::load(&home.lock()).unwrap();
        assert!(!lock.pkgs.contains_key("jq"));
        // The other package is untouched.
        assert!(lock.pkgs.contains_key("just"));
        assert!(home.bins().join("just").exists());
    }

    #[test]
    fn leaves_a_link_that_points_elsewhere() {
        let (_dir, home) = setup();
        let mut lock = Lock::default();
        installed(&home, &mut lock, "jq", "1.8.2");
        lock.save(&home.lock()).unwrap();
        std::fs::remove_file(home.bins().join("jq")).unwrap();
        std::os::unix::fs::symlink("/usr/bin/true", home.bins().join("jq")).unwrap();

        remove(&home, "jq").unwrap();

        assert_eq!(
            std::fs::read_link(home.bins().join("jq")).unwrap(),
            PathBuf::from("/usr/bin/true")
        );
        assert!(!home.pkgs().join("jq").exists());
    }

    #[test]
    fn unknown_package_is_an_error() {
        let (_dir, home) = setup();
        let err = remove(&home, "nope").unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }
}
