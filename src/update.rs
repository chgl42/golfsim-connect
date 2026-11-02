//! Update check as an extension point (ADR-014): the core asks nobody on its own. Another crate
//! registers an [`UpdateSource`] with [`register`]; the core then asks it once at start (opt-out
//! in the settings) and from the settings, on its own `update` thread, and shows a newer release
//! as a pill in the top bar. Without a source there is no check and no request.

use std::sync::OnceLock;

use crate::{log, state};

/// A release newer than the installed version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.10.0`.
    pub version: String,
    /// The page that describes the release; the pill in the top bar opens it.
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

/// Where the app learns about newer versions, registered by another crate with [`register`].
pub trait UpdateSource: Send + Sync {
    /// The installed version, e.g. `1.2.0` (shown as "Installed: …" in the settings).
    fn installed(&self) -> String;
    /// A release newer than [`installed`](Self::installed), or `None` when it is up to date.
    /// Called on the core's `update` thread, never on the UI thread, so it may block.
    fn check(&self) -> Result<Option<Release>, String>;
}

static SOURCE: OnceLock<Box<dyn UpdateSource>> = OnceLock::new();

/// Registers the update source. Call it once, before the UI is shown (`ui::run_with`); a second
/// call is logged and ignored, the first source stays.
pub fn register(source: Box<dyn UpdateSource>) {
    register_in(&SOURCE, source);
}

fn register_in(lock: &OnceLock<Box<dyn UpdateSource>>, source: Box<dyn UpdateSource>) {
    if lock.set(source).is_err() {
        log::info("An update source is already registered; the new one is ignored.");
    }
}

/// The registered source, if any.
pub(crate) fn source() -> Option<&'static dyn UpdateSource> {
    SOURCE.get().map(Box::as_ref)
}

/// The installed version: the source's, else the core's.
#[cfg_attr(not(feature = "gui"), expect(dead_code, reason = "shown by the UI"))]
pub(crate) fn installed() -> String {
    source().map_or_else(|| env!("CARGO_PKG_VERSION").to_string(), UpdateSource::installed)
}

/// Check in the background (at start, or from the settings); a newer release ends up in
/// `state.update`, the outcome in `state.update_check`. Does nothing without a registered source.
pub fn check_in_background() {
    let Some(source) = source() else { return };
    state::with(|s| s.update_check = Some(Check::Running));
    std::thread::Builder::new().name("update".into()).spawn(move || run_check(source)).expect("spawn update thread");
}

/// Asks `source` and puts the outcome into the state.
fn run_check(source: &dyn UpdateSource) {
    let check = match source.check() {
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
}

/// Compare `major.minor.patch` versions numerically (`0.10.0` > `0.9.1`); anything unparsable is
/// not newer. For sources, to compare their latest version with the installed one.
#[must_use]
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|p| p.parse().ok()).collect() };
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source that answers with a fixed result.
    struct Fake(Result<Option<Release>, String>);

    impl UpdateSource for Fake {
        fn installed(&self) -> String {
            "1.0.0".into()
        }
        fn check(&self) -> Result<Option<Release>, String> {
            self.0.clone()
        }
    }

    fn reset() {
        state::with(|s| {
            s.update = None;
            s.update_check = None;
        });
    }

    #[test]
    fn version_order() {
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("0.9.1", "0.9.0"));
        assert!(is_newer("1.0.0", "0.99.9"));
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("0.8.5", "0.9.0"));
        assert!(!is_newer("0.10.0-beta", "0.9.0"), "pre-releases are ignored");
    }

    /// The tests never register a source (the `OnceLock` is shared by the whole test binary).
    #[test]
    fn no_source_no_check() {
        let _state = crate::test_util::state_lock();
        reset();
        assert!(source().is_none());
        assert_eq!(installed(), env!("CARGO_PKG_VERSION"));
        check_in_background();
        assert_eq!(state::with(|s| (s.update.clone(), s.update_check.clone())), (None, None), "no thread, no state");
    }

    #[test]
    fn a_newer_release_is_kept() {
        let _state = crate::test_util::state_lock();
        reset();
        let release = Release { version: "9.9.9".into(), url: "https://example.com/9.9.9".into() };
        run_check(&Fake(Ok(Some(release.clone()))));
        assert_eq!(state::with(|s| (s.update.clone(), s.update_check.clone())), (Some(release), Some(Check::Available)));
        reset();
    }

    #[test]
    fn up_to_date() {
        let _state = crate::test_util::state_lock();
        reset();
        run_check(&Fake(Ok(None)));
        assert_eq!(state::with(|s| (s.update.clone(), s.update_check.clone())), (None, Some(Check::UpToDate)));
        reset();
    }

    #[test]
    fn the_first_source_stays() {
        let lock = OnceLock::new();
        register_in(&lock, Box::new(Fake(Ok(None))));
        register_in(&lock, Box::new(Fake(Err("second".into()))));
        assert_eq!(lock.get().unwrap().check(), Ok(None));
    }

    #[test]
    fn a_failed_check_keeps_its_error() {
        let _state = crate::test_util::state_lock();
        reset();
        run_check(&Fake(Err("offline".into())));
        assert_eq!(state::with(|s| (s.update.clone(), s.update_check.clone())), (None, Some(Check::Failed("offline".into()))));
        reset();
    }
}
