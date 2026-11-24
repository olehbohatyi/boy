//! `boy i <name>`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::defs::{self, Def};
use crate::github;
use crate::home::Home;
use crate::lock::{Entry, Lock};

pub fn install(home: &Home, name: &str) -> Result<()> {
    home.ensure()?;
    let def = Def::load(&home.defs(), name)?;
    if def.from != "github" {
        bail!(
            "`{name}` uses source `{}`, which this boy does not support",
            def.from
        );
    }
    let plat = def.current_plat()?;

    let mut lock = Lock::load(&home.lock())?;
    if let Some(entry) = lock.pkgs.get(name) {
        println!("{name} {} is already installed", entry.version);
        return Ok(());
    }

    let tag = github::latest_tag(&def.repo)?;
    let version = def
        .tag_pattern()
        .version(&tag)
        .with_context(|| {
            format!(
                "latest tag `{tag}` does not match `tags = \"{}\"`",
                def.tags
            )
        })?
        .to_string();

    let file = defs::expand(&plat.file, &version);
    let exe = safe_relative(&defs::expand(&plat.path, &version))?;
    if is_archive(&file) {
        bail!("`{file}` is an archive; archives are not supported yet");
    }

    // Download into temp/, then move into place, so pkgs/ never holds a half file.
    let part = home.temp().join(format!("{name}-{version}.part"));
    github::download(&github::download_url(&def.repo, &tag, &file), &part)?;

    let pkg_dir = home.pkgs().join(name).join(&version);
    if pkg_dir.exists() {
        std::fs::remove_dir_all(&pkg_dir)?;
    }
    let dest = pkg_dir.join(&exe);
    std::fs::create_dir_all(dest.parent().expect("inside pkg_dir"))?;
    std::fs::rename(&part, &dest)
        .with_context(|| format!("cannot move into {}", dest.display()))?;
    make_executable(&dest)?;

    for bin in &def.bins {
        let target = Path::new("..")
            .join("pkgs")
            .join(name)
            .join(&version)
            .join(bin_path(&exe, bin, def.bins.len()));
        link(&target, &home.bins().join(bin))?;
    }

    lock.pkgs.insert(
        name.to_string(),
        Entry {
            version: version.clone(),
            pinned: false,
            file,
            bins: def.bins.clone(),
        },
    );
    lock.save(&home.lock())?;

    println!("installed {name} {version}");
    Ok(())
}

/// With one command, `path` is the executable. With several, they sit next to it.
fn bin_path(exe: &Path, bin: &str, bin_count: usize) -> PathBuf {
    if bin_count == 1 {
        exe.to_path_buf()
    } else {
        exe.with_file_name(bin)
    }
}

/// `path` from a definition must stay inside the package folder.
fn safe_relative(path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    let ok = !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)));
    if !ok {
        bail!("`path = \"{path}\"` must be a plain relative path");
    }
    Ok(p)
}

fn is_archive(file: &str) -> bool {
    [".tar.gz", ".tgz", ".tar.xz", ".zip"]
        .iter()
        .any(|ext| file.ends_with(ext))
}

fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("cannot make {} executable", path.display()))
}

/// Point `link` at `target`, replacing an old symlink atomically.
fn link(target: &Path, link: &Path) -> Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(link)
        && !meta.file_type().is_symlink()
    {
        bail!("{} exists and is not a boy symlink", link.display());
    }
    let tmp = link.with_extension("boy-tmp");
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp)
        .with_context(|| format!("cannot create {}", tmp.display()))?;
    std::fs::rename(&tmp, link).with_context(|| format!("cannot create {}", link.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_command_uses_path() {
        assert_eq!(
            bin_path(Path::new("dir/jq"), "jq", 1),
            PathBuf::from("dir/jq")
        );
    }

    #[test]
    fn several_commands_sit_next_to_path() {
        assert_eq!(
            bin_path(Path::new("dir/rga"), "rga-preproc", 2),
            PathBuf::from("dir/rga-preproc")
        );
    }

    #[test]
    fn path_must_stay_inside_the_package() {
        assert!(safe_relative("jq").is_ok());
        assert!(safe_relative("a/b/jq").is_ok());
        assert!(safe_relative("../jq").is_err());
        assert!(safe_relative("/usr/bin/jq").is_err());
        assert!(safe_relative("").is_err());
    }

    #[test]
    fn detects_archives() {
        assert!(is_archive("x.tar.gz"));
        assert!(!is_archive("jq-macos-amd64"));
    }

    #[test]
    fn link_replaces_an_old_symlink_but_not_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let l = dir.path().join("jq");
        link(Path::new("a"), &l).unwrap();
        link(Path::new("b"), &l).unwrap();
        assert_eq!(std::fs::read_link(&l).unwrap(), PathBuf::from("b"));

        let f = dir.path().join("real");
        std::fs::write(&f, "x").unwrap();
        assert!(link(Path::new("b"), &f).is_err());
    }
}
