import { ICONS, arr, cssVar, domId, esc, fmtHz, num, plural } from "../utils.js";
import { S, deviceById, devicesList, getEq, headsetImage, isExpert, local } from "../state.js";
import { advancedHint, batteryBadge, deviceBlueprintSvg, deviceFans, deviceSettings, deviceVisual, emptyState, pageHeader, sensorChips, sliderRow } from "../components.js";
import { cardPointer } from "./performance.js";
import { cardOnboard } from "./input.js";

export function viewDevices() {
  const devs = devicesList();
  if (!devs.length) {
    return `${pageHeader("Appareils", "Fiche détaillée de chaque appareil.")}
      ${emptyState("Aucun appareil détecté", "Branche ton clavier, ta souris, ton casque ou le récepteur sans fil : OpenGear les détecte automatiquement.", `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`)}`;
  }
  if (!devs.some((d) => d.key === local.deviceKey)) local.deviceKey = devs[0].key;
  const d = devs.find((x) => x.key === local.deviceKey);
  return `
    ${pageHeader("Appareils", "Fiche détaillée de chaque appareil et réglages propres à chaque marque.", `<button type="button" class="btn btn-sm btn-ghost" data-act="rescan">${ICONS.refresh}Rechercher des appareils</button>`)}
    <div class="dev-layout">
      <nav class="dev-list" aria-label="Liste des appareils">
        ${devs.map((x) => `
          <button type="button" class="dev-item ${x.key === local.deviceKey ? "active" : ""}" data-act="select-device" data-key="${esc(x.key)}" ${x.key === local.deviceKey ? 'aria-current="true"' : ""}>
            <span class="glyph">${ICONS[x.icon] || ICONS.chip}</span>
            <span class="min-w-0"><span class="dev-item-name d-block">${esc(x.name)}</span><span class="dev-item-sub">${esc(x.vendor)} · ${esc(x.type)}</span></span>
          </button>`).join("")}
      </nav>
      <div class="stack">${fiche(d)}</div>
    </div>`;
}

function ficheHead(d, extraHtml = "") {
  return `
    <div class="fiche-head">
      <div class="fiche-photo">${deviceVisual(d)}</div>
      <div class="grow">
        <h2 class="fiche-title">${esc(d.name)}</h2>
        <div class="muted">${esc(d.vendor)} · ${esc(d.type)}</div>
        <div class="row mt-8"><span class="pill ${d.status.cls}"><span class="dot"></span>${esc(d.status.label)}</span>${d.battery != null ? batteryBadge(d.battery, d.charging, d.batteryHours) : ""}${extraHtml}</div>
      </div>
    </div>`;
}

export function infoTable(rows) {
  const r = rows.filter((x) => x && x[1] != null && x[1] !== "");
  if (!r.length) return "";
  return `<div class="table-scroll"><table class="meta-table">${r.map(([k, v, mono]) => `<tr><td class="k">${esc(k)}</td><td class="${mono ? "mono" : ""}">${esc(v)}</td></tr>`).join("")}</table></div>`;
}

function linkCard(text, tabId, subId, label) {
  return `<div class="note row-between"><span>${esc(text)}</span><button type="button" class="btn btn-sm btn-ghost" data-act="nav" data-tab="${tabId}" ${subId ? `data-sub="${subId}"` : ""}>${esc(label)} ${ICONS.chev}</button></div>`;
}

