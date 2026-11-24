//! Logging: the GUI and the fan watchdog have no console (`windows_subsystem = "windows"`), so
//! their logs go to `%LOCALAPPDATA%\OpenGear\logs\opengear.log` (rotated, 3 files kept). The CLI
//! keeps logging to the terminal.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;

/// Rotate when the current log exceeds this size.
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
/// Rotated files kept: opengear.1.log .. opengear.<N>.log
const KEEP_ROTATED: u32 = 2;

pub fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenGear")
        .join("logs")
}

pub fn log_path() -> PathBuf {
    log_dir().join("opengear.log")
}

fn rotated_path(i: u32) -> PathBuf {
    log_dir().join(format!("opengear.{i}.log"))
}

/// Shift opengear.log -> opengear.1.log -> opengear.2.log when the current file is too big.
fn rotate_if_needed() {
    let current = log_path();
    let too_big = fs::metadata(&current).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false);
    if !too_big {
        return;
    }
    let _ = fs::remove_file(rotated_path(KEEP_ROTATED));
    for i in (1..KEEP_ROTATED).rev() {
        let _ = fs::rename(rotated_path(i), rotated_path(i + 1));
    }
    let _ = fs::rename(&current, rotated_path(1));
}

fn env_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
}

/// Initialise logging. `to_file`: GUI / watchdog (file), otherwise CLI (stderr).
/// `rotate`: only the main GUI process rotates, so the watchdog never races it.
pub fn init(to_file: bool, rotate: bool) {
    if to_file {
        let _ = fs::create_dir_all(log_dir());
        if rotate {
            rotate_if_needed();
        }
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(log_path()) {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter())
                .with_ansi(false)
                .with_writer(Mutex::new(file))
                .init();
            return;
        }
    }
    tracing_subscriber::fmt().with_env_filter(env_filter()).init();
}

/// Last `max_lines` lines of the current log file (for the diagnostic report).
pub fn tail(max_lines: usize) -> String {
    let Ok(mut f) = File::open(log_path()) else {
        return String::from("(journal introuvable)");
    };
    // Read at most the last 256 KiB.
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(256 * 1024);
    let _ = f.seek(SeekFrom::Start(start));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    last_lines(&String::from_utf8_lossy(&buf), max_lines)
}

pub(crate) fn last_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(max_lines);
    lines[from..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::last_lines;

    #[test]
    fn keeps_only_the_last_lines() {
        assert_eq!(last_lines("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(last_lines("a\nb", 10), "a\nb");
        assert_eq!(last_lines("", 3), "");
    }
}
