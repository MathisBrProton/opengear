use flate2::{Compression, write::ZlibEncoder};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let stage = PathBuf::from("../../dist/stage");
    let exe_src = stage.join("OpenGear.exe");
    let dll_src = stage.join("WebView2Loader.dll");
    println!("cargo:rerun-if-changed={}", exe_src.display());
    println!("cargo:rerun-if-changed={}", dll_src.display());

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    compress(&exe_src, &out_dir.join("opengear_exe.zlib"));
    compress(&dll_src, &out_dir.join("webview2_dll.zlib"));
}

fn compress(src: &PathBuf, dst: &PathBuf) {
    let raw = fs::read(src).unwrap_or_else(|e| panic!("Impossible de lire {}: {e}", src.display()));
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&raw).unwrap();
    let compressed = enc.finish().unwrap();
    fs::write(dst, compressed).unwrap();
}
