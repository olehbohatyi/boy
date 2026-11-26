//! Where a program comes from (the `from` key). Each source finds the latest
//! release, checks that a pinned one exists and builds download links.

use anyhow::{Context, Result, bail};

use crate::defs::Def;
use crate::{github, oracle};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Source {
    Github,
    Oracle,
}

impl Source {
    /// The source a definition names, or an error when this boy does not know it.
    pub fn of(def: &Def) -> Result<Self> {
        match def.from.as_str() {
            "github" => Ok(Self::Github),
            "oracle" => {
                oracle::check_major(&def.repo)?;
                Ok(Self::Oracle)
            }
            other => bail!(
                "`{}` uses source `{other}`, which this boy does not support",
                def.name
            ),
        }
    }

    /// The latest release as `(tag, version)`.
    pub fn latest(self, def: &Def) -> Result<(String, String)> {
        let tag = match self {
            Self::Github => github::latest_tag(&def.repo)?,
            Self::Oracle => {
                let version = oracle::latest_version(&def.repo)?;
                def.tag_pattern().tag(&version)
            }
        };
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
        Ok((tag, version))
    }

    /// The `(tag, version)` for a version asked for by name, if it was released.
    /// `file` is the release file name for this platform.
    pub fn pinned(self, def: &Def, version: &str, file: &str) -> Result<(String, String)> {
        let tag = def.tag_pattern().tag(version);
        let (found, place) = match self {
            Self::Github => (
                github::tag_exists(&def.repo, &tag)?,
                format!("no tag `{tag}` in {}", def.repo),
            ),
            Self::Oracle => {
                let url = self.url(def, &tag, file);
                (oracle::exists(&url)?, format!("not found: {url}"))
            }
        };
        if !found {
            bail!("`{}` has no release {version} ({place})", def.name);
        }
        Ok((tag, version.to_string()))
    }

    /// The download link for one file of a release.
    pub fn url(self, def: &Def, tag: &str, file: &str) -> String {
        match self {
            Self::Github => github::download_url(&def.repo, tag, file),
            Self::Oracle => oracle::download_url(&def.repo, file),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(from: &str, repo: &str) -> Def {
        Def::parse(&format!(
            "name = \"x\"\nfrom = \"{from}\"\nrepo = \"{repo}\"\ntags = \"{{version}}\"\n\
             bins = [\"x\"]\n[plat]\n"
        ))
        .unwrap()
    }

    #[test]
    fn knows_its_sources() {
        assert_eq!(Source::of(&def("github", "a/b")).unwrap(), Source::Github);
        assert_eq!(Source::of(&def("oracle", "27")).unwrap(), Source::Oracle);
        let err = Source::of(&def("ftp", "a")).unwrap_err().to_string();
        assert!(err.contains("does not support"), "{err}");
    }

    #[test]
    fn an_oracle_repo_must_be_a_java_major_version() {
        for bad in ["", "oracle/jdk", "27.0.1", "../27"] {
            assert!(Source::of(&def("oracle", bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn builds_links_for_each_source() {
        assert_eq!(
            Source::Github.url(&def("github", "a/b"), "1.0", "x.zip"),
            "https://github.com/a/b/releases/download/1.0/x.zip"
        );
        assert_eq!(
            Source::Oracle.url(&def("oracle", "27"), "27.0.1", "x.tar.gz"),
            "https://download.oracle.com/java/27/archive/x.tar.gz"
        );
    }
}