function fiche(d) {
  const ex = isExpert();
  const dev = deviceById(d.deviceId);
  if (!dev) return ficheHead(d);
  const id = dev.info.id;
  const kind = dev.info.kind;
  const conn = { usb: "USB", wireless: "Sans fil", bluetooth: "Bluetooth", internal: "Interne" }[dev.info.connection];
  const pm = dev.battery_model;
  const rrHz = dev.pointer && dev.pointer.report_rate_hz;
  const esports = arr(dev.specs).some((sp) => sp.label === "Interrupteur tournoi" && sp.value === "Activé");
  const hi = kind === "headset" ? headsetImage() : null;
  const eq = dev.equalizer;
  const e = eq ? getEq(dev) : null;
  const freqs = eq && arr(eq.frequencies_hz).length ? eq.frequencies_hz : [114, 398, 1000, 4999, 9999];
  const fmtF = (f) => (f >= 1000 ? `${(f / 1000).toFixed(f % 1000 ? 1 : 0)} kHz` : `${f} Hz`);
  const bv = kind === "headset" ? arr(S.local_assets.blue_voice_presets) : [];
  const specsRows = arr(dev.specs)
    .filter((sp) => ex || !sp.expert)
    .map((sp) => [sp.label, sp.value, !!sp.mono]);

  return `
    ${ficheHead(d, esports ? `<span class="pill rog">Interrupteur tournoi activé</span>` : "")}
    ${!(dev.onboard && kind === "mouse") && !eq ? deviceBlueprintSvg(d, dev) : ""}
    ${dev.pointer ? cardPointer(dev) : ""}
    ${pm ? `
      <div class="card">
        <div class="card-head"><h2 class="card-title">Autonomie</h2>${d.battery != null ? `<span class="pill emerald">${d.charging ? "En charge" : "Sur batterie"}</span>` : ""}</div>
        <div class="row-between">
          <div><div class="stat-big">≈ ${num(pm.estimated_hours).toFixed(1)} h</div><div class="muted">Estimation à ${fmtHz(num(rrHz, 1000))}. Baisser la fréquence d'envoi allonge l'autonomie.</div></div>
          ${ex ? `<div class="mono muted small txt-r">Capteur ${num(pm.sensor_mw).toFixed(2)} mW<br/>Microcontrôleur ${num(pm.mcu_mw).toFixed(2)} mW<br/>Radio ${num(pm.radio_mw).toFixed(2)} mW<br/>Capacité ${Math.round(num(pm.capacity_mwh))} mWh</div>` : ""}
        </div>
      </div>` : ""}
    ${dev.onboard ? cardOnboard(dev) : ""}
    ${hi && hi.variants.length > 1 ? `<div class="card"><div class="row-between"><span class="field-label">Apparence du casque</span><div class="row">${hi.variants.map((v, vi) => `<button type="button" class="variant-chip ${vi === local.selectedHeadsetVariant ? "active" : ""}" data-act="headset-variant" data-idx="${vi}">${esc(String(v.label).replace("PRO X ", ""))}</button>`).join("")}</div></div></div>` : ""}
    ${eq ? `
      <div class="card">
        <div class="card-head"><div><h2 class="card-title">Égaliseur</h2><div class="card-desc">Appliqué directement dans le casque.</div></div>${e.dirty ? `<span class="pill amber">Non appliqué</span>` : ""}</div>
        <div class="row mb-10">${arr(eq.presets).map((p) => `<button type="button" class="btn btn-sm btn-secondary" data-act="eq-preset" data-id="${esc(id)}" data-preset="${esc(p.id)}">${esc(p.name)}</button>`).join("")}</div>
        <svg class="eq-graph" data-eq-svg="${esc(id)}" viewBox="0 0 560 108" role="img" aria-label="Courbe de l'égaliseur"></svg>
        <div class="eq-faders">
          ${freqs.slice(0, 5).map((f, b) => `
            <div class="eq-band">
              <span class="eq-db" id="eq-db-${domId(id)}-${b}">${num(e.gains[b]) > 0 ? "+" : ""}${num(e.gains[b])} dB</span>
              <div class="eq-vslider-wrap"><input type="range" class="eq-vslider" min="-12" max="12" step="1" value="${num(e.gains[b])}" data-input="eq-band" data-id="${esc(id)}" data-band="${b}" aria-label="${esc(fmtF(f))}" /></div>
              <span class="eq-freq">${esc(fmtF(f))}</span>
            </div>`).join("")}
        </div>
        <div class="row mt-16">
          <button type="button" class="btn btn-primary" data-act="eq-apply" data-id="${esc(id)}">Appliquer l'égaliseur</button>
          <button type="button" class="btn btn-secondary" data-act="eq-reset" data-id="${esc(id)}">Remettre à plat</button>
        </div>
      </div>` : ""}
    ${dev.cooling && dev.curves ? linkCard("Modes de ventilation, pompe et courbes se règlent dans Performances.", "performance", "cooling", "Ventilation") : ""}
    ${dev.lighting ? linkCard("Effets logiciels, effets matériels et placement bureau se règlent dans Éclairage.", "lighting", "devices", "Éclairage") : ""}
    ${kind === "headset" ? deviceSettings(id, { sections: ["audio"], title: "Audio" }) : ""}
    ${deviceSettings(id, { sections: kind === "headset" ? ["general", "performance", "input", "power"] : ["general", "performance", "input", "audio", "power"] })}
    ${ex ? deviceSettings(id, { sections: ["lighting"], title: "Couleurs des voyants" }) : ""}
    ${kind === "keyboard" && !ex && arr(dev.settings).some((s) => s.level === "expert") ? advancedHint("Anti-rebond matériel, voyants et raccourcis bloqués sont en mode Expert.") : ""}
    ${kind === "headset" ? cardWasapi() : ""}
    ${deviceFans(id)}
    <div class="card"><div class="card-head"><h2 class="card-title">Informations</h2></div>${infoTable([
      ["Fabricant", dev.info.vendor],
      ["Connexion", conn],
      ["Firmware", dev.info.firmware, true],
      d.batteryHours != null && !d.charging && !pm ? ["Autonomie estimée", `≈ ${num(d.batteryHours)} heures`] : null,
      ...specsRows,
      ex ? ["Identifiant", id, true] : null,
    ])}
      ${ex && bv.length ? `<div class="note mt-12">Les filtres micro Blue VO!CE et le son surround DTS sont des traitements Windows du pilote Logitech, pas des réglages du casque : OpenGear ne les pilote pas. ${plural(bv.length, "préréglage Blue VO!CE trouvé", "préréglages Blue VO!CE trouvés")} sur ce PC.</div>` : ""}
    </div>
    ${dev.cooling || dev.curves ? `<div class="card"><div class="card-head"><h2 class="card-title">Capteurs en direct</h2></div><div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div></div>` : ""}`;
}

