import { arr, clone, num, plural } from "./utils.js";

export let S = null;
export const local = {
  tab: "home",
  sub: { lighting: "global", performance: "mouse", input: "keys", system: "prefs" },
  deviceKey: null,
  selectedKeyId: "ESC",
  paintMode: false,
  paintColor: "#00D4FF",
  kbViewMode: "interactive",
  selectedCoolingIdx: 0,
  selectedMouseVariant: 0,
  selectedHeadsetVariant: 0,
  uiModeOverride: null,
  configWarningShown: false,
  diag: { data: null, loading: false, error: null },
  diagSorted: [],
  backups: { data: null, corsairData: null, corsairDev: 0, loading: false, error: null, mouse: 0 },
  macroSteps: [
    { key: "LCTRL", down: true, delay_ms: 10 },
    { key: "C", down: true, delay_ms: 10 },
    { key: "C", down: false, delay_ms: 10 },
    { key: "LCTRL", down: false, delay_ms: 10 },
  ],
  profSel: {},
  btnSel: {},
  hwpProfSel: {},
  drafts: {},
  eqDrafts: {},
  lightingDrafts: {},
  canvasSelectedId: null,
  pendingRender: false,
  curveDragging: false,
  lastPreviewSig: "",
  animStart: performance.now(),
};

let renderHook = () => {};
let toastHook = () => {};

export function registerHooks({ onRender, onToast }) {
  renderHook = onRender;
  toastHook = onToast;
}

export function ui() {
  const u = (S && S.ui) || {};
  return {
    mode: u.mode === "simple" || u.mode === "expert" ? u.mode : local.uiModeOverride || "expert",
    onboarding_done: S && S.ui ? u.onboarding_done !== false : true,
    can_undo: !!u.can_undo,
    undo_label: u.undo_label || null,
  };
}

export const isExpert = () => ui().mode === "expert";

export function normalizeState() {
  if (!S) return;
  [
    "corsair_devices", "logi_mice", "logi_headsets", "power_plans", "effects",
    "button_presets", "bindable_keys", "devices", "device_list", "firmware_inventory",
  ].forEach((k) => {
    if (!Array.isArray(S[k])) S[k] = [];
  });
  S.presets = arr(S.presets);
  S.keyboard = S.keyboard || {};
  S.keyboard.keys = arr(S.keyboard.keys);
  S.keyboard.form_factors = arr(S.keyboard.form_factors);
  S.keyboard.os_layout = S.keyboard.os_layout || {};
  S.telemetry = S.telemetry || {};
  S.cooling = S.cooling || {};
  S.cooling.channels = arr(S.cooling.channels);
  S.static_info = S.static_info || {};
  S.system_prefs = S.system_prefs || {};
  S.local_assets = S.local_assets || {};
  S.hp_omen = S.hp_omen || {};
  S.laptop_battery = S.laptop_battery || {};
  S.laptop_display = S.laptop_display || {};
  S.config = S.config || {};
  S.config.profiles = arr(S.config.profiles);
  S.config.device_placements = S.config.device_placements || {};
  const p = (S.config.profile = S.config.profile || {});
  p.rgb = p.rgb || { kind: "Static", primary: "#00D4FF", secondary: "#9B3CFF", speed: 1, brightness: 1, reverse: false, per_key: {} };
  p.rgb.per_key = p.rgb.per_key || {};
  p.device_lighting = p.device_lighting || {};
  p.bindings = p.bindings || {};
  p.linked_apps = arr(p.linked_apps);
  if (S.ui && local.uiModeOverride && !S.ui.mode) S.ui.mode = local.uiModeOverride;
}

export function syncDrafts() {
  Object.keys(local.drafts).forEach((k) => {
    if (!local.drafts[k].dirty) delete local.drafts[k];
  });
  Object.keys(local.eqDrafts).forEach((k) => {
    if (!local.eqDrafts[k].dirty) delete local.eqDrafts[k];
  });
  if (local.selectedCoolingIdx >= S.cooling.channels.length) local.selectedCoolingIdx = 0;
}

export function getDraft(m, i) {
  const profiles = arr(m.onboard_profiles);
  if (!profiles.length) return null;
  let idx = local.profSel[i] || 0;
  if (idx >= profiles.length) idx = 0;
  local.profSel[i] = idx;
  const d = local.drafts[i];
  if (!d || d.idx !== idx) local.drafts[i] = { idx, profile: clone(profiles[idx]), dirty: false };
  const p = local.drafts[i].profile;
  p.buttons = arr(p.buttons);
  p.dpi_stages = arr(p.dpi_stages);
  return local.drafts[i];
}

export function getDraftFor(i) {
  const m = S && S.logi_mice[i];
  return m ? getDraft(m, i) : null;
}

