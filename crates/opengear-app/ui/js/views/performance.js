import { ICONS, POLICIES, arr, clamp, clone, cssVar, domId, esc, fmtHz, num } from "../utils.js";
import { S, curveDevice, deviceById, isExpert, local } from "../state.js";
import { advancedHint, coolingDeviceIds, deviceSettings, emptyState, expertTag, pageHeader, seg, sensorChips, sliderRow, subtabs } from "../components.js";

export function viewPerformance() {
  const items = [
    { id: "mouse", label: "Souris & clavier" },
    { id: "cooling", label: "Ventilation" },
    { id: "power", label: "Alimentation" },
  ];
  const cur = local.sub.performance;
  let body;
  if (cur === "cooling") body = viewCooling();
  else if (cur === "power") body = cardPowerPlans();
  else body = perfInputs();
  return `
    ${pageHeader("Performances", "Précision de la souris, réactivité du clavier, ventilation et énergie.", "", "amber")}
    ${subtabs("performance", items, cur)}
    ${body}`;
}

function perfInputs() {
  const cards = [];
  arr(S.device_list).forEach((d) => {
    const id = d.info.id;
    const kind = d.info.kind;
    if (d.pointer) {
      cards.push(cardPointer(d));
      return;
    }
    if (kind !== "mouse" && kind !== "keyboard") return;
    const html = deviceSettings(id, { sections: ["performance", "input"], bare: true });
    if (!html) return;
    const poll = d.settings && d.settings.find((s) => s.id === "polling_rate");
    cards.push(`
      <div class="card accent-cyan">
        <div class="card-head">
          <div><h2 class="card-title">${esc(d.info.model)}</h2><div class="card-desc">${esc(d.info.vendor)} · Réglages matériels</div></div>
          <span class="pill ${poll ? "emerald" : "cyan"}">${poll && poll.value ? fmtHz(num(poll.value)) : "Connecté"}</span>
        </div>
        ${html}
      </div>`);
  });
  if (!cards.length) {
    return emptyState("Aucune souris ni clavier réglable détecté", "Branche une souris ou un clavier compatible (Logitech G, Corsair, HyperX, SteelSeries, Razer…), puis relance la recherche.",
      `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`);
  }
  return `<div class="grid-2">${cards.join("")}</div>${advancedHint("Angle snapping et sensibilité X/Y séparée sont disponibles en mode Expert.")}`;
}

export function cardPointer(dev) {
  const id = dev.info.id;
  const did = domId(id);
  const dpi = dev.pointer;
  const ex = isExpert();
  const stages = arr(dpi && dpi.stages);
  const activeStage = dpi && dpi.active_stage != null ? num(dpi.active_stage) : 0;
  const rrHz = dpi && dpi.report_rate_hz;
  if (!dpi) {
    const html = deviceSettings(id, { sections: ["performance"], bare: true });
    return html
      ? `<div class="card accent-cyan"><div class="card-head"><h2 class="card-title">${esc(dev.info.model)}</h2></div>${html}</div>`
      : `<div class="card"><h2 class="card-title">${esc(dev.info.model)}</h2><div class="muted mt-8">Cette souris n'expose pas de réglage de sensibilité.</div></div>`;
  }
  const presets = dpi.max_dpi > 0
    ? [400, 800, 1200, 1600, 3200, 6400].filter((v) => v >= num(dpi.min_dpi, 100) && v <= num(dpi.max_dpi, 25600))
    : [];
  const pillText = `${num(dpi.dpi_x, stages[activeStage] || 0)} DPI${rrHz ? ` · ${fmtHz(num(rrHz))}` : ""}`;
  return `
    <div class="card accent-cyan">
      <div class="card-head">
        <div><h2 class="card-title">${esc(dev.info.model)}</h2><div class="card-desc">${stages.length && !presets.length ? "Paliers de sensibilité (bouton DPI de la souris)" : "Sensibilité et fréquence d'envoi"}</div></div>
        <span class="pill cyan">${pillText}</span>
      </div>
      ${presets.length ? `
        <div class="row" role="group" aria-label="DPI rapides">
          ${presets.map((v) => `<button type="button" class="btn btn-sm ${num(dpi.dpi_x) === v ? "btn-primary" : "btn-secondary"}" data-act="ptr-dpi" data-id="${esc(id)}" data-dpi="${v}" aria-pressed="${num(dpi.dpi_x) === v}">${v}</button>`).join("")}
        </div>
        <div class="stack mt-12 gap-10px">
          ${sliderRow(ex && dpi.independent_xy ? "DPI horizontal (X)" : "DPI précis", { id: `pdpi-${did}`, min: num(dpi.min_dpi, 100), max: num(dpi.max_dpi, 25600), step: num(dpi.step_dpi, 50) || 50, value: num(dpi.dpi_x, 800), input: "label", change: "ptr-dpi-slider", data: { device: id, fmt: "dpi" }, display: `${num(dpi.dpi_x, 800)} DPI` })}
          ${ex && dpi.independent_xy ? sliderRow("DPI vertical (Y)", { id: `pdpiy-${did}`, min: num(dpi.min_dpi, 100), max: num(dpi.max_dpi, 25600), step: num(dpi.step_dpi, 50) || 50, value: num(dpi.dpi_y, dpi.dpi_x), input: "label", change: "ptr-dpi-slider", data: { device: id, fmt: "dpi" }, display: `${num(dpi.dpi_y, dpi.dpi_x)} DPI` }) : ""}
        </div>
      ` : ""}
      ${stages.length ? `
        <div class="stack mt-12 gap-2">
          ${stages.map((v, s) => `
            <div class="row-nowrap">
              <button type="button" class="btn btn-sm ${activeStage === s ? "btn-primary" : "btn-secondary"} min-w-96" data-act="ptr-stage" data-id="${esc(id)}" data-stage="${s}" aria-pressed="${activeStage === s}">Palier ${s + 1}${activeStage === s ? " (actif)" : ""}</button>
              <input type="number" class="og-input narrow" min="100" max="30000" step="50" value="${num(v)}" id="pstage-${did}-${s}" aria-label="DPI du palier ${s + 1}" />
              <button type="button" class="btn btn-sm btn-secondary" data-act="ptr-stage-save" data-id="${esc(id)}" data-stage="${s}">Enregistrer</button>
            </div>`).join("")}
        </div>` : ""}
      ${dpi.supports_lod ? `
        <div class="row-between pair-row mt-12">
          <span class="field-label">Distance de décrochage</span>
          ${seg([["Low", "Basse"], ["Medium", "Moyenne"], ["High", "Haute"]], dpi.lod, "ptr-lod", { id })}
        </div>` : ""}
      <div class="mt-12">${deviceSettings(id, { sections: ["performance"], bare: true })}</div>
    </div>`;
}

