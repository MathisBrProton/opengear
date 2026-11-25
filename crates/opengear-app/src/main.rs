#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_log;
mod audio_ext;
mod fan_watchdog;
mod local_assets;
mod selftest;
mod system_events;
mod system_ext;
mod tray_icon;
mod ui;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use opengear_asus::{
    AuraHwMode, AuraMainboardController, EneRamMode, apply_ram_rgb, probe_asus_wmi_cooling,
    probe_ram_rgb, read_hardware_sensors,
};
use opengear_core::{
    EffectKind, OperatingMode, PollingRate, Rgb, RgbEffectConfig, scan_hid_devices,
};
use opengear_corsair::CorsairDevice;
use opengear_hp::probe_hp_omen;
use opengear_logitech::{
    BatteryPowerModel, LodLevel, LogitechHeadset, LogitechKeyboard, LogitechMouse, OnboardMode,
};
use opengear_platform::{
    TelemetryCollector, list_power_plans, read_display_info, read_laptop_battery,
};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "opengear",
    version,
    about = "OpenGear — Contrôleur matériel unifié open-source en Rust (Corsair, ASUS ROG, Logitech, HP OMEN)"
)]
struct Cli {
    /// Démarrer réduit dans la barre des tâches (System Tray)
    #[arg(long, global = true)]
    minimized: bool,
    /// Installer OpenGear dans Windows (%LOCALAPPDATA%\Programs\OpenGear + Menu Démarrer + Bureau)
    #[arg(long)]
    install: bool,
    /// Désinstaller OpenGear proprement de Windows
    #[arg(long)]
    uninstall: bool,
    /// (interne) Surveille le processus <PID> et rend les ventilateurs au BIOS s'il meurt.
    #[arg(long, hide = true, value_name = "PID")]
    fan_watchdog: Option<u32>,
    /// (interne) Nettoie le dossier temporaire d'extraction du setup après installation.
    #[arg(long, hide = true, value_name = "DIR")]
    cleanup_dir: Option<std::path::PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Lancer l'interface graphique native (par défaut)
    Gui,
    /// Scanner tous les périphériques HID connectés
    Scan {
        #[arg(long)]
        all: bool,
    },
    /// Afficher la télémétrie système en direct (CPU, RAM, GPU AMD ADL2, Power Plans)
    Telemetry,
    /// Contrôler le clavier Corsair (K70 RGB TKL / Bragi v2)
    Corsair {
        #[command(subcommand)]
        action: CorsairCmd,
    },
    /// Contrôler la carte mère ASUS ROG (AURA LED Controller 0B05:19AF)
    Asus {
        #[command(subcommand)]
        action: AsusCmd,
    },
    /// Contrôler les périphériques Logitech G (Souris LIGHTSPEED & Casque PRO X via HID++ 2.0)
    Logitech {
        #[command(subcommand)]
        action: LogitechCmd,
    },
    /// Contrôler la ventilation & watercooling (be quiet! Silent Loop 3 + ASUS WMI)
    Cooling {
        #[command(subcommand)]
        action: CoolingCmd,
    },
    /// Lancer l'auto-test matériel complet (read-back vérifié + rapport %TEMP%\opengear_selftest.txt)
    Selftest,
    /// Sonder en lecture seule un PC portable (GPU NVML/ADL2, batterie, écran, HP OMEN hpqBIntM)
    HpProbe,
}

#[derive(Subcommand)]
enum CorsairCmd {
    Info,
    Pollrate {
        hz: Option<u16>,
    },
    Brightness {
        percent: u8,
    },
    Mode {
        mode: String,
    },
    Rgb {
        #[arg(short, long, default_value = "#00D2FF")]
        color: String,
        #[arg(short, long, default_value = "static")]
        effect: String,
        #[arg(short, long, default_value_t = 5)]
        seconds: u64,
    },
    /// Lire et inspecter le système de fichiers Flash embarqué (HWP) du clavier Corsair
    HwpDump,
}