export function getEq(h, i) {
  if (!local.eqDrafts[i]) {
    const g = arr(h.equalizer && h.equalizer.gains_db);
    local.eqDrafts[i] = { gains: g.length ? g.map((x) => num(x)) : [0, 0, 0, 0, 0], dirty: false };
  }
  return local.eqDrafts[i];
}

export function mouseImage() {
  const vars = arr(S && S.local_assets.mouse_variants);
  if (vars.length) {
    const idx = Math.min(local.selectedMouseVariant, vars.length - 1);
    return { url: vars[idx].asset_url, official: true, variants: vars };
  }
  const builtin = [
    { label: "Noir", asset_url: "/assets/mouse.svg" },
    { label: "Blanc", asset_url: "/assets/mouse_white.svg" },
  ];
  const idx = Math.min(local.selectedMouseVariant, builtin.length - 1);
  return { url: builtin[idx].asset_url, official: false, variants: builtin };
}

export function headsetImage() {
  const vars = arr(S && S.local_assets.headset_variants);
  if (vars.length) {
    const idx = Math.min(local.selectedHeadsetVariant, vars.length - 1);
    return { url: vars[idx].asset_url, official: true, variants: vars };
  }
  return { url: "/assets/headset.svg", official: false, variants: [] };
}

/** Label Windows gives the key bound to `id` (falls back to the identifier itself). */
export function keyLabel(id) {
  const k = S && S.keyboard ? S.keyboard.keys.find((x) => x.id === id) : null;
  return k ? k.label : id;
}

/** Label of the key at a physical position (W3C code), e.g. `KeyA` -> `Q` on AZERTY. */
export function codeLabel(code) {
  const k = S && S.keyboard ? S.keyboard.keys.find((x) => x.code === code) : null;
  return k ? k.label : code.replace(/^Key/, "");
}

/** Device of the shared model (`S.device_list`) by id. */
export function deviceById(id) {
  return arr(S && S.device_list).find((d) => d.info.id === id) || null;
}

/** Current value of one setting of a device (`undefined` when the device does not offer it). */
export function settingOf(id, settingId) {
  const d = deviceById(id);
  const s = d && d.settings.find((x) => x.id === settingId);
  return s ? s.value : undefined;
}

const KIND_LABEL = {
  keyboard: "Clavier", mouse: "Souris", headset: "Casque", mousepad: "Tapis", controller: "Manette", motherboard: "Carte mère",
  memory: "Mémoire", cooler: "Refroidissement", led_controller: "Contrôleur LED", laptop: "PC portable", other: "Appareil",
};
const KIND_ICON = { keyboard: "keyboard", mouse: "mouse", headset: "headset", cooler: "fan", motherboard: "board" };

export function k70Image() {
  const imgs = arr(S && S.local_assets.k70_images);
  if (imgs.length) {
    const wanted = S.keyboard && S.keyboard.standard === "ansi" ? "na" : "fr";
    const f = imgs.find((x) => x.id === wanted) || imgs.find((x) => x.id === "thumb") || imgs[0];
    return { url: f.asset_url, official: true };
  }
  return { url: "/assets/keyboard.svg", official: false };
}

const qa = (label, act, data = {}, title = "") => ({ label, act, data, title });
const fmtHzShort = (hz) => (hz >= 1000 ? `${hz / 1000} kHz` : `${hz} Hz`);