function cardPowerPlans() {
  const showLaptop = !!(S.is_laptop || (S.laptop_battery && S.laptop_battery.present));
  const tel = S.telemetry || {};
  const gpu = tel.gpu || {};
  const sensors = tel.sensors || {};
  const sp = S.system_prefs || {};
  const plansCard = !S.power_plans.length
    ? emptyState("Aucun mode d'alimentation trouvé", "Windows n'a renvoyé aucun mode d'alimentation.")
    : `
      <div class="card">
        <div class="card-head"><div><h2 class="card-title">Mode d'alimentation Windows</h2><div class="card-desc">Choisis le compromis entre performances et consommation.</div></div></div>
        <div class="plan-grid">
          ${S.power_plans.map((p) => `
            <div class="plan-card ${p.active ? "active" : ""}">
              <div>
                <div class="fw-650 txt-white">${esc(p.name)}</div>
                ${isExpert() ? `<div class="mono muted small mt-3">${esc(p.guid)}</div>` : ""}
              </div>
              <div>${p.active ? `<span class="pill cyan"><span class="dot"></span>Actif</span>` : `<button type="button" class="btn btn-sm btn-secondary" data-act="power-plan" data-guid="${esc(p.guid)}">Activer</button>`}</div>
            </div>`).join("")}
        </div>
      </div>`;
  const rightCard = showLaptop
    ? cardLaptopPower()
    : `
      <div class="card accent-cyan">
        <div class="card-head">
          <div><h2 class="card-title">Consommation &amp; veille automatique</h2><div class="card-desc">Mesure en direct et économie d'éclairage configurée sur ce PC.</div></div>
          <button type="button" class="btn btn-sm btn-ghost" data-act="nav" data-tab="system" data-sub="prefs">Réglages auto</button>
        </div>
        <div class="list">
          <div class="list-row"><span class="muted">Processeur</span><strong class="mono">${Math.round(num(tel.cpu_usage_pct))} %${sensors.cpu_temp_c != null ? ` · ${Math.round(num(sensors.cpu_temp_c))} °C` : ""}</strong></div>
          <div class="list-row"><span class="muted">Carte graphique</span><strong class="mono">${gpu.available && gpu.power_watts != null ? `${Math.round(num(gpu.power_watts))} W` : gpu.available && gpu.temp_c != null ? `${Math.round(num(gpu.temp_c))} °C` : "—"}</strong></div>
          <div class="list-row"><span class="muted">Coupure RGB sur verrouillage</span><strong>${sp.lights_off_on_lock !== false ? "Activée" : "Désactivée"}</strong></div>
          <div class="list-row"><span class="muted">Coupure RGB sur inactivité</span><strong>${num(sp.idle_lights_off_min) > 0 ? `${num(sp.idle_lights_off_min)} min` : "Jamais"}</strong></div>
        </div>
      </div>`;
  return `<div class="grid-2 grid-balanced">${plansCard}${rightCard}</div>`;
}

function cardLaptopPower() {
  const b = S.laptop_battery || {};
  const pwrText = b.power_watts != null ? `${b.power_watts > 0 ? "+" : ""}${num(b.power_watts).toFixed(1)} W` : b.ac_online ? "Sur secteur" : "Sur batterie";
  // Built-in panel and other devices with power settings (brightness, refresh rate…).
  const powerSettings = arr(S.device_list)
    .map((d) => deviceSettings(d.info.id, { sections: ["power"], bare: true }))
    .filter(Boolean)
    .join("");
  return `
    <div class="card accent-cyan mb-16">
      <div class="card-head">
        <div>
          <h2 class="card-title">Batterie & Écran Portable</h2>
          <div class="card-desc">${b.present ? `Batterie ${num(b.percent)} % · ${pwrText}${b.health_pct != null ? ` · Santé ${num(b.health_pct).toFixed(0)} %` : ""}${b.cycle_count != null ? ` · ${num(b.cycle_count)} cycles` : ""}` : "Gestion de l'écran intégré"}</div>
        </div>
        ${b.present ? `<span class="pill ${b.ac_online ? "emerald" : "amber"}"><span class="dot"></span>${b.ac_online ? "Secteur" : "Batterie"}</span>` : ""}
      </div>
      ${powerSettings ? `<div class="mb-16">${powerSettings}</div>` : ""}
      ${b.present ? `<div>${sw(!!S.system_prefs.laptop_auto_battery_saver, "Économie auto sur batterie (écran à 60 Hz, profil silencieux et clavier éteint quand c'est possible ; restauré sur secteur)", "laptop-auto-bat")}</div>` : ""}
    </div>`;
}

