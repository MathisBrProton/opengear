#![cfg_attr(windows, windows_subsystem = "windows")]

use flate2::read::ZlibDecoder;
use std::fs;
use std::io::Read;
use std::process::Command;

const OPENGEAR_EXE_ZLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/opengear_exe.zlib"));
const WEBVIEW2_DLL_ZLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webview2_dll.zlib"));

fn decompress_to(compressed: &[u8], dst: &std::path::Path) -> std::io::Result<()> {
    let mut dec = ZlibDecoder::new(compressed);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    fs::write(dst, out)
}

fn main() {
    let tmp = std::env::temp_dir().join(format!("opengear_setup_{}", std::process::id()));
    let _ = fs::create_dir_all(&tmp);
    let exe_path = tmp.join("OpenGear.exe");
    let dll_path = tmp.join("WebView2Loader.dll");

    if decompress_to(OPENGEAR_EXE_ZLIB, &exe_path).is_ok()
        && decompress_to(WEBVIEW2_DLL_ZLIB, &dll_path).is_ok()
    {
        let _ = Command::new(&exe_path)
            .arg("--install")
            .current_dir(&tmp)
            .spawn();
    }
}
