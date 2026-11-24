//! The local copy of the registry (`<home>/defs/`), fetched from the boy repository.

use std::fs::File;
use std::io::BufReader;
use std::path::{Component, Path};

use anyhow::{Context, Result};

use crate::defs;
use crate::github;
use crate::home::Home;

/// The boy repository as one archive, by direct link (no API).
const ARCHIVE: &str = "https://github.com/olehbohatyi/boy/archive/refs/heads/main.tar.gz";

/// Replace `<home>/defs/` with the definitions from the boy repository.
/// Returns how many definitions there are now.
pub fn refresh(home: &Home) -> Result<usize> {
    home.ensure()?;
    let part = home.temp().join("registry.tar.gz");
    let fresh = home.temp().join("defs.new");
    if fresh.exists() {
        std::fs::remove_dir_all(&fresh)?;
    }
    std::fs::create_dir_all(&fresh)?;

    let count = github::download(ARCHIVE, &part).and_then(|()| extract_defs(&part, &fresh));
    let _ = std::fs::remove_file(&part);
    let count = match count {
        Ok(0) => Err(anyhow::anyhow!("the registry archive has no definitions")),
        other => other,
    };
    if let Err(e) = count {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(e).context("cannot update the registry");
    }

    swap(&home.defs(), &fresh, &home.temp().join("defs.old"))?;
    count
}

/// Copy `<root>/defs/<name>.toml` entries of a GitHub repository archive into `dest`.
/// Everything else in the archive is ignored.
fn extract_defs(archive: &Path, dest: &Path) -> Result<usize> {
    let file = File::open(archive).with_context(|| format!("cannot read {}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(BufReader::new(file)));
    let mut count = 0;
    for entry in tar.entries().context("cannot read the registry archive")? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.into_owned();
        let parts: Vec<_> = path.components().collect();
        let [
            Component::Normal(_),
            Component::Normal(dir),
            Component::Normal(file),
        ] = parts[..]
        else {
            continue;
        };
        let Some(name) = file.to_str().and_then(|f| f.strip_suffix(".toml")) else {
            continue;
        };
        if dir != "defs" || defs::check_name(name).is_err() {
            continue;
        }
        entry.unpack(dest.join(file))?;
        count += 1;
    }
    Ok(count)
}

/// Put `fresh` in place of `current`; the old copy goes through `old` and is deleted.
///
/// This takes two renames, so a crash between them leaves no `defs/` at all. That is
/// safe: the next `boy i` finds no definition and refreshes the registry again.
fn swap(current: &Path, fresh: &Path, old: &Path) -> Result<()> {
    if old.exists() {
        std::fs::remove_dir_all(old)?;
    }
    let had_current = current.exists();
    if had_current {
        std::fs::rename(current, old)?;
    }
    if let Err(e) = std::fs::rename(fresh, current) {
        if had_current {
            let _ = std::fs::rename(old, current);
        }
        return Err(e).with_context(|| format!("cannot replace {}", current.display()));
    }
    if had_current {
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(path: &Path, entries: &[(&str, &str)]) {
        let enc = flate2::write::GzEncoder::new(File::create(path).unwrap(), Default::default());
        let mut tar = tar::Builder::new(enc);
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, name, data.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn takes_only_top_level_definitions() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.tar.gz");
        archive(
            &a,
            &[
                ("boy-main/defs/jq.toml", "name = \"jq\""),
                ("boy-main/defs/just.toml", "name = \"just\""),
                ("boy-main/defs/README.md", "not a definition"),
                ("boy-main/defs/sub/x.toml", "nested"),
                ("boy-main/src/main.rs", "fn main() {}"),
                ("boy-main/.defs.toml", "hidden"),
            ],
        );
        let dest = dir.path().join("out");
        std::fs::create_dir(&dest).unwrap();

        assert_eq!(extract_defs(&a, &dest).unwrap(), 2);
        let mut names: Vec<_> = std::fs::read_dir(&dest)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["jq.toml", "just.toml"]);
    }

    #[test]
    fn swap_replaces_the_old_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (current, fresh, old) = (
            dir.path().join("defs"),
            dir.path().join("defs.new"),
            dir.path().join("defs.old"),
        );
        std::fs::create_dir(&current).unwrap();
        std::fs::write(current.join("gone.toml"), "").unwrap();
        std::fs::create_dir(&fresh).unwrap();
        std::fs::write(fresh.join("jq.toml"), "").unwrap();

        swap(&current, &fresh, &old).unwrap();

        assert!(current.join("jq.toml").exists());
        assert!(!current.join("gone.toml").exists());
        assert!(!fresh.exists() && !old.exists());
    }

    #[test]
    fn swap_works_without_an_old_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (current, fresh) = (dir.path().join("defs"), dir.path().join("defs.new"));
        std::fs::create_dir(&fresh).unwrap();
        swap(&current, &fresh, &dir.path().join("defs.old")).unwrap();
        assert!(current.is_dir());
    }

    /// Hits the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn refreshes_from_github() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        assert!(refresh(&home).unwrap() >= 3);
        assert!(home.defs().join("jq.toml").exists());
    }
}
