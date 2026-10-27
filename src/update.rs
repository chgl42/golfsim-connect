//! Update check: asks GitHub once at start whether a newer release exists (opt-out in the
//! settings). Only the public releases API is queried; nothing is sent besides the request.

use std::time::Duration;

use serde_json::Value;

use crate::{log, state};

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/chgl42/golfsim-connect/releases/latest";

/// A release newer than the running version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.10.0`.
    pub version: String,
    /// Release page on GitHub.
    pub url: String,
}

/// Outcome of an update check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Running,
    UpToDate,
    Available,
    /// With the error message.
    Failed(String),
}

/// Check in the background (at start, or from the settings); a newer release ends up in
/// `state.update`, the outcome in `state.update_check`.
pub fn check_in_background() {
    state::with(|s| s.update_check = Some(Check::Running));
    std::thread::Builder::new()
        .name("update".into())
        .spawn(|| {
            let check = match latest_release() {
                Ok(Some(release)) => {
                    log::info(&format!("Update available: {} ({})", release.version, release.url));
                    state::with(|s| s.update = Some(release));
                    Check::Available
                }
                Ok(None) => {
                    log::info("No update available.");
                    Check::UpToDate
                }
                Err(e) => {
                    log::info(&format!("Update check failed: {e}"));
                    Check::Failed(e)
                }
            };
            state::with(|s| s.update_check = Some(check));
        })
        .expect("spawn update thread");
}

fn latest_release() -> Result<Option<Release>, String> {
    let json = fetch_latest(LATEST_RELEASE_API)?;
    Ok(newer_release(&json, env!("CARGO_PKG_VERSION")))
}

/// The GitHub API's answer for the latest release at `url` (redirects are followed, so an
/// app built with the repository's old name still finds it).
fn fetch_latest(url: &str) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    let body = agent
        .get(url)
        .header("User-Agent", concat!("OpenLaunch-Connect/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// The release described by the GitHub API `json`, if it is newer than `current`.
fn newer_release(json: &Value, current: &str) -> Option<Release> {
    let tag = json.get("tag_name")?.as_str()?;
    let url = json.get("html_url")?.as_str()?;
    let version = tag.trim_start_matches('v');
    is_newer(version, current).then(|| Release { version: version.to_string(), url: url.to_string() })
}

/// Compare `major.minor.patch` versions numerically (`0.10.0` > `0.9.1`); anything unparsable is not newer.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|p| p.parse().ok()).collect() };
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("0.9.1", "0.9.0"));
        assert!(is_newer("1.0.0", "0.99.9"));
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("0.8.5", "0.9.0"));
        assert!(!is_newer("0.10.0-beta", "0.9.0"), "pre-releases are ignored");
    }

    #[test]
    fn release_from_github_json() {
        let json: Value = serde_json::json!({
            "tag_name": "v0.10.0",
            "html_url": "https://github.com/chgl42/golfsim-connect/releases/tag/v0.10.0"
        });
        assert_eq!(
            newer_release(&json, "0.9.0"),
            Some(Release {
                version: "0.10.0".into(),
                url: "https://github.com/chgl42/golfsim-connect/releases/tag/v0.10.0".into()
            })
        );
        assert_eq!(newer_release(&json, "0.10.0"), None);
        assert_eq!(newer_release(&serde_json::json!({}), "0.9.0"), None);
    }

    /// Apps released before the move ask the old URL; GitHub redirects it to the renamed
    /// repository. The current URL is asked too. Needs the network.
    #[test]
    #[ignore = "network: asks GitHub"]
    fn old_url_still_finds_the_release() {
        for url in ["https://api.github.com/repos/chgl42/openlaunch-connect/releases/latest", LATEST_RELEASE_API] {
            let json = fetch_latest(url).unwrap();
            let release = newer_release(&json, "0.0.0").expect("a release");
            assert!(release.url.starts_with("https://github.com/chgl42/golfsim-connect/releases/tag/v"), "{url}: {}", release.url);
        }
    }
}