const DEFAULT_SW_CURVE = () => ([
  { temp_c: 30, duty_pct: 25 }, { temp_c: 40, duty_pct: 30 },
  { temp_c: 50, duty_pct: 40 }, { temp_c: 60, duty_pct: 50 },
  { temp_c: 70, duty_pct: 65 }, { temp_c: 78, duty_pct: 80 },
  { temp_c: 85, duty_pct: 95 }, { temp_c: 92, duty_pct: 100 },
]);

function normTempSource(src) {
  if (src === "GpuLive") return "GpuEdge";
  if (src === "MixCpuGpu") return "MaxCpuGpu";
  return src || "Cpu";
}

function normSwSource(src) {
  const map = { cpu_bios: "Cpu", cpu: "Cpu", gpu_edge: "GpuEdge", gpu_hotspot: "GpuHotspot", nvme: "Nvme", max_cpu_gpu: "MaxCpuGpu", avg_cpu_gpu: "AvgCpuGpu", max_all: "MaxAll", custom_mix: "CustomMix" };
  return map[src] || normTempSource(src);
}

/** Collect every controllable `Cooling` fan across devices, omitting ASUS WMI duplicate when BIOS curves or SuperIO already cover it. */
export function unifiedCoolingFans() {
  const hasSuperio = arr(S && S.device_list).some((d) => d.info.id === "superio:mainboard");
  const hasBiosCurves = !!curveDevice();
  const out = [];
  coolingDeviceIds().forEach((devId) => {
    if (devId === "asus_wmi_cooling:mainboard" && (hasSuperio || hasBiosCurves)) return;
    const d = deviceById(devId);
    const live = S && S.telemetry && S.telemetry.device_fans && S.telemetry.device_fans[devId];
    const c = live || (d && d.cooling);
    if (!d || !c) return;
    arr(c.fans).forEach((f) => {
      out.push({
        key: `${devId}::${f.id}`,
        deviceId: devId,
        deviceName: d.info.model,
        kind: d.info.kind,
        fan: f,
      });
    });
  });
  return out;
}

export function getSwFanDraft(entry) {
  const key = entry.key;
  if (!local.fanRuleDrafts[key]) {
    const saved = (S && S.config && S.config.profile && S.config.profile.fan_rules && S.config.profile.fan_rules[key]) || null;
    const floor = num(entry.fan.min_pct, 30);
    const mode = saved
      ? String(saved.mode || "auto").toLowerCase()
      : entry.fan.manual_pct != null ? "fixed" : "auto";
    local.fanRuleDrafts[key] = {
      mode,
      fixed_pct: clamp(num(saved && saved.fixed_pct, num(entry.fan.manual_pct, Math.max(60, floor))), floor, 100),
      source: normSwSource(saved ? saved.source : "Cpu"),
      custom_mix: saved && saved.custom_mix ? clone(saved.custom_mix) : { op: "max", a: "cpu", b: "gpu_edge" },
      curve: arr(saved && saved.curve).length === 8 ? clone(saved.curve) : DEFAULT_SW_CURVE(),
      hysteresis_c: num(saved && saved.hysteresis_c, 3),
      smoothing_secs: num(saved && saved.smoothing_secs, 2),
      ramp_pct_per_sec: num(saved && saved.ramp_pct_per_sec, 0),
      mix_source: normSwSource(saved ? saved.mix_source : "GpuEdge"),
      mix_custom: saved && saved.mix_custom ? clone(saved.mix_custom) : { op: "max", a: "cpu", b: "gpu_edge" },
      mix_curve: arr(saved && saved.mix_curve).length === 8 ? clone(saved.mix_curve) : DEFAULT_SW_CURVE(),
      mix_op: String((saved && saved.mix_op) || "max").toLowerCase(),
    };
  }
  return local.fanRuleDrafts[key];
}

function tempSourceOptions(selected, includeBios = false) {
  const cur = includeBios ? normTempSource(selected) : normSwSource(selected);
  const items = [
    ...(includeBios ? [["CpuBios", "Processeur (régulation BIOS)"]] : [["Cpu", "Processeur (CPU)"]]),
    ["GpuEdge", "Carte graphique (cœur)"],
    ["GpuHotspot", "Carte graphique (point chaud)"],
    ["Nvme", "SSD NVMe"],
    ["MaxCpuGpu", "Max CPU / GPU"],
    ["AvgCpuGpu", "Moyenne CPU + GPU"],
    ["MaxAll", "Max global (CPU / GPU / NVMe)"],
    ["CustomMix", "Source composée personnalisée…"],
  ];
  return items.map(([v, lbl]) => `<option value="${v}" ${cur === v ? "selected" : ""}>${lbl}</option>`).join("");
}

function customMixRow(mix, actChange, extraData) {
  const m = mix || { op: "max", a: "cpu", b: "gpu_edge" };
  const op = String(m.op || "max").toLowerCase();
  const mixA = String(m.a || m.sensor_a || "cpu").toLowerCase();
  const mixB = String(m.b || m.sensor_b || "gpu_edge").toLowerCase();
  const sensors = [
    ["cpu", "Processeur"],
    ["gpu_edge", "GPU cœur"],
    ["gpu_hotspot", "GPU point chaud"],
    ["nvme", "SSD NVMe"],
    ["motherboard", "Carte mère"],
  ];
  const attrs = Object.entries(extraData).map(([k, v]) => `data-${k}="${esc(v)}"`).join(" ");
  return `
    <div class="row mt-8 gap-2">
      <select class="og-select auto" data-change="${actChange}" data-part="op" ${attrs} aria-label="Opération thermique">
        <option value="max" ${op === "max" ? "selected" : ""}>Max(A, B)</option>
        <option value="avg" ${op === "avg" ? "selected" : ""}>Moyenne(A, B)</option>
        <option value="delta" ${op === "delta" ? "selected" : ""}>Écart |A − B|</option>
      </select>
      <select class="og-select auto" data-change="${actChange}" data-part="a" ${attrs} aria-label="Capteur A">${sensors.map(([id, lbl]) => `<option value="${id}" ${mixA === id ? "selected" : ""}>${lbl}</option>`).join("")}</select>
      <select class="og-select auto" data-change="${actChange}" data-part="b" ${attrs} aria-label="Capteur B">${sensors.map(([id, lbl]) => `<option value="${id}" ${mixB === id ? "selected" : ""}>${lbl}</option>`).join("")}</select>
    </div>`;
}

