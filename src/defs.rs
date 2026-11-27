//! Program definitions from the registry (`defs/<name>.toml`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Def {
    pub name: String,
    /// One-line description, shown and searched by `boy s`.
    #[serde(default)]
    pub info: String,
    pub from: String,
    pub repo: String,
    #[serde(default = "default_tags")]
    pub tags: String,
    pub bins: Vec<String>,
    pub sums: Option<String>,
    pub plat: BTreeMap<String, Plat>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plat {
    pub file: String,
    pub path: String,
}

fn default_tags() -> String {
    "v{version}".into()
}

impl Def {
    pub fn parse(text: &str) -> Result<Self> {
        let def: Def = toml::from_str(text)?;
        if def.bins.is_empty() {
            bail!("`bins` is empty");
        }
        TagPattern::new(&def.tags)?;
        Ok(def)
    }

    /// Load `<defs>/<name>.toml`.
    pub fn load(defs: &Path, name: &str) -> Result<Self> {
        Self::find(defs, name)?.with_context(|| format!("`{name}` is not in the registry"))
    }

    /// Like `load`, but `None` when the registry has no such program.
    pub fn find(defs: &Path, name: &str) -> Result<Option<Self>> {
        check_name(name)?;
        let file = defs.join(format!("{name}.toml"));
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", file.display())),
        };
        let def =
            Self::parse(&text).with_context(|| format!("bad definition {}", file.display()))?;
        if def.name != name {
            bail!("{} says its name is `{}`", file.display(), def.name);
        }
        Ok(Some(def))
    }

    /// The entry for the platform boy is running on.
    pub fn current_plat(&self) -> Result<&Plat> {
        let key = platform_key();
        self.plat.get(key).with_context(|| {
            let have: Vec<&str> = self.plat.keys().map(String::as_str).collect();
            format!(
                "`{}` is not available for this system ({key}); it has releases for {}",
                self.name,
                have.join(", ")
            )
        })
    }

    pub fn tag_pattern(&self) -> TagPattern {
        TagPattern::new(&self.tags).expect("validated in parse")
    }
}

/// A version from the command line becomes part of a URL and a folder name.
pub fn check_version(version: &str) -> Result<()> {
    let plain = !version.is_empty()
        && !version.starts_with('.')
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b));
    if !plain {
        bail!("`{version}` is not a valid version");
    }
    Ok(())
}

/// A package name becomes a file and folder name, so it must not contain a path.
pub fn check_name(name: &str) -> Result<()> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    if !plain {
        bail!("`{name}` is not a valid package name");
    }
    Ok(())
}

/// `macos-x86_64`, `macos-aarch64`, ...
pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => "macos-x86_64",
        ("macos", "aarch64") => "macos-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "unsupported",
    }
}

/// Replace `{version}` in a definition string.
pub fn expand(template: &str, version: &str) -> String {
    template.replace("{version}", version)
}

/// A tag pattern such as `v{version}` or `jq-{version}`.
#[derive(Debug, Clone, PartialEq)]
pub struct TagPattern {
    prefix: String,
    suffix: String,
}

impl TagPattern {
    pub fn new(pattern: &str) -> Result<Self> {
        match pattern.split_once("{version}") {
            Some((prefix, suffix)) if !suffix.contains("{version}") => Ok(Self {
                prefix: prefix.into(),
                suffix: suffix.into(),
            }),
            _ => bail!("`tags` must contain `{{version}}` exactly once: `{pattern}`"),
        }
    }

    pub fn tag(&self, version: &str) -> String {
        format!("{}{version}{}", self.prefix, self.suffix)
    }

