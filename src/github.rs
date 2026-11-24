//! The `github` source: public release links only, no API, no tokens.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ureq::Agent;

const USER_AGENT: &str = concat!("boy/", env!("CARGO_PKG_VERSION"));

fn agent(max_redirects: u32) -> Agent {
    Agent::config_builder()
        .user_agent(USER_AGENT)
        .max_redirects(max_redirects)
        .timeout_connect(Some(Duration::from_secs(15)))
        .build()
        .into()
}

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

fn tag_from_location(location: &str) -> Option<String> {
    let (_, tag) = location.split_once("/releases/tag/")?;
    let tag = tag.split(['?', '#']).next()?.trim_end_matches('/');
    (!tag.is_empty()).then(|| tag.to_string())
}

pub fn download_url(repo: &str, tag: &str, file: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{file}")
}

/// Download `url` into `dest`, streaming to disk.
pub fn download(url: &str, dest: &Path) -> Result<()> {
    let mut resp = match agent(10).get(url).call() {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(404)) => bail!("not found: {url}"),
        Err(e) => return Err(e).with_context(|| format!("cannot download {url}")),
    };
    let mut file =
        std::fs::File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
    std::io::copy(&mut resp.body_mut().as_reader(), &mut file)
        .with_context(|| format!("download interrupted: {url}"))?;
    file.flush()?;
    Ok(())
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
    fn latest_jq_tag() {
        assert!(latest_tag("jqlang/jq").unwrap().starts_with("jq-"));
    }
}