function unifiedCoolingCard(fans, sp) {
  if (!fans.length) return "";
  if (!fans.some((f) => f.key === local.selectedFanKey)) local.selectedFanKey = fans[0].key;
  const active = fans.find((f) => f.key === local.selectedFanKey) || fans[0];
  const ex = isExpert();
  const draft = getSwFanDraft(active);
  const f = active.fan;
  const floor = num(f.min_pct, 30);
  const cal = S.config && S.config.fan_calibrations && S.config.fan_calibrations[active.key];
  const calActive = sp.fan_calibration_active;
  const calBusy = !!(calActive && calActive.device_id === active.deviceId && calActive.fan_id === f.id);
  const anyCalBusy = !!calActive;
  const modeOpts = ex
    ? [["auto", "Auto (BIOS)"], ["fixed", "Vitesse fixe"], ["curve", "Courbe"], ["mix", "Mix CPU + GPU"]]
    : [["auto", "Auto (BIOS)"], ["fixed", "Vitesse fixe"], ...(draft.mode === "curve" || draft.mode === "mix" ? [[draft.mode, draft.mode === "mix" ? "Mix (mode Expert)" : "Courbe (mode Expert)", true]] : [])];

  const isMix = draft.mode === "mix";
  const editSub = isMix && local.selectedSwCurveIdx === 1 ? 1 : 0;
  const activeCurve = editSub === 1 ? draft.mix_curve : draft.curve;
  const activeSrc = editSub === 1 ? draft.mix_source : draft.source;
  const activeCustom = editSub === 1 ? draft.mix_custom : draft.custom_mix;
  const domFixed = `sw-fixed-${domId(active.key)}`;
  const devObj = deviceById(active.deviceId);
  const perfSettings = devObj && devObj.kind === "laptop" ? deviceSettings(active.deviceId, { sections: ["performance"], title: `Performances · ${active.deviceName}` }) : "";

  return `
    <div class="card accent-cyan mb-16">
      <div class="card-head">
        <div>
          <h2 class="card-title">Contrôle unifié des ventilateurs</h2>
          <div class="card-desc">Auto BIOS, vitesse fixe, courbe intelligente ou Mix CPU + GPU sur tous les ventilateurs du PC.</div>
        </div>
        <div class="row">
          <button type="button" class="btn btn-sm btn-secondary" data-act="sio-refresh" data-device="${esc(active.deviceId)}">${ICONS.refresh}Actualiser</button>
        </div>
      </div>
      ${calActive ? `
        <div class="alert info mb-12">
          <span class="alert-icon">${ICONS.spark}</span>
          <div class="grow">
            <div class="alert-title">Calibration de « ${esc(calActive.fan_name || calActive.fan_id)} » en cours (${num(calActive.step)} / ${num(calActive.total)})</div>
            <div class="muted small">Mesure des vitesses réelles jusqu'au plancher de sécurité, puis retour automatique.</div>
          </div>
          <button type="button" class="btn btn-sm btn-secondary" data-act="sw-fan-cal-cancel">Annuler</button>
        </div>` : ""}
      ${seg(fans.map((x) => {
        const rpmTxt = x.fan.rpm != null ? ` · ${num(x.fan.rpm)} tr/min` : "";
        return [x.key, `${x.fan.name}${rpmTxt}`];
      }), active.key, "sw-fan-ch", {}, "cyan")}
      <div class="row-between mt-12">
        <div class="row">
          <span class="fw-650">${esc(active.deviceName)} · ${esc(f.name)}</span>
          <span class="pill ${num(f.rpm) > 0 ? "amber" : ""}">${f.rpm != null ? `${num(f.rpm)} tr/min` : "RPM —"}</span>
          ${f.manual_pct != null ? `<span class="pill cyan">Piloté : ${num(f.manual_pct)} %</span>` : `<span class="pill emerald">Auto BIOS</span>`}
        </div>
        <div class="row">
          ${cal ? `<span class="pill" title="Résultat de calibration">${cal.min_spinning_pct != null ? `Démarrage ≥ ${cal.min_spinning_pct} %` : "0 tr/min"}${cal.max_rpm ? ` · max ${cal.max_rpm} tr/min` : ""}${cal.zero_rpm_supported ? " · 0 RPM OK" : ""}</span>` : ""}
          ${ex ? `<button type="button" class="btn btn-sm btn-secondary" data-act="sw-fan-calibrate" data-device="${esc(active.deviceId)}" data-fan="${esc(f.id)}" ${anyCalBusy ? "disabled" : ""}>${calBusy ? "Calibration…" : "Calibrer"}</button>` : ""}
        </div>
      </div>
      <div class="mt-8">${seg(modeOpts, draft.mode, "sw-fan-mode", { key: active.key })}</div>
      ${draft.mode === "fixed" ? `
        <div class="stack mt-12 gap-10px">
          ${sliderRow("Vitesse manuelle", { id: domFixed, min: floor, max: 100, value: clamp(draft.fixed_pct, floor, 100), input: "label", change: "sw-fan-fixed", data: { key: active.key, fmt: "pct100" }, display: `${clamp(draft.fixed_pct, floor, 100)} %` })}
          <div class="row">
            <button type="button" class="btn btn-primary btn-sm" data-act="sw-fan-apply" data-key="${esc(active.key)}">Appliquer la vitesse</button>
            <button type="button" class="btn btn-secondary btn-sm" data-act="sw-fan-auto" data-key="${esc(active.key)}">Auto BIOS</button>
          </div>
          <div class="muted small">Plancher de sécurité à ${floor} %. OpenGear rend les ventilateurs au BIOS à la fermeture, même après un plantage.</div>
        </div>` : ""}
      ${(draft.mode === "curve" || draft.mode === "mix") && ex ? `
        ${isMix ? `
          <div class="row-between mt-12">
            ${seg([[0, "Courbe A (CPU)"], [1, "Courbe B (GPU)"]], editSub, "sw-fan-sub", {}, "purple")}
            ${seg([["max", "Max(A, B)"], ["avg", "Moyenne(A, B)"]], draft.mix_op, "sw-fan-mix-op", { key: active.key })}
          </div>` : ""}
        <div class="row mt-12 pair-row">
          <label class="field-label" for="sw-fan-src">Température de référence${isMix ? (editSub === 1 ? " (Courbe B)" : " (Courbe A)") : ""}</label>
          <select id="sw-fan-src" class="og-select auto" data-change="sw-fan-src" data-key="${esc(active.key)}" data-sub="${editSub}">
            ${tempSourceOptions(activeSrc, false)}
          </select>
        </div>
        ${activeSrc === "CustomMix" ? customMixRow(activeCustom, "sw-fan-mix", { key: active.key, sub: String(editSub) }) : ""}
        <div class="stack mt-12 gap-10px">
          ${sliderRow("Hystérésis anti-yoyo", { id: "sw-fan-hyst", min: 0, max: 10, value: num(draft.hysteresis_c, 3), input: "label", change: "sw-fan-hyst", data: { key: active.key, fmt: "deg" }, display: `${num(draft.hysteresis_c, 3)} °C` })}
          ${sliderRow("Lissage anti-pics", { id: "sw-fan-smooth", min: 0, max: 20, value: num(draft.smoothing_secs, 2), input: "label", change: "sw-fan-smooth", data: { key: active.key, fmt: "sec" }, display: num(draft.smoothing_secs, 2) ? `${num(draft.smoothing_secs, 2)} s` : "Instantané" })}
          ${sliderRow("Rampe max (%/s)", { id: "sw-fan-ramp", min: 0, max: 50, step: 5, value: num(draft.ramp_pct_per_sec, 0), input: "label", change: "sw-fan-ramp", data: { key: active.key, fmt: "raw" }, display: num(draft.ramp_pct_per_sec, 0) ? `${num(draft.ramp_pct_per_sec, 0)} %/s` : "Instantané" })}
        </div>
        <div class="curve-wrap mt-12"><svg id="sw-curve-svg" class="curve-svg" viewBox="0 0 680 250" role="img" aria-label="Courbe logicielle de ventilation"></svg></div>
        <div class="curve-pts" id="sw-curve-pts">${swCurvePointsHtml(activeCurve, active.key, editSub)}</div>
        <div class="row mt-16">
          <button type="button" class="btn btn-primary" data-act="sw-fan-apply" data-key="${esc(active.key)}">Appliquer la courbe</button>
          <button type="button" class="btn btn-secondary" data-act="sw-fan-auto" data-key="${esc(active.key)}">Auto BIOS</button>
        </div>` : ""}
      ${(draft.mode === "curve" || draft.mode === "mix") && !ex ? `
        <div class="row mt-12">
          <span class="pill purple">${draft.mode === "mix" ? "Mix CPU + GPU actif (mode Expert)" : "Courbe logicielle active (mode Expert)"}</span>
          <button type="button" class="btn btn-secondary btn-sm" data-act="sw-fan-auto" data-key="${esc(active.key)}">Auto BIOS</button>
        </div>` : ""}
      ${draft.mode === "auto" ? `<div class="muted small mt-10">Le BIOS / firmware gère automatiquement ce ventilateur.</div>` : ""}
    </div>
    ${perfSettings ? `<div class="mb-16">${perfSettings}</div>` : ""}`;
}

