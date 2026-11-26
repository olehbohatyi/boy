//! SHA-256 checks against a release's checksum file: `hash  filename` lines, BSD
//! `SHA256 (filename) = hash` lines, or a bare hash in a file made for one download.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// Check that `path` (downloaded as `file`) matches its hash in `sums`. `per_file`
/// says the checksum file belongs to `file` alone, so it may hold only the hash.
pub fn verify(path: &Path, sums: &str, file: &str, per_file: bool) -> Result<()> {
    let expected = find(sums, file)
        .or_else(|| if per_file { lone_hash(sums) } else { None })
        .with_context(|| format!("`{file}` is not in the checksum file"))?;
    let actual = sha256_file(path)?;
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

/// The hash when it is the only thing in the file.
fn lone_hash(sums: &str) -> Option<&str> {
    let hash = sums.trim();
    is_sha256(hash).then_some(hash)
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hasher = Sha256::new();
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
        verify(f.path(), &sums, "jq-macos-amd64", false).unwrap();
    }

    #[test]
    fn refuses_a_corrupted_file() {
        let f = file_with("hello, tampered\n");
        let sums = format!("{HELLO}  jq-macos-amd64\n");
        let err = verify(f.path(), &sums, "jq-macos-amd64", false).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn refuses_a_file_missing_from_the_list() {
        let f = file_with("hello\n");
        let sums = format!("{HELLO}  jq-macos-arm64\n");
        assert!(verify(f.path(), &sums, "jq-macos-amd64", false).is_err());
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
            verify(f.path(), &sums, "starship.tar.gz", true).unwrap();
            assert!(verify(f.path(), &sums, "starship.tar.gz", false).is_err());
        }
        let two = format!("{HELLO}\n{HELLO}\n");
        assert!(verify(f.path(), &two, "starship.tar.gz", true).is_err());
    }

    #[test]
    fn refuses_a_corrupted_file_against_a_bare_hash() {
        let f = file_with("hello, tampered\n");
        let err = verify(f.path(), HELLO, "starship.tar.gz", true).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn a_per_file_checksum_naming_another_file_is_refused() {
        let f = file_with("hello\n");
        let sums = format!("{HELLO}  other.tar.gz\n");
        assert!(verify(f.path(), &sums, "rg.tar.gz", true).is_err());
    }
}