#[derive(Subcommand)]
enum AsusCmd {
    /// Lire le firmware Aura et la table de configuration (zones CM + headers ARGB Gen2)
    Info,
    /// Appliquer un mode matériel Aura intégré (static, breathing, rainbow, spectrum, wave, redpulse, off)
    Mode {
        mode: String,
        #[arg(short, long, default_value = "#FF1848")]
        color: String,
        /// Appliquer à l'effet "PC éteint" (Shutdown / Stealth)
        #[arg(long)]
        shutdown: bool,
        /// Enregistrer de façon permanente dans la mémoire flash Aura
        #[arg(long)]
        commit: bool,
    },
    /// Tester le streaming RGB direct sur toutes les zones Aura pendant N secondes
    Direct {
        #[arg(short, long, default_value = "#00D2FF")]
        color: String,
        #[arg(short, long, default_value = "rainbow")]
        effect: String,
        /// Durée en secondes
        #[arg(short, long, default_value_t = 4)]
        seconds: u64,
    },
    /// Scanner en lecture seule la RAM DDR5 & les contrôleurs ENE SMBus (via ASUSManagement WMI, Admin)
    RamScan,
    /// Appliquer un effet matériel RGB sur la RAM DDR5 ENE (via ASUSManagement WMI, Admin)
    RamRgb {
        #[arg(short, long, default_value = "static")]
        mode: String,
        #[arg(short, long, default_value = "#00D2FF")]
        color: String,
        /// Vitesse ENE (0 = très rapide .. 4 = très lent)
        #[arg(long, default_value_t = 2)]
        speed: u8,
        /// Sauvegarder de façon permanente dans l'EEPROM de la RAM (0xAA)
        #[arg(long)]
        save: bool,
    },
}

#[derive(Subcommand)]
enum LogitechCmd {
    /// Lire toutes les infos & table complète des features HID++ 2.0 (souris + casque PRO X)
    Info,
    /// Régler le DPI de la souris Logitech (et optionnellement DPI Y + LOD: low/medium/high)
    Dpi {
        x: u16,
        #[arg(long)]
        y: Option<u16>,
        #[arg(long)]
        lod: Option<String>,
    },
    /// Régler le Polling Rate de la souris Logitech (125..8000 Hz selon support)
    Pollrate { hz: u16 },
    /// Basculer mode souris: onboard ou host
    Mode { mode: String },
    /// Régler le sidetone (retour micro 0..100%) du casque Logitech PRO X
    Sidetone { percent: u8 },
    /// Régler les 5 bandes d'égaliseur matériel (dB entre -12 et +12) du casque PRO X
    Eq {
        #[arg(allow_hyphen_values = true, num_args = 5)]
        bands: Vec<i8>,
    },
}

#[derive(Subcommand)]
enum CoolingCmd {
    /// Vérifier le statut de l'interface ASUSManagement WMI (courbes PWM & Silent Loop 3)
    Status,
}

fn attach_parent_console_for_cli() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(parent_pid) = cli.fan_watchdog {
        app_log::init(true, false);
        return fan_watchdog::run_watchdog(parent_pid);
    }
    if cli.install {
        tray_icon::install_to_windows(cli.cleanup_dir.as_deref())?;
        return Ok(());
    }
    if let Some(dir) = cli.cleanup_dir.as_deref() {
        tray_icon::cleanup_installer_temp_dir(dir);
    }
    if cli.uninstall {
        tray_icon::uninstall_from_windows()?;
        return Ok(());
    }

    let is_gui = matches!(cli.command, None | Some(Commands::Gui));
    if !is_gui {
        attach_parent_console_for_cli();
    }

    // GUI has no console: log to %LOCALAPPDATA%\OpenGear\logs. CLI logs to the terminal.
    app_log::init(is_gui, is_gui);
    if is_gui {
        tracing::info!("OpenGear {} démarré", env!("CARGO_PKG_VERSION"));
    }

    match cli.command.unwrap_or(Commands::Gui) {
        Commands::Gui => ui::run_gui(cli.minimized),
        Commands::Scan { all } => cmd_scan(all),
        Commands::Telemetry => cmd_telemetry(),
        Commands::Corsair { action } => cmd_corsair(action),
        Commands::Asus { action } => cmd_asus(action),
        Commands::Logitech { action } => cmd_logitech(action),
        Commands::Cooling { action } => cmd_cooling(action),
        Commands::Selftest => {
            let rep = selftest::run_full_selftest();
            println!(
                "=== OpenGear Self-Test (Admin={}) : {} PASS | {} WARN | {} FAIL | {} SKIP_ADMIN ===",
                rep.is_admin, rep.passed, rep.warned, rep.failed, rep.skipped_admin
            );
            for c in &rep.checks {
                println!("  [{:<10}] {} -> {}", c.status, c.name, c.detail);
            }
            println!("Rapport sauvegardé : {}", rep.report_path);
            Ok(())
        }
        Commands::HpProbe => cmd_hp_probe(),
    }
}

