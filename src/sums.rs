//! SHA-256 checks against a release's checksum file (`hash  filename` lines).

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// Check that `path` (downloaded as `file`) matches its line in `sums`.
pub fn verify(path: &Path, sums: &str, file: &str) -> Result<()> {
    let expected =
        find(sums, file).with_context(|| format!("`{file}` is not in the checksum file"))?;
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("checksum mismatch for `{file}`: expected {expected}, got {actual}");
    }
    Ok(())
}

/// The hash for `file`. Accepts `hash  name`, `hash *name` and `./name`.
fn find<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        let name = name.trim_start_matches('*').trim_start_matches("./");
        (name == file && is_sha256(hash)).then_some(hash)
    })
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
        verify(f.path(), &sums, "jq-macos-amd64").unwrap();
    }

    #[test]
    fn refuses_a_corrupted_file() {
        let f = file_with("hello, tampered\n");
        let sums = format!("{HELLO}  jq-macos-amd64\n");
        let err = verify(f.path(), &sums, "jq-macos-amd64").unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn refuses_a_file_missing_from_the_list() {
        let f = file_with("hello\n");
        let sums = format!("{HELLO}  jq-macos-arm64\n");
        assert!(verify(f.path(), &sums, "jq-macos-amd64").is_err());
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
}
