//! `boy i <name>`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::archive::{self, Kind};
use crate::defs::{self, Def};
use crate::home::Home;
use crate::lock::{Entry, Lock};
use crate::net;
use crate::registry;
use crate::source::Source;
use crate::sums;

/// Install the latest release of `name`, or with `pin` that exact version, pinned.
pub fn install(home: &Home, name: &str, pin: Option<&str>) -> Result<()> {
    if let Some(version) = pin {
        defs::check_version(version)?;
    }
    home.ensure()?;
    // Not in the local copy: the registry may have it by now.
    let def = match Def::find(&home.defs(), name)? {
        Some(def) => def,
        None => {
            eprintln!("updating the registry");
            registry::refresh(home)?;
            Def::load(&home.defs(), name)?
        }
    };
    let source = Source::of(&def)?;
    // Fail early, before any network, when this system has no release file.
    let plat = def.current_plat()?;

    let mut lock = Lock::load(&home.lock())?;
    // With another version installed, `name@version` switches to it (the rollback path).
    let mut previous: Option<Entry> = None;
    if let Some(entry) = lock.pkgs.get_mut(name) {
        match pin {
            Some(version) if version == entry.version => {
                if entry.pinned {
                    println!("{name} {version} is already installed and pinned");
                } else {
                    entry.pinned = true;
                    lock.save(&home.lock())?;
                    println!("pinned {name} {version}");
                }
                return Ok(());
            }
            Some(_) => previous = Some(entry.clone()),
            None => {
                println!("{name} {} is already installed", entry.version);
                return Ok(());
            }
        }
    }

    let (tag, version) = match pin {
        Some(version) => source.pinned(&def, version, &defs::expand(&plat.file, version))?,
        None => source.latest(&def)?,
    };

    put_version(home, &def, &tag, &version, pin.is_some())?;

    let pinned = if pin.is_some() { " (pinned)" } else { "" };
    match previous {
        Some(old) => {
            drop_old_version(home, name, &old, &def.bins);
            println!("switched {name} {} -> {version}{pinned}", old.version);
        }
        None => println!("installed {name} {version}{pinned}"),
    }
    Ok(())
}

/// Download, verify and unpack one version into `pkgs/<name>/<version>/`, point the
/// links at it and record it in the lock. Other versions in `pkgs/<name>/` stay.
pub fn put_version(home: &Home, def: &Def, tag: &str, version: &str, pinned: bool) -> Result<()> {
    let name = def.name.as_str();
    let source = Source::of(def)?;
    let plat = def.current_plat()?;
    let file = defs::expand(&plat.file, version);
    let exe = safe_relative(&defs::expand(&plat.path, version))?;
    let kind = Kind::of(&file);

    // Download into temp/, then move into place, so pkgs/ never holds a half file.
    let part = home.temp().join(format!("{name}-{version}.part"));
    let fetched = net::download(&source.url(def, tag, &file), &part)
        .and_then(|()| check_sums(home, def, source, tag, version, &file, &part));
    if let Err(e) = fetched {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }

    // Unpack into a staging folder in temp/, then move it into pkgs/ in one step.
    let stage = home.temp().join(format!("{name}-{version}.d"));
    let staged = stage_release(&part, kind, &stage, &exe, &def.bins, &file);
    let _ = std::fs::remove_file(&part);
    if let Err(e) = staged {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(e);
    }

    let pkg_dir = home.pkgs().join(name).join(version);
    if pkg_dir.exists() {
        std::fs::remove_dir_all(&pkg_dir)?;
    }
    std::fs::create_dir_all(home.pkgs().join(name))?;
    std::fs::rename(&stage, &pkg_dir)
        .with_context(|| format!("cannot move into {}", pkg_dir.display()))?;

    for bin in &def.bins {
        let target = Path::new("..")
            .join("pkgs")
            .join(name)
            .join(version)
            .join(bin_path(&exe, bin, def.bins.len()));
        link(&target, &home.bins().join(bin))?;
    }

    let mut lock = Lock::load(&home.lock())?;
    lock.pkgs.insert(
        name.to_string(),
        Entry {
            version: version.to_string(),
            pinned,
            file,
            bins: def.bins.clone(),
        },
    );
    lock.save(&home.lock())
}

/// After switching versions: remove the old version's folder and any of its links
/// the new version no longer provides. The switch is already done, so failures only warn.
fn drop_old_version(home: &Home, name: &str, old: &Entry, new_bins: &[String]) {
    let old_dir = Path::new("..").join("pkgs").join(name).join(&old.version);
    for bin in old.bins.iter().filter(|b| !new_bins.contains(b)) {
        let link = home.bins().join(bin);
        if std::fs::read_link(&link).is_ok_and(|t| t.starts_with(&old_dir)) {
            let _ = std::fs::remove_file(&link);
        }
    }
    let dir = home.pkgs().join(name).join(&old.version);
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        eprintln!("warning: cannot delete {}: {e}", dir.display());
    }
}

