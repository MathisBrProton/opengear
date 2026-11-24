use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=ui/index.html");
    println!("cargo:rerun-if-changed=ui/app.css");
    println!("cargo:rerun-if-changed=ui/app.js");

    // Automatically place x64 WebView2Loader.dll beside opengear.exe for GNU/gnullvm Windows targets.
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        // OUT_DIR is target/<triple>/<profile>/build/opengear-app-.../out
        let out_path = PathBuf::from(&out_dir);
        if let Some(build_dir) = out_path.ancestors().nth(2) {
            if let Some(profile_dir) = build_dir.parent() {
                if let Some(dll) = find_webview2_loader(build_dir) {
                    let dest = profile_dir.join("WebView2Loader.dll");
                    let _ = fs::copy(&dll, &dest);
                }
            }
        }
    }
}

fn find_webview2_loader(build_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(build_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("webview2-com-sys-") {
            let candidate = entry.path().join("out").join("x64").join("WebView2Loader.dll");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}
