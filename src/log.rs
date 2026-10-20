//! Minimal timestamped console logging.

use std::time::{SystemTime, UNIX_EPOCH};

fn stamp() -> String {
    // Local time of day (HH:MM:SS), without pulling in a date/time crate.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
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
        if localtime_r(&now, &mut tm).is_null() {
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
    eprintln!("{line}");
    crate::state::push_log(line);
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
