import { ICONS, POLICIES, arr, clamp, esc, fmtHz, num } from "../utils.js";
import { S, deviceById, isExpert, local } from "../state.js";
import { advancedHint, coolingDeviceIds, deviceFans, deviceSettings, emptyState, expertTag, pageHeader, seg, sensorChips, sliderRow, subtabs, sw } from "../components.js";

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
  S.logi_mice.forEach((m, i) => cards.push(cardLogiDpi(m, i)));
  S.corsair_devices.forEach((d, i) => {
    if (d.kind === "Mouse" && d.mouse) cards.push(cardCorsairMouse(d, i));
    else if (d.kind === "Keyboard" && d.pollrate_hz) cards.push(cardCorsairPoll(d, i));
  });
  if (!cards.length) {
    return emptyState("Aucune souris ni clavier réglable détecté", "Branche une souris Logitech G (récepteur LIGHTSPEED / Bolt ou câble) ou un périphérique Corsair compatible, puis relance la recherche.",
      `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`);
  }
  return `<div class="grid-2">${cards.join("")}</div>${advancedHint("Angle snapping et sensibilité X/Y séparée sont disponibles en mode Expert.")}`;
}

export function cardLogiDpi(m, i) {
  const dpi = m.dpi;
  const rr = m.report_rate;
  const ex = isExpert();
  if (!dpi && !rr) return `<div class="card"><h2 class="card-title">${esc(m.name)}</h2><div class="muted mt-8">Cette souris n'expose pas de réglage de sensibilité.</div></div>`;
  const presets = dpi ? [400, 800, 1200, 1600, 3200, 6400].filter((v) => v >= num(dpi.min_dpi, 100) && v <= num(dpi.max_dpi, 25600)) : [];
  return `
    <div class="card accent-cyan">
      <div class="card-head">
        <div><h2 class="card-title">${esc(m.name)}</h2><div class="card-desc">Sensibilité et fréquence d'envoi</div></div>
        <span class="pill cyan">${dpi ? `${num(dpi.dpi_x)} DPI` : ""}${dpi && rr ? " · " : ""}${rr ? fmtHz(num(rr.current_hz, 1000)) : ""}</span>
      </div>
      ${dpi ? `
        <div class="row" role="group" aria-label="DPI rapides">
          ${presets.map((v) => `<button type="button" class="btn btn-sm ${num(dpi.dpi_x) === v ? "btn-primary" : "btn-secondary"}" data-act="logi-dpi" data-dev="${i}" data-dpi="${v}" aria-pressed="${num(dpi.dpi_x) === v}">${v}</button>`).join("")}
        </div>
        <div class="stack mt-12" style="gap:10px">
          ${sliderRow(ex && dpi.supports_independent_xy ? "DPI horizontal (X)" : "DPI précis", { id: `ldpi-${i}`, min: num(dpi.min_dpi, 100), max: num(dpi.max_dpi, 25600), step: num(dpi.step_dpi, 50) || 50, value: num(dpi.dpi_x, 800), input: "label", data: { fmt: "dpi" }, display: `${num(dpi.dpi_x, 800)} DPI` })}
          ${ex && dpi.supports_independent_xy ? sliderRow("DPI vertical (Y)", { id: `ldpiy-${i}`, min: num(dpi.min_dpi, 100), max: num(dpi.max_dpi, 25600), step: num(dpi.step_dpi, 50) || 50, value: num(dpi.dpi_y, dpi.dpi_x), input: "label", data: { fmt: "dpi" }, display: `${num(dpi.dpi_y, dpi.dpi_x)} DPI` }) : ""}
        </div>
        <div class="row mt-12"><button type="button" class="btn btn-sm btn-primary" data-act="logi-dpi-apply" data-dev="${i}">Appliquer le DPI précis</button></div>
        ${dpi.supports_lod ? `
          <div class="row-between mt-12">
            <span class="field-label">Distance de décrochage</span>
            ${seg([["Low", "Basse"], ["Medium", "Moyenne"], ["High", "Haute"]], dpi.lod, "logi-lod", { dev: i })}
          </div>` : ""}` : ""}
      <div class="mt-12">${deviceSettings(m.device_id, { sections: ["performance"], bare: true })}</div>
    </div>`;
}

