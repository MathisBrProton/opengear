use std::fs;
use std::path::{Path, PathBuf};

fn emit_rerun_tree(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            emit_rerun_tree(&p);
        } else {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }
}

fn main() {
    emit_rerun_tree(Path::new("ui"));
    emit_rerun_tree(Path::new("assets"));

    let pawnio_src = Path::new("../opengear-asus/pawnio_modules");
    emit_rerun_tree(pawnio_src);

    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".into());
    let wv_arch_dir = match target_arch.as_str() {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "x64",
    };

    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        // OUT_DIR is target/<triple>/<profile>/build/opengear-app-.../out
        let out_path = PathBuf::from(&out_dir);
        if let Some(build_dir) = out_path.ancestors().nth(2) {
            if let Some(profile_dir) = build_dir.parent() {
                if let Some(dll) = find_webview2_loader(build_dir, wv_arch_dir) {
                    let dest = profile_dir.join("WebView2Loader.dll");
                    let _ = fs::copy(&dll, &dest);
                }

                // Stage LGPL-2.1 PawnIO modules as external replaceable files beside the binary.
                if pawnio_src.is_dir() {
                    let pio_dest = profile_dir.join("pawnio_modules");
                    let _ = fs::create_dir_all(&pio_dest);
                    if let Ok(entries) = fs::read_dir(pawnio_src) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.is_file() {
                                if let Some(name) = p.file_name() {
                                    let _ = fs::copy(&p, pio_dest.join(name));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn find_webview2_loader(build_dir: &Path, arch_dir: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(build_dir).ok()?;
    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("webview2-com-sys-") {
            let candidate = entry
                .path()
                .join("out")
                .join(arch_dir)
                .join("WebView2Loader.dll");
            if candidate.exists() {
                let mtime = fs::metadata(&candidate)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                candidates.push((mtime, candidate));
            }
        }
    }
    candidates.sort_by_key(|(t, _)| *t);
    candidates.pop().map(|(_, p)| p)
}