fn cmd_scan(all: bool) -> Result<()> {
    let devices = scan_hid_devices(all)?;
    println!(
        "=== OpenGear HID Scanner ({} interfaces détectées) ===\n",
        devices.len()
    );
    println!(
        "{:<10} {:<11} {:<4} {:<6} {:<6} {:<38} STATUT",
        "MARQUE", "VID:PID", "IF", "UPAGE", "USAGE", "PRODUIT"
    );
    println!("{}", "-".repeat(110));
    for d in devices {
        println!(
            "{:<10} {:04X}:{:04X}   {:<4} {:04X}   {:04X}   {:<38} {}",
            d.vendor.name(),
            d.vid,
            d.pid,
            d.interface_number,
            d.usage_page,
            d.usage,
            truncate(&d.product_name, 37),
            d.driver_status
        );
    }
    Ok(())
}

fn cmd_telemetry() -> Result<()> {
    let mut tel = TelemetryCollector::new();
    std::thread::sleep(Duration::from_millis(250));
    let sample = tel.sample();
    let s = &tel.static_info;
    println!("=== OpenGear Telemetry ===");
    println!("Carte mère : {} (BIOS {})", s.motherboard, s.bios_version);
    println!("CPU        : {} — {:.1}%", s.cpu_name, sample.cpu_usage_pct);
    println!(
        "RAM        : {:.1} / {:.1} Go ({:.0}%)",
        sample.ram_used_gb, sample.ram_total_gb, sample.ram_usage_pct
    );
    println!("GPU        : {}", s.gpu_name);
    if sample.gpu.available {
        println!(
            "  {}     : Edge={:?}°C | Hotspot={:?}°C | Charge={:?}% | Fan={:?} RPM | Core={:?} MHz | Power={:?} W",
            sample.gpu.backend,
            sample.gpu.edge_temp_c,
            sample.gpu.hotspot_temp_c,
            sample.gpu.activity_pct,
            sample.gpu.fan_rpm,
            sample.gpu.core_clock_mhz,
            sample.gpu.power_watts
        );
    } else {
        println!("  GPU      : indisponible");
    }
    println!("\n=== Modes d'alimentation Windows ===");
    for p in list_power_plans() {
        println!(
            "  {} {} ({})",
            if p.active { "*" } else { " " },
            p.name,
            p.guid
        );
    }
    Ok(())
}

fn cmd_hp_probe() -> Result<()> {
    let mut tel = TelemetryCollector::new();
    std::thread::sleep(Duration::from_millis(200));
    let sample = tel.sample();
    let s = &tel.static_info;
    let bat = read_laptop_battery();
    let disp = read_display_info();
    let omen = probe_hp_omen();

    println!("=== OpenGear Laptop / HP OMEN Probe ===");
    println!("Machine     : {} | {}", s.manufacturer, s.model);
    println!("Carte mère  : {} (BIOS {})", s.motherboard, s.bios_version);
    println!(
        "CPU         : {} — {:.1}%",
        s.cpu_name, sample.cpu_usage_pct
    );
    println!(
        "GPU         : {} [{}] | Edge={:?}°C | Charge={:?}% | Core={:?} MHz | Power={:?} W",
        s.gpu_name,
        if sample.gpu.backend.is_empty() {
            "N/A"
        } else {
            &sample.gpu.backend
        },
        sample.gpu.edge_temp_c,
        sample.gpu.activity_pct,
        sample.gpu.core_clock_mhz,
        sample.gpu.power_watts
    );
    if bat.present {
        println!(
            "Batterie    : {:?}% | Santé {:?}% ({:?}/{:?} mWh) | Cycles={:?} | Secteur={} | Tension={:?}V | Puissance={:?}W",
            bat.percent,
            bat.health_pct,
            bat.full_charged_capacity_mwh,
            bat.design_capacity_mwh,
            bat.cycle_count,
            bat.ac_online,
            bat.voltage_v,
            bat.power_watts
        );
    }
    println!(
        "Écran       : {}x{} @ {:?} Hz (supportés: {:?}) | Luminosité={:?}%",
        disp.width, disp.height, disp.current_hz, disp.supported_hz, disp.brightness_pct
    );
    println!(
        "HP OMEN WMI : detected={} admin={} wmi={} thermal_v={} fans={} rpms={:?} kbd={} backlight={}",
        omen.detected,
        omen.is_admin,
        omen.wmi_accessible,
        omen.thermal_version,
        omen.fan_count,
        omen.fan_rpms,
        omen.keyboard_label,
        omen.backlight_on
    );
    println!("Diagnostic  : {}", omen.diagnostic);
    for line in &omen.probe_log {
        println!("  [WMI] {line}");
    }
    Ok(())
}

