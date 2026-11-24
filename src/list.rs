//! `boy l`.

use anyhow::Result;

use crate::home::Home;
use crate::lock::Lock;

pub fn list(home: &Home) -> Result<()> {
    let lock = Lock::load(&home.lock())?;
    print!("{}", render(&lock));
    Ok(())
}

/// One line per package, names and versions in aligned columns:
///
/// ```text
/// hyperfine  2.0.0
/// jq         1.7.1   pinned
/// just       1.59.0
/// ```
fn render(lock: &Lock) -> String {
    if lock.pkgs.is_empty() {
        return "nothing installed\n".into();
    }
    let name_w = lock.pkgs.keys().map(String::len).max().unwrap_or(0);
    let ver_w = lock
        .pkgs
        .values()
        .map(|e| e.version.len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (name, entry) in &lock.pkgs {
        let line = if entry.pinned {
            format!("{name:name_w$}  {:ver_w$}  pinned", entry.version)
        } else {
            format!("{name:name_w$}  {}", entry.version)
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

    fn entry(version: &str, pinned: bool) -> Entry {
        Entry {
            version: version.into(),
            pinned,
            file: String::new(),
            bins: vec![],
        }
    }

    #[test]
    fn empty() {
        assert_eq!(render(&Lock::default()), "nothing installed\n");
    }

    #[test]
    fn sorted_aligned_with_pinned_marker() {
        let mut lock = Lock::default();
        lock.pkgs.insert("just".into(), entry("1.59.0", false));
        lock.pkgs.insert("jq".into(), entry("1.7.1", true));
        lock.pkgs.insert("hyperfine".into(), entry("2.0.0", false));
        assert_eq!(
            render(&lock),
            "hyperfine  2.0.0\n\
             jq         1.7.1   pinned\n\
             just       1.59.0\n"
        );
    }
}
