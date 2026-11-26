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
  activeLayer: "base",
  configWarningShown: false,
  diag: { data: null, loading: false, error: null },
  diagSorted: [],
  backups: { data: null, loading: false, error: null, deviceId: "" },
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
    language: u.language === "en" ? "en" : "fr",
    onboarding_done: S && S.ui ? u.onboarding_done !== false : true,
    can_undo: !!u.can_undo,
    undo_label: u.undo_label || null,
  };
}

export const isExpert = () => ui().mode === "expert";

export function normalizeState() {
  if (!S) return;
  [
    "power_plans", "effects",
    "blend_modes", "sensor_kinds", "button_presets", "bindable_keys", "devices",
    "device_list", "firmware_inventory",
  ].forEach((k) => {
    if (!Array.isArray(S[k])) S[k] = [];
  });
  S.presets = arr(S.presets);
  S.keyboard = S.keyboard || {};
  S.keyboard.keys = arr(S.keyboard.keys);
  S.keyboard.form_factors = arr(S.keyboard.form_factors);
  S.keyboard.os_layout = S.keyboard.os_layout || {};
  S.telemetry = S.telemetry || {};
  S.static_info = S.static_info || {};
  S.system_prefs = S.system_prefs || {};
  S.local_assets = S.local_assets || {};
  S.laptop_battery = S.laptop_battery || {};
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
  const cd = curveDevice();
  const len = cd ? arr(cd.curves && cd.curves.channels).length : 0;
  if (local.selectedCoolingIdx >= len) local.selectedCoolingIdx = 0;
}

/** Primary motherboard/cooling device carrying firmware fan curves (`S.device_list[].curves`). */
export function curveDevice() {
  return arr(S && S.device_list).find((d) => d.curves && arr(d.curves.channels).length) || null;
}

export function getDraft(dev) {
  const id = dev && dev.info && dev.info.id;
  const profiles = arr(dev && dev.onboard && dev.onboard.profiles);
  if (!id || !profiles.length) return null;
  let idx = local.profSel[id] || 0;
  if (idx >= profiles.length) idx = 0;
  local.profSel[id] = idx;
  const d = local.drafts[id];
  if (!d || d.idx !== idx) local.drafts[id] = { idx, profile: clone(profiles[idx]), dirty: false };
  const p = local.drafts[id].profile;
  p.buttons = arr(p.buttons);
  p.dpi_stages = arr(p.dpi_stages);
  return local.drafts[id];
}

export function getDraftFor(id) {
  const d = deviceById(id);
  return d ? getDraft(d) : null;
}

export function getEq(dev) {
  const id = dev && dev.info && dev.info.id;
  if (!id) return { gains: [0, 0, 0, 0, 0], dirty: false };
  if (!local.eqDrafts[id]) {
    const g = arr(dev.equalizer && dev.equalizer.gains_db);
    local.eqDrafts[id] = { gains: g.length ? g.map((x) => num(x)) : [0, 0, 0, 0, 0], dirty: false };
  }
  return local.eqDrafts[id];
}