fn cmd_corsair(action: CorsairCmd) -> Result<()> {
    if let CorsairCmd::Info = action {
        let devices = CorsairDevice::connect_all();
        if devices.is_empty() {
            println!("Aucun périphérique Corsair Bragi v2 détecté.");
            return Ok(());
        }
        println!(
            "=== Corsair Bragi v2 Multi-Device ({} détecté(s)) ===",
            devices.len()
        );
        for dev in &devices {
            let i = &dev.info;
            println!(
                "\n[{}] {} ({:04X}:{:04X}, sub={})",
                i.kind.label(),
                i.product_name,
                i.vid,
                i.pid,
                i.subdevice_id
            );
            println!(
                "  Firmware       : {} (Bootloader {})",
                i.firmware_version, i.bootloader_version
            );
            println!(
                "  Mode / Polling : {} | {:?} (max {:?})",
                i.mode, i.pollrate, i.max_pollrate
            );
            println!(
                "  Luminosité HW  : {}% | Report OUT={}B",
                dev.hw_brightness_pct(),
                i.out_report_len
            );
            if let Some(b) = i.battery_pct {
                println!("  Batterie       : {b}% (status={:?})", i.battery_status);
            }
            if i.hw_winlock.is_some() || i.debounce_enabled.is_some() {
                println!(
                    "  Clavier MCU    : Layout={} | WinLock={:?} (mask={:?}) | Debounce={:?} ({:?}ms) | Esports={:?} | ProfilHW={:?} | Flash={:?}/{:?} Ko",
                    i.layout.label(),
                    i.hw_winlock,
                    i.winlock_shortcuts_mask,
                    i.debounce_enabled,
                    i.debounce_timeout_ms,
                    i.esports_switch_on,
                    i.active_hw_profile,
                    i.storage_free_kb,
                    i.storage_total_kb
                );
                if !i.hwp_profiles.is_empty() {
                    println!("  Profils Flash HWP ({}) :", i.hwp_profiles.len());
                    for hp in &i.hwp_profiles {
                        let light_desc = hp
                            .lighting
                            .as_ref()
                            .map(|l| {
                                let cols: Vec<String> =
                                    l.colors.iter().map(|c| c.to_hex()).collect();
                                format!(
                                    "{} [file=0x{:04X}, {} LEDs, cols={:?}]",
                                    l.kind.label(),
                                    l.file_id,
                                    l.led_indices.len(),
                                    cols
                                )
                            })
                            .unwrap_or_else(|| "Aucun".into());
                        println!(
                            "    #{} '{}' (file=0x{:04X}, {} actions, {} props) -> {}",
                            hp.slot_index + 1,
                            hp.name,
                            hp.profile_file_id,
                            hp.assignment_count,
                            hp.property_count,
                            light_desc
                        );
                    }
                }
            }
            if let Some(m) = &i.mouse {
                println!(
                    "  Souris Bragi   : DPIs={:?} (actif=#{}, live={:?}x{:?}) | LOD={:?} | MotionSync={:?}",
                    m.dpi_stages,
                    m.active_stage,
                    m.current_dpi_x,
                    m.current_dpi_y,
                    m.lift_height,
                    m.motion_sync
                );
            }
            if let Some(h) = &i.headset {
                println!(
                    "  Casque Bragi   : Sidetone={:?}% (muted={:?}) | MicMuted={:?} | EQ={:?} | ANC={:?}",
                    h.sidetone_pct, h.sidetone_muted, h.mic_muted, h.active_eq_preset, h.anc_mode
                );
            }
            if let Some(c) = &i.cooler {
                println!(
                    "  Cooler Bragi   : Line={:?} | Pump={:?} | Radiator={:?}",
                    c.product_line_id, c.pump_version_id, c.radiator_size
                );
            }
        }
        return Ok(());
    }

    if let CorsairCmd::HwpDump = action {
        let mut devices = CorsairDevice::connect_all();
        let Some(dev) = devices.first_mut() else {
            bail!("Aucun périphérique Corsair Bragi détecté");
        };
        println!(
            "=== Dump Flash Bragi v2 (READ-ONLY) : {} ===",
            dev.info.product_name
        );
        dev.with_flash_session(|t| {
            let mut to_inspect: Vec<u16> = Vec::new();
            for &(fid, label) in &[
                (
                    opengear_corsair::proto::FILE_LISTING,
                    "FileListing (0x0011)",
                ),
                (
                    opengear_corsair::proto::FILE_PROFILES_LIST,
                    "ProfilesList (0x000F)",
                ),
            ] {
                match t.read_file(fid) {
                    Ok(bytes) => {
                        print!("\n[{label}] {} octets : ", bytes.len());
                        for b in &bytes {
                            print!("{b:02X} ");
                        }
                        println!();
                        for chunk in bytes.as_chunks::<2>().0 {
                            let id = u16::from_le_bytes([chunk[0], chunk[1]]);
                            if id >= opengear_corsair::proto::FILE_FREE_MIN
                                && !to_inspect.contains(&id)
                            {
                                to_inspect.push(id);
                            }
                        }
                    }
                    Err(e) => println!("[{label}] -> {e}"),
                }
            }
            println!(
                "\n=== Fichiers utilisateurs détectés ({}) ===",
                to_inspect.len()
            );
            for fid in to_inspect {
                match t.read_file(fid) {
                    Ok(bytes) => {
                        let hdr = if bytes.len() >= 2 {
                            u16::from_le_bytes([bytes[0], bytes[1]])
                        } else {
                            0
                        };
                        println!(
                            "\n  File 0x{fid:04X} ({} B, magic=0x{hdr:04X}):",
                            bytes.len()
                        );
                        for (row, chunk) in bytes.chunks(16).enumerate() {
                            print!("    {:04X}: ", row * 16);
                            for b in chunk {
                                print!("{b:02X} ");
                            }
                            print!("  ");
                            for &b in chunk {
                                let ch = if (0x20..=0x7E).contains(&b) {
                                    b as char
                                } else {
                                    '.'
                                };
                                print!("{ch}");
                            }
                            println!();
                        }
                    }
                    Err(e) => println!("  File 0x{fid:04X} -> {e}"),
                }
            }
            Ok(())
        })?;
        return Ok(());
    }

    let devices = CorsairDevice::connect_all();
    let mut kb = devices
        .into_iter()
        .find(|d| d.info.kind == opengear_corsair::CorsairDeviceKind::Keyboard)
        .context("Aucun clavier Corsair Bragi détecté")?;

    match action {
        CorsairCmd::Info | CorsairCmd::HwpDump => unreachable!(),
        CorsairCmd::Pollrate { hz } => {
            kb.set_restore_hw_on_drop(false);
            match hz {
                None => {
                    let current = kb.info.pollrate.context("Polling rate indisponible")?;
                    println!("Fréquence actuelle : {current}");
                    if let Some(max) = kb.info.max_pollrate {
                        println!("Fréquence maximale : {max}");
                    }
                }
                Some(target_hz) => {
                    let rate = PollingRate::from_hz(target_hz)
                        .context("Fréquence invalide (125, 250, 500, 1000, 2000, 4000, 8000)")?;
                    kb.set_pollrate(rate)?;
                    println!("Fréquence appliquée : {rate}");
                }
            }
        }
        CorsairCmd::Brightness { percent } => {
            kb.set_restore_hw_on_drop(false);
            kb.set_hw_brightness_pct(percent)?;
            println!("Luminosité matérielle réglée sur {}%", percent.min(100));
        }
        CorsairCmd::Mode { mode } => {
            kb.set_restore_hw_on_drop(false);
            let m = match mode.to_ascii_lowercase().as_str() {
                "hw" | "hardware" => OperatingMode::Hardware,
                "sw" | "software" => OperatingMode::Software,
                _ => bail!("Mode invalide (attendu: hw ou sw)"),
            };
            kb.set_mode(m)?;
            println!("Mode basculé sur : {m}");
        }
        CorsairCmd::Rgb {
            color,
            effect,
            seconds,
        } => {
            let primary = Rgb::from_hex(&color).context("Couleur hex invalide")?;
            let kind = parse_effect_kind(&effect)?;
            let cfg = RgbEffectConfig {
                kind,
                primary,
                ..RgbEffectConfig::default()
            };

            if seconds == 0 {
                kb.set_restore_hw_on_drop(false);
                kb.render_effect(&cfg, 0.0)?;
            } else {
                kb.set_restore_hw_on_drop(true);
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(seconds) {
                    kb.render_effect(&cfg, start.elapsed().as_secs_f32())?;
                    std::thread::sleep(Duration::from_millis(33));
                }
            }
        }
    }
    Ok(())
}

