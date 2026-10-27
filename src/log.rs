//! Minimal timestamped console logging.
//!
//! Every line written here is masked, and `state::push_log` masks the lines it stores: the home
//! folder becomes `~` and email addresses become `<email>`. Nothing else is masked, and a line
//! written straight into `State::log` is not masked at all, so look a log over before posting it.
//! Never log a token or a player's name; the mask is a safety net, not a licence.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

fn stamp() -> String {
    // Local time of day (HH:MM:SS), without pulling in a date/time crate.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let local = secs.wrapping_add_signed(local_offset_secs());
    let day = local % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
}

#[cfg(unix)]
fn local_offset_secs() -> i64 {
    // SAFETY: localtime_r with a valid time_t and out-pointer is thread-safe.
    unsafe {
        let now = libc_time();
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&raw const now, &raw mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff
    }
}

#[cfg(not(unix))]
fn local_offset_secs() -> i64 {
    0
}

#[cfg(unix)]
#[repr(C)]
struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: i64,
    tm_zone: *const std::ffi::c_char,
}

#[cfg(unix)]
unsafe extern "C" {
    fn time(t: *mut i64) -> i64;
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

#[cfg(unix)]
unsafe fn libc_time() -> i64 {
    unsafe { time(std::ptr::null_mut()) }
}

fn emit(line: String) {
    emit_with_home(&mut std::io::stderr(), line, home().as_deref());
}

/// [`emit`] with the terminal `out` and the home folder given (tests pass their own).
fn emit_with_home(out: &mut impl Write, line: String, home: Option<&str>) {
    let line = mask(&line, home);
    let _ = writeln!(out, "{line}");
    crate::state::push_log(line);
}

/// The home folder from `HOME`, if it is set.
pub(crate) fn home() -> Option<String> {
    std::env::var("HOME").ok()
}

/// `line` as it may be logged: the home folder `home` (e.g. `/Users/jane`, often the real name)
/// replaced by `~`, and every email-shaped word (one `@`, a dot after it) by `<email>`.
#[must_use]
pub(crate) fn mask(line: &str, home: Option<&str>) -> String {
    mask_emails(&tilde_home(line, home))
}

/// Every `home` in `s` that is a whole path prefix (followed by `/`, the end or a non-name
/// character) replaced by `~`. No `home`, an empty one or `/` leaves `s` as it is.
#[must_use]
pub(crate) fn tilde_home(s: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(|h| h.trim_end_matches('/')).filter(|h| !h.is_empty()) else {
        return s.to_string();
    };
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find(home) {
        let after = &rest[at + home.len()..];
        let whole = after.chars().next().is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-')));
        out.push_str(&rest[..at]);
        out.push_str(if whole { "~" } else { home });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Every email-shaped word in `s` replaced by `<email>`: a local part, one `@` and a domain
/// with a dot followed by at least one character (no `regex`).
fn mask_emails(s: &str) -> String {
    let local = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let domain = |c: char| c.is_alphanumeric() || matches!(c, '.' | '-');
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut out = String::with_capacity(s.len());
    let mut copied = 0; // byte offset in `s` up to which `out` holds the text
    let mut floor = 0; // the first char a local part may start at (after the last match)
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1 != '@' {
            i += 1;
            continue;
        }
        let mut start = i;
        while start > floor && local(chars[start - 1].1) {
            start -= 1;
        }
        let mut end = i + 1;
        while end < chars.len() && domain(chars[end].1) {
            end += 1;
        }
        // A sentence's full stop or a trailing dash is not part of the domain.
        while end > i + 1 && matches!(chars[end - 1].1, '.' | '-') {
            end -= 1;
        }
        let dom: String = chars[i + 1..end].iter().map(|&(_, c)| c).collect();
        let dotted = dom.split('.').count() > 1 && dom.split('.').all(|part| !part.is_empty());
        if start < i && dotted {
            out.push_str(&s[copied..chars[start].0]);
            out.push_str("<email>");
            copied = chars.get(end).map_or(s.len(), |&(b, _)| b);
            i = end;
            floor = end;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[copied..]);
    out
}

pub fn r10(msg: &str) {
    emit(format!("{} [R10]   {msg}", stamp()));
}

pub fn gspro(msg: &str) {
    emit(format!("{} [GSPro] {msg}", stamp()));
}

pub fn info(msg: &str) {
    emit(format!("{} {msg}", stamp()));
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: Option<&str> = Some("/Users/jane");

    #[test]
    fn the_home_folder_becomes_a_tilde() {
        assert_eq!(
            mask("Starting GSPro: /Users/jane/Applications/CrossOver/GSPro/GSPro.app", HOME),
            "Starting GSPro: ~/Applications/CrossOver/GSPro/GSPro.app"
        );
        assert_eq!(mask("Data folder: /Users/jane", HOME), "Data folder: ~");
        assert_eq!(mask("from /Users/jane/a to /Users/jane/b", HOME), "from ~/a to ~/b");
        assert_eq!(mask("in '/Users/jane'.", Some("/Users/jane/")), "in '~'.");
        // Another account that starts with the same letters stays.
        assert_eq!(mask("/Users/janet/x", HOME), "/Users/janet/x");
    }

    #[test]
    fn without_a_home_the_line_is_unchanged() {
        let line = "Starting GSPro: /Users/jane/Applications/GSPro.app";
        assert_eq!(mask(line, None), line);
        assert_eq!(mask(line, Some("")), line);
        assert_eq!(mask(line, Some("/")), line);
    }

    #[test]
    fn email_addresses_are_masked() {
        assert_eq!(mask("[GSPro] Message: hello a.b@c.de", HOME), "[GSPro] Message: hello <email>");
        assert_eq!(mask(r#"{"Message":"jane.doe+golf@mail.example.com"}"#, HOME), r#"{"Message":"<email>"}"#);
        assert_eq!(mask("write to x@y.org.", HOME), "write to <email>.");
        assert_eq!(mask("a@b.cd and e@f.gh", HOME), "<email> and <email>");
        assert_eq!(mask("Grüße an jürgen@straße.de!", HOME), "Grüße an <email>!");
    }

    /// The mask is applied where a line is written, so the Log page and the copy get it.
    #[test]
    fn written_lines_are_masked() {
        let _state = crate::test_util::state_lock();
        gspro("Message: contact a.b@c.de");
        let last = crate::state::with(|s| s.log.back().cloned()).unwrap();
        assert!(last.ends_with("[GSPro] Message: contact <email>"), "{last}");
        let mut terminal = Vec::new();
        emit_with_home(&mut terminal, format!("{} Launcher: /Users/jane/Applications/GSPro.app", stamp()), HOME);
        let last = crate::state::with(|s| s.log.back().cloned()).unwrap();
        assert!(last.ends_with(" Launcher: ~/Applications/GSPro.app"), "{last}");
        // The terminal gets the masked line too, not only the stored copy.
        assert_eq!(String::from_utf8(terminal).unwrap(), format!("{last}\n"));
    }

    /// The home folder the masks use is `HOME`.
    #[test]
    fn home_is_the_home_variable() {
        assert_eq!(home(), std::env::var("HOME").ok());
    }

    /// A line put into the log directly is masked as well: the Log page and COPY LOG see no
    /// line that got past the mask.
    #[test]
    fn stored_lines_are_masked() {
        let _state = crate::test_util::state_lock();
        crate::state::push_log("extension: mail jane.doe@example.com".into());
        let last = crate::state::with(|s| s.log.back().cloned()).unwrap();
        assert_eq!(last, "extension: mail <email>");
        // Masking twice changes nothing.
        assert_eq!(mask("~/x <email>", HOME), "~/x <email>");
    }

    #[test]
    fn other_lines_are_unchanged() {
        for line in [
            "Player: RH, club: DR",
            "18:10:41 [GSPro] Sending shot to GSPro (ShotNumber 56728296) …",
            "user@localhost has no dot",
            "@ alone, a@ and @b.c",
            "a@b. and a@.b",
            "",
        ] {
            assert_eq!(mask(line, HOME), line);
        }
    }
}
