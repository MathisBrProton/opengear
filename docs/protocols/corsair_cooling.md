# Corsair Commander Pro / 1000D USB HID Cooling Protocol

Clean-room specification written from public Linux `corsair-cpro` documentation before implementation.

## USB IDs (`VID = 0x1B1C`)

| PID | Device | Fan Channels | Thermistors |
|---|---|---|---|
| `0x0C10` | Corsair Commander Pro | 6 | 4 |
| `0x1D00` | Corsair Obsidian 1000D Fan Hub | 6 | 4 |

## Framing

Unnumbered 63-byte HID output reports (64 bytes on Windows `hidapi` with leading `0x00` Report ID byte).
Input replies are 16 bytes (leading byte `status`: `0x00 = OK`, `0x01 = unsupported`, `0x10 = invalid arg`, `0x11 = no sensor`, `0x12 = no manual PWM`).

## Commands

| Byte 0 | Name | Args | Reply |
|---|---|---|---|
| `0x02` | `CTL_GET_FW_VER` | — | `b[1..=3]` = `major.minor.patch` |
| `0x10` | `CTL_GET_TMP_CNCT` | — | `b[1..=4]` = `1` if thermistor connected |
| `0x11` | `CTL_GET_TMP` | `b[1] = ch (0..3)` | BE `i16` in `b[1..=2]` (centi-°C) |
| `0x20` | `CTL_GET_FAN_CNCT` | — | `b[1..=6]` = `0` none, `1` 3-pin DC, `2` 4-pin PWM |
| `0x21` | `CTL_GET_FAN_RPM` | `b[1] = ch (0..5)` | BE `u16` in `b[1..=2]` (RPM) |
| `0x22` | `CTL_GET_FAN_PWM` | `b[1] = ch (0..5)` | `b[1]` = manual PWM % (returns `0x12` in curve/RPM mode) |
| `0x23` | `CTL_SET_FAN_FPWM` | `b[1] = ch`, `b[2] = pct (0..=100)` | `b[0] = 0x00` |
| `0x25` | `CTL_SET_FAN_CURVE` | `b[1] = ch`, `b[2] = temp_ch`, `b[3..15]` 6x BE centi-°C, `b[15..27]` 6x BE RPM | Restores hardware curve mode |

## Safety Floors

- Manual duty clamped to `30..=100` %.
- `restore_firmware_control()` writes a monotonic balanced 6-point hardware curve on every active channel so autonomous firmware control resumes even after crash recovery.
