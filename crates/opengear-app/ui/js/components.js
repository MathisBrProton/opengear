import { ICONS, SWATCHES, arr, clamp, dataAttrs, esc, fmtHz, num, setBar, setTxt } from "./utils.js";
import { S, isExpert, k70Image, local } from "./state.js";

export function pageHeader(title, subHtml, actionsHtml = "", accent = "") {
  return `
    <header class="page-header">
      <div class="page-title-wrap">
        <div class="page-bar ${accent}"></div>
        <div><h1 class="page-title">${esc(title)}</h1>${subHtml ? `<div class="page-sub">${subHtml}</div>` : ""}</div>
      </div>
      ${actionsHtml ? `<div class="header-actions">${actionsHtml}</div>` : ""}
    </header>`;
}

export function subtabs(group, items, current) {
  return `<div class="subtabs" role="tablist" aria-label="Sections">${items
    .map((it) => `<button type="button" role="tab" class="subtab ${it.id === current ? "active" : ""}" aria-selected="${it.id === current}" data-act="subtab" data-group="${group}" data-id="${it.id}">${esc(it.label)}${it.badge ? `<span class="subtab-badge">${esc(it.badge)}</span>` : ""}</button>`)
    .join("")}</div>`;
}

export function emptyState(title, text, actionHtml = "") {
  return `
    <div class="empty">
      <div class="empty-icon">${ICONS.plug}</div>
      <div class="empty-title">${esc(title)}</div>
      <div class="empty-text">${esc(text)}</div>
      ${actionHtml ? `<div class="row mt-8" style="justify-content:center">${actionHtml}</div>` : ""}
    </div>`;
}

export function advancedHint(text) {
  if (isExpert()) return "";
  return `
    <div class="adv-hint">${ICONS.lock}
      <span>${esc(text || "Des réglages avancés sont masqués en mode Simple.")}</span>
      <button type="button" class="btn btn-sm btn-ghost" data-act="set-mode" data-mode="expert">Afficher le mode Expert</button>
    </div>`;
}

export const expertTag = () => `<span class="expert-tag">Expert</span>`;

export function sw(checked, labelHtml, change, data = {}, opts = {}) {
  return `
    <label class="switch">
      <input type="checkbox" role="switch" ${checked ? "checked" : ""} ${opts.disabled ? "disabled" : ""} data-change="${change}" ${dataAttrs(data)} ${opts.id ? `id="${opts.id}"` : ""} />
      <span class="switch-track"></span>
      <span>${labelHtml}</span>
    </label>`;
}

export function seg(items, current, act, data = {}, cls = "") {
  return `<div class="seg-group" role="group">${items
    .map(([val, label]) => `<button type="button" class="seg-btn ${cls} ${String(val) === String(current) ? "active" : ""}" aria-pressed="${String(val) === String(current)}" data-act="${act}" ${dataAttrs(data)} data-val="${esc(val)}">${esc(label)}</button>`)
    .join("")}</div>`;
}

export function swatches(act, current, data = {}) {
  const cur = String(current || "").toUpperCase();
  return `<div class="swatch-row" role="group" aria-label="Couleurs rapides">${SWATCHES.map(
    ([n, hex]) => `<button type="button" class="swatch ${cur === hex ? "active" : ""}" style="background:${hex}" title="${esc(n)}" aria-label="${esc(n)}" data-act="${act}" ${dataAttrs(data)} data-color="${hex}"></button>`
  ).join("")}</div>`;
}

export function sliderRow(label, { id, min, max, step = 1, value, input, change, data = {}, display }) {
  return `
    <div class="slider-row">
      <label class="field-label" for="${id}">${esc(label)}</label>
      <input type="range" class="og-range" id="${id}" min="${min}" max="${max}" step="${step}" value="${esc(value)}" ${input ? `data-input="${input}"` : ""} ${change ? `data-change="${change}"` : ""} ${dataAttrs(data)} data-label="${id}-val" />
      <span class="val" id="${id}-val">${esc(display != null ? display : value)}</span>
    </div>`;
}

export function batteryBadge(pct, charging) {
  const p = num(pct, -1);
  if (p < 0) return "";
  const cls = p <= 15 ? "low" : p <= 35 ? "mid" : "";
  return `<span class="battery ${cls}" title="Batterie ${p} %${charging ? " (en charge)" : ""}">${ICONS.battery}<span>${p} %${charging ? " · en charge" : ""}</span></span>`;
}

