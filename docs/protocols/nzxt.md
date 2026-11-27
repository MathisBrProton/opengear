# NZXT Kraken & Smart Device USB HID Protocol

Clean-room specification written from public Linux `nzxt-smart2` and `nzxt-kraken3` hardware specs.

## USB IDs (`VID = 0x1E71`)

### Smart Device V2 / RGB & Fan Controller (`3 fan channels`)

| PIDs | Model |
|---|---|
| `0x2006`, `0x200D`, `0x200F` | NZXT Smart Device V2 |
| `0x2009`, `0x200E`, `0x2010`, `0x2011`, `0x2019`, `0x2020` | NZXT RGB & Fan Controller |

- **Output report `0x60`** (64 B): `[0x60, 0x03]` triggers `0x61` fan-detection config input report (`fan_type[0..3]`: `0` none, `1` DC, `2` PWM); `[0x60, 0x02, 0x01, 0xE8, 0x03]` sets 1000 ms telemetry interval.
- **Output report `0x62`** (64 B): `[0x62, 0x01, channel_bitmask, duty_0, duty_1, duty_2, ...]`.
- **Input report `0x67`**: `b[1] == 0x02` speed report with `fan_type` at `b[16..24]`, LE `u16` RPM at `b[24..40]`, `duty_pct` at `b[40..48]`.

### Kraken X53/X63/X73, Z53/Z63/Z73, 2023 & 2024 Elite

| PID | Series | Controllable Channels |
|---|---|---|
| `0x2007`, `0x2014` | Kraken X53 / X63 / X73 | `pump` (coolant sensor + pump RPM) |
| `0x3008` | Kraken Z53 / Z63 / Z73 | `pump`, `fan` |
| `0x300E`, `0x300C`, `0x3012` | Kraken 2023 / Elite / 2024 Elite | `pump`, `fan` (extra `[1, 1]` header flags) |

- **Init**: `[0x70, 0x02, 0x01, 0xB8, 0x01]` (0.5 s report period) + `[0x70, 0x01]`; Z53/2023 status request `[0x74, 0x01]`.
- **Firmware query**: `[0x10, 0x01]` -> reply `0x11` with `b[17..=19]` = `major.minor.patch`.
- **Status report `0x75`**: coolant °C `b[15] + b[16]/10.0`, pump RPM LE `u16` at `b[17..=18]`, pump duty `b[19]`; Z53/2023 fan RPM LE `u16` at `b[23..=24]`, fan duty `b[25]`.
- **Set 40-point coolant curve (20..=59 °C)**: `0x72` + channel ID (`1 = pump`, `2 = fan`, plus `KRAKEN2023` bytes `[b2=1, b3=1 for fan]`) + 40 `u8` duty percentages (last point pinned to `100`% at 59 °C for thermal safety).

## Safety Floors

- AIO pump (`pump`) clamped to `45..=100` %.
- Hub / radiator fans clamped to `30..=100` %.
- `restore_firmware_control()` restores balanced autonomous curves on Kraken or safe 50% baseline on Smart Device V2.