fn cmd_asus(action: AsusCmd) -> Result<()> {
    match action {
        AsusCmd::RamScan => {
            let st = probe_ram_rgb();
            println!("=== RAM DDR5 & ENE SMBus (ASUSManagement WMI) ===");
            println!(
                "Admin UAC      : {}",
                if st.is_admin { "OUI" } else { "NON" }
            );
            println!(
                "WMI SMBus      : {}",
                if st.wmi_smbus_available {
                    "Actif"
                } else {
                    "Indisponible"
                }
            );
            println!("Barrettes DDR  : {}", st.physical_dimms.len());
            for d in &st.physical_dimms {
                println!(
                    "  [{}] {} {} — {} Go {} @ {} MT/s",
                    d.slot,
                    d.manufacturer,
                    d.part_number,
                    d.capacity_gb,
                    d.memory_type,
                    d.speed_mts
                );
            }
            println!("Contrôleurs ENE: {}", st.ene_modules.len());
            for m in &st.ene_modules {
                println!(
                    "  addr=0x{:02X} (wmi_slave=0x{:02X}) | FW={} | {} | {} LEDs | mode={}",
                    m.address_7bit,
                    m.wmi_slave_arg,
                    m.firmware,
                    if m.is_v2 { "V2" } else { "V1" },
                    m.led_count,
                    m.current_mode
                );
            }
            println!("Statut         : {}", st.message);
            return Ok(());
        }
        AsusCmd::RamRgb {
            mode,
            color,
            speed,
            save,
        } => {
            let st = probe_ram_rgb();
            let rgb = Rgb::from_hex(&color).context("Couleur hex invalide")?;
            let m = EneRamMode::from_str_id(&mode);
            let res = apply_ram_rgb(&st.ene_modules, None, m, rgb, speed, false, save)?;
            println!(
                "RAM DDR5 RGB appliqué : {} ({}, speed={}, save_eeprom={}) -> {}",
                m.label(),
                rgb.to_hex(),
                speed,
                save,
                res
            );
            return Ok(());
        }
        _ => {}
    }

    let mut aura = AuraMainboardController::connect()?;
    match action {
        AsusCmd::RamScan | AsusCmd::RamRgb { .. } => unreachable!(),
        AsusCmd::Info => {
            let i = &aura.info;
            println!("=== ASUS AURA Mainboard Info ===");
            println!(
                "Contrôleur     : {} ({:04X}:{:04X})",
                i.product_name, i.vid, i.pid
            );
            println!("Firmware Aura  : {}", i.firmware_version);
            println!("LEDs CM fixes  : {}", i.mainboard_fixed_leds);
            println!("Headers 12V RGB: {}", i.rgb_12v_headers);
            println!("Headers 5V ARGB: {}", i.argb_5v_headers);
            println!("Config table   : {}", i.config_table_hex);
            println!("Canaux détectés:");
            for ch in &i.channels {
                println!(
                    "  [{}] {} — kind={:?}, eff_ch={}, direct_ch={}, leds={}",
                    ch.index, ch.name, ch.kind, ch.effect_channel, ch.direct_channel, ch.led_count
                );
            }
        }
        AsusCmd::Mode {
            mode,
            color,
            shutdown,
            commit,
        } => {
            let rgb = Rgb::from_hex(&color).context("Couleur hex invalide")?;
            let hw_mode = match mode.to_ascii_lowercase().as_str() {
                "off" => AuraHwMode::Off,
                "static" => AuraHwMode::Static,
                "breathing" => AuraHwMode::Breathing,
                "flashing" => AuraHwMode::Flashing,
                "spectrum" => AuraHwMode::SpectrumCycle,
                "rainbow" => AuraHwMode::Rainbow,
                "wave" => AuraHwMode::Wave,
                "chase" => AuraHwMode::Chase,
                "comet" => AuraHwMode::ChaseFade,
                "redpulse" => AuraHwMode::RedPulse,
                _ => bail!(
                    "Mode Aura inconnu (off, static, breathing, flashing, spectrum, rainbow, wave, chase, comet, redpulse)"
                ),
            };
            aura.set_all_hw_mode(hw_mode, rgb, shutdown)?;
            if commit {
                aura.commit()?;
            }
            println!(
                "ASUS AURA appliqué : {} ({}, shutdown={}, commit={})",
                hw_mode.label(),
                rgb.to_hex(),
                shutdown,
                commit
            );
        }
        AsusCmd::Direct {
            color,
            effect,
            seconds,
        } => {
            let primary = Rgb::from_hex(&color).context("Couleur hex invalide")?;
            let kind = parse_effect_kind(&effect)?;
            let cfg = RgbEffectConfig {
                kind,
                primary,
                ..RgbEffectConfig::default()
            };
            aura.enter_direct_mode()?;
            let start = Instant::now();
            println!("Streaming Direct ASUS AURA '{effect}' pendant {seconds}s...");
            while start.elapsed() < Duration::from_secs(seconds) {
                let t = start.elapsed().as_secs_f32();
                let ch_counts: Vec<usize> =
                    aura.info.channels.iter().map(|c| c.led_count).collect();
                for (ch_idx, count) in ch_counts.into_iter().enumerate() {
                    let leds: Vec<Rgb> = (0..count)
                        .map(|i| {
                            let nx = if count <= 1 {
                                0.5
                            } else {
                                i as f32 / (count - 1) as f32
                            };
                            cfg.sample(i, nx, 0.5, t)
                        })
                        .collect();
                    aura.send_channel_direct(ch_idx, &leds)?;
                }
                std::thread::sleep(Duration::from_millis(33));
            }
            println!("Terminé.");
        }
    }
    Ok(())
}

