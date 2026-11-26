//! The `oracle` source: Oracle JDK. Versions come from Oracle's public list of Java
//! releases, files from the download archive. No account and no token.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{defs, net};

const VERSIONS: &str = "https://java.oraclecloud.com/javaVersions";

#[derive(Deserialize)]
struct Versions {
    items: Vec<Item>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    jdk_version: String,
    latest_release_version: String,
}

/// For this source `repo` is the Java major version, such as `27`.
pub fn check_major(repo: &str) -> Result<()> {
    if repo.is_empty() || !repo.bytes().all(|b| b.is_ascii_digit()) {
        bail!("`repo` must be a Java major version such as `27`, not `{repo}` (check `repo`)");
    }
    Ok(())
}

/// The latest release of a Java major version.
pub fn latest_version(major: &str) -> Result<String> {
    let text = net::text(VERSIONS)?;
    pick_version(&text, major)
}

fn pick_version(json: &str, major: &str) -> Result<String> {
    let versions: Versions =
        serde_json::from_str(json).with_context(|| format!("unexpected answer from {VERSIONS}"))?;
    let item = versions
        .items
        .into_iter()
        .find(|item| item.jdk_version == major)
        .with_context(|| format!("{VERSIONS} does not list Java {major} (check `repo`)"))?;
    let version = item.latest_release_version;
    // The version becomes part of a link and a folder name.
    defs::check_version(&version)?;
    if version != major && !version.starts_with(&format!("{major}.")) {
        bail!("{VERSIONS} says the latest Java {major} is `{version}`");
    }
    Ok(version)
}

pub fn download_url(major: &str, file: &str) -> String {
    format!("https://download.oracle.com/java/{major}/archive/{file}")
}

/// Whether the file is there to download.
pub fn exists(url: &str) -> Result<bool> {
    net::exists(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"items": [
        {"jdkVersion": "27", "latestReleaseVersion": "27.0.1.1", "isLts": false},
        {"jdkVersion": "26", "latestReleaseVersion": "26"},
        {"jdkVersion": "25", "latestReleaseVersion": "../25"},
        {"jdkVersion": "2", "latestReleaseVersion": "27.0.1"}
    ]}"#;

    #[test]
    fn picks_the_latest_release_of_the_major() {
        assert_eq!(pick_version(LIST, "27").unwrap(), "27.0.1.1");
        assert_eq!(pick_version(LIST, "26").unwrap(), "26");
    }

    #[test]
    fn refuses_a_missing_major_and_odd_versions() {
        let err = pick_version(LIST, "99").unwrap_err().to_string();
        assert!(err.contains("does not list Java 99"), "{err}");
        assert!(pick_version(LIST, "25").is_err(), "not a plain version");
        assert!(pick_version(LIST, "2").is_err(), "another major");
        assert!(pick_version("<html>", "27").is_err());
    }

    /// Hits the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn reads_versions_and_checks_files_on_oracle() {
        let version = latest_version("27").unwrap();
        assert!(version.starts_with("27"), "{version}");
        let file = format!("jdk-{version}_macos-x64_bin.tar.gz");
        assert!(exists(&download_url("27", &file)).unwrap());
        assert!(!exists(&download_url("27", "jdk-27.9.9_macos-x64_bin.tar.gz")).unwrap());
    }
}
