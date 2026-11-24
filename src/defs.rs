//! Program definitions from the registry (`defs/<name>.toml`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Def {
    pub name: String,
    pub from: String,
    pub repo: String,
    #[serde(default = "default_tags")]
    pub tags: String,
    pub bins: Vec<String>,
    #[allow(dead_code, reason = "checksums arrive in OLE-7")]
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
        let file = defs.join(format!("{name}.toml"));
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                bail!("`{name}` is not in the registry")
            }
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", file.display())),
        };
        let def =
            Self::parse(&text).with_context(|| format!("bad definition {}", file.display()))?;
        if def.name != name {
            bail!("{} says its name is `{}`", file.display(), def.name);
        }
        Ok(def)
    }

    /// The entry for the platform boy is running on.
    pub fn current_plat(&self) -> Result<&Plat> {
        let key = platform_key();
        self.plat
            .get(key)
            .with_context(|| format!("`{}` is not available for this system ({key})", self.name))
    }

    pub fn tag_pattern(&self) -> TagPattern {
        TagPattern::new(&self.tags).expect("validated in parse")
    }
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

    #[allow(dead_code, reason = "pinned installs arrive in OLE-12")]
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

    #[test]
    fn expands_version() {
        assert_eq!(
            expand("hyperfine-v{version}-x86_64-apple-darwin.tar.gz", "2.0.0"),
            "hyperfine-v2.0.0-x86_64-apple-darwin.tar.gz"
        );
    }
}