function cardWasapi() {
  const au = S.telemetry.audio || { available: false };
  if (!au.available && au.output_volume_pct == null) return "";
  const out = num(au.output_volume_pct, 50);
  const mic = num(au.mic_volume_pct, 80);
  return `
    <div class="card accent-cyan">
      <div class="card-head"><div><h2 class="card-title">Volume Windows</h2><div class="card-desc">Périphérique audio par défaut</div></div><span class="pill cyan" id="wa-out-state">${au.output_muted ? "Muet" : `${out} %`}</span></div>
      <div class="row-nowrap">
        <div class="grow">${sliderRow("Sortie", { id: "wa-out", min: 0, max: 100, value: out, input: "label", change: "wa-vol", data: { mic: "0", fmt: "pct100" }, display: `${out} %` })}</div>
        <button type="button" class="btn btn-sm ${au.output_muted ? "btn-danger" : "btn-secondary"}" data-act="wa-mute" data-mic="0" aria-pressed="${!!au.output_muted}">${au.output_muted ? "Rétablir le son" : "Couper le son"}</button>
      </div>
      ${au.mic_available ? `
        <div class="row-nowrap mt-12">
          <div class="grow">${sliderRow("Microphone", { id: "wa-mic", min: 0, max: 100, value: mic, input: "label", change: "wa-vol", data: { mic: "1", fmt: "pct100" }, display: `${mic} %` })}</div>
          <button type="button" class="btn btn-sm ${au.mic_muted ? "btn-danger" : "btn-secondary"}" data-act="wa-mute" data-mic="1" aria-pressed="${!!au.mic_muted}">${au.mic_muted ? "Réactiver le micro" : "Couper le micro"}</button>
        </div>` : ""}
    </div>`;
}

export function renderEqSvg(id) {
  const did = domId(id);
  const svg = document.querySelector(`[data-eq-svg="${CSS.escape(id)}"]`);
  const e = local.eqDrafts[id];
  if (!svg || !e) return;
  const W = 560, H = 108, padX = 48, midY = H / 2;
  const g = [0, 1, 2, 3, 4].map((b) => num(e.gains[b]));
  const xs = g.map((_, b) => padX + (b / 4) * (W - padX * 2));
  const ys = g.map((db) => midY - (db / 12) * (H * 0.38));
  let d = `M ${xs[0]},${ys[0]}`;
  for (let b = 0; b < 4; b++) d += ` Q ${xs[b]},${ys[b]} ${(xs[b] + xs[b + 1]) / 2},${(ys[b] + ys[b + 1]) / 2}`;
  d += ` T ${xs[4]},${ys[4]}`;
  const purple = cssVar("--purple", "#AD80FF");
  const muted = cssVar("--text-3", "#8592AB");
  svg.innerHTML = `
    <defs><linearGradient id="eqg${did}" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="${purple}" stop-opacity="0.38"/><stop offset="100%" stop-color="${purple}" stop-opacity="0.02"/></linearGradient></defs>
    <line x1="12" y1="${midY}" x2="${W - 12}" y2="${midY}" stroke="rgba(255,255,255,0.12)" stroke-dasharray="4 4"/>
    <text x="14" y="16" fill="${muted}" font-size="9.5" font-family="Consolas">+12 dB</text>
    <text x="14" y="${midY - 4}" fill="${muted}" font-size="9.5" font-family="Consolas">0 dB</text>
    <text x="14" y="${H - 8}" fill="${muted}" font-size="9.5" font-family="Consolas">-12 dB</text>
    <path d="${d} L ${xs[4]},${H} L ${xs[0]},${H} Z" fill="url(#eqg${did})"/>
    <path d="${d}" fill="none" stroke="${purple}" stroke-width="2.6" stroke-linecap="round"/>
    ${xs.map((x, b) => `<circle cx="${x}" cy="${ys[b]}" r="4.5" fill="#fff" stroke="${purple}" stroke-width="2.2"/>`).join("")}`;
}
