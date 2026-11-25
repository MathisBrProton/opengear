import { ICONS, arr, esc, fmtHz, num, plural, safeHex } from "../utils.js";
import { S, deviceById, devicesList, getEq, headsetImage, isExpert, local } from "../state.js";
import { advancedHint, batteryBadge, deviceFans, deviceSettings, deviceVisual, emptyState, expertTag, pageHeader, sensorChips, sliderRow } from "../components.js";
import { cardCorsairMouse, cardLogiDpi } from "./performance.js";
import { cardLogiOnboard } from "./input.js";

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
            <span style="min-width:0"><span class="dev-item-name" style="display:block">${esc(x.name)}</span><span class="dev-item-sub">${esc(x.vendor)} · ${esc(x.type)}</span></span>
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
        <div class="row mt-8"><span class="pill ${d.status.cls}"><span class="dot"></span>${esc(d.status.label)}</span>${d.battery != null ? batteryBadge(d.battery, d.charging) : ""}${extraHtml}</div>
      </div>
    </div>`;
}

export function infoTable(rows) {
  const r = rows.filter((x) => x && x[1] != null && x[1] !== "");
  if (!r.length) return "";
  return `<table class="meta-table">${r.map(([k, v, mono]) => `<tr><td class="k">${esc(k)}</td><td class="${mono ? "mono" : ""}">${esc(v)}</td></tr>`).join("")}</table>`;
}

function linkCard(text, tabId, subId, label) {
  return `<div class="note row-between"><span>${esc(text)}</span><button type="button" class="btn btn-sm btn-ghost" data-act="nav" data-tab="${tabId}" ${subId ? `data-sub="${subId}"` : ""}>${esc(label)} ${ICONS.chev}</button></div>`;
}

function fiche(d) {
  const [kind, idxStr] = d.key.split(":");
  const i = Number(idxStr || 0);
  const ex = isExpert();
  if (kind === "dev") return ficheGeneric(d);
  if (kind === "corsair") return ficheCorsair(d, S.corsair_devices[i], i);
  if (kind === "logi-mouse") {
    const m = S.logi_mice[i];
    const pm = m.power_model;
    return `
      ${ficheHead(d)}
      ${cardLogiDpi(m, i)}
      ${pm ? `
        <div class="card">
          <div class="card-head"><h2 class="card-title">Autonomie</h2>${m.battery ? `<span class="pill emerald">${esc(m.battery.state)}</span>` : ""}</div>
          <div class="row-between">
            <div><div style="font-size:26px;font-weight:750;color:var(--emerald)">≈ ${num(pm.estimated_hours).toFixed(1)} h</div><div class="muted">Estimation à ${fmtHz(num(m.report_rate && m.report_rate.current_hz, 1000))}. Baisser la fréquence d'envoi allonge l'autonomie.</div></div>
            ${ex ? `<div class="mono muted small" style="text-align:right">Capteur ${num(pm.sensor_mw).toFixed(2)} mW<br/>Microcontrôleur ${num(pm.mcu_mw).toFixed(2)} mW<br/>Radio ${num(pm.radio_mw).toFixed(2)} mW<br/>Capacité ${Math.round(num(pm.capacity_mwh))} mWh</div>` : ""}
          </div>
        </div>` : ""}
      ${cardLogiOnboard(m, i)}
      ${arr(m.led_zones).length ? linkCard("L'éclairage de cette souris se règle dans Éclairage.", "lighting", "devices", "Éclairage") : ""}
      ${ex ? `<div class="card expert-card"><div class="card-head"><h2 class="card-title">Informations techniques ${expertTag()}</h2></div>${infoTable([
        ["Identifiant USB", `${m.vid}:${m.pid} (emplacement ${m.device_index})`, true],
        ["Protocole", `HID++ ${m.proto_version}`, true],
        ["Firmware", arr(m.firmware).join(" | "), true],
        ["Fonctions exposées", `${arr(m.features).length}`],
      ])}</div>` : ""}`;
  }
  if (kind === "logi-hs") return ficheLogiHeadset(d, S.logi_headsets[i], i);
  if (kind === "cooling") {
    const c = S.cooling;
    return `
      ${ficheHead(d)}
      ${linkCard("Modes de ventilation, pompe et courbes se règlent dans Performances.", "performance", "cooling", "Ventilation")}
      ${linkCard("L'éclairage par zone / température se règle dans Éclairage.", "lighting", "devices", "Éclairage")}
      <div class="card"><div class="card-head"><h2 class="card-title">État</h2></div>${infoTable([
        ["Droits administrateur", c.is_admin ? "Oui" : "Non (lecture seule)"],
        ["Accès BIOS (ASUS WMI)", c.wmi_available ? "Disponible" : "Indisponible"],
        ["Message", c.message],
        ["Canaux", c.channels.map((x) => x.short_label || x.label).join(", ")],
        ex && c.superio ? ["Puce de contrôle", c.superio.detected ? `${c.superio.chip_name} (${c.superio.chip_id_hex || "?"})` : "Non détectée", true] : null,
      ])}</div>
      <div class="card"><div class="card-head"><h2 class="card-title">Capteurs en direct</h2></div><div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div></div>`;
  }
  return ficheHead(d);
}

/** Page of a device known only through the shared model: identity, status and its settings. */
function ficheGeneric(d) {
  const dev = deviceById(d.deviceId);
  if (!dev) return ficheHead(d);
  const conn = { usb: "USB", wireless: "Sans fil", bluetooth: "Bluetooth", internal: "Interne" }[dev.info.connection];
  return `
    ${ficheHead(d)}
    ${dev.lighting ? linkCard("Effets logiciels, effets matériels et placement bureau se règlent dans Éclairage.", "lighting", "devices", "Éclairage") : ""}
    ${deviceSettings(d.deviceId)}
    ${deviceFans(d.deviceId)}
    <div class="card"><div class="card-head"><h2 class="card-title">Informations</h2></div>${infoTable([
      ["Fabricant", dev.info.vendor], ["Connexion", conn], ["Firmware", dev.info.firmware, true], isExpert() ? ["Identifiant", dev.info.id, true] : null,
    ])}</div>
    ${dev.cooling ? `<div class="card"><div class="card-head"><h2 class="card-title">Capteurs en direct</h2></div><div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div></div>` : ""}`;
}

function ficheCorsair(d, dev, i) {
  const ex = isExpert();
  const info = `
    <div class="card"><div class="card-head"><h2 class="card-title">Informations</h2></div>${infoTable([
      ["Firmware", dev.firmware, true],
      ["Disposition", dev.layout],
      dev.storage_total_kb ? ["Mémoire interne", `${dev.storage_free_kb != null ? dev.storage_free_kb : "?"} Ko libres sur ${dev.storage_total_kb} Ko`] : null,
      ex ? ["Identifiant USB", `${dev.vid}:${dev.pid}${dev.subdevice_id ? ` (sous-appareil ${dev.subdevice_id})` : ""}`, true] : null,
      ex ? ["Bootloader", dev.bootloader, true] : null,
      ex ? ["Numéro de série", dev.serial, true] : null,
      ex ? ["Taille de rapport", dev.out_report_len ? `${dev.out_report_len} octets` : null, true] : null,
    ])}</div>`;
  if (dev.kind === "Mouse" && dev.mouse) return `${ficheHead(d)}${cardCorsairMouse(dev, i)}${info}`;
  if (dev.kind === "Headset" && dev.headset) {
    const h = dev.headset;
    return `
      ${ficheHead(d)}
      ${deviceSettings(dev.device_id, { title: "Réglages du casque" })}
      ${cardWasapi()}${info}`;
  }
  if (dev.kind === "CoolerHub" && dev.cooler) {
    return `${ficheHead(d)}<div class="card"><div class="card-head"><h2 class="card-title">Watercooling Corsair</h2></div>${infoTable([
      ["Gamme", dev.cooler.product_line_id, true], ["Version de pompe", dev.cooler.pump_version_id, true], ["Taille du radiateur", dev.cooler.radiator_size, true],
    ]) || `<div class="muted">Aucune information supplémentaire.</div>`}</div>${info}`;
  }
  if (dev.kind !== "Keyboard") return `${ficheHead(d)}${info}`;

  return `
    ${ficheHead(d, dev.esports_switch_on ? `<span class="pill rog">Interrupteur tournoi activé</span>` : "")}
    ${linkCard("Effets, couleurs, peinture par touche et profils matériels se règlent dans Éclairage.", "lighting", "global", "Éclairage")}
    <div class="card accent-cyan">
      <div class="card-head"><h2 class="card-title">Réglages du clavier</h2></div>
      ${deviceSettings(dev.device_id, { bare: true })}
    </div>
    ${ex ? `
      <div class="card expert-card">
        <div class="card-head"><h2 class="card-title">Couleurs des voyants ${expertTag()}</h2></div>
        <div class="row">
          <select id="cs-ind-${i}" class="og-select auto" aria-label="Voyant">
            <option value="esports">Rétroéclairage mode tournoi</option><option value="winlock_on">Voyant verrou Windows actif</option><option value="winlock_off">Voyant verrou Windows inactif</option><option value="profile">Voyant du bouton profil</option><option value="brightness">Voyant de luminosité</option>
          </select>
          <input type="color" id="cs-ind-col-${i}" class="og-color" value="#FF1848" aria-label="Couleur du voyant" />
          <button type="button" class="btn btn-sm btn-secondary" data-act="cs-ind" data-dev="${i}">Appliquer</button>
        </div>
      </div>` : advancedHint("Anti-rebond matériel, voyants et raccourcis bloqués sont en mode Expert.")}
    ${info}`;
}

function ficheLogiHeadset(d, h, i) {
  const eq = h.equalizer;
  const e = getEq(h, i);
  const freqs = arr(eq && eq.frequencies_hz).length ? eq.frequencies_hz : [114, 398, 1000, 4999, 9999];
  const fmtF = (f) => (f >= 1000 ? `${(f / 1000).toFixed(f % 1000 ? 1 : 0)} kHz` : `${f} Hz`);
  const bv = arr(S.local_assets.blue_voice_presets);
  const hi = headsetImage();
  return `
    ${ficheHead(d)}
    ${hi.variants.length > 1 ? `<div class="row">${hi.variants.map((v, vi) => `<button type="button" class="variant-chip ${vi === local.selectedHeadsetVariant ? "active" : ""}" data-act="headset-variant" data-idx="${vi}">${esc(String(v.label).replace("PRO X ", ""))}</button>`).join("")}</div>` : ""}
    ${deviceSettings(h.device_id, { sections: ["audio"], title: "Audio" })}
    ${eq ? `
      <div class="card">
        <div class="card-head"><div><h2 class="card-title">Égaliseur</h2><div class="card-desc">Appliqué directement dans le casque.</div></div>${e.dirty ? `<span class="pill amber">Non appliqué</span>` : ""}</div>
        <div class="row" style="margin-bottom:10px">${arr(h.presets).map((p, idx) => `<button type="button" class="btn btn-sm btn-secondary" data-act="eq-preset" data-dev="${i}" data-idx="${idx}">${esc(p.name)}</button>`).join("")}</div>
        <svg class="eq-graph" data-eq-svg="${i}" viewBox="0 0 560 108" role="img" aria-label="Courbe de l'égaliseur"></svg>
        <div class="eq-faders">
          ${freqs.slice(0, 5).map((f, b) => `
            <div class="eq-band">
              <span class="eq-db" id="eq-db-${i}-${b}">${num(e.gains[b]) > 0 ? "+" : ""}${num(e.gains[b])} dB</span>
              <div class="eq-vslider-wrap"><input type="range" class="eq-vslider" min="-12" max="12" step="1" value="${num(e.gains[b])}" data-input="eq-band" data-dev="${i}" data-band="${b}" aria-label="${esc(fmtF(f))}" /></div>
              <span class="eq-freq">${esc(fmtF(f))}</span>
            </div>`).join("")}
        </div>
        <div class="row mt-16">
          <button type="button" class="btn btn-primary" data-act="eq-apply" data-dev="${i}">Appliquer l'égaliseur</button>
          <button type="button" class="btn btn-secondary" data-act="eq-reset" data-dev="${i}">Remettre à plat</button>
        </div>
      </div>` : ""}
    ${cardWasapi()}
    ${isExpert() ? `
      <div class="card expert-card">
        <div class="card-head"><h2 class="card-title">Informations techniques ${expertTag()}</h2></div>
        ${infoTable([["Identifiant USB", `${h.vid}:${h.pid}`, true], ["Protocole", `HID++ ${h.proto_version}`, true], ["Firmware", arr(h.firmware).join(" | "), true]])}
        <div class="note mt-12">Les filtres micro Blue VO!CE et le son surround DTS sont des traitements Windows du pilote Logitech, pas des réglages du casque : OpenGear ne les pilote pas.${bv.length ? ` ${plural(bv.length, "préréglage Blue VO!CE trouvé", "préréglages Blue VO!CE trouvés")} sur ce PC.` : ""}</div>
      </div>` : ""}`;
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

export function renderEqSvg(i) {
  const svg = document.querySelector(`[data-eq-svg="${i}"]`);
  const e = local.eqDrafts[i];
  if (!svg || !e) return;
  const W = 560, H = 108, padX = 48, midY = H / 2;
  const g = [0, 1, 2, 3, 4].map((b) => num(e.gains[b]));
  const xs = g.map((_, b) => padX + (b / 4) * (W - padX * 2));
  const ys = g.map((db) => midY - (db / 12) * (H * 0.38));
  let d = `M ${xs[0]},${ys[0]}`;
  for (let b = 0; b < 4; b++) d += ` Q ${xs[b]},${ys[b]} ${(xs[b] + xs[b + 1]) / 2},${(ys[b] + ys[b + 1]) / 2}`;
  d += ` T ${xs[4]},${ys[4]}`;
  svg.innerHTML = `
    <defs><linearGradient id="eqg${i}" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#AD80FF" stop-opacity="0.38"/><stop offset="100%" stop-color="#AD80FF" stop-opacity="0.02"/></linearGradient></defs>
    <line x1="12" y1="${midY}" x2="${W - 12}" y2="${midY}" stroke="rgba(255,255,255,0.12)" stroke-dasharray="4 4"/>
    <text x="14" y="16" fill="#8592ab" font-size="9.5" font-family="Consolas">+12 dB</text>
    <text x="14" y="${midY - 4}" fill="#8592ab" font-size="9.5" font-family="Consolas">0 dB</text>
    <text x="14" y="${H - 8}" fill="#8592ab" font-size="9.5" font-family="Consolas">-12 dB</text>
    <path d="${d} L ${xs[4]},${H} L ${xs[0]},${H} Z" fill="url(#eqg${i})"/>
    <path d="${d}" fill="none" stroke="#AD80FF" stroke-width="2.6" stroke-linecap="round"/>
    ${xs.map((x, b) => `<circle cx="${x}" cy="${ys[b]}" r="4.5" fill="#fff" stroke="#AD80FF" stroke-width="2.2"/>`).join("")}`;
}
