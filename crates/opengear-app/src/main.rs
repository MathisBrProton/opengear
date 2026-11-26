#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_log;
mod audio_ext;
mod drivers;
mod fan_watchdog;
mod local_assets;
mod selftest;
mod system_events;
mod system_ext;
mod tray_icon;
mod ui;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use opengear_core::driver::{Device, DeviceKind, Value as DriverValue, setting_ids};
use opengear_core::lighting::{EffectRequest, Placement, render_frame};
use opengear_core::{EffectKind, PollingRate, Rgb, RgbEffectConfig, scan_hid_devices};
use opengear_mainboard::{EneRamMode, read_hardware_sensors};
use opengear_platform::{
    TelemetryCollector, is_elevated, list_power_plans, read_display_info, read_laptop_battery,
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
    /// (interne) Dossier cible transmis par OpenGear-Setup avant élévation UAC.
    #[arg(long, hide = true, value_name = "DIR")]
    install_dir: Option<std::path::PathBuf>,
    /// (interne) Compte Windows (DOMAINE\Utilisateur) transmis par OpenGear-Setup.
    #[arg(long, hide = true, value_name = "USER")]
    install_user: Option<String>,
    /// (interne) Marque un lancement déjà passé par le Planificateur de tâches pour éviter toute boucle.
    #[arg(long, hide = true)]
    elevated_entry: bool,
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
        tray_icon::install_to_windows(
            cli.install_dir.as_deref(),
            cli.install_user.as_deref(),
            cli.cleanup_dir.as_deref(),
        )?;
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
    if is_gui
        && !cli.elevated_entry
        && !opengear_platform::is_elevated()
        && tray_icon::try_launch_via_elevated_task(cli.minimized)
    {
        return Ok(());
    }
    if !is_gui {
        attach_parent_console_for_cli();
    }

    // GUI and Selftest log to %LOCALAPPDATA%\OpenGear\logs; other CLI subcommands log to terminal.
    let is_selftest = matches!(cli.command, Some(Commands::Selftest));
    app_log::init(is_gui || is_selftest, is_gui);
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

fn discover_for_driver(driver_name: &str) -> Vec<Box<dyn Device>> {
    drivers::device_drivers()
        .into_iter()
        .filter(|d| d.name() == driver_name)
        .flat_map(|d| d.discover())
        .collect()
}

fn print_device_summary(dev: &mut dyn Device) {
    let i = dev.info();
    let st = dev.status();
    println!(
        "\n[{:?}] {} ({}) — {} | FW={}",
        i.kind,
        i.model,
        i.vendor,
        i.id,
        i.firmware.as_deref().unwrap_or("—")
    );
    if let Some(pct) = st.battery_pct {
        println!(
            "  Batterie       : {pct}% (en charge={:?}, autonomie est.={:?} h)",
            st.charging, st.battery_hours
        );
    }
    for sp in dev.specs() {
        println!("  {:<15}: {}", sp.label, sp.value);
    }
    for s in dev.settings() {
        println!("  [{}] {} = {:?}", s.id, s.label, s.value);
    }
    if let Some(ptr) = dev.pointer().and_then(|p| p.pointer_info()) {
        println!(
            "  Pointeur       : {}x{} DPI (paliers={:?}, actif={:?}, LOD={:?}, polling={:?} Hz)",
            ptr.dpi_x, ptr.dpi_y, ptr.stages, ptr.active_stage, ptr.lod, ptr.report_rate_hz
        );
    }
    if let Some(eq) = dev.equalizer().and_then(|e| e.eq_info()) {
        println!(
            "  Égaliseur      : {} bandes {:?} Hz => {:?} dB",
            eq.band_count, eq.frequencies_hz, eq.gains_db
        );
    }
    if let Some(ob) = dev.onboard() {
        for p in ob.onboard_profiles() {
            println!(
                "  Profil Flash #{} '{}' (actif={}, rom={}, crc={:?}, Hz={:?}, DPIs={:?})",
                p.slot, p.name, p.active, p.from_rom, p.crc_valid, p.report_rate_hz, p.dpi_stages
            );
        }
    }
    if let Some(l) = dev.lighting() {
        for z in l.zones() {
            println!(
                "  Zone RGB       : {} ({}) — {} LEDs",
                z.id,
                z.name,
                z.leds.len()
            );
        }
    }
    if let Some(c) = dev.cooling() {
        for f in c.fans() {
            println!(
                "  Ventilateur    : {} ({}) = {:?} tr/min (min {}%, manuel={:?})",
                f.id, f.name, f.rpm, f.min_pct, f.manual_pct
            );
        }
    }
    for line in dev.diagnostic_lines() {
        println!("  [diag] {line}");
    }
}

fn cmd_hp_probe() -> Result<()> {
    let mut tel = TelemetryCollector::new();
    std::thread::sleep(Duration::from_millis(200));
    let sample = tel.sample();
    let s = &tel.static_info;
    let bat = read_laptop_battery();
    let disp = read_display_info();
    let mut hp_devs = discover_for_driver("hp_omen");

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
    if hp_devs.is_empty() {
        println!("HP OMEN WMI : aucun contrôleur détecté (ou droits admin requis).");
    } else {
        for dev in &mut hp_devs {
            print_device_summary(dev.as_mut());
        }
    }
    Ok(())
}

fn cmd_corsair(action: CorsairCmd) -> Result<()> {
    if let CorsairCmd::Info = action {
        let mut devices = discover_for_driver("corsair");
        if devices.is_empty() {
            println!("Aucun périphérique Corsair Bragi v2 détecté.");
            return Ok(());
        }
        println!(
            "=== Corsair Bragi v2 Multi-Device ({} détecté(s)) ===",
            devices.len()
        );
        for dev in &mut devices {
            print_device_summary(dev.as_mut());
        }
        return Ok(());
    }

    if let CorsairCmd::HwpDump = action {
        let dump = drivers::dump_corsair_hwp()?;
        println!("{dump}");
        return Ok(());
    }

    let mut devices = discover_for_driver("corsair");
    let kb = devices
        .iter_mut()
        .find(|d| d.info().kind == DeviceKind::Keyboard)
        .context("Aucun clavier Corsair Bragi détecté")?;

    match action {
        CorsairCmd::Info | CorsairCmd::HwpDump => unreachable!(),
        CorsairCmd::Pollrate { hz } => match hz {
            None => {
                let s = kb
                    .settings()
                    .into_iter()
                    .find(|s| s.id == setting_ids::POLLING_RATE)
                    .context("Polling rate indisponible")?;
                println!("Fréquence actuelle : {:?}", s.value);
            }
            Some(target_hz) => {
                let rate = PollingRate::from_hz(target_hz)
                    .context("Fréquence invalide (125, 250, 500, 1000, 2000, 4000, 8000)")?;
                kb.apply_setting(
                    setting_ids::POLLING_RATE,
                    &DriverValue::Text(rate.hz().to_string()),
                )?;
                println!("Fréquence appliquée : {rate}");
            }
        },
        CorsairCmd::Brightness { percent } => {
            kb.apply_setting(
                setting_ids::HW_BRIGHTNESS,
                &DriverValue::Number(f64::from(percent.min(100))),
            )?;
            println!("Luminosité matérielle réglée sur {}%", percent.min(100));
        }
        CorsairCmd::Mode { mode } => {
            let target = match mode.to_ascii_lowercase().as_str() {
                "hw" | "hardware" => "hw",
                "sw" | "software" => "sw",
                _ => bail!("Mode invalide (attendu: hw ou sw)"),
            };
            if let Some(lighting) = kb.lighting() {
                if target == "hw" {
                    lighting.release()?;
                } else {
                    let zones = lighting.zones();
                    let colors = render_frame(
                        &zones,
                        &Placement::default(),
                        &RgbEffectConfig::default(),
                        0.0,
                    );
                    lighting.set_frame(&colors)?;
                }
            }
            println!("Mode basculé sur : {target}");
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
            let lighting = kb.lighting().context("Éclairage indisponible")?;
            let zones = lighting.zones();
            if seconds == 0 {
                let colors = render_frame(&zones, &Placement::default(), &cfg, 0.0);
                lighting.set_frame(&colors)?;
            } else {
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(seconds) {
                    let colors = render_frame(
                        &zones,
                        &Placement::default(),
                        &cfg,
                        start.elapsed().as_secs_f32(),
                    );
                    lighting.set_frame(&colors)?;
                    std::thread::sleep(Duration::from_millis(33));
                }
                let _ = lighting.release();
            }
        }
    }
    Ok(())
}

fn cmd_asus(action: AsusCmd) -> Result<()> {
    match action {
        AsusCmd::RamScan => {
            let st = drivers::ram_rgb().probe();
            println!("=== RAM DDR RGB (contrôleurs ENE) ===");
            println!(
                "Admin UAC      : {}",
                if st.is_admin { "OUI" } else { "NON" }
            );
            println!(
                "Bus SMBus      : {}",
                if st.bus_available {
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
                    "  addr=0x{:02X} ({} / bus 0x{:02X}) | FW={} | {} | {} LEDs | mode={}",
                    m.address_7bit,
                    m.backend,
                    m.bus_arg,
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
            let st = drivers::ram_rgb().probe();
            let rgb = Rgb::from_hex(&color).context("Couleur hex invalide")?;
            let m = EneRamMode::from_str_id(&mode);
            let effect = opengear_mainboard::RamEffect {
                mode: m,
                color: rgb,
                speed,
                reverse: false,
                save_eeprom: save,
            };
            let res = drivers::ram_rgb().apply(&st.ene_modules, None, &effect)?;
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

    let mut aura_devs = discover_for_driver("asus_aura");
    let dev = aura_devs
        .first_mut()
        .context("Contrôleur ASUS Aura USB introuvable")?;
    match action {
        AsusCmd::RamScan | AsusCmd::RamRgb { .. } => unreachable!(),
        AsusCmd::Info => {
            println!("=== ASUS AURA Mainboard Info ===");
            print_device_summary(dev.as_mut());
        }
        AsusCmd::Mode {
            mode,
            color,
            shutdown,
            commit,
        } => {
            let rgb = Rgb::from_hex(&color).context("Couleur hex invalide")?;
            let lighting = dev.lighting().context("Éclairage Aura indisponible")?;
            let matched = lighting
                .hardware_effects()
                .into_iter()
                .find(|e| e.id.eq_ignore_ascii_case(&mode))
                .with_context(|| format!("Mode Aura inconnu : {mode}"))?;
            let req = EffectRequest {
                effect: matched.id.clone(),
                zone: None,
                colors: vec![rgb],
                speed: None,
                brightness: None,
                reverse: shutdown,
                save: commit,
            };
            lighting.apply_hardware_effect(&req)?;
            println!(
                "ASUS AURA appliqué : {} ({}, commit={})",
                matched.label,
                rgb.to_hex(),
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
            let lighting = dev.lighting().context("Éclairage Aura indisponible")?;
            let zones = lighting.zones();
            let start = Instant::now();
            println!("Streaming Direct ASUS AURA '{effect}' pendant {seconds}s...");
            while start.elapsed() < Duration::from_secs(seconds) {
                let colors = render_frame(
                    &zones,
                    &Placement::default(),
                    &cfg,
                    start.elapsed().as_secs_f32(),
                );
                lighting.set_frame(&colors)?;
                std::thread::sleep(Duration::from_millis(33));
            }
            let _ = lighting.release();
            println!("Terminé.");
        }
    }
    Ok(())
}

fn cmd_logitech(action: LogitechCmd) -> Result<()> {
    let mut devices = discover_for_driver("logitech");
    match action {
        LogitechCmd::Info => {
            let assets = local_assets::LocalAssetCatalog::discover();
            println!("=== Logitech HID++ ({} détecté(s)) ===", devices.len());
            for dev in &mut devices {
                print_device_summary(dev.as_mut());
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
            let m = devices
                .iter_mut()
                .find(|d| d.info().kind == DeviceKind::Mouse)
                .context("Aucune souris Logitech détectée")?;
            let ptr = m.pointer().context("Réglage DPI indisponible")?;
            let lod_norm = lod
                .as_deref()
                .map(|s| match s.to_ascii_lowercase().as_str() {
                    "low" => "Low",
                    "high" => "High",
                    _ => "Medium",
                });
            ptr.set_dpi_with_lod(x, y, lod_norm)?;
            println!(
                "DPI souris appliqué : X={x}, Y={}, LOD={:?}",
                y.unwrap_or(x),
                lod_norm
            );
        }
        LogitechCmd::Pollrate { hz } => {
            let m = devices
                .iter_mut()
                .find(|d| d.info().kind == DeviceKind::Mouse)
                .context("Aucune souris Logitech détectée")?;
            m.apply_setting(
                setting_ids::POLLING_RATE,
                &DriverValue::Text(hz.to_string()),
            )?;
            println!("Polling rate souris appliqué : {hz} Hz");
        }
        LogitechCmd::Mode { mode } => {
            let m = devices
                .iter_mut()
                .find(|d| d.info().kind == DeviceKind::Mouse)
                .context("Aucune souris Logitech détectée")?;
            let val = match mode.to_ascii_lowercase().as_str() {
                "onboard" | "hw" => "onboard",
                "host" | "sw" => "host",
                _ => bail!("Mode invalide (onboard ou host)"),
            };
            m.apply_setting("logitech.onboard_mode", &DriverValue::Text(val.into()))?;
            println!("Mode souris basculé sur : {val}");
        }
        LogitechCmd::Sidetone { percent } => {
            let h = devices
                .iter_mut()
                .find(|d| d.info().kind == DeviceKind::Headset)
                .context("Aucun casque Logitech détecté")?;
            h.apply_setting(
                setting_ids::SIDETONE,
                &DriverValue::Number(f64::from(percent.min(100))),
            )?;
            println!("Sidetone casque réglé sur {}%", percent.min(100));
        }
        LogitechCmd::Eq { bands } => {
            let h = devices
                .iter_mut()
                .find(|d| d.info().kind == DeviceKind::Headset)
                .context("Aucun casque Logitech détecté")?;
            h.equalizer()
                .context("Égaliseur indisponible")?
                .set_eq_gains(&bands)?;
            println!("Égaliseur matériel casque appliqué : {:?} dB", bands);
        }
    }
    Ok(())
}

fn cmd_cooling(action: CoolingCmd) -> Result<()> {
    match action {
        CoolingCmd::Status => {
            let mut devs = discover_for_driver("asus_wmi_cooling");
            let (wmi_available, message) = devs
                .iter_mut()
                .find_map(|d| d.fan_curves().map(|fc| fc.cooling_diagnostic()))
                .unwrap_or_else(|| {
                    (
                        false,
                        if is_elevated() {
                            "Aucun contrôleur de courbes carte mère détecté.".into()
                        } else {
                            "Droits administrateur requis pour l'interface WMI carte mère.".into()
                        },
                    )
                });
            let sensors = read_hardware_sensors();
            println!("=== OpenGear Cooling — Courbes Carte Mère ===");
            println!("Droits Admin   : {}", is_elevated());
            println!("WMI Disponible : {wmi_available}");
            println!("Statut         : {message}");
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
