// Shared constants, HTML escaping, formatting helpers, and throttler.

export const SWATCHES = [
  ["Rouge ROG", "#FF1848"],
  ["Cyan", "#00D4FF"],
  ["Jaune", "#FFC400"],
  ["Violet", "#9B3CFF"],
  ["Émeraude", "#00EB91"],
  ["Orange", "#FF6A00"],
  ["Blanc", "#F0F6FF"],
  ["Bleu", "#195AFF"],
];

export const CONFIG_CMDS = new Set([
  "set_rgb_config", "set_rgb_layers", "lighting_sync_all", "lighting_set_device", "lighting_resize_zone", "lighting_set_placement",
  "set_per_key_color", "clear_per_key", "set_keyboard_layout",
  "input_set_binding", "input_remove_binding", "input_set_layer_binding", "input_remove_layer_binding",
  "input_set_mouse_binding", "input_remove_mouse_binding", "input_set_mouse_layer_binding", "input_remove_mouse_layer_binding",
  "input_set_esports", "input_set_winlock",
  "apply_preset", "switch_profile", "create_profile", "profile_set_linked_apps", "profile_import_json",
]);

export const SVG = (inner, vb = "0 0 20 20") =>
  `<svg viewBox="${vb}" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${inner}</svg>`;

export const ICONS = {
  warn: SVG('<path d="M10 3L18 17H2L10 3z"/><path d="M10 8.5v3.5M10 14.6v.1"/>'),
  check: SVG('<path d="M4.5 10.5l3.5 3.5 7.5-8"/>'),
  x: SVG('<path d="M5.5 5.5l9 9M14.5 5.5l-9 9"/>'),
  info: SVG('<circle cx="10" cy="10" r="7.5"/><path d="M10 9v5M10 6.2v.1"/>'),
  chev: SVG('<path d="M8 5l5 5-5 5"/>'),
  plug: SVG('<path d="M7 2.5v4M13 2.5v4M5 6.5h10v3a5 5 0 0 1-10 0v-3zM10 14.5v3"/>'),
  lock: SVG('<rect x="4" y="9" width="12" height="8.5" rx="2"/><path d="M6.8 9V6.5a3.2 3.2 0 0 1 6.4 0V9"/>'),
  battery: SVG('<rect x="1.5" y="1.5" width="17" height="9" rx="2"/><path d="M20.5 4.5v3"/>', "0 0 22 12"),
  refresh: SVG('<path d="M16 10a6 6 0 1 1-1.8-4.3"/><path d="M16.5 3v3.5H13"/>'),
  keyboard: SVG('<rect x="2" y="5" width="16" height="10" rx="2"/><path d="M5 8.5h1M8 8.5h1M11 8.5h1M14 8.5h1M6 12h8"/>'),
  mouse: SVG('<rect x="5" y="2.5" width="10" height="15" rx="5"/><path d="M10 6v3"/>'),
  headset: SVG('<path d="M3.5 12.5V10a6.5 6.5 0 0 1 13 0v2.5"/><rect x="2.5" y="11.5" width="3.5" height="5.5" rx="1.2"/><rect x="14" y="11.5" width="3.5" height="5.5" rx="1.2"/>'),
  fan: SVG('<circle cx="10" cy="10" r="7.5"/><circle cx="10" cy="10" r="1.6"/><path d="M10 8.4C10 5.5 12.5 4 14 5.5M11.6 10C14.5 10 16 12.5 14.5 14M10 11.6C10 14.5 7.5 16 6 14.5M8.4 10C5.5 10 4 7.5 5.5 6"/>'),
  board: SVG('<rect x="3" y="3" width="14" height="14" rx="2"/><rect x="7" y="7" width="6" height="6" rx="1"/><path d="M7 3v2M13 3v2M7 15v2M13 15v2M3 7h2M3 13h2M15 7h2M15 13h2"/>'),
  ram: SVG('<rect x="2" y="6" width="16" height="8" rx="1.5"/><path d="M5 9v2M8 9v2M11 9v2M14 9v2M4 14v2.5M16 14v2.5"/>'),
  laptop: SVG('<rect x="3" y="4" width="14" height="9" rx="1.5"/><path d="M1.5 15.5h17l-1.5-2.5H3l-1.5 2.5z"/>'),
  chip: SVG('<rect x="4.5" y="4.5" width="11" height="11" rx="2"/><path d="M7.5 2v2.5M12.5 2v2.5M7.5 15.5V18M12.5 15.5V18M2 7.5h2.5M2 12.5h2.5M15.5 7.5H18M15.5 12.5H18"/>'),
  undo: SVG('<path d="M7 5L3.5 8.5 7 12"/><path d="M3.5 8.5H12a4.5 4.5 0 0 1 0 9H9"/>'),
  spark: SVG('<path d="M10 2.5l1.8 5.7 5.7 1.8-5.7 1.8L10 17.5l-1.8-5.7L2.5 10l5.7-1.8L10 2.5z"/>'),
};

