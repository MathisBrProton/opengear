//! Architecture guard: fails `cargo test` when a crate breaks the layering described in AGENTS.md.
//!
//! - `opengear-core`, `opengear-platform` and `opengear-mainboard` know no hardware vendor.
//! - Every other library crate except `opengear-input` is a vendor crate (`opengear-<brand>`) and
//!   may depend only on those neutral crates, never on another vendor crate.
//! - A vendor's protocol identifiers (WMI classes, HID protocol names…) appear in code only inside
//!   that vendor's crate. Comments and string literals (labels, docs) are ignored.
//! - `opengear-app` orchestrates: no direct HID / WMI / IOCTL access.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const NEUTRAL: &[&str] = &["opengear-core", "opengear-platform", "opengear-mainboard"];
const APP: &str = "opengear-app";
const INPUT: &str = "opengear-input";

/// Protocol identifiers owned by one vendor crate (matched case-insensitively in code).
const VENDOR_MARKERS: &[(&str, &[&str])] = &[
    ("opengear-hp", &["hpqbintm", "hpqbdatain", "hpqbios"]),
    ("opengear-asus", &["asusmanagement"]),
    ("opengear-corsair", &["bragi"]),
    ("opengear-logitech", &["hidpp", "hid++"]),
    ("opengear-openrgb", &["orgb"]),
    ("opengear-razer", &["rzreport"]),
    (
        "opengear-steelseries",
        &["rival3_color_pkt", "apex_direct_frame_pkt"],
    ),
    (
        "opengear-hyperx",
        &["origins_core_packets", "pulsefire_dart_pkt"],
    ),
];

/// Low-level I/O that only driver crates may perform.
const APP_FORBIDDEN: &[&str] = &[
    "wmiconn",
    "exec_method",
    "hidapi",
    "send_feature_report",
    "deviceiocontrol",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

/// `crate name -> internal (opengear-*) dependencies` for every crate under `crates/`.
fn internal_deps() -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(workspace_root().join("crates")).unwrap() {
        let dir = entry.unwrap().path();
        let Ok(manifest) = fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let mut section = String::new();
        let mut deps = Vec::new();
        for line in manifest.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line.to_string();
                continue;
            }
            if section.contains("dependencies") && line.starts_with("opengear-") {
                let dep = line.split(['=', '.', ' ']).next().unwrap().to_string();
                deps.push(dep);
            }
        }
        out.insert(name, deps);
    }
    out
}

fn is_vendor(name: &str) -> bool {
    !NEUTRAL.contains(&name) && name != APP && name != INPUT
}

/// Remove `//` comments and the contents of string / char literals from one source line.
fn code_only(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '/' if chars.peek() == Some(&'/') => break,
            _ => out.push(c),
        }
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// `(file:line, marker)` for each forbidden marker found in the code of `crate_name`.
fn find_markers(crate_name: &str, markers: &[&str]) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(
        &workspace_root().join("crates").join(crate_name).join("src"),
        &mut files,
    );
    let mut hits = Vec::new();
    for f in files {
        let text = fs::read_to_string(&f).unwrap();
        for (i, line) in text.lines().enumerate() {
            let code = code_only(line).to_ascii_lowercase();
            for m in markers {
                if code.contains(m) {
                    hits.push(format!("{}:{} -> `{m}`", f.display(), i + 1));
                }
            }
        }
    }
    hits
}

#[test]
fn crate_dependencies_follow_the_layering() {
    let deps = internal_deps();
    for expected in [
        "opengear-platform",
        "opengear-mainboard",
        "opengear-hp",
        "opengear-openrgb",
    ] {
        assert!(deps.contains_key(expected), "{expected} manquant");
    }
    let mut violations = Vec::new();
    for (name, ds) in &deps {
        for d in ds {
            let allowed = match name.as_str() {
                "opengear-core" => false,
                "opengear-platform" => d == "opengear-core",
                "opengear-mainboard" => d == "opengear-core" || d == "opengear-platform",
                APP => true,
                _ => NEUTRAL.contains(&d.as_str()),
            };
            if !allowed {
                violations.push(format!("{name} -> {d}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Dépendances interdites (voir AGENTS.md) : {violations:?}"
    );
}

#[test]
fn vendor_protocols_stay_in_their_crate() {
    let deps = internal_deps();
    let mut violations = Vec::new();
    for name in deps.keys().filter(|n| n.as_str() != APP) {
        for (owner, markers) in VENDOR_MARKERS {
            if name != owner {
                violations.extend(find_markers(name, markers));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Code propre à une marque trouvé hors de son crate (voir AGENTS.md) :\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_vendor_crate_is_registered_here() {
    let deps = internal_deps();
    let missing: Vec<_> = deps
        .keys()
        .filter(|n| is_vendor(n))
        .filter(|n| !VENDOR_MARKERS.iter().any(|(owner, _)| owner == n))
        .collect();
    assert!(
        missing.is_empty(),
        "Nouveau crate marque sans marqueurs de protocole dans tests/architecture.rs : {missing:?}"
    );
}

#[test]
fn app_does_no_direct_hardware_io() {
    let hits = find_markers(APP, APP_FORBIDDEN);
    assert!(
        hits.is_empty(),
        "opengear-app doit passer par les crates pilotes :\n{}",
        hits.join("\n")
    );
}

#[test]
fn code_only_strips_comments_and_strings() {
    assert_eq!(code_only(r#"let a = "hpqBIntM"; // hpq"#), "let a = ; ");
    assert_eq!(code_only(r#"f("a\"b", x)"#), "f(, x)");
}
