"""Generate crates/opengear-razer/src/tables.rs from docs/protocols/razer.md (sections 9 and 10).

Clean-room rule: the Rust tables are derived ONLY from the protocol sheet, never from another
project's source. Re-run after editing the sheet:

    python -I tools/protocol-lab/gen_razer_tables.py

The script is deterministic and fails loudly on any line it cannot parse.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SHEET = ROOT / "docs" / "protocols" / "razer.md"
OUT = ROOT / "crates" / "opengear-razer" / "src" / "tables.rs"

SUPPORTED_MATRICES = {"extended": "Extended", "standard": "Standard", "linear": "Linear"}
KINDS = {
    "keyboard": "Keyboard", "laptop": "Laptop", "mouse": "Mouse", "mousemat": "Mousemat",
    "keypad": "Keypad", "headset": "Headset", "headset_stand": "HeadsetStand", "accessory": "Accessory",
    "speaker": "Speaker", "microphone": "Microphone", "ledstrip": "LedStrip", "light": "Light", "gpu": "Gpu",
}
ZONE_KINDS = {"matrix": "Matrix", "linear": "Linear", "single": "Single"}
# 0x0F21 is shared by a headset and a dock; the sheet only gives the dock geometry (§5), so only
# the dock interface is matched.
PID_SELECTOR_OVERRIDE = {0x0F21: "if=* page=0x0C usage=0x01"}


def fail(msg):
    sys.exit(f"gen_razer_tables: {msg}")


def rust_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def parse_selectors(cell, pid):
    cell = PID_SELECTOR_OVERRIDE.get(pid, cell)
    out = []
    for part in cell.split("<br>"):
        m = re.match(r"\s*if=(\*|0x[0-9A-Fa-f]+) page=(\*|0x[0-9A-Fa-f]+) usage=(\*|0x[0-9A-Fa-f]+)", part)
        if not m:
            fail(f"PID {pid:04X}: sélecteur HID illisible : {part!r}")

        def opt(v, width):
            return "None" if v == "*" else f"Some(0x{int(v, 16):0{width}X})"

        out.append(
            f"HidSelector {{ interface: {opt(m.group(1), 2)}, usage_page: {opt(m.group(2), 4)}, usage: {opt(m.group(3), 4)} }}"
        )
    return out


def parse_zones(cell, pid):
    zones = []
    for part in cell.split(";"):
        m = re.match(r"\s*(.+?) \((matrix|linear|single) (\d+)x(\d+)\)\s*$", part)
        if not m:
            fail(f"PID {pid:04X}: zone illisible : {part!r}")
        zones.append((m.group(1), ZONE_KINDS[m.group(2)], int(m.group(3)), int(m.group(4))))
    return zones


def parse_devices(lines):
    devices = []
    for line in lines:
        if not line.startswith("| Razer "):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) != 10:
            fail(f"ligne d'appareil inattendue : {line!r}")
        name, vidpid, kind, driver, matrix, txid, led, grid, zones, hid = cells
        if driver != "std" or matrix not in SUPPORTED_MATRICES or "desactive" in hid:
            continue
        vid, pid = (int(x, 16) for x in vidpid.split(":"))
        if vid != 0x1532:
            fail(f"{name}: VID inattendu {vid:04X}")
        if kind not in KINDS:
            fail(f"{name}: type inconnu {kind!r}")
        rows, cols = (int(x) for x in grid.split("x"))
        devices.append({
            "name": name, "pid": pid, "kind": KINDS[kind], "matrix": SUPPORTED_MATRICES[matrix],
            "txid": int(txid, 16), "led": int(led, 16), "rows": rows, "cols": cols,
            "zones": parse_zones(zones, pid), "hid": parse_selectors(hid, pid),
        })
    pids = [d["pid"] for d in devices]
    if len(pids) != len(set(pids)):
        dup = sorted({p for p in pids if pids.count(p) > 1})
        fail(f"PID en double : {[hex(p) for p in dup]}")
    return devices


def parse_layouts(lines):
    layouts = []
    in_section = False
    laptop_iso = False
    current = None
    for line in lines:
        if line.startswith("## 10."):
            in_section = True
            continue
        if line.startswith("## 11."):
            break
        if not in_section:
            continue
        if line.startswith("### Variante ISO des portables"):
            laptop_iso = True
            continue
        m = re.match(r"#### Grille (\d+) lignes x (\d+) colonnes", line)
        if m:
            current = {"rows": int(m.group(1)), "cols": int(m.group(2)), "pids": [], "keys": [], "iso": laptop_iso}
            layouts.append(current)
            continue
        if line.startswith("Appareils :") and current is not None:
            current["pids"] = [int(p, 16) for p in re.findall(r"\(0x([0-9A-Fa-f]{4})\)", line)]
            continue
        m = re.match(r"\| (\d+) \| (.*) \|$", line.strip())
        if m and current is not None:
            row = int(m.group(1))
            for col, key in re.findall(r'(\d+):("[^"]*"|\S+)', m.group(2)):
                col = int(col)
                if row >= current["rows"] or col >= current["cols"]:
                    fail(f"touche hors grille : {row}:{col} {key}")
                is_code = not key.startswith('"')
                current["keys"].append((row, col, key.strip('"'), is_code))
    for lay in layouts:
        if not lay["pids"] or not lay["keys"]:
            fail(f"grille sans appareils ou sans touches : {lay['rows']}x{lay['cols']}")
    return layouts


def main():
    lines = SHEET.read_text(encoding="utf-8").splitlines()
    devices = parse_devices(lines)
    layouts = parse_layouts(lines)
    out = [
        "//! Device and keyboard tables generated from `docs/protocols/razer.md` (§9 and §10).",
        "//!",
        "//! DO NOT EDIT: run `python -I tools/protocol-lab/gen_razer_tables.py` after changing the sheet.",
        "",
        "use crate::model::{DeviceDef, DeviceKind, HidSelector, KeyLayout, Matrix, ZoneDef};",
        "use opengear_core::lighting::ZoneKind;",
        "",
        "pub static DEVICES: &[DeviceDef] = &[",
    ]
    for d in devices:
        zones = ", ".join(
            f"ZoneDef {{ name: {rust_str(n)}, kind: ZoneKind::{k}, rows: {r}, cols: {c} }}" for n, k, r, c in d["zones"]
        )
        out.append(
            f"    DeviceDef {{ name: {rust_str(d['name'])}, pid: 0x{d['pid']:04X}, kind: DeviceKind::{d['kind']}, "
            f"matrix: Matrix::{d['matrix']}, tx_id: 0x{d['txid']:02X}, led_id: 0x{d['led']:02X}, "
            f"rows: {d['rows']}, cols: {d['cols']}, zones: &[{zones}], hid: &[{', '.join(d['hid'])}] }},"
        )
    out += ["];", "", "pub static KEY_LAYOUTS: &[KeyLayout] = &["]
    for lay in layouts:
        keys = ", ".join(f"({r}, {c}, {rust_str(k)}, {'true' if code else 'false'})" for r, c, k, code in lay["keys"])
        pids = ", ".join(f"0x{p:04X}" for p in lay["pids"])
        out.append(
            f"    KeyLayout {{ pids: &[{pids}], rows: {lay['rows']}, cols: {lay['cols']}, laptop_iso: {'true' if lay['iso'] else 'false'}, keys: &[{keys}] }},"
        )
    out += ["];", ""]
    OUT.write_text("\n".join(out), encoding="utf-8")
    print(f"{len(devices)} appareils, {len(layouts)} dispositions -> {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
