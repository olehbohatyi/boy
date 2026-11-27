//! HTTP shared by the sources: a blocking client and downloads.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ureq::Agent;

const USER_AGENT: &str = concat!("boy/", env!("CARGO_PKG_VERSION"));

/// Every request, redirects included, must be `https`.
pub fn agent(max_redirects: u32) -> Agent {
    Agent::config_builder()
        .user_agent(USER_AGENT)
        .https_only(true)
        .max_redirects(max_redirects)
        .timeout_connect(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// The body of `url` as text.
pub fn text(url: &str) -> Result<String> {
    agent(10)
        .get(url)
        .call()
        .and_then(|mut resp| resp.body_mut().read_to_string())
        .with_context(|| format!("cannot read {url}"))
}

/// Whether `url` answers with success; a 404 means no.
pub fn exists(url: &str) -> Result<bool> {
    match agent(10).head(url).call() {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(ureq::Error::StatusCode(404)) => Ok(false),
        Err(e) => Err(e).with_context(|| format!("cannot reach {url}")),
    }
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

    /// ureq applies this to every hop, so a redirect cannot leave `https` either.
    /// The request is refused before any connection.
    #[test]
    fn refuses_plain_http() {
        let err = text("http://127.0.0.1:1/x").unwrap_err();
        assert!(format!("{err:#}").contains("https"), "{err:#}");
    }
}