export function deviceVisual(d, cls = "") {
  if (d.img) return `<img src="${esc(d.img)}" class="${cls} ${d.imgCls || ""}" alt="" />`;
  return `<span class="glyph">${ICONS[d.icon] || ICONS.chip}</span>`;
}

export function quickBtn(q) {
  return `<button type="button" class="btn btn-sm btn-secondary" data-act="${q.act}" ${dataAttrs(q.data)} ${q.title ? `title="${esc(q.title)}"` : ""}>${esc(q.label)}</button>`;
}

export function telemetryTiles() {
  return `
    <div class="tel-grid">
      <div class="tel-tile"><div class="card-eyebrow">Processeur</div><div class="tel-val" id="tel-cpu-val">—</div><div class="tel-sub" id="tel-cpu-sub">${esc(S.static_info.cpu_name || "")}</div><div class="bar-track"><div class="bar-fill" id="tel-cpu-bar" style="width:0;background:var(--cyan)"></div></div></div>
      <div class="tel-tile"><div class="card-eyebrow">Carte graphique</div><div class="tel-val" id="tel-gpu-val" style="color:#ff8aa2">—</div><div class="tel-sub" id="tel-gpu-sub">${esc(S.static_info.gpu_name || "")}</div><div class="bar-track"><div class="bar-fill" id="tel-gpu-bar" style="width:0;background:var(--rog)"></div></div></div>
      <div class="tel-tile"><div class="card-eyebrow">Mémoire vive</div><div class="tel-val" id="tel-ram-val">—</div><div class="tel-sub" id="tel-ram-sub"></div><div class="bar-track"><div class="bar-fill" id="tel-ram-bar" style="width:0;background:var(--purple)"></div></div></div>
      <div class="tel-tile"><div class="card-eyebrow">Températures</div><div class="tel-val" id="tel-temp-val" style="color:var(--amber)">—</div><div class="tel-sub" id="tel-temp-sub"></div><div class="bar-track"><div class="bar-fill" id="tel-temp-bar" style="width:0;background:var(--amber)"></div></div></div>
    </div>`;
}

export function sensorChips(tel) {
  const s = (tel && tel.sensors) || {};
  const chips = [];
  if (s.cpu_temp_c != null) chips.push(`<div class="sensor-chip">Processeur <strong>${num(s.cpu_temp_c).toFixed(1)} °C</strong></div>`);
  if (tel && tel.gpu && tel.gpu.temp_c != null) chips.push(`<div class="sensor-chip">Carte graphique <strong>${Math.round(tel.gpu.temp_c)} °C</strong></div>`);
  if (s.motherboard_temp_c != null) chips.push(`<div class="sensor-chip">Carte mère <strong>${num(s.motherboard_temp_c).toFixed(1)} °C</strong></div>`);
  if (s.aio_pump_rpm != null) chips.push(`<div class="sensor-chip">Pompe <strong>${num(s.aio_pump_rpm)} tr/min</strong></div>`);
  arr(s.fans).forEach((f) => chips.push(`<div class="sensor-chip">${esc(f.name)} <strong>${num(f.rpm)} tr/min</strong></div>`));
  const lb = tel && tel.laptop_battery;
  if (lb && lb.present && lb.percent != null) {
    const pwr = lb.power_watts != null ? ` (${lb.power_watts > 0 ? "+" : ""}${num(lb.power_watts).toFixed(0)} W)` : lb.ac_online ? " (Secteur)" : "";
    chips.push(`<div class="sensor-chip">Batterie <strong>${num(lb.percent)} %${pwr}</strong></div>`);
  }
  return chips.length ? chips.join("") : `<span class="muted">Aucun capteur de vitesse disponible. Lance LibreHardwareMonitor en arrière-plan ou installe PawnIO (mode Expert) pour voir les vitesses ici.</span>`;
}