export function mouseImage() {
  const vars = arr(S && S.local_assets.mouse_variants);
  if (vars.length) {
    const idx = Math.min(local.selectedMouseVariant, vars.length - 1);
    return { url: vars[idx].asset_url, official: true, variants: vars };
  }
  const builtin = [
    { label: "Noir", asset_url: "/assets/mouse.webp" },
    { label: "Blanc", asset_url: "/assets/mouse_white.webp" },
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
  return { url: "/assets/headset.webp", official: false, variants: [] };
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
  memory: "Mémoire", cooler: "Refroidissement", led_controller: "Contrôleur LED", laptop: "PC portable", display: "Écran", other: "Appareil",
};
const KIND_ICON = { keyboard: "keyboard", mouse: "mouse", headset: "headset", cooler: "fan", motherboard: "board", laptop: "laptop", display: "laptop" };

export function k70Image() {
  const imgs = arr(S && S.local_assets.k70_images);
  if (imgs.length) {
    const wanted = S.keyboard && S.keyboard.standard === "ansi" ? "na" : "fr";
    const f = imgs.find((x) => x.id === wanted) || imgs.find((x) => x.id === "thumb") || imgs[0];
    return { url: f.asset_url, official: true };
  }
  return { url: "/assets/keyboard.webp", official: false };
}

const qa = (label, act, data = {}, title = "") => ({ label, act, data, title });
const fmtHzShort = (hz) => (hz >= 1000 ? `${hz / 1000} kHz` : `${hz} Hz`);

export function devicesList() {
  if (!S) return [];
  const out = [];
  const prof = S.config.profile;
  const KIND_IMG = {
    memory: "/assets/ram.webp",
    motherboard: "/assets/motherboard.webp",
    keyboard: "/assets/keyboard.webp",
    mouse: "/assets/mouse.webp",
    headset: "/assets/headset.webp",
    cooler: "/assets/aio.webp",
    laptop: "/assets/laptop.webp",
    led_controller: "/assets/generic_device.webp",
    other: "/assets/generic_device.webp",
  };
  arr(S.device_list).forEach((d) => {
    const id = d.info.id;
    const kind = d.info.kind;
    const bat = d.status.battery_pct;
    const icon = KIND_ICON[kind] || (kind === "memory" ? "ram" : "chip");
    let img = KIND_IMG[kind] || "/assets/generic_device.webp";
    if (kind === "mouse" && /logitech|pro\s*x|corsair|mouse/i.test(`${d.info.vendor} ${d.info.model}`)) {
      img = mouseImage().url;
    } else if (kind === "headset" && /logitech|pro\s*x|corsair|headset/i.test(`${d.info.vendor} ${d.info.model}`)) {
      img = headsetImage().url;
    } else if (kind === "keyboard" && /k70/i.test(d.info.model)) {
      img = k70Image().url;
    }
    const sysBat = kind === "laptop" && S.laptop_battery && S.laptop_battery.present ? S.laptop_battery : null;
    const settings = arr(d.settings);
    const findS = (sid) => settings.find((s) => s.id === sid);
    const perfProf = findS("performance_profile");
    const forced = d.cooling && d.cooling.overridden;
    const curProf = perfProf && arr(perfProf.control && perfProf.control.options).find((o) => o.id === perfProf.value);
    const ptr = d.pointer;
    const eq = d.equalizer;
    const quick = [];

    if (perfProf && perfProf.control && perfProf.control.type === "choice") {
      arr(perfProf.control.options).forEach((o) => quick.push(qa(o.label, "dev-set", { device: id, setting: perfProf.id, json: JSON.stringify(o.id) })));
    }
    if (ptr) {
      const stages = arr(ptr.stages);
      if (stages.length > 1) {
        const st = num(ptr.active_stage);
        quick.push(qa("Palier DPI suivant", "ptr-stage", { id, stage: (st + 1) % stages.length }));
      } else if (ptr.max_dpi > 0) {
        [800, 1600, 3200]
          .filter((v) => v >= num(ptr.min_dpi, 100) && v <= num(ptr.max_dpi, 25600))
          .forEach((v) => quick.push(qa(`${v} DPI`, "ptr-dpi", { id, dpi: v })));
      }
    }
    if (eq) {
      const presets = arr(eq.presets);
      const flat = presets.find((p) => p.id === "flat" || arr(p.gains_db).every((g) => num(g) === 0));
      const fps = presets.find((p) => p.id === "fps" || /fps/i.test(p.name));
      if (flat) quick.push(qa("Son neutre", "eq-preset", { id, preset: flat.id }));
      if (fps) quick.push(qa("Son FPS", "eq-preset", { id, preset: fps.id }));
    }
    const side = findS("sidetone");
    if (side) {
      const cur = num(side.value);
      quick.push(qa(cur > 0 ? "Couper le retour micro" : "Activer le retour micro", "dev-set", { device: id, setting: side.id, json: cur > 0 ? 0 : 50 }));
    }
    if (kind === "keyboard" && d.lighting) {
      quick.push(qa(prof.enable_rgb ? "Couper l'éclairage" : "Allumer l'éclairage", "rgb-enable", { on: prof.enable_rgb ? "0" : "1" }));
    }
    const hwBr = findS("hw_brightness");
    if (hwBr) {
      const br = num(hwBr.value, 100);
      const next = br >= 100 ? 50 : br >= 50 ? 0 : 100;
      const lbl = next === 0 ? "Couper le rétroéclairage" : next === 100 ? "Rétroéclairage max" : "Rétroéclairage 50 %";
      quick.push(qa(lbl, "dev-set", { device: id, setting: hwBr.id, json: next }));
    }
    const winLock = findS("win_lock");
    if (winLock) {
      quick.push(qa(winLock.value ? "Débloquer Windows" : "Bloquer Windows", "dev-set", { device: id, setting: winLock.id, json: !winLock.value }, "Bloque la touche Windows pendant le jeu"));
    }

    const pollVal = findS("polling_rate") && num(findS("polling_rate").value, 0);
    const crv = d.curves;
    const cool = d.cooling;
    const isCurveBoard = !!(crv && arr(crv.channels).length);
    const hasPump = isCurveBoard && crv.channels.some((x) => x.is_pump);
    if (isCurveBoard && hasPump) {
      img = "/assets/aio.webp";
      quick.push(
        qa("Silence", "sl3-preset", { preset: "ultra_silent" }),
        qa("Équilibré", "sl3-preset", { preset: "default" }),
        qa("Performance", "sl3-preset", { preset: "max_perf" }),
      );
    }

    let devType = KIND_LABEL[kind] || "Appareil";
    if (isCurveBoard) devType = plural(arr(crv.channels).length, "canal", "canaux");
    else if (kind === "motherboard" && cool) devType = plural(arr(cool.fans).length, "canal PWM", "canaux PWM");

    let status;
    if (isCurveBoard) {
      status = crv.is_admin
        ? { label: crv.available ? "Pilotable" : "Lecture seule", cls: crv.available ? "emerald" : "amber" }
        : { label: "Admin requis", cls: "amber" };
    } else if (forced) status = { label: "Ventilation forcée", cls: "amber" };
    else if (ptr) status = { label: `${num(ptr.dpi_x, arr(ptr.stages)[num(ptr.active_stage)] || 0)} DPI`, cls: "cyan" };
    else if (curProf) status = { label: curProf.label, cls: "emerald" };
    else if (kind === "keyboard" && pollVal) status = { label: fmtHzShort(pollVal), cls: "emerald" };
    else if (kind === "headset") status = { label: "Connecté", cls: "purple" };
    else status = { label: "Connecté", cls: "emerald" };

    out.push({
      key: `dev:${id}`, deviceId: id, vendor: d.info.vendor, kind, name: d.info.model,
      type: devType, icon: isCurveBoard ? "fan" : icon, img, imgCls: "cutout",
      battery: sysBat ? sysBat.percent : bat != null ? bat : null,
      charging: sysBat ? !!sysBat.charging : !!d.status.charging,
      batteryHours: !sysBat && d.status.battery_hours != null ? d.status.battery_hours : null,
      status, quick,
    });
  });

  return out;
}

export function updateChrome() {
  if (!S) return;
  const isEn = ui().language === "en";
  const verEl = document.getElementById("app-version");
  if (verEl && S.app_version) verEl.textContent = `OpenGear v${S.app_version} · ${isEn ? "no telemetry" : "aucune télémétrie"}`;
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
    bs.title = errs
      ? (isEn ? `${errs} diagnostic ${errs > 1 ? "errors" : "error"}` : `${plural(errs, "erreur", "erreurs")} au diagnostic`)
      : warns
        ? (isEn ? `${warns} diagnostic ${warns > 1 ? "warnings" : "warning"}` : `${plural(warns, "avertissement", "avertissements")} au diagnostic`)
        : "";
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
  if (mh) {
    mh.textContent = mode === "simple"
      ? (isEn ? "Advanced settings hidden." : "Réglages avancés masqués.")
      : (isEn ? "All settings visible." : "Tous les réglages sont visibles.");
  }
  const u = ui();
  const ub = document.getElementById("btn-undo");
  if (ub) {
    ub.hidden = !u.can_undo;
    ub.textContent = u.undo_label
      ? (isEn ? `Undo: ${u.undo_label}` : `Annuler : ${u.undo_label}`)
      : (isEn ? "Undo last change" : "Annuler la dernière modification");
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