export const PRESET_ICONS = {
  silent: SVG('<path d="M15.5 12.5A6.5 6.5 0 0 1 7.5 4.5a6.5 6.5 0 1 0 8 8z"/>'),
  gaming: SVG('<path d="M11 2.5L4.5 11h5l-1 6.5L15.5 9h-5l.5-6.5z"/>'),
  office: SVG('<rect x="2.5" y="6" width="15" height="10.5" rx="2"/><path d="M7 6V4.5A1.5 1.5 0 0 1 8.5 3h3A1.5 1.5 0 0 1 13 4.5V6M2.5 10.5h15"/>'),
  lights_off: SVG('<path d="M10 2.5a5 5 0 0 0-3 9v2h6v-2a5 5 0 0 0-3-9z"/><path d="M7.5 16h5M3 3l14 14"/>'),
};

export const LABEL_FMT = {
  pct: (v) => `${Math.round(Number(v) * 100)} %`,
  x: (v) => `${Number(v).toFixed(1)}×`,
  pct100: (v) => `${v} %`,
  leds: (v) => `${v} LEDs`,
  dpi: (v) => `${v} DPI`,
  deg: (v) => `${v} °C`,
  rot: (v) => `${v} °`,
  sec: (v) => (Number(v) ? `${v} s` : document.documentElement.lang === "en" ? "Instant" : "Instantané"),
  ms: (v) => (Number(v) ? `${v} ms` : document.documentElement.lang === "en" ? "Disabled" : "Désactivé"),
  ramspeed: (v) => {
    const en = ["Very fast", "Fast", "Normal", "Slow", "Very slow"];
    const fr = ["Très rapide", "Rapide", "Normal", "Lent", "Très lent"];
    return (document.documentElement.lang === "en" ? en : fr)[Number(v)] || "Normal";
  },
};

export const MACRO_PRESETS = {
  double_click: {
    label: "Double clic gauche",
    steps: [
      { kind: "mouse_down", value: 1 }, { kind: "delay", value: 15 }, { kind: "mouse_up", value: 1 },
      { kind: "delay", value: 25 },
      { kind: "mouse_down", value: 1 }, { kind: "delay", value: 15 }, { kind: "mouse_up", value: 1 },
    ],
  },
  rapid_e: {
    label: "Double appui sur E",
    steps: [
      { kind: "key_down", value: 0x08 }, { kind: "delay", value: 20 }, { kind: "key_up", value: 0x08 },
      { kind: "delay", value: 30 },
      { kind: "key_down", value: 0x08 }, { kind: "delay", value: 20 }, { kind: "key_up", value: 0x08 },
    ],
  },
  jump_crouch: {
    label: "Saut + accroupi (Espace + Ctrl)",
    steps: [
      { kind: "key_down", value: 0x2c }, { kind: "delay", value: 25 }, { kind: "key_down", value: 0xe0 },
      { kind: "delay", value: 60 }, { kind: "key_up", value: 0xe0 }, { kind: "key_up", value: 0x2c },
    ],
  },
};