export function devicesList() {
  if (!S) return [];
  const out = [];
  const prof = S.config.profile;
  S.corsair_devices.forEach((d, i) => {
    const kind = d.kind;
    const base = { key: `corsair:${i}`, deviceId: d.device_id, vendor: "Corsair", kind, name: d.product_name || "Appareil Corsair", type: d.kind_label || "Corsair", battery: d.battery_pct, status: { label: "Connecté", cls: "emerald" }, quick: [] };
    if (kind === "Mouse") {
      Object.assign(base, { img: mouseImage().url, icon: "mouse" });
      if (d.mouse) {
        const st = num(d.mouse.active_stage);
        const stages = arr(d.mouse.dpi_stages);
        base.status = { label: `${num(d.mouse.current_dpi_x, stages[st] || 0)} DPI`, cls: "cyan" };
        if (stages.length) base.quick.push(qa("Palier DPI suivant", "cm-stage", { dev: i, stage: (st + 1) % stages.length }));
      }
    } else if (kind === "Headset") {
      Object.assign(base, { img: headsetImage().url, icon: "headset" });
      const side = num(d.headset && d.headset.sidetone_pct);
      base.quick.push(qa(side > 0 ? "Couper le retour micro" : "Activer le retour micro", "dev-set", { device: d.device_id, setting: "sidetone", json: side > 0 ? 0 : 50 }));
    } else if (kind === "CoolerHub") {
      Object.assign(base, { img: "/assets/aio.svg", icon: "fan" });
    } else if (kind === "Keyboard") {
      const k = k70Image();
      Object.assign(base, { img: k.url, imgCls: k.official ? "cutout" : "", icon: "keyboard" });
      base.status = d.pollrate_hz ? { label: fmtHzShort(d.pollrate_hz), cls: "emerald" } : base.status;
      base.quick.push(qa(prof.enable_rgb ? "Couper l'éclairage" : "Allumer l'éclairage", "rgb-enable", { on: prof.enable_rgb ? "0" : "1" }));
      const br = num(d.hw_brightness_pct, 100);
      const next = br >= 100 ? 50 : br >= 50 ? 0 : 100;
      base.quick.push(qa(`Luminosité ${next} %`, "dev-set", { device: d.device_id, setting: "hw_brightness", json: next }));
      if (d.hw_winlock != null) base.quick.push(qa(d.hw_winlock ? "Débloquer Windows" : "Bloquer Windows", "dev-set", { device: d.device_id, setting: "win_lock", json: !d.hw_winlock }, "Bloque la touche Windows pendant le jeu"));
    } else {
      Object.assign(base, { img: "/assets/generic_device.svg", icon: "chip" });
    }
    out.push(base);
  });
  S.logi_mice.forEach((m, i) => {
    const mi = mouseImage();
    const bat = m.battery || null;
    const dpi = m.dpi;
    const item = {
      key: `logi-mouse:${i}`, deviceId: m.device_id, vendor: "Logitech", kind: "Mouse", name: m.name || "Souris Logitech", type: "Souris Logitech G",
      img: mi.url, imgCls: mi.official ? "cutout" : "", icon: "mouse",
      battery: bat ? bat.percent : null, charging: !!(bat && bat.is_charging),
      status: { label: dpi ? `${dpi.dpi_x} DPI` : "Connectée", cls: "cyan" }, quick: [],
    };
    if (dpi) {
      [800, 1600, 3200].filter((v) => v >= num(dpi.min_dpi, 100) && v <= num(dpi.max_dpi, 25600)).forEach((v) => item.quick.push(qa(`${v} DPI`, "logi-dpi", { dev: i, dpi: v })));
    }
    out.push(item);
  });
  S.logi_headsets.forEach((h, i) => {
    const hi = headsetImage();
    const presets = arr(h.presets);
    const flatIdx = presets.findIndex((p) => p.is_flat);
    const fpsIdx = presets.findIndex((p) => p.id === "fps" || /fps/i.test(p.name));
    const bat = h.battery || null;
    const quick = [];
    if (h.equalizer && flatIdx >= 0) quick.push(qa("Son neutre", "eq-preset", { dev: i, idx: flatIdx }));
    if (h.equalizer && fpsIdx >= 0) quick.push(qa("Son FPS", "eq-preset", { dev: i, idx: fpsIdx }));
    quick.push(qa(num(h.sidetone_pct) > 0 ? "Couper le retour micro" : "Retour micro 50 %", "dev-set", { device: h.device_id, setting: "sidetone", json: num(h.sidetone_pct) > 0 ? 0 : 50 }));
    out.push({
      key: `logi-hs:${i}`, deviceId: h.device_id, vendor: "Logitech", kind: "Headset", name: h.name || "Casque Logitech", type: "Casque Logitech G",
      img: hi.url, imgCls: hi.official ? "cutout" : "", icon: "headset",
      battery: bat ? bat.percent : null, charging: !!(bat && bat.is_charging),
      status: { label: "Connecté", cls: "purple" }, quick,
    });
  });
  if (S.hp_omen && S.hp_omen.detected) {
    const o = S.hp_omen;
    const bat = S.laptop_battery || {};
    const quick = o.wmi_accessible
      ? [
          qa("Éco", "omen-thermal", { mode: "cool" }),
          qa("Équilibré", "omen-thermal", { mode: "default" }),
          qa("Perf", "omen-thermal", { mode: "performance" }),
        ]
      : o.is_admin ? [] : [qa("Relancer en admin", "relaunch-admin", {})];
    out.push({
      key: "hp-omen",
      vendor: "HP OMEN",
      kind: "Laptop",
      name: o.model || "HP OMEN",
      type: `PC portable · ${o.board_id || "BIOS WMI"}`,
      img: "/assets/laptop.svg",
      icon: "laptop",
      battery: bat.present ? bat.percent : null,
      charging: !!(bat && bat.charging),
      status: o.wmi_accessible
        ? { label: o.max_fan ? "Max Fan" : (o.thermal_mode_label || "Connecté"), cls: o.max_fan ? "amber" : "emerald" }
        : { label: o.is_admin ? "WMI indisponible" : "Admin requis", cls: "amber" },
      quick,
    });
  }
  if (!S.is_laptop && S.cooling.channels.length) {
    const c = S.cooling;
    out.push({
      key: "cooling", vendor: "Refroidissement", kind: "Cooling", name: "Ventilation & watercooling", type: `${c.channels.length} canaux`,
      img: "/assets/aio.svg", icon: "fan",
      status: c.is_admin ? { label: c.wmi_available ? "Pilotable" : "Lecture seule", cls: c.wmi_available ? "emerald" : "amber" } : { label: "Admin requis", cls: "amber" },
      quick: [qa("Silence", "sl3-preset", { preset: "ultra_silent" }), qa("Équilibré", "sl3-preset", { preset: "default" }), qa("Performance", "sl3-preset", { preset: "max_perf" })],
    });
  }
  // Devices discovered through the shared driver registry appear automatically.
  const known = new Set(out.map((x) => x.deviceId).filter(Boolean));
  if (S.hp_omen && S.hp_omen.detected) known.add("hp_omen:keyboard");
  arr(S.device_list).filter((d) => !known.has(d.info.id)).forEach((d) => {
    const bat = d.status.battery_pct;
    const icon = KIND_ICON[d.info.kind] || (d.info.kind === "memory" ? "ram" : "chip");
    const img = d.info.kind === "memory" ? "/assets/ram.svg"
      : d.info.kind === "motherboard" ? "/assets/motherboard.svg"
      : d.info.kind === "keyboard" ? "/assets/keyboard.svg"
      : undefined;
    out.push({
      key: `dev:${d.info.id}`, deviceId: d.info.id, vendor: d.info.vendor, kind: d.info.kind, name: d.info.model,
      type: KIND_LABEL[d.info.kind] || "Appareil", icon, img,
      battery: bat != null ? bat : null, charging: !!d.status.charging,
      status: { label: "Connecté", cls: "emerald" }, quick: [],
    });
  });
  return out;
}

