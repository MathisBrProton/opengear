use flate2::{Compression, write::ZlibEncoder};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let stage = PathBuf::from("../../dist/stage");
    let exe_src = stage.join("OpenGear.exe");
    let dll_src = stage.join("WebView2Loader.dll");
    let pawnio_dir = stage.join("pawnio_modules");

    println!("cargo:rerun-if-changed={}", exe_src.display());
    println!("cargo:rerun-if-changed={}", dll_src.display());
    println!("cargo:rerun-if-changed={}", pawnio_dir.display());

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    compress(&exe_src, &out_dir.join("opengear_exe.zlib"));
    compress(&dll_src, &out_dir.join("webview2_dll.zlib"));
    bundle_pawnio_modules(&pawnio_dir, &out_dir.join("pawnio_modules.tar_simple"));
}

fn compress(src: &Path, dst: &Path) {
    let raw = fs::read(src).unwrap_or_else(|e| panic!("Impossible de lire {}: {e}", src.display()));
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&raw).unwrap();
    let compressed = enc.finish().unwrap();
    fs::write(dst, compressed).unwrap();
}

/// Simple length-prefixed archive (`[u16 name_len LE][name utf8][u64 data_len LE][bytes...]`)
/// zlib-compressed into `OUT_DIR/pawnio_modules.tar_simple`, empty if no `pawnio_modules` dir.
fn bundle_pawnio_modules(src_dir: &Path, dst: &Path) {
    let mut blob = Vec::new();
    if src_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(src_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if let Ok(data) = fs::read(&path) {
                    let nb = name.as_bytes();
                    blob.extend_from_slice(&(nb.len() as u16).to_le_bytes());
                    blob.extend_from_slice(nb);
                    blob.extend_from_slice(&(data.len() as u64).to_le_bytes());
                    blob.extend_from_slice(&data);
                }
            }
        }
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&blob).unwrap();
    let compressed = enc.finish().unwrap();
    fs::write(dst, compressed).unwrap();
}