/// Put the release into `stage` and check every command is where the definition says.
fn stage_release(
    part: &Path,
    kind: Kind,
    stage: &Path,
    exe: &Path,
    bins: &[String],
    file: &str,
) -> Result<()> {
    if stage.exists() {
        std::fs::remove_dir_all(stage)?;
    }
    std::fs::create_dir_all(stage)?;
    if kind == Kind::Bare {
        let dest = stage.join(exe);
        std::fs::create_dir_all(dest.parent().expect("inside stage"))?;
        std::fs::copy(part, &dest)?;
    } else {
        archive::extract(part, kind, stage)?;
    }
    let root = stage.canonicalize()?;
    for bin in bins {
        let rel = bin_path(exe, bin, bins.len());
        let path = stage.join(&rel);
        if !path.is_file() {
            bail!("`{}` is not in `{file}` (check `path`)", rel.display());
        }
        // A command may be a symlink inside the release, but never to a file outside it.
        if !path.canonicalize()?.starts_with(&root) {
            bail!("`{}` in `{file}` points outside the package", rel.display());
        }
        make_executable(&path)?;
    }
    Ok(())
}

/// Verify the download against `sums`, or warn when the definition has none.
fn check_sums(
    home: &Home,
    def: &Def,
    source: Source,
    tag: &str,
    version: &str,
    file: &str,
    part: &Path,
) -> Result<()> {
    let Some(sums) = &def.sums else {
        eprintln!(
            "warning: `{}` has no checksum file; installing unverified",
            def.name
        );
        return Ok(());
    };
    let (sums_file, per_file) = sums_name(sums, version, file);
    let sums_path = home.temp().join(format!("{}-{version}.sums", def.name));
    net::download(&source.url(def, tag, &sums_file), &sums_path)?;
    let text = std::fs::read_to_string(&sums_path);
    let _ = std::fs::remove_file(&sums_path);
    let text = text.with_context(|| format!("cannot read `{sums_file}`"))?;
    sums::verify(part, &text, file, per_file)
}

