//! Finding and starting GSPro (via its CrossOver launcher on macOS).

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Look for CrossOver-generated launchers whose name mentions GSPro,
/// e.g. `~/Applications/CrossOver/GSPro/GSPro.app`. Best match first.
#[must_use]
pub fn find_gspro_launchers() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications/CrossOver"));
    }
    roots.push(PathBuf::from("/Applications/CrossOver"));

    let mut found = Vec::new();
    for root in roots {
        walk(&root, 0, &mut found);
    }
    // Prefer "GSPro" over e.g. "GSPro Uninstaller"; launcher before anything else.
    found.sort_by_key(|p| {
        let name = file_name_lower(p);
        let penalty = if name.contains("uninstall") || name.contains("config") { 10 } else { 0 };
        let bonus = if name.contains("launcher") { 0 } else { 1 };
        (penalty, bonus, name.len())
    });
    found
}

fn file_name_lower(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = file_name_lower(&path);
        if name.ends_with(".app") {
            if name.contains("gsp") {
                out.push(path);
            }
            continue; // don't descend into bundles
        }
        if path.is_dir() {
            walk(&path, depth + 1, out);
        }
    }
}

/// Whether GSPro already appears to be running: OpenConnect port answers,
/// or a GSPro Windows process exists.
#[must_use]
pub fn gspro_running(addr: &str) -> bool {
    let port_open = addr
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok());
    if port_open {
        return true;
    }
    Command::new("pgrep")
        .args(["-i", "-f", r"gsp(ro|launcher)[^/\\]*\.exe"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Start GSPro via its launcher (a CrossOver `.app`).
///
/// # Errors
/// Returns a message if the launcher cannot be opened.
pub fn launch(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Launcher not found: {}", path.display()));
    }
    let status = Command::new("open")
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`open` failed ({status})"))
    }
}
