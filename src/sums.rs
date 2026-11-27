//! Checks against a release's checksum file. SHA-256: `hash  filename` lines, BSD
//! `SHA256 (filename) = hash` lines, or a bare hash in a file made for one download.
//! SHA-1 only from a `.sha1` file made for one download, as Maven publishes.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use sha1::Sha1;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Algo {
    Sha256,
    Sha1,
}

impl Algo {
    /// The algorithm of the checksum file `name`. `per_file` says it belongs to one
    /// download; only such a file can be SHA-1.
    pub fn of(name: &str, per_file: bool) -> Self {
        if per_file && name.ends_with(".sha1") {
            Algo::Sha1
        } else {
            Algo::Sha256
        }
    }
}

/// Check that `path` (downloaded as `file`) matches its hash in `sums`. `per_file`
/// says the checksum file belongs to `file` alone, so it may hold only the hash.
pub fn verify(path: &Path, sums: &str, file: &str, per_file: bool, algo: Algo) -> Result<()> {
    let expected = match algo {
        Algo::Sha256 => {
            find(sums, file).or_else(|| per_file.then(|| lone_hash(sums, 64)).flatten())
        }
        Algo::Sha1 => lone_hash(sums, 40),
    }
    .with_context(|| format!("`{file}` is not in the checksum file"))?;
    let actual = match algo {
        Algo::Sha256 => hash_file::<Sha256>(path)?,
        Algo::Sha1 => hash_file::<Sha1>(path)?,
    };
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("checksum mismatch for `{file}`: expected {expected}, got {actual}");
    }
    Ok(())
}

/// The hash on the line naming `file`. Accepts `hash  name`, `hash *name`, `./name`
/// and BSD `SHA256 (name) = hash`.
fn find<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, name) = bsd_line(line).or_else(|| plain_line(line))?;
        (name.trim_start_matches("./") == file && is_sha256(hash)).then_some(hash)
    })
}

/// `hash  name` as `(hash, name)`.
fn plain_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.split_whitespace();
    let hash = parts.next()?;
    let name = parts.next()?;
    Some((hash, name.trim_start_matches('*')))
}

/// `SHA256 (name) = hash` as `(hash, name)`. Lines for other algorithms are skipped.
fn bsd_line(line: &str) -> Option<(&str, &str)> {
    let (name, hash) = line.strip_prefix("SHA256 (")?.rsplit_once(") = ")?;
    Some((hash.trim(), name))
}

/// The hash of `len` hex digits when it is the only thing in the file.
fn lone_hash(sums: &str, len: usize) -> Option<&str> {
    let hash = sums.trim();
    is_hex(hash, len).then_some(hash)
}

fn is_sha256(s: &str) -> bool {
    is_hex(s, 64)
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn hash_file<D: Digest>(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hasher = D::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // sha256("hello\n")
    const HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

    fn file_with(content: &str) -> tempfile::NamedTempFile {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), content).unwrap();
        f
    }

    #[test]
    fn accepts_a_matching_file() {
        let f = file_with("hello\n");
        let sums = format!("{}  other\n{HELLO}  jq-macos-amd64\n", "0".repeat(64));
        verify(f.path(), &sums, "jq-macos-amd64", false, Algo::Sha256).unwrap();
    }

    #[test]
    fn refuses_a_corrupted_file() {
        let f = file_with("hello, tampered\n");
        let sums = format!("{HELLO}  jq-macos-amd64\n");
        let err = verify(f.path(), &sums, "jq-macos-amd64", false, Algo::Sha256).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn refuses_a_file_missing_from_the_list() {
        let f = file_with("hello\n");
        let sums = format!("{HELLO}  jq-macos-arm64\n");
        assert!(verify(f.path(), &sums, "jq-macos-amd64", false, Algo::Sha256).is_err());
    }

    #[test]
    fn understands_common_line_styles() {
        assert_eq!(find(&format!("{HELLO} *a.tar.gz"), "a.tar.gz"), Some(HELLO));
        assert_eq!(
            find(&format!("{HELLO}  ./a.tar.gz"), "a.tar.gz"),
            Some(HELLO)
        );
        assert!(find(&format!("{}  a.tar.gz", HELLO.to_uppercase()), "a.tar.gz").is_some());
        assert_eq!(find("not-a-hash  a.tar.gz", "a.tar.gz"), None);
    }

    #[test]
    fn understands_bsd_lines() {
        let other = "0".repeat(64);
        let sums = format!(
            "SHA1 (yq) = {}\nSHA256 (yq.tar.gz) = {other}\nSHA256 (yq) = {HELLO}\n\
             SHA3-256 (yq) = {other}\n",
            "0".repeat(40)
        );
        assert_eq!(find(&sums, "yq"), Some(HELLO));
        assert_eq!(find(&format!("SHA256 (./yq) = {HELLO}"), "yq"), Some(HELLO));
        assert_eq!(find(&format!("SHA3-256 (yq) = {HELLO}"), "yq"), None);
        assert_eq!(find("SHA256 (yq) = not-a-hash", "yq"), None);
    }

    #[test]
    fn accepts_a_bare_hash_only_from_a_per_file_checksum() {
        let f = file_with("hello\n");
        for sums in [HELLO.to_string(), format!("{HELLO}\r\n")] {
            verify(f.path(), &sums, "starship.tar.gz", true, Algo::Sha256).unwrap();
            assert!(verify(f.path(), &sums, "starship.tar.gz", false, Algo::Sha256).is_err());
        }
        let two = format!("{HELLO}\n{HELLO}\n");
        assert!(verify(f.path(), &two, "starship.tar.gz", true, Algo::Sha256).is_err());
    }

    #[test]
    fn refuses_a_corrupted_file_against_a_bare_hash() {
        let f = file_with("hello, tampered\n");
        let err = verify(f.path(), HELLO, "starship.tar.gz", true, Algo::Sha256).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn a_per_file_checksum_naming_another_file_is_refused() {
        let f = file_with("hello\n");
        let sums = format!("{HELLO}  other.tar.gz\n");
        assert!(verify(f.path(), &sums, "rg.tar.gz", true, Algo::Sha256).is_err());
    }

    // sha1("hello\n")
    const HELLO_SHA1: &str = "f572d396fae9206628714fb2ce00f72e94f2258f";

    #[test]
    fn checks_a_per_file_sha1() {
        let f = file_with("hello\n");
        verify(f.path(), HELLO_SHA1, "mill.sh", true, Algo::Sha1).unwrap();
        let bad = file_with("tampered\n");
        let err = verify(bad.path(), HELLO_SHA1, "mill.sh", true, Algo::Sha1).unwrap_err();
        assert!(err.to_string().contains("mismatch"), "{err}");
        assert!(verify(f.path(), HELLO, "mill.sh", true, Algo::Sha1).is_err());
    }

    #[test]
    fn sha1_comes_only_from_a_per_file_sha1_name() {
        assert_eq!(Algo::of("tool.sh.sha1", true), Algo::Sha1);
        assert_eq!(Algo::of("checksums.sha1", false), Algo::Sha256);
        assert_eq!(Algo::of("tool.sh.sha256", true), Algo::Sha256);
        let f = file_with("hello\n");
        let sums = format!("{HELLO_SHA1}  mill.sh\n");
        assert!(verify(f.path(), &sums, "mill.sh", false, Algo::Sha256).is_err());
    }
}