/// The checksum file's name in the release, and whether it belongs to `file` alone.
fn sums_name(template: &str, version: &str, file: &str) -> (String, bool) {
    let name = defs::expand(template, version).replace("{file}", file);
    (name, template.contains("{file}"))
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

    /// A definition without this system's platform stops before any network or change.
    #[test]
    fn a_program_without_this_platform_is_refused_and_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        home.ensure().unwrap();
        let other = if defs::platform_key() == "macos-aarch64" {
            "macos-x86_64"
        } else {
            "macos-aarch64"
        };
        let def = format!(
            "name = \"only\"\nfrom = \"github\"\nrepo = \"nobody/only\"\nbins = [\"only\"]\n\
             [plat]\n{other} = {{ file = \"only\", path = \"only\" }}\n"
        );
        std::fs::write(home.defs().join("only.toml"), def).unwrap();

        let err = install(&home, "only", None).unwrap_err().to_string();
        assert!(err.contains("not available for this system"), "{err}");
        assert!(
            err.ends_with(&format!("it has releases for {other}")),
            "{err}"
        );
        for dir in [home.bins(), home.pkgs(), home.temp()] {
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "{dir:?}");
        }
        assert!(!home.lock().exists());
    }

    #[test]
    fn sums_name_expands_version_and_file() {
        let file = "rg-1.0.0.tar.gz";
        assert_eq!(
            sums_name("tool_{version}_checksums.txt", "1.0.0", file),
            ("tool_1.0.0_checksums.txt".to_string(), false)
        );
        assert_eq!(
            sums_name("{file}.sha256", "1.0.0", file),
            ("rg-1.0.0.tar.gz.sha256".to_string(), true)
        );
    }

    /// delta ships Apple Silicon only.
    #[test]
    fn delta_definition_is_apple_silicon_only() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let def = Def::load(&dir, "delta").unwrap();
        assert_eq!(def.plat.keys().collect::<Vec<_>>(), ["macos-aarch64"]);
        let arm = &def.plat["macos-aarch64"];
        assert_eq!(
            defs::expand(&arm.file, "0.22.0"),
            "delta-0.22.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            defs::expand(&arm.path, "0.22.0"),
            "delta-0.22.0-aarch64-apple-darwin/delta"
        );
        assert_eq!(def.tag_pattern().tag("0.22.0"), "0.22.0");
    }

    /// The fallback in `install`: a home without `defs/` fetches the registry first.
    /// Hits the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn installs_on_a_clean_home_by_fetching_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        install(&home, "jq", None).unwrap();
        assert!(home.defs().join("jq.toml").exists());
        assert!(home.bins().join("jq").exists());
        assert!(Lock::load(&home.lock()).unwrap().pkgs.contains_key("jq"));
    }

    /// Hits the network (16 MB); run with `cargo test -- --ignored`.
    /// A real `.tar.xz` release, with `runtime/` kept next to the binary.
    #[test]
    #[ignore]
    fn installs_helix_from_tar_xz_with_its_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        home.ensure().unwrap();
        let repo_defs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        std::fs::copy(repo_defs.join("helix.toml"), home.defs().join("helix.toml")).unwrap();

        install(&home, "helix", Some("25.07.1")).unwrap();

        let link = home.bins().join("hx");
        let target = std::fs::read_link(&link).unwrap();
        assert!(target.starts_with("../pkgs/helix/25.07.1"), "{target:?}");
        let real = std::fs::canonicalize(&link).unwrap();
        assert!(real.parent().unwrap().join("runtime/themes").is_dir());
        assert!(real.parent().unwrap().join("runtime/queries").is_dir());
        assert_eq!(std::fs::read_dir(home.temp()).unwrap().count(), 0);
    }

    /// Hits the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn installs_and_pins_a_given_version() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        install(&home, "jq", Some("1.7.1")).unwrap();
        let entry = Lock::load(&home.lock()).unwrap().pkgs["jq"].clone();
        assert_eq!((entry.version.as_str(), entry.pinned), ("1.7.1", true));
        assert!(home.pkgs().join("jq").join("1.7.1").join("jq").exists());

        // Another version switches to it and drops the old one.
        install(&home, "jq", Some("1.8.2")).unwrap();
        let entry = Lock::load(&home.lock()).unwrap().pkgs["jq"].clone();
        assert_eq!((entry.version.as_str(), entry.pinned), ("1.8.2", true));
        assert!(!home.pkgs().join("jq").join("1.7.1").exists());
        assert_eq!(
            std::fs::read_link(home.bins().join("jq")).unwrap(),
            Path::new("../pkgs/jq/1.8.2/jq")
        );

        // A missing version is explained and changes nothing.
        let err = install(&home, "jq", Some("0.0.0")).unwrap_err();
        assert!(err.to_string().contains("no release 0.0.0"), "{err}");
        assert_eq!(
            Lock::load(&home.lock()).unwrap().pkgs["jq"].version,
            "1.8.2"
        );
    }

    #[test]
    fn dropping_the_old_version_keeps_shared_and_foreign_links() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::at(dir.path());
        home.ensure().unwrap();
        let old_dir = home.pkgs().join("rga").join("1.0");
        std::fs::create_dir_all(&old_dir).unwrap();
        let old_link = |bin: &str| PathBuf::from(format!("../pkgs/rga/1.0/{bin}"));
        // `rga` moved on to the new version already; `rga-old` only the old one had;
        // `other` belongs to someone else.
        link(Path::new("../pkgs/rga/2.0/rga"), &home.bins().join("rga")).unwrap();
        link(&old_link("rga-old"), &home.bins().join("rga-old")).unwrap();
        link(Path::new("../pkgs/x/1/other"), &home.bins().join("other")).unwrap();
        let old = Entry {
            version: "1.0".into(),
            pinned: false,
            file: String::new(),
            bins: vec!["rga".into(), "rga-old".into(), "other".into()],
        };

        drop_old_version(&home, "rga", &old, &["rga".into()]);

        assert!(!old_dir.exists());
        assert!(std::fs::symlink_metadata(home.bins().join("rga")).is_ok());
        assert!(std::fs::symlink_metadata(home.bins().join("rga-old")).is_err());
        assert!(std::fs::symlink_metadata(home.bins().join("other")).is_ok());
    }

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
    fn staging_refuses_a_command_that_points_outside_the_package() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::write(&outside, "x").unwrap();
        // A release whose `jq` is a symlink to a file outside the package.
        let tarball = dir.path().join("a.tar.gz");
        let enc = flate2::write::GzEncoder::new(
            std::fs::File::create(&tarball).unwrap(),
            Default::default(),
        );
        let mut tar = tar::Builder::new(enc);
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        tar.append_link(&mut h, "jq", &outside).unwrap();
        tar.into_inner().unwrap().finish().unwrap();

        let stage = dir.path().join("stage");
        let bins = vec!["jq".to_string()];
        let err =
            stage_release(&tarball, Kind::TarGz, &stage, Path::new("jq"), &bins, "f").unwrap_err();
        assert!(err.to_string().contains("outside"), "{err}");
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
