//! Logging: the GUI and the fan watchdog have no console (`windows_subsystem = "windows"`), so
//! their logs go to `%LOCALAPPDATA%\OpenGear\logs\opengear.log` (rotated, 3 files kept). The CLI
//! keeps logging to the terminal.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
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

fn rotated_path_in(dir: &Path, i: u32) -> PathBuf {
    dir.join(format!("opengear.{i}.log"))
}

/// Shift `opengear.log -> opengear.1.log -> opengear.2.log` inside `dir` when size exceeds `max_bytes`.
fn rotate_in_dir(dir: &Path, max_bytes: u64, keep: u32) {
    let current = dir.join("opengear.log");
    let too_big = fs::metadata(&current)
        .map(|m| m.len() >= max_bytes)
        .unwrap_or(false);
    if !too_big {
        return;
    }
    let _ = fs::remove_file(rotated_path_in(dir, keep));
    for i in (1..keep).rev() {
        let _ = fs::rename(rotated_path_in(dir, i), rotated_path_in(dir, i + 1));
    }
    let _ = fs::rename(&current, rotated_path_in(dir, 1));
}

/// Runtime size-rotating file writer for long-running GUI sessions.
struct RotatingFileWriter {
    dir: PathBuf,
    max_bytes: u64,
    keep: u32,
    rotate: bool,
    file: File,
    bytes_written: u64,
}

impl RotatingFileWriter {
    fn open(dir: PathBuf, max_bytes: u64, keep: u32, rotate: bool) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        if rotate {
            rotate_in_dir(&dir, max_bytes, keep);
        }
        let path = dir.join("opengear.log");
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let bytes_written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            dir,
            max_bytes,
            keep,
            rotate,
            file,
            bytes_written,
        })
    }
}

impl Write for RotatingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.rotate && self.bytes_written >= self.max_bytes {
            let _ = self.file.flush();
            // Re-open after closing the active handle so Windows rename succeeds cleanly.
            let placeholder = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("opengear.tmp.lock"));
            if let Ok(tmp_file) = placeholder {
                self.file = tmp_file;
            }
            rotate_in_dir(&self.dir, self.max_bytes, self.keep);
            let _ = fs::remove_file(self.dir.join("opengear.tmp.lock"));
            if let Ok(fresh) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("opengear.log"))
            {
                self.file = fresh;
                self.bytes_written = 0;
            }
        }
        let n = self.file.write(buf)?;
        self.bytes_written = self.bytes_written.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn env_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
}

/// Initialise logging. `to_file`: GUI / watchdog (file), otherwise CLI (stderr).
/// `rotate`: only the main GUI process rotates, so the watchdog never races it.
pub fn init(to_file: bool, rotate: bool) {
    if to_file
        && let Ok(writer) = RotatingFileWriter::open(log_dir(), MAX_LOG_BYTES, KEEP_ROTATED, rotate)
    {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter())
            .with_ansi(false)
            .with_writer(Mutex::new(writer))
            .init();
        return;
    }
    tracing_subscriber::fmt()
        .with_env_filter(env_filter())
        .init();
}

/// Last `max_lines` lines of the current log file (for the diagnostic report).
pub fn tail(max_lines: usize) -> String {
    let Ok(mut f) = File::open(log_path()) else {
        return String::from("(journal introuvable)");
    };
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
    use super::*;

    #[test]
    fn keeps_only_the_last_lines() {
        assert_eq!(last_lines("a\nb\nc\nd\n", 2), "c\nd");
        assert_eq!(last_lines("one\ntwo", 10), "one\ntwo");
        assert_eq!(last_lines("", 5), "");
    }

    #[test]
    fn rotating_writer_rotates_at_runtime() {
        let dir =
            std::env::temp_dir().join(format!("opengear_log_rot_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        {
            let mut w = RotatingFileWriter::open(dir.clone(), 16, 2, true).unwrap();
            w.write_all(b"0123456789abcdef").unwrap();
            w.write_all(b"second-file-line").unwrap();
            w.write_all(b"third-file-line!").unwrap();
            w.flush().unwrap();
        }
        assert!(dir.join("opengear.log").exists());
        assert!(dir.join("opengear.1.log").exists());
        assert!(dir.join("opengear.2.log").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
