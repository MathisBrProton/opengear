#![cfg_attr(windows, windows_subsystem = "windows")]

use flate2::read::ZlibDecoder;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

const OPENGEAR_EXE_ZLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/opengear_exe.zlib"));
const WEBVIEW2_DLL_ZLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webview2_dll.zlib"));
const PAWNIO_MODULES_ZLIB: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/pawnio_modules.tar_simple"));

#[cfg(windows)]
fn show_error(title: &str, msg: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    let w_title: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
    let w_msg: Vec<u16> = OsStr::new(msg).encode_wide().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(0, w_msg.as_ptr(), w_title.as_ptr(), 0x0000_0010);
    }
}

#[cfg(not(windows))]
fn show_error(_title: &str, msg: &str) {
    eprintln!("{msg}");
}

fn create_unique_temp_dir() -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..64u32 {
        let candidate = base.join(format!("opengear_setup_{pid}_{nanos:x}_{attempt}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Impossible de créer un dossier temporaire unique",
    ))
}

fn decompress_stream_to(compressed: &[u8], dst: &Path) -> io::Result<()> {
    let mut dec = ZlibDecoder::new(compressed);
    let mut out = File::create(dst)?;
    io::copy(&mut dec, &mut out)?;
    Ok(())
}

fn extract_pawnio_modules(dst_dir: &Path) -> io::Result<()> {
    let mut dec = ZlibDecoder::new(PAWNIO_MODULES_ZLIB);
    let mut raw = Vec::new();
    dec.read_to_end(&mut raw)?;
    if raw.is_empty() {
        return Ok(());
    }
    let mod_dir = dst_dir.join("pawnio_modules");
    fs::create_dir_all(&mod_dir)?;
    let mut pos = 0usize;
    while pos + 2 <= raw.len() {
        let nlen = u16::from_le_bytes([raw[pos], raw[pos + 1]]) as usize;
        pos += 2;
        if pos + nlen + 8 > raw.len() {
            break;
        }
        let name = String::from_utf8_lossy(&raw[pos..pos + nlen]).to_string();
        pos += nlen;
        let dlen = u64::from_le_bytes(raw[pos..pos + 8].try_into().unwrap()) as usize;
        pos += 8;
        if pos + dlen > raw.len() {
            break;
        }
        if !name.contains('/') && !name.contains('\\') && !name.contains("..") && !name.is_empty() {
            fs::write(mod_dir.join(&name), &raw[pos..pos + dlen])?;
        }
        pos += dlen;
    }
    Ok(())
}

fn run_install() -> Result<(), String> {
    let tmp = create_unique_temp_dir().map_err(|e| format!("Création dossier temporaire : {e}"))?;
    let exe_path = tmp.join("OpenGear.exe");
    let dll_path = tmp.join("WebView2Loader.dll");

    if let Err(e) = decompress_stream_to(OPENGEAR_EXE_ZLIB, &exe_path) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("Extraction OpenGear.exe échouée : {e}"));
    }
    if let Err(e) = decompress_stream_to(WEBVIEW2_DLL_ZLIB, &dll_path) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("Extraction WebView2Loader.dll échouée : {e}"));
    }
    if let Err(e) = extract_pawnio_modules(&tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("Extraction pawnio_modules échouée : {e}"));
    }

    let mut cmd = Command::new(&exe_path);
    cmd.arg("--install")
        .arg("--cleanup-dir")
        .arg(&tmp)
        .current_dir(&tmp);
    cmd.spawn().map_err(|e| {
        let _ = fs::remove_dir_all(&tmp);
        format!("Lancement de OpenGear.exe --install échoué : {e}")
    })?;
    Ok(())
}

fn main() {
    if let Err(err) = run_install() {
        show_error("OpenGear Setup — Erreur", &err);
    }
}