    pub fn version<'a>(&self, tag: &'a str) -> Option<&'a str> {
        tag.strip_prefix(self.prefix.as_str())?
            .strip_suffix(self.suffix.as_str())
            .filter(|v| !v.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JQ: &str = r#"
        name = "jq"
        from = "github"
        repo = "jqlang/jq"
        tags = "jq-{version}"
        bins = ["jq"]

        [plat]
        macos-x86_64  = { file = "jq-macos-amd64", path = "jq" }
        macos-aarch64 = { file = "jq-macos-arm64", path = "jq" }
    "#;

    #[test]
    fn find_is_none_for_a_missing_definition_and_load_says_so() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Def::find(dir.path(), "jq").unwrap().is_none());
        let err = Def::load(dir.path(), "jq").unwrap_err();
        assert!(err.to_string().contains("not in the registry"));

        std::fs::write(dir.path().join("jq.toml"), JQ).unwrap();
        assert_eq!(
            Def::find(dir.path(), "jq").unwrap().unwrap().repo,
            "jqlang/jq"
        );
    }

    #[test]
    fn checks_versions() {
        for ok in ["1.7.1", "2.0.0-rc.1", "1.0+build"] {
            assert!(check_version(ok).is_ok(), "{ok}");
        }
        for bad in ["", "../1", "1/2", ".1", "1 2"] {
            assert!(check_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_names_with_paths() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../x", "a/b", ".hidden", ""] {
            let err = Def::load(dir.path(), bad).unwrap_err();
            assert!(
                err.to_string().contains("not a valid package name"),
                "{bad}"
            );
        }
    }

    #[test]
    fn parses_a_definition() {
        let def = Def::parse(JQ).unwrap();
        assert_eq!(def.repo, "jqlang/jq");
        assert_eq!(def.plat["macos-x86_64"].file, "jq-macos-amd64");
        assert!(def.sums.is_none());
    }

    #[test]
    fn tags_default_to_v_prefix() {
        let def = Def::parse(&JQ.replace("tags = \"jq-{version}\"", "")).unwrap();
        assert_eq!(def.tag_pattern().tag("1.2.3"), "v1.2.3");
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(Def::parse(&format!("{JQ}\nnope = 1")).is_err());
    }

    #[test]
    fn tag_pattern_round_trip() {
        let p = TagPattern::new("jq-{version}").unwrap();
        assert_eq!(p.tag("1.8.2"), "jq-1.8.2");
        assert_eq!(p.version("jq-1.8.2"), Some("1.8.2"));
        assert_eq!(p.version("v1.8.2"), None);
        assert_eq!(p.version("jq-"), None);
    }

    #[test]
    fn tag_pattern_without_prefix() {
        let p = TagPattern::new("{version}").unwrap();
        assert_eq!(p.version("1.59.0"), Some("1.59.0"));
    }

    #[test]
    fn tag_pattern_needs_exactly_one_placeholder() {
        assert!(TagPattern::new("v1").is_err());
        assert!(TagPattern::new("{version}-{version}").is_err());
    }

    /// fzf: `v` only in the tag, its own platform names, `{version}` in the sums file name.
    #[test]
    fn fzf_definition_expands_tag_files_and_sums() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let def = Def::load(&dir, "fzf").unwrap();
        assert_eq!(def.tag_pattern().tag("0.75.0"), "v0.75.0");
        assert_eq!(def.tag_pattern().version("v0.75.0"), Some("0.75.0"));
        assert_eq!(
            expand(&def.plat["macos-x86_64"].file, "0.75.0"),
            "fzf-0.75.0-darwin_amd64.tar.gz"
        );
        assert_eq!(
            expand(&def.plat["macos-aarch64"].file, "0.75.0"),
            "fzf-0.75.0-darwin_arm64.tar.gz"
        );
        assert_eq!(
            expand(def.sums.as_deref().unwrap(), "0.75.0"),
            "fzf_0.75.0_checksums.txt"
        );
    }

    /// helix: no `v` in the tag, `.tar.xz`, the binary inside a versioned folder.
    #[test]
    fn helix_definition_expands_tag_and_paths() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let def = Def::load(&dir, "helix").unwrap();
        assert_eq!(def.tag_pattern().tag("25.07.1"), "25.07.1");
        let intel = &def.plat["macos-x86_64"];
        assert_eq!(
            expand(&intel.file, "25.07.1"),
            "helix-25.07.1-x86_64-macos.tar.xz"
        );
        assert_eq!(
            expand(&intel.path, "25.07.1"),
            "helix-25.07.1-x86_64-macos/hx"
        );
        let arm = &def.plat["macos-aarch64"];
        assert_eq!(
            expand(&arm.file, "25.07.1"),
            "helix-25.07.1-aarch64-macos.tar.xz"
        );
        assert_eq!(
            expand(&arm.path, "25.07.1"),
            "helix-25.07.1-aarch64-macos/hx"
        );
        assert!(def.sums.is_none(), "helix publishes no checksum file");
    }

    /// ripgrep-all: several commands from one release folder.
    #[test]
    fn ripgrep_all_definition_lists_every_command() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let def = Def::load(&dir, "ripgrep-all").unwrap();
        assert_eq!(def.bins, ["rga", "rga-preproc", "rga-fzf", "rga-fzf-open"]);
        assert_eq!(def.tag_pattern().tag("0.10.10"), "v0.10.10");
        for (key, arch) in [("macos-x86_64", "x86_64"), ("macos-aarch64", "aarch64")] {
            let plat = &def.plat[key];
            assert_eq!(
                expand(&plat.file, "0.10.10"),
                format!("ripgrep_all-v0.10.10-{arch}-apple-darwin.tar.gz")
            );
            assert_eq!(
                expand(&plat.path, "0.10.10"),
                format!("ripgrep_all-v0.10.10-{arch}-apple-darwin/rga")
            );
        }
    }

    /// The tag and the Intel and arm64 `file` and `path` of definitions added together,
    /// checked against the names in their releases.
    #[test]
    fn registry_definitions_expand_to_the_release_names() {
        #[rustfmt::skip]
        let cases = [
            ("ripgrep", "16.0.0", "16.0.0",
             ("ripgrep-16.0.0-x86_64-apple-darwin.tar.gz", "ripgrep-16.0.0-x86_64-apple-darwin/rg"),
             ("ripgrep-16.0.0-aarch64-apple-darwin.tar.gz", "ripgrep-16.0.0-aarch64-apple-darwin/rg")),
            ("fd", "10.5.2", "v10.5.2",
             ("fd-v10.5.2-x86_64-apple-darwin.tar.gz", "fd-v10.5.2-x86_64-apple-darwin/fd"),
             ("fd-v10.5.2-aarch64-apple-darwin.tar.gz", "fd-v10.5.2-aarch64-apple-darwin/fd")),
            ("bat", "0.26.1", "v0.26.1",
             ("bat-v0.26.1-x86_64-apple-darwin.tar.gz", "bat-v0.26.1-x86_64-apple-darwin/bat"),
             ("bat-v0.26.1-aarch64-apple-darwin.tar.gz", "bat-v0.26.1-aarch64-apple-darwin/bat")),
            ("zoxide", "0.10.0", "v0.10.0",
             ("zoxide-0.10.0-x86_64-apple-darwin.tar.gz", "zoxide"),
             ("zoxide-0.10.0-aarch64-apple-darwin.tar.gz", "zoxide")),
            ("starship", "1.27.0", "v1.27.0",
             ("starship-x86_64-apple-darwin.tar.gz", "starship"),
             ("starship-aarch64-apple-darwin.tar.gz", "starship")),
            ("lazygit", "0.66.3", "v0.66.3",
             ("lazygit_0.66.3_darwin_x86_64.tar.gz", "lazygit"),
             ("lazygit_0.66.3_darwin_arm64.tar.gz", "lazygit")),
            ("yq", "4.54.3", "v4.54.3",
             ("yq_darwin_amd64", "yq"),
             ("yq_darwin_arm64", "yq")),
            ("gh", "2.106.0", "v2.106.0",
             ("gh_2.106.0_macOS_amd64.zip", "gh_2.106.0_macOS_amd64/bin/gh"),
             ("gh_2.106.0_macOS_arm64.zip", "gh_2.106.0_macOS_arm64/bin/gh")),
            ("neovim", "0.12.6", "v0.12.6",
             ("nvim-macos-x86_64.tar.gz", "nvim-macos-x86_64/bin/nvim"),
             ("nvim-macos-arm64.tar.gz", "nvim-macos-arm64/bin/nvim")),
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        for (name, version, tag, intel, arm) in cases {
            let def = Def::load(&dir, name).unwrap();
            assert_eq!(def.tag_pattern().tag(version), tag, "{name}");
            for (key, (file, path)) in [("macos-x86_64", intel), ("macos-aarch64", arm)] {
                let plat = &def.plat[key];
                assert_eq!(expand(&plat.file, version), file, "{name} {key}");
                assert_eq!(expand(&plat.path, version), path, "{name} {key}");
            }
        }
        for (name, sums) in [
            ("ripgrep", "{file}.sha256"),
            ("starship", "{file}.sha256"),
            ("yq", "checksums-bsd"),
            ("lazygit", "checksums.txt"),
            ("gh", "gh_2.106.0_checksums.txt"),
        ] {
            let def = Def::load(&dir, name).unwrap();
            assert_eq!(expand(def.sums.as_deref().unwrap(), "2.106.0"), sums);
        }
    }

    /// uv, bun and coursier: their tags, release names and checksum files.
    #[test]
    fn uv_bun_and_coursier_definitions_expand_to_the_release_names() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        for (name, version, tag, sums, intel, arm) in [
            (
                "uv",
                "0.13.10",
                "0.13.10",
                Some("{file}.sha256"),
                "uv-x86_64-apple-darwin.tar.gz",
                "uv-aarch64-apple-darwin.tar.gz",
            ),
            (
                "bun",
                "1.4.5",
                "bun-v1.4.5",
                Some("SHASUMS256.txt"),
                "bun-darwin-x64.zip",
                "bun-darwin-aarch64.zip",
            ),
            (
                "coursier",
                "2.1.27",
                "v2.1.27",
                None,
                "cs-x86_64-apple-darwin-sdk.zip",
                "cs-aarch64-apple-darwin-sdk.zip",
            ),
        ] {
            let def = Def::load(&dir, name).unwrap();
            assert_eq!(def.tag_pattern().tag(version), tag, "{name}");
            assert_eq!(def.sums.as_deref(), sums, "{name}");
            assert_eq!(def.plat["macos-x86_64"].file, intel);
            assert_eq!(def.plat["macos-aarch64"].file, arm);
        }
    }

    /// sbt and the Scala tools: their tags, release names, paths and checksum files.
    #[test]
    fn scala_tool_definitions_expand_to_the_release_names() {
        #[rustfmt::skip]
        let cases = [
            ("sbt", "2.1.1", "v2.1.1", Some("{file}.sha256"),
             ("sbt-2.1.1.tgz", "sbt/bin/sbt"),
             ("sbt-2.1.1.tgz", "sbt/bin/sbt")),
            ("scala", "3.10.1", "3.10.1", Some("{file}.sha256"),
             ("scala3-3.10.1-x86_64-apple-darwin.tar.gz", "scala3-3.10.1-x86_64-apple-darwin/bin/scala"),
             ("scala3-3.10.1-aarch64-apple-darwin.tar.gz", "scala3-3.10.1-aarch64-apple-darwin/bin/scala")),
            ("scala-cli", "1.19.0", "v1.19.0", None,
             ("scala-cli-x86_64-apple-darwin-sdk.zip", "scala-cli-x86_64-apple-darwin-sdk/bin/scala-cli"),
             ("scala-cli-aarch64-apple-darwin-sdk.zip", "scala-cli-aarch64-apple-darwin-sdk/bin/scala-cli")),
            ("scalafmt", "3.11.5", "v3.11.5", None,
             ("scalafmt-x86_64-apple-darwin.zip", "scalafmt"),
             ("scalafmt-aarch64-apple-darwin.zip", "scalafmt")),
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        for (name, version, tag, sums, intel, arm) in cases {
            let def = Def::load(&dir, name).unwrap();
            assert_eq!(def.tag_pattern().tag(version), tag, "{name}");
            assert_eq!(def.sums.as_deref(), sums, "{name}");
            for (key, (file, path)) in [("macos-x86_64", intel), ("macos-aarch64", arm)] {
                let plat = &def.plat[key];
                assert_eq!(expand(&plat.file, version), file, "{name} {key}");
                assert_eq!(expand(&plat.path, version), path, "{name} {key}");
            }
        }
    }

    /// Ammonite names its release files by Scala version, then its own version.
    #[test]
    fn amm_definition_picks_the_scala_3_8_build() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let def = Def::load(&dir, "amm").unwrap();
        assert_eq!(def.tag_pattern().tag("3.0.9"), "3.0.9");
        assert!(def.sums.is_none(), "Ammonite publishes no checksum file");
        for key in ["macos-x86_64", "macos-aarch64"] {
            let plat = &def.plat[key];
            assert_eq!(expand(&plat.file, "3.0.9"), "3.8-3.0.9", "{key}");
            assert_eq!(plat.path, "amm", "{key}");
        }
    }

    /// sbtn ships one universal macOS archive.
    #[test]
    fn sbtn_definition_uses_the_universal_archive() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let def = Def::load(&dir, "sbtn").unwrap();
        assert_eq!(def.tag_pattern().tag("2.1.1"), "v2.1.1");
        assert!(def.sums.is_none(), "sbtn publishes no checksum file");
        for key in ["macos-x86_64", "macos-aarch64"] {
            let plat = &def.plat[key];
            assert_eq!(
                expand(&plat.file, "2.1.1"),
                "sbtn-universal-apple-darwin-2.1.1.tar.gz",
                "{key}"
            );
            assert_eq!(plat.path, "sbtn", "{key}");
        }
    }

    /// Oracle JDK: a Java major as `repo`, the version in the file and folder names.
    #[test]
    fn jdk_definition_expands_to_the_oracle_names() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("defs");
        let def = Def::load(&dir, "jdk").unwrap();
        assert_eq!(
            crate::source::Source::of(&def).unwrap(),
            crate::source::Source::Oracle
        );
        assert_eq!(def.tag_pattern().tag("27.0.1.1"), "27.0.1.1");
        assert_eq!(def.sums.as_deref(), Some("{file}.sha256"));
        for (key, arch) in [("macos-x86_64", "x64"), ("macos-aarch64", "aarch64")] {
            let plat = &def.plat[key];
            assert_eq!(
                expand(&plat.file, "27.0.1.1"),
                format!("jdk-27.0.1.1_macos-{arch}_bin.tar.gz")
            );
            assert_eq!(
                expand(&plat.path, "27.0.1.1"),
                "jdk-27.0.1.1.jdk/Contents/Home/bin/java"
            );
        }
    }

    #[test]
    fn expands_version() {
        assert_eq!(
            expand("hyperfine-v{version}-x86_64-apple-darwin.tar.gz", "2.0.0"),
            "hyperfine-v2.0.0-x86_64-apple-darwin.tar.gz"
        );
    }
}