function swCurvePointsHtml(curve, fanKey, subIdx) {
  return arr(curve).map((pt, i) => `
    <div class="curve-pt">
      <span>P${i + 1}</span>
      <input type="number" min="20" max="100" value="${num(pt.temp_c)}" data-change="sw-curve-pt" data-key="${esc(fanKey)}" data-sub="${subIdx}" data-pt="${i}" data-field="temp_c" aria-label="Point ${i + 1} : température °C" />
      <input type="number" min="0" max="100" value="${num(pt.duty_pct)}" data-change="sw-curve-pt" data-key="${esc(fanKey)}" data-sub="${subIdx}" data-pt="${i}" data-field="duty_pct" aria-label="Point ${i + 1} : vitesse %" />
    </div>`).join("");
}

function missingPawnIoCard(sp) {
  if (S.is_laptop || arr(S.device_list).some((d) => d.info.id === "superio:mainboard")) return "";
  return `
    <div class="card expert-card mt-16">
      <div class="card-head">
        <div><h2 class="card-title">Contrôle direct carte mère (SuperIO) ${expertTag()}</h2><div class="card-desc">Installe PawnIO et lance OpenGear en administrateur pour piloter directement tous les ventilateurs PWM de la carte mère.</div></div>
        <div class="row">
          <span class="pill amber"><span class="dot"></span>${sp.is_admin ? (sp.pawnio_ready ? "Aucune puce SuperIO supportée" : "Pilote PawnIO absent") : "Administrateur requis"}</span>
          ${!sp.pawnio_ready ? `<button type="button" class="btn btn-sm btn-warn" data-act="pawnio-install">Installer / réparer PawnIO</button>` : ""}
        </div>
      </div>
    </div>`;
}