fn cmd_logitech(action: LogitechCmd) -> Result<()> {
    match action {
        LogitechCmd::Info => {
            let assets = local_assets::LocalAssetCatalog::discover();
            let mice = LogitechMouse::connect_all();
            println!(
                "=== Logitech Souris / LIGHTSPEED ({} détectée(s)) ===",
                mice.len()
            );
            for m in &mice {
                let i = &m.info;
                println!(
                    "Modèle       : {} ({:04X}:{:04X}, slot 0x{:02X})",
                    i.name, i.vid, i.pid, i.device_index
                );
                println!("HID++        : v{}", i.proto_version);
                if !i.firmware.is_empty() {
                    println!("Firmware     : {}", i.firmware.join(" | "));
                }
                if let Some(b) = &i.battery {
                    let hz = i.report_rate.as_ref().map(|r| r.current_hz).unwrap_or(1000);
                    let est = b
                        .percent
                        .map(|p| {
                            let pm = assets
                                .find_battery_model(i.pid, &i.name)
                                .map(|bm| {
                                    BatteryPowerModel::from_params(
                                        p,
                                        bm.capacity_mwh,
                                        bm.sensor_mw,
                                        bm.mcu_mw,
                                        bm.radio_mw_for_hz(hz),
                                    )
                                })
                                .unwrap_or_else(|| BatteryPowerModel::for_pro_x_wireless(p, hz));
                            format!(
                                " | ~{:.1}h restantes ({:.1} mW @ {hz}Hz)",
                                pm.estimated_hours, pm.total_mw
                            )
                        })
                        .unwrap_or_default();
                    println!(
                        "Batterie     : {:?}% ({}, {:?} mV){est}",
                        b.percent,
                        b.state.label(),
                        b.voltage_mv
                    );
                }
                if let Some(d) = &i.dpi {
                    println!(
                        "Capteur DPI  : X={} Y={} (min={}, max={}, step={}, ext={}, XY_indep={}, LOD={:?})",
                        d.dpi_x,
                        d.dpi_y,
                        d.min_dpi,
                        d.max_dpi,
                        d.step_dpi,
                        d.extended,
                        d.supports_independent_xy,
                        d.lod
                    );
                }
                if let Some(r) = &i.report_rate {
                    println!(
                        "Polling Rate : {} Hz (supportés: {:?} Hz, ext={})",
                        r.current_hz, r.supported_hz, r.extended
                    );
                }
                if let Some(o) = &i.onboard {
                    println!(
                        "Mémoire Emb. : {} (profils={}, boutons={}, actif=#{}, stage_dpi=#{})",
                        o.mode.label(),
                        o.profile_count,
                        o.button_count,
                        o.active_profile,
                        o.active_dpi_index
                    );
                }
                if !i.onboard_profiles.is_empty() {
                    println!("Profils embarqués ({}) :", i.onboard_profiles.len());
                    for p in &i.onboard_profiles {
                        let btns: Vec<String> = p.buttons.iter().map(|b| b.label()).collect();
                        println!(
                            "  #{} '{}' [sec=0x{:04X}, {}, enabled={}, crc_ok={}] {} Hz | DPIs={:?} (def=#{}) | Boutons=[{}]",
                            p.slot_index,
                            p.name,
                            p.sector_addr,
                            if p.from_rom { "ROM" } else { "FLASH" },
                            p.enabled,
                            p.crc_valid,
                            p.report_rate_hz,
                            p.dpi_stages,
                            p.default_dpi_idx,
                            btns.join(", ")
                        );
                    }
                }
                println!("Features HID++: {} détectées", i.features.len());
            }

            let kbs = LogitechKeyboard::connect_all();
            if !kbs.is_empty() {
                println!("\n=== Logitech Claviers ({} détecté(s)) ===", kbs.len());
                for k in &kbs {
                    println!(
                        "  {} ({:04X}:{:04X}, slot 0x{:02X}) | RGB={} | Luminosité={:?}%",
                        k.info.name,
                        k.info.vid,
                        k.info.pid,
                        k.info.device_index,
                        k.info.supports_rgb_effects,
                        k.info.brightness_pct
                    );
                }
            }

            let headsets = LogitechHeadset::connect_all();
            println!(
                "\n=== Logitech Casques Audio ({} détecté(s)) ===",
                headsets.len()
            );
            for h in &headsets {
                let i = &h.info;
                println!("Modèle       : {} ({:04X}:{:04X})", i.name, i.vid, i.pid);
                println!(
                    "HID++        : v{} | FW: {}",
                    i.proto_version,
                    i.firmware.join(" | ")
                );
                println!(
                    "Sidetone     : supporté={} (actuel={}%)",
                    i.supports_sidetone, i.sidetone_pct
                );
                if let Some(eq) = &i.equalizer {
                    println!(
                        "Égaliseur    : {} bandes ({}..{} dB) freqs={:?} gains={:?}",
                        eq.band_count, eq.min_db, eq.max_db, eq.frequencies_hz, eq.gains_db
                    );
                }
            }
            println!(
                "\nAssets locaux: G HUB Depot={} ({} modèles battery.xml, {} variantes souris, {} variantes casque) | iCUE 5 RCC={} ({} images K70)",
                assets.lghub_depot.is_some(),
                assets.battery_models.len(),
                assets.mouse_variants.len(),
                assets.headset_variants.len(),
                assets.icue_devices_dir.is_some(),
                assets.k70_images.len(),
            );
        }
        LogitechCmd::Dpi { x, y, lod } => {
            let mut m = LogitechMouse::connect()?;
            let lod_val = lod
                .as_deref()
                .map(|s| match s.to_ascii_lowercase().as_str() {
                    "low" => LodLevel::Low,
                    "high" => LodLevel::High,
                    _ => LodLevel::Medium,
                });
            m.set_dpi(x, y, lod_val)?;
            println!(
                "DPI souris appliqué : X={x}, Y={}, LOD={:?}",
                y.unwrap_or(x),
                lod_val
            );
        }
        LogitechCmd::Pollrate { hz } => {
            let mut m = LogitechMouse::connect()?;
            m.set_report_rate_hz(hz)?;
            println!("Polling rate souris appliqué : {hz} Hz");
        }
        LogitechCmd::Mode { mode } => {
            let mut m = LogitechMouse::connect()?;
            let om = match mode.to_ascii_lowercase().as_str() {
                "onboard" | "hw" => OnboardMode::Onboard,
                "host" | "sw" => OnboardMode::Host,
                _ => bail!("Mode invalide (onboard ou host)"),
            };
            m.set_onboard_mode(om)?;
            println!("Mode souris basculé sur : {}", om.label());
        }
        LogitechCmd::Sidetone { percent } => {
            let mut h = LogitechHeadset::connect()?;
            h.set_sidetone(percent)?;
            println!("Sidetone casque réglé sur {}%", percent.min(100));
        }
        LogitechCmd::Eq { bands } => {
            let mut h = LogitechHeadset::connect()?;
            h.set_equalizer_gains(&bands)?;
            println!("Égaliseur matériel casque appliqué : {:?} dB", bands);
        }
    }
    Ok(())
}

