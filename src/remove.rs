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

    home.ensure()?;

    // 1. Move the package out of pkgs/ in one rename. Until the lock is saved this is
    //    undone on any error, so a failed removal leaves the package as it was.
    let pkg_dir = home.pkgs().join(name);
    let trash = home.temp().join(format!("{name}.removing"));
    if trash.exists() {
        std::fs::remove_dir_all(&trash)?;
    }
    let moved = match std::fs::rename(&pkg_dir, &trash) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e).with_context(|| format!("cannot remove {}", pkg_dir.display())),
    };

    let mut removed: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = unlink_and_forget(home, &mut lock, name, &entry.bins, &mut removed);
    if let Err(e) = result {
        for (link, target) in &removed {
            let _ = std::os::unix::fs::symlink(target, link);
        }
        if moved {
            let _ = std::fs::rename(&trash, &pkg_dir);
        }
        return Err(e);
    }

    // 4. The package is gone for boy now; deleting the moved files can only leave clutter.
    if moved && let Err(e) = std::fs::remove_dir_all(&trash) {
        eprintln!("warning: cannot delete {}: {e}", trash.display());
    }
    println!("removed {name} {}", entry.version);
    Ok(())
}

/// 2. Remove the package's links (only those still pointing into it), then
/// 3. drop it from the lock. Links removed so far are recorded for a rollback.
fn unlink_and_forget(
    home: &Home,
    lock: &mut Lock,
    name: &str,
    bins: &[String],
    removed: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<()> {
    let ours = Path::new("..").join("pkgs").join(name);
    for bin in bins {
        let link = home.bins().join(bin);
        match std::fs::read_link(&link) {
            Ok(target) if target.starts_with(&ours) => {
                std::fs::remove_file(&link)
                    .with_context(|| format!("cannot remove {}", link.display()))?;
                removed.push((link, target));
            }
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
    lock.pkgs.remove(name);
    lock.save(&home.lock())
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
    fn a_missing_link_does_not_touch_other_packages() {
        let (_dir, home) = setup();
        let mut lock = Lock::default();
        installed(&home, &mut lock, "jq", "1.8.2");
        installed(&home, &mut lock, "just", "1.59.0");
        lock.save(&home.lock()).unwrap();
        std::fs::remove_file(home.bins().join("jq")).unwrap();

        remove(&home, "jq").unwrap();

        assert!(!home.pkgs().join("jq").exists());
        assert!(
            home.pkgs()
                .join("just")
                .join("1.59.0")
                .join("just")
                .exists()
        );
        assert!(home.bins().join("just").exists());
        assert!(Lock::load(&home.lock()).unwrap().pkgs.contains_key("just"));
        assert!(!home.temp().join("jq.removing").exists());
    }

    #[test]
    fn a_failed_lock_save_puts_everything_back() {
        let (_dir, home) = setup();
        let mut lock = Lock::default();
        installed(&home, &mut lock, "jq", "1.8.2");
        lock.save(&home.lock()).unwrap();
        // Make saving the lock fail: its temporary file's place is taken by a folder.
        std::fs::create_dir(home.lock().with_extension("toml.tmp")).unwrap();

        assert!(remove(&home, "jq").is_err());

        assert!(home.pkgs().join("jq").join("1.8.2").join("jq").exists());
        assert!(
            home.bins().join("jq").exists(),
            "link restored and resolves"
        );
        assert!(Lock::load(&home.lock()).unwrap().pkgs.contains_key("jq"));
    }

    #[test]
    fn unknown_package_is_an_error() {
        let (_dir, home) = setup();
        let err = remove(&home, "nope").unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }
}