export function cardCorsairMouse(dev, i) {
  const m = dev.mouse;
  const stages = arr(m.dpi_stages);
  const active = num(m.active_stage);
  return `
    <div class="card accent-cyan">
      <div class="card-head">
        <div><h2 class="card-title">${esc(dev.product_name)}</h2><div class="card-desc">Paliers de sensibilité (bouton DPI de la souris)</div></div>
        <span class="pill cyan">${num(m.current_dpi_x, stages[active] || 0)} DPI</span>
      </div>
      <div class="stack" style="gap:8px">
        ${stages.map((v, s) => `
          <div class="row-nowrap">
            <button type="button" class="btn btn-sm ${active === s ? "btn-primary" : "btn-secondary"}" data-act="cm-stage" data-dev="${i}" data-stage="${s}" aria-pressed="${active === s}" style="min-width:96px">Palier ${s + 1}${active === s ? " (actif)" : ""}</button>
            <input type="number" class="og-input narrow" min="100" max="30000" step="50" value="${num(v)}" id="cm-dpi-${i}-${s}" aria-label="DPI du palier ${s + 1}" />
            <button type="button" class="btn btn-sm btn-secondary" data-act="cm-dpi-save" data-dev="${i}" data-stage="${s}">Enregistrer</button>
          </div>`).join("")}
      </div>
      <div class="mt-12">${deviceSettings(dev.device_id, { sections: ["performance"], bare: true })}</div>
    </div>`;
}

function cardCorsairPoll(dev, i) {
  const maxHz = num(dev.max_pollrate_hz, 8000) || 8000;
  const rates = [125, 250, 500, 1000, 2000, 4000, 8000].filter((hz) => hz <= maxHz);
  return `
    <div class="card accent-cyan">
      <div class="card-head">
        <div><h2 class="card-title">${esc(dev.product_name)}</h2><div class="card-desc">Fréquence d'envoi du clavier : plus elle est haute, plus la latence est faible (consommation CPU légèrement supérieure).</div></div>
        <span class="pill emerald">${fmtHz(dev.pollrate_hz)}</span>
      </div>
      ${deviceSettings(dev.device_id, { sections: ["performance"], bare: true })}
    </div>`;
}