export function updateChrome() {
  if (!S) return;
  const verEl = document.getElementById("app-version");
  if (verEl && S.app_version) verEl.textContent = `OpenGear v${S.app_version} · aucune télémétrie`;
  document.querySelectorAll(".nav-item").forEach((b) => {
    const on = b.dataset.tab === local.tab;
    b.classList.toggle("active", on);
    if (on) b.setAttribute("aria-current", "page");
    else b.removeAttribute("aria-current");
  });
  const devCount = devicesList().length;
  const bd = document.getElementById("badge-devices");
  if (bd) {
    bd.textContent = devCount ? String(devCount) : "";
    bd.className = "nav-badge" + (devCount ? " ok" : "");
  }
  const bs = document.getElementById("badge-system");
  if (bs) {
    const checks = arr(local.diag.data && local.diag.data.checks);
    const errs = checks.filter((c) => c.status === "error").length;
    const warns = checks.filter((c) => c.status === "warn").length;
    bs.textContent = errs ? String(errs) : warns ? String(warns) : "";
    bs.className = "nav-badge" + (errs ? " err" : warns ? " warn" : "");
    bs.title = errs ? `${errs} erreur(s) au diagnostic` : warns ? `${warns} avertissement(s) au diagnostic` : "";
  }
  const sel = document.getElementById("profile-select");
  if (sel && document.activeElement !== sel) {
    sel.innerHTML = S.config.profiles
      .map((n) => `<option value="${escOption(n)}" ${n === S.config.active_profile ? "selected" : ""}>${escOption(n)}</option>`)
      .join("");
  }
  const mode = ui().mode;
  const ms = document.getElementById("mode-simple");
  const me = document.getElementById("mode-expert");
  if (ms) ms.setAttribute("aria-pressed", String(mode === "simple"));
  if (me) me.setAttribute("aria-pressed", String(mode === "expert"));
  const mh = document.getElementById("mode-hint");
  if (mh) mh.textContent = mode === "simple" ? "Réglages avancés masqués." : "Tous les réglages sont visibles.";
  const u = ui();
  const ub = document.getElementById("btn-undo");
  if (ub) {
    ub.hidden = !u.can_undo;
    ub.textContent = u.undo_label ? `Annuler : ${u.undo_label}` : "Annuler la dernière modification";
    ub.title = ub.textContent;
  }
}

function escOption(s) {
  return String(s ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

export function applyState(st, rerender = true) {
  S = st;
  normalizeState();
  syncDrafts();
  updateChrome();
  if (S.config_warning && !local.configWarningShown) {
    local.configWarningShown = true;
    toastHook(S.config_warning, "err");
  }
  if (rerender) renderHook();
}
