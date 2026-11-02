//! An update source as another crate registers it: only the library's public interface (ADR-014).
//! Its own test binary, so registering the process-wide source touches no other test.

use std::time::{Duration, Instant};

use openlaunch_connect::state;
use openlaunch_connect::update::{self, Check, Release, UpdateSource};

struct Latest;

impl UpdateSource for Latest {
    fn installed(&self) -> String {
        "1.0.0".into()
    }
    fn check(&self) -> Result<Option<Release>, String> {
        Ok(Some(Release { version: "9.9.9".into(), url: "https://example.com/9.9.9".into() }))
    }
}

#[test]
fn a_registered_source_finds_the_newer_release() {
    update::register(Box::new(Latest));
    update::check_in_background();
    let deadline = Instant::now() + Duration::from_secs(5);
    while state::with(|s| s.update_check.clone()) == Some(Check::Running) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state::with(|s| s.update_check.clone()), Some(Check::Available));
    assert_eq!(state::with(|s| s.update.clone()), Some(Release { version: "9.9.9".into(), url: "https://example.com/9.9.9".into() }));
}
