//! Lightweight append-only diagnostic log for the online / networking flow.
//!
//! Windowed release builds have no console, so a "join failed" toast is all the
//! player sees. To make co-op issues debuggable, every step of hosting/joining
//! is also written to `cmd_zoo_net.log` next to the executable (and echoed to
//! stderr for `cargo run`). Best-effort and panic-free: logging never affects
//! gameplay.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// Where the log lives: next to the executable, falling back to the current
/// working directory if the exe path can't be resolved.
fn log_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("cmd_zoo_net.log");
        }
    }
    PathBuf::from("cmd_zoo_net.log")
}

/// Append one timestamped line to the networking log and echo it to stderr.
/// Silently ignores any I/O failure.
pub fn log(msg: impl AsRef<str>) {
    let line = format!(
        "[{}] {}\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        msg.as_ref()
    );
    eprint!("{line}");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `net_log!("fmt", ..)` — formatted convenience wrapper over [`log`]. Available
/// crate-wide as `crate::net_log!`.
#[macro_export]
macro_rules! net_log {
    ($($arg:tt)*) => { $crate::net::diag::log(format!($($arg)*)) };
}