function cardPowerPlans() {
  const showLaptop = !!(S.is_laptop || (S.laptop_battery && S.laptop_battery.present));
  return `
    ${showLaptop ? cardLaptopPower() : ""}
    ${!S.power_plans.length ? emptyState("Aucun mode d'alimentation trouvé", "Windows n'a renvoyé aucun mode d'alimentation.") : `
      <div class="card">
        <div class="card-head"><div><h2 class="card-title">Mode d'alimentation Windows</h2><div class="card-desc">Choisis le compromis entre performances et consommation.</div></div></div>
        <div class="plan-grid">
          ${S.power_plans.map((p) => `
            <div class="plan-card ${p.active ? "active" : ""}">
              <div>
                <div style="font-weight:650;color:#fff">${esc(p.name)}</div>
                ${isExpert() ? `<div class="mono muted small" style="margin-top:3px">${esc(p.guid)}</div>` : ""}
              </div>
              <div>${p.active ? `<span class="pill cyan"><span class="dot"></span>Actif</span>` : `<button type="button" class="btn btn-sm btn-secondary" data-act="power-plan" data-guid="${esc(p.guid)}">Activer</button>`}</div>
            </div>`).join("")}
        </div>
      </div>`}`;
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

/** Fans driven through a device's `Cooling` (laptop embedded controller…), with its performance settings. */
function viewDeviceCooling(ids) {
  return `
    <div class="card mb-16">
      <div class="card-head"><div><h2 class="card-title">Capteurs en direct</h2>${isExpert() ? `<div class="card-desc">${esc((S.telemetry.sensors && S.telemetry.sensors.source) || "")}</div>` : ""}</div></div>
      <div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div>
    </div>
    ${ids.map((id) => {
      const d = deviceById(id);
      const name = d ? d.info.model : id;
      return `
        <div class="grid-2 mb-16">
          ${deviceFans(id, { title: `Ventilateurs · ${name}` })}
          ${deviceSettings(id, { sections: ["performance"], title: `Performances · ${name}` })}
        </div>`;
    }).join("")}`;
}

function normTempSource(src) {
  if (src === "GpuLive") return "GpuEdge";
  if (src === "MixCpuGpu") return "MaxCpuGpu";
  return src || "CpuBios";
}

function viewCooling() {
  const c = S.cooling;
  const deviceIds = coolingDeviceIds();
  if (deviceIds.length && !c.channels.length) return viewDeviceCooling(deviceIds);
  const ex = isExpert();
  if (!c.channels.length) return emptyState("Ventilation non pilotable", c.message || "Aucun canal de ventilation n'a été trouvé sur cette carte mère.");
  const ch = c.channels[local.selectedCoolingIdx] || c.channels[0];
  const pols = ex ? POLICIES : POLICIES.filter(([id]) => id !== "Manual");
  const sio = c.superio || { detected: false, fans: [], diagnostic: "" };
  const curSrc = normTempSource(ch.source);
  return `
    ${!c.is_admin ? `
      <div class="alert warn">
        <span class="alert-icon">${ICONS.lock}</span>
        <div class="grow"><div class="alert-title">Droits administrateur nécessaires pour modifier la ventilation</div><div class="muted">Windows protège l'accès aux réglages de ventilation de la carte mère. Tu peux préparer tes réglages ici, puis relancer OpenGear en administrateur pour les appliquer.</div></div>
        <button type="button" class="btn btn-warn btn-sm" data-act="relaunch-admin">Relancer en administrateur</button>
      </div>` : ""}
    <div class="card mb-16">
      <div class="card-head"><div><h2 class="card-title">Capteurs en direct</h2>${ex ? `<div class="card-desc">${esc((S.telemetry.sensors && S.telemetry.sensors.source) || "")}</div>` : ""}</div></div>
      <div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div>
    </div>
    <div class="grid-stage">
      <div class="stack">
        <div class="card accent-amber">
          <div class="card-head"><div><h2 class="card-title">Pompe du watercooling</h2><div class="card-desc">Un plancher de sécurité est toujours conservé.</div></div></div>
          <div class="stack" style="gap:8px">
            <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="ultra_silent">Silence (45–95 %)</button>
            <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="default">Équilibré (55–100 %)</button>
            <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="max_perf">Performance (75–100 %)</button>
          </div>
        </div>
      </div>
      <div class="card">
        <div class="card-head">
          <div><h2 class="card-title">Ventilateurs</h2><div class="card-desc">Choisis un canal puis un mode.</div></div>
          ${ex && c.wmi_available ? `<button type="button" class="btn btn-sm btn-secondary" data-act="cool-read-bios">Relire le BIOS</button>` : ""}
        </div>
        ${seg(c.channels.map((x) => [x.index, x.short_label || x.label]), local.selectedCoolingIdx, "cool-ch", {}, "amber")}
        <div class="row-between mt-12">
          <div style="font-weight:650">${esc(ch.label)}</div>
          ${!ex && ch.policy === "Manual" ? `<span class="pill purple">Courbe personnalisée</span>` : ""}
        </div>
        <div class="mt-8">${seg(pols, ch.policy, "cool-pol")}</div>
        ${ex ? `
          <div class="row mt-12">
            <label class="field-label" for="cool-src">Température de référence</label>
            <select id="cool-src" class="og-select auto" data-change="cool-src">
              <option value="CpuBios" ${curSrc === "CpuBios" ? "selected" : ""}>Processeur (régulation BIOS)</option>
              <option value="GpuEdge" ${curSrc === "GpuEdge" ? "selected" : ""}>Carte graphique (cœur)</option>
              <option value="GpuHotspot" ${curSrc === "GpuHotspot" ? "selected" : ""}>Carte graphique (point chaud)</option>
              <option value="MaxCpuGpu" ${curSrc === "MaxCpuGpu" ? "selected" : ""}>Le plus chaud des deux</option>
            </select>
          </div>
          <div class="stack mt-12">${sliderRow("Hystérésis anti-yoyo", { id: "cool-hyst", min: 0, max: 10, value: num(ch.hysteresis_c, 3), input: "label", change: "cool-hyst", data: { fmt: "deg" }, display: `${num(ch.hysteresis_c, 3)} °C` })}</div>
          <div class="curve-wrap mt-12"><svg id="fan-curve-svg" class="curve-svg" viewBox="0 0 680 250" role="img" aria-label="Courbe de ventilation"></svg></div>
          <div class="curve-pts" id="curve-pts">${curvePointsHtml(ch)}</div>
          <div class="row mt-16">
            <button type="button" class="btn btn-ok" data-act="cool-apply">Appliquer à la carte mère</button>
            <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="default">Réinitialiser la pompe</button>
          </div>` : `
          <div class="row mt-16">
            <button type="button" class="btn btn-ok" data-act="cool-apply">Appliquer à la carte mère</button>
          </div>`}
      </div>
    </div>
    ${ex ? `
      <div class="card expert-card mt-16">
        <div class="card-head">
          <div><h2 class="card-title">Contrôle direct des ventilateurs ${expertTag()}</h2><div class="card-desc">${esc(sio.diagnostic || "Pilote directement la puce de contrôle de la carte mère (SuperIO).")}</div></div>
          <div class="row">
            <span class="pill ${sio.detected ? "emerald" : "amber"}"><span class="dot"></span>${sio.detected ? esc(sio.chip_name) : c.is_admin ? "Pilote PawnIO absent" : "Administrateur requis"}</span>
            ${!sio.detected ? `<button type="button" class="btn btn-sm btn-warn" data-act="pawnio-install">Installer / réparer PawnIO</button>` : ""}
            <button type="button" class="btn btn-sm btn-secondary" data-act="sio-refresh">Actualiser</button>
          </div>
        </div>
        ${sio.detected && arr(sio.fans).length ? `
          <div class="stack" style="gap:8px">
            ${arr(sio.fans).map((f) => `
              <div class="list-row">
                <div style="min-width:140px"><strong>${esc(f.label)}</strong><div class="muted small">${f.is_manual ? "Vitesse forcée" : "Automatique (BIOS)"}</div></div>
                <span class="pill ${num(f.rpm) > 0 ? "amber" : ""}">${num(f.rpm)} tr/min</span>
                <input type="range" class="og-range grow" min="0" max="100" value="${num(f.pwm_pct)}" id="sio-${num(f.index)}" data-input="label" data-label="sio-${num(f.index)}-val" data-fmt="pct100" aria-label="Vitesse ${esc(f.label)}" />
                <span class="mono" id="sio-${num(f.index)}-val" style="width:44px;text-align:right">${num(f.pwm_pct)} %</span>
                <button type="button" class="btn btn-sm btn-primary" data-act="sio-apply" data-ch="${num(f.index)}">Forcer</button>
                <button type="button" class="btn btn-sm btn-secondary" data-act="sio-auto" data-ch="${num(f.index)}">Auto BIOS</button>
              </div>`).join("")}
          </div>` : ""}
      </div>` : advancedHint("Courbes 8 points, source de température et contrôle direct des ventilateurs sont en mode Expert.")}`;
}

function curvePointsHtml(ch) {
  return arr(ch.curve).map((pt, i) => `
    <div class="curve-pt">
      <span>P${i + 1}</span>
      <input type="number" min="20" max="100" value="${num(pt.temp_c)}" data-change="curve-pt" data-pt="${i}" data-field="temp_c" aria-label="Point ${i + 1} : température °C" />
      <input type="number" min="0" max="100" value="${num(pt.duty_pct)}" data-change="curve-pt" data-pt="${i}" data-field="duty_pct" aria-label="Point ${i + 1} : vitesse %" />
    </div>`).join("");
}

export function renderFanCurve() {
  const svg = document.getElementById("fan-curve-svg");
  if (!svg || !S) return;
  const ch = S.cooling.channels[local.selectedCoolingIdx];
  if (!ch || arr(ch.curve).length < 2) return;
  const accent = ch.is_pump ? "#FFA53D" : "#00D4FF";
  const padL = 44, padR = 18, padT = 18, padB = 30, W = 680, H = 250;
  const pw = W - padL - padR, ph = H - padT - padB;
  const tx = (t) => padL + ((t - 20) / 80) * pw;
  const dy = (d) => padT + (1 - d / 100) * ph;

  const computePath = () => {
    const pts = ch.curve.map((p) => [tx(num(p.temp_c, 50)), dy(num(p.duty_pct, 50))]);
    const poly = [[padL, pts[0][1]], ...pts, [W - padR, pts[pts.length - 1][1]]];
    const lineD = poly.map((p, i) => `${i === 0 ? "M" : "L"}${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(" ");
    const fillD = `${lineD} L${W - padR},${H - padB} L${padL},${H - padB} Z`;
    return { pts, lineD, fillD };
  };

  const { pts, lineD, fillD } = computePath();
  let html = `<defs><linearGradient id="cg" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="${accent}" stop-opacity="0.32"/><stop offset="100%" stop-color="${accent}" stop-opacity="0.02"/></linearGradient></defs>`;
  for (const d of [0, 25, 50, 75, 100]) {
    const y = dy(d);
    html += `<line x1="${padL}" y1="${y}" x2="${W - padR}" y2="${y}" stroke="rgba(255,255,255,0.06)"/><text x="${padL - 8}" y="${y + 4}" fill="#8592ab" font-size="10.5" text-anchor="end" font-family="Consolas,monospace">${d}%</text>`;
  }
  for (let t = 20; t <= 100; t += 10) {
    const x = tx(t);
    html += `<line x1="${x}" y1="${padT}" x2="${x}" y2="${H - padB}" stroke="rgba(255,255,255,0.06)"/><text x="${x}" y="${H - 10}" fill="#8592ab" font-size="10.5" text-anchor="middle" font-family="Consolas,monospace">${t}°C</text>`;
  }
  if (ch.is_pump) {
    const fy = dy(40);
    html += `<rect x="${padL}" y="${fy}" width="${pw}" height="${H - padB - fy}" fill="rgba(255,61,99,0.08)"/><line x1="${padL}" y1="${fy}" x2="${W - padR}" y2="${fy}" stroke="rgba(255,61,99,0.45)" stroke-dasharray="4 4"/><text x="${padL + 8}" y="${fy + 15}" fill="rgba(255,150,170,0.9)" font-size="10.5">Plancher de sécurité de la pompe (40 % min.)</text>`;
  }
  const sensors = S.telemetry.sensors || {};
  const liveT = sensors.cpu_temp_c != null ? sensors.cpu_temp_c : S.telemetry.gpu && S.telemetry.gpu.temp_c;
  if (liveT != null) {
    const lx = tx(clamp(liveT, 20, 100));
    html += `<line x1="${lx}" y1="${padT}" x2="${lx}" y2="${H - padB}" stroke="#FF3D63" stroke-width="1.4" stroke-dasharray="3 3"/><text x="${lx + 5}" y="${padT + 12}" fill="#FF7a93" font-size="10.5" font-family="Consolas,monospace">${sensors.cpu_temp_c != null ? "CPU" : "GPU"} ${Math.round(liveT)}°C</text>`;
  }
  html += `<path id="fc-fill" d="${fillD}" fill="url(#cg)"/><path id="fc-line" d="${lineD}" fill="none" stroke="${accent}" stroke-width="2.6" stroke-linejoin="round"/>`;
  pts.forEach(([x, y], i) => {
    const p = ch.curve[i];
    html += `<g class="curve-node" data-pt="${i}" style="cursor:grab"><circle class="fc-hit" cx="${x}" cy="${y}" r="12" fill="transparent"/><circle class="fc-dot" cx="${x}" cy="${y}" r="6.5" fill="${accent}" stroke="#090b10" stroke-width="2"/><title>Point ${i + 1} : ${num(p.temp_c)} °C → ${num(p.duty_pct)} %</title></g>`;
  });
  svg.innerHTML = html;

  const fillEl = svg.querySelector("#fc-fill");
  const lineEl = svg.querySelector("#fc-line");
  const nodeEls = [...svg.querySelectorAll(".curve-node")];

  const syncDomInPlace = (idx) => {
    const { pts: curPts, lineD: nextLine, fillD: nextFill } = computePath();
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
      const pt = ch.curve[idx];
      if (ttl && pt) ttl.textContent = `Point ${idx + 1} : ${num(pt.temp_c)} °C → ${num(pt.duty_pct)} %`;
    }
    const strip = document.getElementById("curve-pts");
    if (strip) {
      const pt = ch.curve[idx];
      const tInp = strip.querySelector(`input[data-pt="${idx}"][data-field="temp_c"]`);
      const dInp = strip.querySelector(`input[data-pt="${idx}"][data-field="duty_pct"]`);
      if (tInp && pt) tInp.value = String(num(pt.temp_c));
      if (dInp && pt) dInp.value = String(num(pt.duty_pct));
    }
  };

  let dragIdx = -1;
  const n = ch.curve.length;
  const onMove = (e) => {
    if (dragIdx < 0) return;
    const r = svg.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const sx = ((e.clientX - r.left) / r.width) * W;
    const sy = ((e.clientY - r.top) / r.height) * H;
    const minT = dragIdx === 0 ? 20 : num(ch.curve[dragIdx - 1].temp_c) + 1;
    const maxT = dragIdx === n - 1 ? 100 - (n - 1 - dragIdx) : num(ch.curve[dragIdx + 1].temp_c) - 1;
    const nt = clamp(Math.round(20 + ((sx - padL) / pw) * 80), minT, Math.max(minT, maxT));
    const nd = clamp(Math.round((1 - (sy - padT) / ph) * 100), ch.is_pump ? 40 : 0, 100);
    ch.curve[dragIdx] = { temp_c: nt, duty_pct: nd };
    ch.policy = "Manual";
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
    document.querySelectorAll("[data-act='cool-pol']").forEach((b) => {
      const on = b.dataset.val === "Manual";
      b.classList.toggle("active", on);
      b.setAttribute("aria-pressed", String(on));
    });
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
