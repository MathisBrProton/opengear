// Mock WebView IPC bridge for offline UI review (`tools/ui-harness/serve.py`).
(() => {
  const qs = new URLSearchParams(location.search);
  const scen = qs.get("scen") || "normal";
  const lang = qs.get("lang") || "fr";
  const mode = qs.get("mode") || "expert";

  const makeCurve = () => ([
    { temp_c: 30, duty_pct: 35 }, { temp_c: 40, duty_pct: 40 },
    { temp_c: 50, duty_pct: 50 }, { temp_c: 60, duty_pct: 62 },
    { temp_c: 70, duty_pct: 75 }, { temp_c: 78, duty_pct: 85 },
    { temp_c: 85, duty_pct: 95 }, { temp_c: 92, duty_pct: 100 },
  ]);

  const MOCK_DEVICES = scen === "empty" ? [] : [
    {
      info: { id: "corsair:k70", vendor: "Corsair", model: "CORSAIR K70 RGB TKL", kind: "keyboard", connection: "usb", firmware: "1.8.25" },
      status: { connected: true, battery_pct: null, charging: null, battery_hours: null },
      settings: [
        { id: "polling_rate", section: "performance", label: "Fréquence d'envoi", value: "1000", level: "simple", persistence: "device", help: "Plus haut = moins de latence.", control: { type: "choice", options: [{ id: "1000", label: "1000 Hz" }, { id: "4000", label: "4000 Hz" }, { id: "8000", label: "8000 Hz" }] } },
        { id: "hw_brightness", section: "lighting", label: "Luminosité matérielle", value: 80, level: "simple", persistence: "device", control: { type: "slider", min: 0, max: 100, step: 5, unit: "%" } },
        { id: "win_lock", section: "input", label: "Bloquer Windows", value: false, level: "simple", persistence: "device", control: { type: "toggle" } },
        { id: "hw_socd", section: "input", label: "FlashTap (SOCD matériel)", value: false, level: "expert", persistence: "device", control: { type: "toggle" } },
      ],
      specs: [{ label: "Disposition", value: "ISO (lu dans le clavier)", expert: false, mono: false }],
      lighting: {
        zones: [{ id: "keys", name: "Touches", leds: [{ x: 0.2, y: 0.5, key_code: "KeyQ" }, { x: 0.3, y: 0.5, key_code: "KeyW" }], resizable: null }],
        hardware_effects: [{ id: "Static", label: "Statique", zones: ["keys"], params: { colors: [1, 1], speed: null, direction: false, savable: true } }],
        max_fps: 30,
        placement: { x: 0.1, y: 0.45, width: 0.45, height: 0.3, rotation_deg: 0 },
      },
    },
    {
      info: { id: "logitech:prox", vendor: "Logitech", model: "PRO X SUPERLIGHT 2", kind: "mouse", connection: "wireless", firmware: "32.1.14" },
      status: { connected: true, battery_pct: 68, charging: false, battery_hours: 62 },
      settings: [{ id: "polling_rate", section: "performance", label: "Fréquence d'envoi", value: "1000", level: "simple", persistence: "device", control: { type: "choice", options: [{ id: "500", label: "500 Hz" }, { id: "1000", label: "1000 Hz" }, { id: "2000", label: "2000 Hz" }] } }],
      specs: [{ label: "Capteur", value: "HERO 2", expert: false, mono: false }],
      pointer: { dpi_x: 1600, dpi_y: 1600, min_dpi: 100, max_dpi: 32000, step_dpi: 50, independent_xy: true, stages: [400, 800, 1600, 3200], active_stage: 2, report_rate_hz: 1000, supports_lod: true, lod: "Medium" },
      onboard: {
        info: { writable: true, active_profile: 1, button_count: 5, macros_supported: true },
        profiles: [{ slot: 1, name: "Profil 1", report_rate_hz: 1000, default_dpi_idx: 2, dpi_stages: [400, 800, 1600, 3200], sector_addr: "0x0100", from_rom: false, crc_valid: true, buttons: [{ hex: "010001", label: "Clic gauche" }, { hex: "010002", label: "Clic droit" }, { hex: "010003", label: "Clic molette" }, { hex: "010004", label: "Précédent" }, { hex: "010005", label: "Suivant" }] }],
        button_presets: [{ hex: "010001", label: "Clic gauche" }, { hex: "010002", label: "Clic droit" }, { hex: "010003", label: "Clic molette" }, { hex: "010004", label: "Précédent" }, { hex: "010005", label: "Suivant" }],
        backups: [],
      },
    },
    {
      info: { id: "asus_wmi_cooling:mainboard", vendor: "ASUS", model: "Carte mère ASUS (Q-Fan WMI)", kind: "motherboard", connection: "internal", firmware: null },
      status: { connected: true, battery_pct: null, charging: null, battery_hours: null },
      settings: [],
      specs: [{ label: "Interface BIOS", value: "ASUSManagement WMI active", expert: false, mono: false }],
      curves: {
        is_admin: true,
        available: true,
        message: "Interface BIOS/EC ASUSManagement WMI active.",
        channels: [
          { index: 0, header: "CpuFan", label: "Ventilateur CPU", short_label: "CPU_FAN", is_pump: false, policy: "Standard", source: "CpuBios", hysteresis_c: 3, smoothing_secs: 0, curve: makeCurve() },
          { index: 1, header: "AioPump", label: "Pompe AIO", short_label: "AIO_PUMP", is_pump: true, policy: "Manual", source: "MaxCpuGpu", hysteresis_c: 3, smoothing_secs: 2, curve: makeCurve() },
        ],
      },
    },
    {
      info: { id: "superio:mainboard", vendor: "Carte mère", model: "SuperIO NCT6798D", kind: "motherboard", connection: "internal", firmware: "0xD428" },
      status: { connected: true, battery_pct: null, charging: null, battery_hours: null },
      settings: [],
      specs: [{ label: "Puce de contrôle", value: "NCT6798D (0xD428)", expert: true, mono: true }],
      cooling: { overridden: false, fans: [{ id: "fan_0", name: "Châssis 1", rpm: 1120, min_pct: 30, manual_pct: null }] },
    },
  ];

  const STATE = {
    static_info: { manufacturer: "ASUS", model: "ROG STRIX B650E", motherboard: "ROG STRIX B650E-F", bios_version: "3057", cpu_name: "AMD Ryzen 7 7800X3D", gpu_name: "AMD Radeon RX 7900 XT" },
    app_version: "0.4.0",
    ui: { mode, language: lang, onboarding_done: scen !== "wizard", can_undo: false, undo_label: null },
    presets: [
      { id: "silent", label: "Silencieux", description: "Ventilateurs en profil silencieux, éclairage doux et tamisé, plan d'alimentation équilibré." },
      { id: "gaming", label: "Jeu", description: "Ventilateurs en profil turbo, éclairage arc-en-ciel, plan d'alimentation hautes performances." },
      { id: "office", label: "Bureau", description: "Ventilateurs en profil standard, éclairage blanc statique, plan d'alimentation équilibré." },
      { id: "lights_off", label: "Tout éteint", description: "Coupe l'éclairage de tous les appareils sans toucher aux ventilateurs." },
    ],
    system_prefs: { autostart: false, minimize_to_tray: true, auto_profile_switch: true, lights_off_on_lock: true, idle_lights_off_min: 15, battery_alerts: true, openrgb_server: false, openrgb_clients: 0, visible_apps: ["cs2.exe"], is_admin: true, pawnio_ready: true, fan_calibration_active: null },
    firmware_inventory: [{ device: "K70 RGB TKL", vendor: "Corsair", version: "1.8.25", status: "Pilote natif OpenGear" }],
    local_assets: { k70_images: [], mouse_variants: [], headset_variants: [], mouse_markers: [], blue_voice_presets: [] },
    telemetry: { cpu_usage_pct: 14, ram_used_gb: 11.4, ram_total_gb: 31.7, ram_usage_pct: 36, gpu: { available: true, temp_c: 44, power_watts: 38, activity_pct: 8 }, sensors: { available: true, source: "PawnIO + ADL2", cpu_temp_c: 48, motherboard_temp_c: 34, nvme_temp_c: 39, aio_pump_rpm: 2850, fans: [] }, audio: { available: true, output_volume_pct: 25, output_muted: false, mic_available: true, mic_volume_pct: 80, mic_muted: false } },
    config: { active_profile: "Default", profiles: ["Default", "Gaming"], device_placements: {}, fan_calibrations: { "superio:mainboard::fan_0": { min_spinning_pct: 30, zero_rpm_supported: false, max_rpm: 1850, samples: [[30, 520], [50, 910], [80, 1480], [100, 1850]] } }, profile: { name: "Default", enable_rgb: true, win_lock: false, bindings: { ESC: { type: "Remap", target: "F13" } }, mouse_bindings: {}, layers: {}, mouse_layers: {}, socd: { enabled: false, pairs: [], blocked_apps: ["cs2.exe"] }, key_chatter_ms: 0, mouse_debounce_ms: 0, linked_apps: [], device_lighting: {}, fan_rules: {}, rgb_layers: [], rgb: { kind: "RainbowWave", primary: "#00D4FF", secondary: "#9B3CFF", speed: 1, brightness: 0.9, reverse: false, per_key: {} } } },
    input_running: true,
    bindable_keys: ["ESC", "F13", "LCTRL", "SPACE", "Q", "D"],
    bindable_mouse_buttons: [{ id: "Middle", label: "Clic molette (M3)" }, { id: "Back", label: "Précédent (M4)" }],
    bindable_mouse_targets: [{ id: "MouseLeft", label: "Clic gauche (M1)" }],
    keyboard: scen === "empty" ? { keys: [], os_layout: {}, form_factors: [] } : { width: 18.8, height: 6.5, standard: "iso", device_name: "CORSAIR K70 RGB TKL", os_layout: { top_row: "AZERTY", lang_id: "040C" }, form_factors: [{ id: "tkl", label: "TKL" }], keys: [{ id: "ESC", code: "Escape", label: "Échap", x: 0, y: 0, w: 1, h: 1, norm_x: 0.03, norm_y: 0.1 }, { id: "A", code: "KeyQ", label: "A", x: 1.5, y: 2.2, w: 1, h: 1, norm_x: 0.12, norm_y: 0.45 }, { id: "D", code: "KeyD", label: "D", x: 3.5, y: 3.2, w: 1, h: 1, norm_x: 0.25, norm_y: 0.6 }] },
    effects: [{ id: "Static", label: "Statique" }, { id: "RainbowWave", label: "Vague arc-en-ciel" }, { id: "Breathing", label: "Respiration" }],
    blend_modes: [{ id: "Normal", label: "Normal" }, { id: "Add", label: "Addition" }],
    sensor_kinds: [{ id: "Cpu", label: "Processeur" }, { id: "GpuEdge", label: "GPU cœur" }],
    power_plans: [{ guid: "1", name: "Usage normal", active: true }, { guid: "2", name: "Performances élevées", active: false }],
    devices: [],
    device_list: MOCK_DEVICES,
    experimental_drivers: [],
  };

  window.ipc = {
    postMessage(raw) {
      const { id, cmd, payload } = JSON.parse(raw);
      setTimeout(() => {
        if (scen === "error" && cmd === "get_state") {
          window.__ogReply(id, { ok: false, error: "Pont IPC indisponible (simulation erreur)" });
          return;
        }
        if (cmd === "get_diagnostics") {
          window.__ogReply(id, { ok: true, data: { checks: scen === "empty" ? [{ id: "devices", label: "Appareils compatibles", status: "warn", detail: "Aucun appareil compatible détecté. Vérifie les branchements et ferme iCUE / G HUB / Armoury Crate." }] : [{ id: "devices", label: "Appareils compatibles", status: "ok", detail: `${STATE.device_list.length} appareil(s) piloté(s).` }] } });
          return;
        }
        if (cmd === "device_list_backups") {
          window.__ogReply(id, { ok: true, data: { backups: [] } });
          return;
        }
        if (cmd === "set_ui_prefs") {
          if (payload.ui_mode) STATE.ui.mode = payload.ui_mode;
          if (payload.language) STATE.ui.language = payload.language;
          if (payload.onboarding_done !== undefined) STATE.ui.onboarding_done = !!payload.onboarding_done;
        }
        window.__ogReply(id, { ok: true, data: STATE });
      }, 20);
    },
  };
})();
