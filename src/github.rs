//! The `github` source: public release links only, no API, no tokens.

use anyhow::{Context, Result};

use crate::net::agent;

/// The latest release tag, read from the `releases/latest` redirect.
pub fn latest_tag(repo: &str) -> Result<String> {
    let url = format!("https://github.com/{repo}/releases/latest");
    let resp = agent(0)
        .get(&url)
        .call()
        .with_context(|| format!("cannot reach {url}"))?;
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .with_context(|| format!("{url} did not redirect (status {})", resp.status()))?;
    tag_from_location(location)
        .with_context(|| format!("{repo} has no releases (redirected to {location})"))
}

/// Whether the repository has a release with this tag (its public release page exists).
pub fn tag_exists(repo: &str, tag: &str) -> Result<bool> {
    let url = format!("https://github.com/{repo}/releases/tag/{tag}");
    match agent(0).head(&url).call() {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(ureq::Error::StatusCode(404)) => Ok(false),
        Err(e) => Err(e).with_context(|| format!("cannot reach {url}")),
    }
}

fn tag_from_location(location: &str) -> Option<String> {
    let (_, tag) = location.split_once("/releases/tag/")?;
    let tag = tag.split(['?', '#']).next()?.trim_end_matches('/');
    (!tag.is_empty()).then(|| tag.to_string())
}

pub fn download_url(repo: &str, tag: &str, file: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{file}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tag_from_redirect() {
        assert_eq!(
            tag_from_location("https://github.com/jqlang/jq/releases/tag/jq-1.8.2").as_deref(),
            Some("jq-1.8.2")
        );
    }

    #[test]
    fn no_tag_when_redirected_elsewhere() {
        assert_eq!(
            tag_from_location("https://github.com/jqlang/jq/releases"),
            None
        );
    }

    #[test]
    fn builds_download_url() {
        assert_eq!(
            download_url("jqlang/jq", "jq-1.8.2", "jq-macos-amd64"),
            "https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-macos-amd64"
        );
    }

    /// Hits the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn knows_which_tags_exist() {
        assert!(tag_exists("jqlang/jq", "jq-1.7.1").unwrap());
        assert!(!tag_exists("jqlang/jq", "jq-0.0.0").unwrap());
    }

    /// Hits the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn latest_jq_tag() {
        assert!(latest_tag("jqlang/jq").unwrap().starts_with("jq-"));
    }
}