export function updateTelemetryDom(tel) {
  if (!tel) return;
  const sensors = tel.sensors || {};
  const gpu = tel.gpu || {};
  const cpuPct = num(tel.cpu_usage_pct);
  const cpuT = sensors.cpu_temp_c;
  const gpuT = gpu.temp_c;
  const ramUsed = num(tel.ram_used_gb);
  const ramTot = num(tel.ram_total_gb);
  const mini = document.getElementById("footer-tel-mini");
  if (mini) {
    const parts = [`CPU ${Math.round(cpuPct)} %${cpuT != null ? ` · ${Math.round(cpuT)} °C` : ""}`];
    if (gpuT != null) parts.push(`GPU ${Math.round(gpuT)} °C${gpu.power_watts != null ? ` · ${Math.round(gpu.power_watts)}W` : ""}`);
    if (sensors.aio_pump_rpm != null) parts.push(`Pompe ${num(sensors.aio_pump_rpm)} tr/min`);
    const lb = tel.laptop_battery;
    if (lb && lb.present && lb.percent != null) parts.push(`Bat ${num(lb.percent)} %`);
    parts.push(`RAM ${ramUsed.toFixed(1)} Go`);
    mini.textContent = parts.join("  ·  ");
  }
  if (document.getElementById("tel-cpu-val")) {
    setTxt("tel-cpu-val", `${Math.round(cpuPct)} %`);
    setTxt("tel-cpu-sub", `${cpuT != null ? `${Math.round(cpuT)} °C · ` : ""}${S.static_info.cpu_name || "Charge processeur"}`);
    setBar("tel-cpu-bar", cpuPct);
    if (gpu.available && gpuT != null) {
      setTxt("tel-gpu-val", `${Math.round(gpuT)} °C`);
      const extra = gpu.power_watts != null ? ` · ${Math.round(gpu.power_watts)} W` : gpu.core_clock_mhz ? ` · ${gpu.core_clock_mhz} MHz` : "";
      setTxt("tel-gpu-sub", `Charge ${num(gpu.activity_pct)} %${extra}`);
      setBar("tel-gpu-bar", (gpuT / 95) * 100);
    } else {
      setTxt("tel-gpu-val", "—");
      setTxt("tel-gpu-sub", "Capteur GPU indisponible");
    }
    setTxt("tel-ram-val", `${ramUsed.toFixed(1)} Go`);
    setTxt("tel-ram-sub", ramTot ? `sur ${Math.round(ramTot)} Go (${Math.round(num(tel.ram_usage_pct))} %)` : "");
    setBar("tel-ram-bar", tel.ram_usage_pct);
    const hot = [cpuT, sensors.motherboard_temp_c, gpuT].filter((x) => x != null);
    if (hot.length) {
      const mx = Math.max(...hot);
      setTxt("tel-temp-val", `${Math.round(mx)} °C`);
      const sub2 = [];
      if (sensors.motherboard_temp_c != null) sub2.push(`Carte mère ${Math.round(sensors.motherboard_temp_c)} °C`);
      if (sensors.aio_pump_rpm != null) sub2.push(`Pompe ${num(sensors.aio_pump_rpm)} tr/min`);
      setTxt("tel-temp-sub", sub2.join(" · ") || "Composant le plus chaud");
      setBar("tel-temp-bar", (mx / 95) * 100);
    } else {
      setTxt("tel-temp-val", "—");
      setTxt("tel-temp-sub", "Capteurs de température indisponibles");
    }
  }
  const live = document.getElementById("cool-live");
  if (live) live.innerHTML = sensorChips(tel);
  const audioOut = document.getElementById("wa-out-state");
  if (audioOut && tel.audio) audioOut.textContent = tel.audio.output_muted ? "Muet" : `${num(tel.audio.output_volume_pct)} %`;
}

