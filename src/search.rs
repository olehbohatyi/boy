//! `boy s [query]`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::defs::Def;
use crate::home::Home;
use crate::lock::Lock;
use crate::registry;

pub fn search(home: &Home, query: Option<&str>) -> Result<()> {
    // Only the very first search fetches the registry; after that it works offline.
    if !has_definitions(&home.defs()) {
        eprintln!("updating the registry");
        registry::refresh(home)?;
    }
    let all = load_all(&home.defs())?;
    let lock = Lock::load(&home.lock())?;
    let found: Vec<&Def> = all.iter().filter(|d| matches(d, query)).collect();
    if found.is_empty() {
        match query {
            Some(q) => println!("nothing found for `{q}`"),
            None => println!("the registry is empty"),
        }
        return Ok(());
    }
    print!("{}", render(&found, &lock));
    Ok(())
}

fn has_definitions(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| {
        entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "toml")))
    })
}

/// Every definition in the folder, sorted by name. A broken file is skipped with a warning.
fn load_all(dir: &Path) -> Result<Vec<Def>> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let path = entry?.path();
        let Some(name) = path
            .file_name()
            .and_then(|f| f.to_str())
            .and_then(|f| f.strip_suffix(".toml"))
        else {
            continue;
        };
        match Def::load(dir, name) {
            Ok(def) => all.push(def),
            Err(e) => eprintln!("warning: skipping {}: {e:#}", path.display()),
        }
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(all)
}

/// Case-insensitive match on the name, the description and the commands.
fn matches(def: &Def, query: Option<&str>) -> bool {
    let Some(q) = query.map(str::to_lowercase) else {
        return true;
    };
    def.name.to_lowercase().contains(&q)
        || def.info.to_lowercase().contains(&q)
        || def.bins.iter().any(|b| b.to_lowercase().contains(&q))
}

/// One line per program, aligned; installed ones are marked with their version:
///
/// ```text
/// hyperfine  Command-line benchmarking tool
/// jq         Command-line JSON processor      installed 1.8.2
/// ```
fn render(found: &[&Def], lock: &Lock) -> String {
    let name_w = found.iter().map(|d| d.name.len()).max().unwrap_or(0);
    let info_w = found.iter().map(|d| d.info.len()).max().unwrap_or(0);
    let mut out = String::new();
    for def in found {
        let line = match lock.pkgs.get(&def.name) {
            Some(e) => format!(
                "{:name_w$}  {:info_w$}  installed {}",
                def.name, def.info, e.version
            ),
            None => format!("{:name_w$}  {}", def.name, def.info),
        };
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Entry;

    fn def(name: &str, info: &str, bins: &[&str]) -> Def {
        Def::parse(&format!(
            "name = \"{name}\"\ninfo = \"{info}\"\nfrom = \"github\"\nrepo = \"a/b\"\nbins = {bins:?}\n\n[plat]\nmacos-x86_64 = {{ file = \"f\", path = \"p\" }}\n"
        ))
        .unwrap()
    }

    #[test]
    fn matches_name_info_and_commands_ignoring_case() {
        let rg = def("ripgrep", "Fast line search", &["rg"]);
        assert!(matches(&rg, Some("rip")));
        assert!(matches(&rg, Some("SEARCH")));
        assert!(matches(&rg, Some("rg")));
        assert!(!matches(&rg, Some("json")));
        assert!(matches(&rg, None));
    }

    #[test]
    fn renders_aligned_with_installed_marker() {
        let jq = def("jq", "Command-line JSON processor", &["jq"]);
        let hf = def(
            "hyperfine",
            "Command-line benchmarking tool",
            &["hyperfine"],
        );
        let mut lock = Lock::default();
        lock.pkgs.insert(
            "jq".into(),
            Entry {
                version: "1.8.2".into(),
                pinned: false,
                file: String::new(),
                bins: vec![],
            },
        );
        assert_eq!(
            render(&[&hf, &jq], &lock),
            "hyperfine  Command-line benchmarking tool\n\
             jq         Command-line JSON processor     installed 1.8.2\n"
        );
    }

    #[test]
    fn loads_every_definition_sorted_and_skips_broken_ones() {
        let dir = tempfile::tempdir().unwrap();
        for (name, info) in [("just", "Command runner"), ("jq", "JSON processor")] {
            let text = format!(
                "name = \"{name}\"\ninfo = \"{info}\"\nfrom = \"github\"\nrepo = \"a/b\"\nbins = [\"{name}\"]\n\n[plat]\nmacos-x86_64 = {{ file = \"f\", path = \"p\" }}\n"
            );
            std::fs::write(dir.path().join(format!("{name}.toml")), text).unwrap();
        }
        std::fs::write(dir.path().join("broken.toml"), "nope").unwrap();
        std::fs::write(dir.path().join("README.md"), "x").unwrap();

        let names: Vec<_> = load_all(dir.path())
            .unwrap()
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, ["jq", "just"]);
    }

    /// Two programs with the same command would fight over one link in `bins/`.
    #[test]
    fn registry_commands_are_unique() {
        let defs = Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let mut seen = std::collections::BTreeMap::new();
        for def in load_all(&defs).unwrap() {
            for bin in &def.bins {
                if let Some(other) = seen.insert(bin.clone(), def.name.clone()) {
                    panic!("`{bin}` is provided by both {other} and {}", def.name);
                }
            }
        }
    }

    /// `load_all` skips broken files with a warning, so check every file loads.
    #[test]
    fn every_registry_file_loads() {
        let defs = Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let files = std::fs::read_dir(&defs).unwrap().count();
        assert_eq!(load_all(&defs).unwrap().len(), files);
    }

    #[test]
    fn the_registry_definitions_all_have_a_description() {
        let defs = Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        for def in load_all(&defs).unwrap() {
            assert!(!def.info.is_empty(), "{} has no `info`", def.name);
        }
    }
}