export const BTN_NAMES = ["M1 · Clic gauche", "M2 · Clic droit", "M3 · Molette", "M4 · Arrière", "M5 · Avant"];
export const POLICIES = [["Silent", "Silencieux"], ["Standard", "Standard"], ["Turbo", "Turbo"], ["FullSpeed", "Pleine vitesse"], ["Manual", "Courbe perso."]];
export const STATUS_LABEL = { ok: "OK", warn: "À vérifier", error: "Problème" };
export const TAG_LABEL = { profil: "Profil", repertoire: "Répertoire", macro: "Macro", lighting: "Éclairage HWP" };

export function esc(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export const arr = (x) => (Array.isArray(x) ? x : []);
export const num = (x, d = 0) => (typeof x === "number" && Number.isFinite(x) ? x : d);
export const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
export const clone = (o) => JSON.parse(JSON.stringify(o));

export function safeHex(h, d = "#00D4FF") {
  const s = String(h || "").trim();
  return /^#[0-9a-fA-F]{6}$/.test(s) ? s.toUpperCase() : d;
}

export const kebab = (k) => k.replace(/[A-Z]/g, (m) => "-" + m.toLowerCase());

export function dataAttrs(obj) {
  return Object.entries(obj || {})
    .filter(([, v]) => v !== undefined && v !== null)
    .map(([k, v]) => `data-${kebab(k)}="${esc(v)}"`)
    .join(" ");
}

export const fmtHz = (hz) => (hz >= 1000 ? `${hz / 1000} kHz` : `${hz} Hz`);

export function fmtDate(ms) {
  const n = Number(ms);
  const en = document.documentElement.lang === "en";
  if (!Number.isFinite(n) || n <= 0) return en ? "Unknown date" : "Date inconnue";
  try {
    return new Date(n).toLocaleString(en ? "en-US" : "fr-FR", { day: "2-digit", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit" });
  } catch (_) {
    return new Date(n).toISOString();
  }
}

export function fmtSize(b) {
  const n = Number(b);
  const en = document.documentElement.lang === "en";
  if (!Number.isFinite(n)) return "—";
  if (n < 1024) return `${n} ${en ? "B" : "o"}`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} ${en ? "KB" : "Ko"}`;
  return `${(n / 1024 / 1024).toFixed(1)} ${en ? "MB" : "Mo"}`;
}

export const plural = (n, one, many) => `${n} ${n > 1 ? many : one}`;

export function setTxt(id, txt) {
  const el = document.getElementById(id);
  if (el) el.textContent = txt;
}

export function setBar(id, pct) {
  const el = document.getElementById(id);
  if (el) el.style.width = `${clamp(num(pct), 0, 100)}%`;
}

export function updateLabel(el) {
  const lbl = el.dataset.label && document.getElementById(el.dataset.label);
  if (!lbl) return;
  const f = LABEL_FMT[el.dataset.fmt];
  lbl.textContent = f ? f(el.value) : el.dataset.unit ? `${el.value} ${el.dataset.unit}` : el.value;
}

const throttles = {};
export function throttled(key, ms, fn) {
  const t = throttles[key] || (throttles[key] = { last: 0, timer: null, fn: null });
  t.fn = fn;
  const run = () => {
    t.last = Date.now();
    t.timer = null;
    const f = t.fn;
    t.fn = null;
    if (f) f();
  };
  const wait = ms - (Date.now() - t.last);
  if (wait <= 0) run();
  else if (!t.timer) t.timer = setTimeout(run, wait);
}

export function downloadText(filename, text) {
  const blob = new Blob([text], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export const devId = (el) => el.dataset.id || "";
export const domId = (id) => String(id || "").replace(/[^a-zA-Z0-9_-]/g, "_");
export function inputVal(id) {
  const el = document.getElementById(id);
  return el ? el.value : null;
}