fn cmd_cooling(action: CoolingCmd) -> Result<()> {
    match action {
        CoolingCmd::Status => {
            let st = probe_asus_wmi_cooling();
            let sensors = read_hardware_sensors();
            println!("=== OpenGear Cooling — be quiet! Silent Loop 3 + ASUS WMI ===");
            println!("Droits Admin   : {}", st.is_admin);
            println!("WMI Disponible : {}", st.wmi_available);
            println!("Statut         : {}", st.message);
            println!(
                "Capteurs Live  : {} (disponible={})",
                sensors.source, sensors.available
            );
            if sensors.available {
                println!("  CPU Temp     : {:?} °C", sensors.cpu_temp_c);
                println!("  AIO Pump RPM : {:?}", sensors.aio_pump_rpm);
                println!("  CPU Fan RPM  : {:?}", sensors.cpu_fan_rpm);
                for f in &sensors.fans {
                    println!("  Fan          : {} = {} RPM", f.name, f.rpm);
                }
            }
        }
    }
    Ok(())
}

fn parse_effect_kind(effect: &str) -> Result<EffectKind> {
    match effect.to_ascii_lowercase().as_str() {
        "static" => Ok(EffectKind::Static),
        "rainbow" => Ok(EffectKind::RainbowWave),
        "spectrum" => Ok(EffectKind::SpectrumCycle),
        "breathing" => Ok(EffectKind::Breathing),
        "gradient" => Ok(EffectKind::Gradient),
        "off" => Ok(EffectKind::Off),
        _ => bail!("Effet inconnu (static, rainbow, spectrum, breathing, gradient, off)"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