export function renderKeyboard(mode, kb) {
  const kbv = S.keyboard;
  const bindings = S.config.profile.bindings || {};
  const maxX = num(kbv.width, 1);
  const maxY = num(kbv.height, 1);
  const hasPhoto = !!kbv.device_name;
  const photo = hasPhoto ? k70Image() : null;
  const showPhoto = hasPhoto && mode === "paint" && local.kbViewMode === "photo";
  if (!kbv.keys.length) return `<div class="note">Disposition du clavier indisponible.</div>`;
  return `
    <div class="kb-chassis">
      <div class="kb-topbar">
        <div class="kb-brand-strip">
          <span>${esc(((kb && kb.product_name) || kbv.device_name || "Clavier").toUpperCase())}</span>
          <span class="pill" title="Disposition Windows active">${esc(kbv.os_layout.top_row || "?")} · ${esc(String(kbv.standard || "").toUpperCase())}</span>
          ${kb && kb.pollrate_hz ? `<span class="pill emerald"><span class="dot"></span>${fmtHz(kb.pollrate_hz)}</span>` : ""}
          ${kb && kb.esports_switch_on ? `<span class="pill rog"><span class="dot"></span>Mode tournoi</span>` : ""}
        </div>
        ${mode === "paint" && hasPhoto
          ? seg([["interactive", "Vue interactive"], ["photo", "Photo"]], local.kbViewMode, "kb-view")
          : mode === "paint"
            ? `<span class="muted">Clique sur une touche pour la peindre.</span>`
            : `<span class="muted">Clique sur une touche pour la configurer. Point orange = touche modifiée.</span>`}
      </div>
      ${showPhoto
        ? `<div style="padding:8px 0"><img class="kb-real-photo ${photo.official ? "cutout" : ""}" src="${esc(photo.url)}" alt="Photo du clavier" /></div>`
        : `<div class="kb-canvas-wrap" role="group" aria-label="Touches du clavier" style="aspect-ratio:${maxX} / ${maxY};max-width:${Math.round((300 * maxX) / maxY)}px">
            ${kbv.keys.map((k) => {
              const left = (num(k.x) / maxX) * 100;
              const top = (num(k.y) / maxY) * 100;
              const width = ((num(k.w, 1) - 0.1) / maxX) * 100;
              const height = ((num(k.h, 1) - 0.12) / maxY) * 100;
              const label = k.label;
              const usable = mode === "paint" ? k.led_index != null : !!k.id;
              const isSel = !!k.id && local.selectedKeyId === k.id;
              const isBound = !!k.id && !!bindings[k.id];
              return `<button type="button" class="kb-key ${isSel ? "selected" : ""} ${isBound ? "bound" : ""}" ${usable ? "" : "disabled"} data-act="kb-key" data-key-id="${esc(k.id || "")}" data-led="${k.led_index == null ? -1 : num(k.led_index)}" data-nx="${num(k.norm_x)}" data-ny="${num(k.norm_y)}" style="left:${left.toFixed(2)}%;top:${top.toFixed(2)}%;width:${width.toFixed(2)}%;height:${height.toFixed(2)}%" aria-label="Touche ${esc(label)}${isBound ? " (modifiée)" : ""}" ${mode === "macro" ? `aria-pressed="${isSel}"` : ""}>${esc(label)}</button>`;
            }).join("")}
          </div>`}
    </div>`;
}

/** Keyboard format / standard selector shown above the key editor. */
export function keyboardSetup() {
  const kbv = S.keyboard;
  const os = kbv.os_layout || {};
  const opt = (val, label, cur) => `<option value="${esc(val)}" ${String(val) === String(cur) ? "selected" : ""}>${esc(label)}</option>`;
  const formCur = kbv.form_factor_auto ? "auto" : kbv.form_factor;
  const stdCur = kbv.standard_auto ? "auto" : kbv.standard;
  if (kbv.device_name) {
    return `<div class="note mb-16">Carte exacte de <strong>${esc(kbv.device_name)}</strong> (${esc(String(kbv.standard || "").toUpperCase())}, lu dans le clavier). Libellés de la disposition Windows active : <strong>${esc(os.top_row || "?")}</strong>.</div>`;
  }
  const autoForm = (kbv.form_factors.find((f) => f.id === kbv.form_factor) || {}).label || "?";
  return `
    <div class="card mb-16">
      <div class="card-head"><div><h2 class="card-title">Mon clavier</h2><div class="card-desc">Les libellés suivent la disposition Windows active (<strong>${esc(os.top_row || "?")}</strong>, langue ${esc(os.lang_id || "?")}) et s'adaptent si tu en changes. Choisis la forme de ton clavier si elle ne correspond pas.</div></div></div>
      <div class="row">
        <label class="field-label" for="kb-form">Format</label>
        <select id="kb-form" class="og-select auto" data-change="kb-form">
          ${opt("auto", `Automatique (${autoForm})`, formCur)}
          ${kbv.form_factors.map((f) => opt(f.id, f.label, formCur)).join("")}
        </select>
        <label class="field-label" for="kb-std">Norme</label>
        <select id="kb-std" class="og-select auto" data-change="kb-std">
          ${opt("auto", `Automatique (${String(kbv.standard || "").toUpperCase()})`, stdCur)}
          ${opt("iso", "ISO — Entrée en L, touche < > (Europe)", stdCur)}
          ${opt("ansi", "ANSI — Entrée horizontale (États-Unis)", stdCur)}
        </select>
      </div>
    </div>`;
}