function viewCooling() {
  const sp = S.system_prefs || {};
  const cd = curveDevice();
  const crv = cd && cd.curves;
  const ex = isExpert();
  const fans = unifiedCoolingFans();

  if (!cd && !fans.length) {
    return `
      ${emptyState("Ventilation non pilotable", sp.is_admin ? "Aucun contrôleur de ventilation compatible n'a été détecté sur cette carte mère." : "Lance OpenGear en administrateur pour accéder aux réglages de ventilation de la carte mère.", !sp.is_admin ? `<button type="button" class="btn btn-warn" data-act="relaunch-admin">Relancer en administrateur</button>` : "")}
      ${ex ? missingPawnIoCard(sp) : ""}`;
  }

  let biosHtml = "";
  if (cd) {
    const channels = arr(crv.channels);
    const ch = channels[local.selectedCoolingIdx] || channels[0];
    // Pump presets drive a pump channel only: hide them on boards without one.
    const hasPump = channels.some((x) => x.is_pump);
    const pols = ex
      ? POLICIES
      : [
          ...POLICIES.filter(([id]) => id !== "Manual"),
          ...(ch.policy === "Manual" ? [["Manual", "Courbe perso.", true]] : []),
        ];
    const curSrc = normTempSource(ch.source);
    const cdId = cd.info.id;
    biosHtml = `
      ${!crv.is_admin ? `
        <div class="alert warn mb-16">
          <span class="alert-icon">${ICONS.lock}</span>
          <div class="grow"><div class="alert-title">Droits administrateur nécessaires pour modifier les courbes BIOS</div><div class="muted">Windows protège l'accès aux réglages BIOS de la carte mère. Tu peux préparer tes réglages ici, puis relancer OpenGear en administrateur pour les appliquer.</div></div>
          <button type="button" class="btn btn-warn btn-sm" data-act="relaunch-admin">Relancer en administrateur</button>
        </div>` : ""}
      <div class="grid-stage${hasPump ? "" : " one-col"}">
        ${hasPump ? `
        <div class="stack">
          <div class="card accent-amber">
            <div class="card-head"><div><h2 class="card-title">Pompe du watercooling</h2><div class="card-desc">Un plancher de sécurité est toujours conservé.</div></div></div>
            <div class="stack gap-2">
              <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-device="${esc(cdId)}" data-preset="ultra_silent">Silence (45–95 %)</button>
              <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-device="${esc(cdId)}" data-preset="default">Équilibré (55–100 %)</button>
              <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-device="${esc(cdId)}" data-preset="max_perf">Performance (75–100 %)</button>
            </div>
          </div>
        </div>` : ""}
        <div class="card">
          <div class="card-head">
            <div><h2 class="card-title">Courbes BIOS carte mère</h2><div class="card-desc">Écrites dans le firmware de la carte mère : actives même quand OpenGear est fermé.</div></div>
            ${ex && crv.available ? `<button type="button" class="btn btn-sm btn-secondary" data-act="cool-read-bios" data-device="${esc(cdId)}">Relire le BIOS</button>` : ""}
          </div>
          ${seg(channels.map((x) => [x.index, x.short_label || x.label]), local.selectedCoolingIdx, "cool-ch", {}, "amber")}
          <div class="row-between mt-12">
            <div class="fw-650">${esc(ch.label)}</div>
            ${!ex && ch.policy === "Manual" ? `<span class="pill purple">Courbe personnalisée (mode Expert)</span>` : ""}
          </div>
          <div class="mt-8">${seg(pols, ch.policy, "cool-pol", { device: cdId })}</div>
          ${ex ? `
            <div class="row mt-12 pair-row">
              <label class="field-label" for="cool-src">Température de référence</label>
              <select id="cool-src" class="og-select auto" data-change="cool-src" data-device="${esc(cdId)}">
                ${tempSourceOptions(curSrc, true)}
              </select>
            </div>
            ${curSrc === "CustomMix" ? customMixRow(ch.custom_mix, "cool-mix", { device: cdId }) : ""}
            <div class="stack mt-12 gap-10px">
              ${sliderRow("Hystérésis anti-yoyo", { id: "cool-hyst", min: 0, max: 10, value: num(ch.hysteresis_c, 3), input: "label", change: "cool-hyst", data: { device: cdId, fmt: "deg" }, display: `${num(ch.hysteresis_c, 3)} °C` })}
              ${sliderRow("Lissage anti-pics", { id: "cool-smooth", min: 0, max: 20, value: num(ch.smoothing_secs, 0), input: "label", change: "cool-smooth", data: { device: cdId, fmt: "sec" }, display: num(ch.smoothing_secs, 0) ? `${num(ch.smoothing_secs, 0)} s` : "Instantané" })}
            </div>
            <div class="curve-wrap mt-12"><svg id="fan-curve-svg" class="curve-svg" viewBox="0 0 680 250" role="img" aria-label="Courbe de ventilation"></svg></div>
            <div class="curve-pts" id="curve-pts">${curvePointsHtml(ch, cdId)}</div>
            <div class="row mt-16">
              <button type="button" class="btn btn-primary" data-act="cool-apply" data-device="${esc(cdId)}">Appliquer à la carte mère</button>
              ${ch.is_pump ? `<button type="button" class="btn btn-secondary" data-act="sl3-preset" data-device="${esc(cdId)}" data-preset="default">Réinitialiser la pompe</button>` : ""}
            </div>` : `
            <div class="row mt-16">
              <button type="button" class="btn btn-primary" data-act="cool-apply" data-device="${esc(cdId)}">Appliquer à la carte mère</button>
            </div>`}
        </div>
      </div>`;
  }

  return `
    <div class="card mb-16">
      <div class="card-head"><div><h2 class="card-title">Capteurs en direct</h2>${ex ? `<div class="card-desc">${esc((S.telemetry.sensors && S.telemetry.sensors.source) || "")}</div>` : ""}</div></div>
      <div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div>
    </div>
    ${unifiedCoolingCard(fans, sp)}
    ${biosHtml}
    ${ex ? missingPawnIoCard(sp) : advancedHint("Courbes 8 points, Mix CPU + GPU, calibration et contrôle direct des ventilateurs sont en mode Expert.")}`;
}

function curvePointsHtml(ch, cdId) {
  return arr(ch.curve).map((pt, i) => `
    <div class="curve-pt">
      <span>P${i + 1}</span>
      <input type="number" min="20" max="100" value="${num(pt.temp_c)}" data-change="curve-pt" data-device="${esc(cdId)}" data-pt="${i}" data-field="temp_c" aria-label="Point ${i + 1} : température °C" />
      <input type="number" min="0" max="100" value="${num(pt.duty_pct)}" data-change="curve-pt" data-device="${esc(cdId)}" data-pt="${i}" data-field="duty_pct" aria-label="Point ${i + 1} : vitesse %" />
    </div>`).join("");
}

function mountCurveSvg({ svgId, ptsId, curve, minDuty, accent, secondaryCurve = null, gradId = "cg", floorLabel = null, onPointChange, onCommit = null }) {
  const svg = document.getElementById(svgId);
  if (!svg || !S || !curve || curve.length < 2) return;
  const muted = cssVar("--text-3", "#8592AB");
  const bg = cssVar("--bg-app", "#090B10");
  const rog = cssVar("--rog", "#FF3D63");
  const purple = cssVar("--purple", "#AD80FF");
  const padL = 44, padR = 18, padT = 18, padB = 30, W = 680, H = 250;
  const pw = W - padL - padR, ph = H - padT - padB;
  const tx = (t) => padL + ((t - 20) / 80) * pw;
  const dy = (d) => padT + (1 - d / 100) * ph;

  const computePath = (c) => {
    const pts = c.map((p) => [tx(num(p.temp_c, 50)), dy(num(p.duty_pct, 50))]);
    const poly = [[padL, pts[0][1]], ...pts, [W - padR, pts[pts.length - 1][1]]];
    const lineD = poly.map((p, i) => `${i === 0 ? "M" : "L"}${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(" ");
    const fillD = `${lineD} L${W - padR},${H - padB} L${padL},${H - padB} Z`;
    return { pts, lineD, fillD };
  };

  const { pts, lineD, fillD } = computePath(curve);
  let html = `<defs><linearGradient id="${gradId}" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="${accent}" stop-opacity="0.32"/><stop offset="100%" stop-color="${accent}" stop-opacity="0.02"/></linearGradient></defs>`;
  for (const d of [0, 25, 50, 75, 100]) {
    const y = dy(d);
    html += `<line x1="${padL}" y1="${y}" x2="${W - padR}" y2="${y}" stroke="rgba(255,255,255,0.06)"/><text x="${padL - 8}" y="${y + 4}" fill="${muted}" font-size="10.5" text-anchor="end" font-family="Consolas,monospace">${d}%</text>`;
  }
  for (let t = 20; t <= 100; t += 10) {
    const x = tx(t);
    html += `<line x1="${x}" y1="${padT}" x2="${x}" y2="${H - padB}" stroke="rgba(255,255,255,0.06)"/><text x="${x}" y="${H - 10}" fill="${muted}" font-size="10.5" text-anchor="middle" font-family="Consolas,monospace">${t}°C</text>`;
  }
  if (minDuty > 0) {
    const fy = dy(minDuty);
    html += `<rect x="${padL}" y="${fy}" width="${pw}" height="${H - padB - fy}" fill="rgba(255,61,99,0.08)"/><line x1="${padL}" y1="${fy}" x2="${W - padR}" y2="${fy}" stroke="rgba(255,61,99,0.45)" stroke-dasharray="4 4"/>${floorLabel ? `<text x="${padL + 8}" y="${fy + 15}" fill="rgba(255,150,170,0.9)" font-size="10.5">${esc(floorLabel)}</text>` : ""}`;
  }
  const sensors = S.telemetry.sensors || {};
  const liveT = sensors.cpu_temp_c != null ? sensors.cpu_temp_c : S.telemetry.gpu && S.telemetry.gpu.temp_c;
  if (liveT != null) {
    const lx = tx(clamp(liveT, 20, 100));
    html += `<line x1="${lx}" y1="${padT}" x2="${lx}" y2="${H - padB}" stroke="${rog}" stroke-width="1.4" stroke-dasharray="3 3"/><text x="${lx + 5}" y="${padT + 12}" fill="${rog}" font-size="10.5" font-family="Consolas,monospace">${sensors.cpu_temp_c != null ? "CPU" : "GPU"} ${Math.round(liveT)}°C</text>`;
  }
  if (secondaryCurve && secondaryCurve.length >= 2) {
    const sec = computePath(secondaryCurve);
    html += `<path d="${sec.lineD}" fill="none" stroke="${purple}" stroke-width="1.8" stroke-dasharray="5 4" opacity="0.75"/>`;
  }
  html += `<path id="${svgId}-fill" d="${fillD}" fill="url(#${gradId})"/><path id="${svgId}-line" d="${lineD}" fill="none" stroke="${accent}" stroke-width="2.6" stroke-linejoin="round"/>`;
  pts.forEach(([x, y], i) => {
    const p = curve[i];
    html += `<g class="curve-node" data-pt="${i}"><circle class="fc-hit" cx="${x}" cy="${y}" r="12" fill="transparent"/><circle class="fc-dot" cx="${x}" cy="${y}" r="6.5" fill="${accent}" stroke="${bg}" stroke-width="2"/><title>Point ${i + 1} : ${num(p.temp_c)} °C → ${num(p.duty_pct)} %</title></g>`;
  });
  svg.innerHTML = html;

  const fillEl = svg.querySelector(`#${svgId}-fill`);
  const lineEl = svg.querySelector(`#${svgId}-line`);
  const nodeEls = [...svg.querySelectorAll(".curve-node")];

  const syncDomInPlace = (idx) => {
    const { pts: curPts, lineD: nextLine, fillD: nextFill } = computePath(curve);
    if (fillEl) fillEl.setAttribute("d", nextFill);
    if (lineEl) lineEl.setAttribute("d", nextLine);
    const node = nodeEls[idx];
    if (node) {
      const [nx, ny] = curPts[idx];
      node.querySelectorAll("circle").forEach((c) => {
        c.setAttribute("cx", String(nx));
        c.setAttribute("cy", String(ny));
      });
      const ttl = node.querySelector("title");
      const pt = curve[idx];
      if (ttl && pt) ttl.textContent = `Point ${idx + 1} : ${num(pt.temp_c)} °C → ${num(pt.duty_pct)} %`;
    }
    const strip = document.getElementById(ptsId);
    if (strip) {
      const pt = curve[idx];
      const tInp = strip.querySelector(`input[data-pt="${idx}"][data-field="temp_c"]`);
      const dInp = strip.querySelector(`input[data-pt="${idx}"][data-field="duty_pct"]`);
      if (tInp && pt) tInp.value = String(num(pt.temp_c));
      if (dInp && pt) dInp.value = String(num(pt.duty_pct));
    }
  };

  let dragIdx = -1;
  const n = curve.length;
  const onMove = (e) => {
    if (dragIdx < 0) return;
    const r = svg.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const sx = ((e.clientX - r.left) / r.width) * W;
    const sy = ((e.clientY - r.top) / r.height) * H;
    const minT = dragIdx === 0 ? 20 : num(curve[dragIdx - 1].temp_c) + 1;
    const maxT = dragIdx === n - 1 ? 100 - (n - 1 - dragIdx) : num(curve[dragIdx + 1].temp_c) - 1;
    const nt = clamp(Math.round(20 + ((sx - padL) / pw) * 80), minT, Math.max(minT, maxT));
    const nd = clamp(Math.round((1 - (sy - padT) / ph) * 100), minDuty, 100);
    curve[dragIdx] = { temp_c: nt, duty_pct: nd };
    if (onPointChange) onPointChange();
    syncDomInPlace(dragIdx);
  };
  const stopDrag = () => {
    if (dragIdx < 0) return;
    dragIdx = -1;
    local.curveDragging = false;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", stopDrag);
    window.removeEventListener("pointercancel", stopDrag);
    window.removeEventListener("blur", stopDrag);
    if (onCommit) onCommit();
  };
  nodeEls.forEach((g) => {
    g.onpointerdown = (e) => {
      e.preventDefault();
      dragIdx = Number(g.dataset.pt);
      local.curveDragging = true;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", stopDrag);
      window.addEventListener("pointercancel", stopDrag);
      window.addEventListener("blur", stopDrag);
    };
  });
}

export function renderFanCurve() {
  const cd = curveDevice();
  const ch = cd && cd.curves && cd.curves.channels[local.selectedCoolingIdx];
  if (!ch) return;
  const accent = ch.is_pump ? cssVar("--amber", "#FFA53D") : cssVar("--cyan", "#00D4FF");
  mountCurveSvg({
    svgId: "fan-curve-svg",
    ptsId: "curve-pts",
    curve: ch.curve,
    minDuty: ch.is_pump ? 45 : 0,
    accent,
    gradId: "cg-bios",
    floorLabel: ch.is_pump ? "Plancher de sécurité de la pompe (45 % min.)" : null,
    onPointChange: () => { ch.policy = "Manual"; },
    onCommit: () => {
      document.querySelectorAll("[data-act='cool-pol']").forEach((b) => {
        const on = b.dataset.val === "Manual";
        b.classList.toggle("active", on);
        b.setAttribute("aria-pressed", String(on));
      });
    },
  });
}

export function renderSwFanCurve() {
  const fans = unifiedCoolingFans();
  const active = fans.find((x) => x.key === local.selectedFanKey) || fans[0];
  if (!active) return;
  const draft = getSwFanDraft(active);
  if (draft.mode !== "curve" && draft.mode !== "mix") return;
  const isMix = draft.mode === "mix";
  const editSub = isMix && local.selectedSwCurveIdx === 1 ? 1 : 0;
  const curve = editSub === 1 ? draft.mix_curve : draft.curve;
  const secondaryCurve = isMix ? (editSub === 1 ? draft.curve : draft.mix_curve) : null;
  const floor = num(active.fan.min_pct, 30);
  mountCurveSvg({
    svgId: "sw-curve-svg",
    ptsId: "sw-curve-pts",
    curve,
    minDuty: floor,
    accent: editSub === 1 ? cssVar("--purple", "#AD80FF") : cssVar("--cyan", "#00D4FF"),
    secondaryCurve,
    gradId: "cg-sw",
    floorLabel: floor > 0 ? `Plancher de sécurité (${floor} % min.)` : null,
  });
}
