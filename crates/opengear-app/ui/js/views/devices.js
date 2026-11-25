import { ICONS, arr, esc, fmtHz, num, plural, safeHex } from "../utils.js";
import { S, devicesList, getEq, headsetImage, isExpert, local } from "../state.js";
import { advancedHint, batteryBadge, deviceVisual, emptyState, expertTag, pageHeader, seg, sensorChips, sliderRow, sw } from "../components.js";
import { cardLogiKbLighting, cardRam } from "./lighting.js";
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
      ${arr(m.led_zones).length ? linkCard("L'éclairage de cette souris se règle dans Éclairage.", "lighting", "logi", "Éclairage") : ""}
      ${ex ? `<div class="card expert-card"><div class="card-head"><h2 class="card-title">Informations techniques ${expertTag()}</h2></div>${infoTable([
        ["Identifiant USB", `${m.vid}:${m.pid} (emplacement ${m.device_index})`, true],
        ["Protocole", `HID++ ${m.proto_version}`, true],
        ["Firmware", arr(m.firmware).join(" | "), true],
        ["Fonctions exposées", `${arr(m.features).length}`],
      ])}</div>` : ""}`;
  }
  if (kind === "logi-kb") {
    const kb = S.logi_keyboards[i];
    return `
      ${ficheHead(d)}
      ${cardLogiKbLighting(kb, i)}
      <div class="card"><div class="card-head"><h2 class="card-title">Informations</h2></div>${infoTable([
        ["Firmware", arr(kb.firmware).join(" | ") || "—", true],
        ex ? ["Identifiant USB", `${kb.vid}:${kb.pid} (emplacement ${kb.device_index})`, true] : null,
        ex ? ["Protocole", `HID++ ${kb.proto_version}`, true] : null,
      ])}</div>`;
  }
  if (kind === "logi-hs") return ficheLogiHeadset(d, S.logi_headsets[i], i);
  if (kind === "asus") {
    const a = S.asus;
    return `
      ${ficheHead(d)}
      ${linkCard("Effets, couleurs et synchronisation de la carte mère se règlent dans Éclairage.", "lighting", "asus", "Éclairage de la carte mère")}
      ${linkCard("Ventilateurs et pompe se règlent dans Performances.", "performance", "cooling", "Ventilation")}
      <div class="card"><div class="card-head"><h2 class="card-title">Informations</h2></div>${infoTable([
        ["Carte mère", S.static_info.motherboard],
        ["BIOS", S.static_info.bios_version],
        ["Processeur", S.static_info.cpu_name],
        ["Carte graphique", S.static_info.gpu_name],
        ["Éclairage", `${num(a.mainboard_fixed_leds)} LEDs intégrées · ${num(a.rgb_12v_headers)} connecteur(s) RGB 12 V · ${num(a.argb_5v_headers)} connecteur(s) ARGB 5 V`],
        ex ? ["Contrôleur Aura", `${a.vid}:${a.pid} · ${a.firmware}`, true] : null,
      ])}</div>
      ${advancedHint("Enregistrement permanent des effets, éclairage PC éteint et calibration ARGB sont en mode Expert (Éclairage › Carte mère).")}`;
  }
  if (kind === "cooling") {
    const c = S.cooling;
    return `
      ${ficheHead(d)}
      ${linkCard("Modes de ventilation, pompe et courbes se règlent dans Performances.", "performance", "cooling", "Ventilation")}
      ${linkCard("L'éclairage du bloc pompe se règle dans Éclairage.", "lighting", "aio", "Éclairage du watercooling")}
      <div class="card"><div class="card-head"><h2 class="card-title">État</h2></div>${infoTable([
        ["Droits administrateur", c.is_admin ? "Oui" : "Non (lecture seule)"],
        ["Accès BIOS (ASUS WMI)", c.wmi_available ? "Disponible" : "Indisponible"],
        ["Message", c.message],
        ["Canaux", c.channels.map((x) => x.short_label || x.label).join(", ")],
        ex && c.superio ? ["Puce de contrôle", c.superio.detected ? `${c.superio.chip_name} (${c.superio.chip_id_hex || "?"})` : "Non détectée", true] : null,
      ])}</div>
      <div class="card"><div class="card-head"><h2 class="card-title">Capteurs en direct</h2></div><div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div></div>`;
  }
  if (kind === "ram") {
    return `${ficheHead(d)}${linkCard("Effets et couleurs de la RAM se règlent dans Éclairage.", "lighting", "ram", "Éclairage de la RAM")}${cardRam()}`;
  }
  if (kind === "hp-omen") {
    const o = S.hp_omen || {};
    const b = S.laptop_battery || {};
    const disp = S.laptop_display || {};
    const omenRgb = o.keyboard_type === "FourZoneRgb" || o.keyboard_type === "PerKeyRgb";
    return `
      ${ficheHead(d)}
      ${omenRgb ? linkCard("Rétroéclairage et zones RGB du clavier se règlent dans Éclairage.", "lighting", "omen", "Clavier OMEN") : ""}
      ${linkCard("Ventilateurs, mode thermique et puissance GPU se règlent dans Performances.", "performance", "cooling", "Ventilation & OMEN")}
      ${linkCard("Batterie, luminosité et fréquence d'écran se règlent dans Performances › Alimentation.", "performance", "power", "Alimentation")}
      <div class="card"><div class="card-head"><h2 class="card-title">Informations</h2></div>${infoTable([
        ["Modèle", o.model || S.static_info.model],
        ["Carte mère / BIOS", `${o.board_id || S.static_info.motherboard} · ${S.static_info.bios_version}`],
        ["Processeur", S.static_info.cpu_name],
        ["Carte graphique", S.static_info.gpu_name],
        ["Clavier", o.keyboard_type === "White" ? "Blanc (contrôle matériel Fn + F4)" : o.keyboard_label],
        disp.width ? ["Écran", `${num(disp.width)}×${num(disp.height)} @ ${num(disp.current_hz)} Hz`] : null,
        b.present && b.health_pct != null ? ["Batterie", `${num(b.percent)} % · Santé ${num(b.health_pct).toFixed(0)} % (${num(b.cycle_count)} cycles)`] : null,
        ex ? ["Contrôleur WMI", `${o.wmi_accessible ? "hpqBIntM actif" : o.diagnostic} (v${num(o.thermal_version, 1)})`, true] : null,
      ])}</div>
      <div class="card"><div class="card-head"><h2 class="card-title">Capteurs en direct</h2></div><div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div></div>`;
  }
  return ficheHead(d);
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
      <div class="card accent-purple">
        <div class="card-head"><div><h2 class="card-title">Retour micro (sidetone)</h2><div class="card-desc">T'entendre dans le casque quand tu parles.</div></div></div>
        ${sliderRow("Niveau", { id: `chs-side-${i}`, min: 0, max: 100, value: num(h.sidetone_pct), input: "label", change: "chs-side", data: { dev: i, fmt: "pct100" }, display: `${num(h.sidetone_pct)} %` })}
        <div class="row mt-12"><button type="button" class="btn btn-sm btn-secondary" data-act="chs-side-quick" data-dev="${i}" data-pct="0">Couper</button></div>
      </div>
      ${cardWasapi()}${info}`;
  }
  if (dev.kind === "CoolerHub" && dev.cooler) {
    return `${ficheHead(d)}<div class="card"><div class="card-head"><h2 class="card-title">Watercooling Corsair</h2></div>${infoTable([
      ["Gamme", dev.cooler.product_line_id, true], ["Version de pompe", dev.cooler.pump_version_id, true], ["Taille du radiateur", dev.cooler.radiator_size, true],
    ]) || `<div class="muted">Aucune information supplémentaire.</div>`}</div>${info}`;
  }
  if (dev.kind !== "Keyboard") return `${ficheHead(d)}${info}`;

  const mask = dev.winlock_shortcuts_mask != null ? num(dev.winlock_shortcuts_mask) : 3;
  const deb = num(dev.debounce_timeout_ms, 30);
  const hwpList = arr(dev.hwp_profiles);
  const hwpIdx = clamp(num(local.hwpProfSel[i], 0), 0, Math.max(0, hwpList.length - 1));
  const hp = hwpList[hwpIdx];
  const l = hp && hp.lighting;

  return `
    ${ficheHead(d, dev.esports_switch_on ? `<span class="pill rog">Interrupteur tournoi activé</span>` : "")}
    ${linkCard("Effets, couleurs et peinture par touche se règlent dans Éclairage.", "lighting", "global", "Éclairage")}
    <div class="card accent-cyan">
      <div class="card-head"><h2 class="card-title">Réglages du clavier</h2></div>
      ${dev.pollrate_hz ? `<div class="row-between"><span class="field-label">Fréquence d'envoi</span>${seg([125, 250, 500, 1000, 2000, 4000, 8000].filter((hz) => hz <= (num(dev.max_pollrate_hz, 8000) || 8000)).map((hz) => [hz, fmtHz(hz)]), dev.pollrate_hz, "cs-hz", { dev: i })}</div>` : ""}
      <div class="stack mt-12">${sliderRow("Luminosité matérielle", { id: `cs-bright-${i}`, min: 0, max: 100, value: num(dev.hw_brightness_pct, 100), input: "label", change: "cs-bright", data: { dev: i, fmt: "pct100" }, display: `${num(dev.hw_brightness_pct, 100)} %` })}</div>
      <div class="row-between mt-12">
        <span class="field-label">Qui contrôle l'éclairage</span>
        ${seg([["hw", "Le clavier (autonome)"], ["sw", "OpenGear (en direct)"]], dev.mode, "cs-mode", { dev: i })}
      </div>
      ${dev.active_hw_profile != null ? `<div class="row-between mt-12"><span class="field-label">Profil interne</span>${seg([0, 1, 2].map((n) => [n, `Profil ${n + 1}`]), dev.active_hw_profile, "cs-hwprof", { dev: i })}</div>` : ""}
      ${dev.hw_winlock != null ? `<div class="mt-12">${sw(!!dev.hw_winlock, "Bloquer la touche Windows (dans le clavier)", "cs-winlock", { dev: i })}</div>` : ""}
    </div>
    ${ex ? `
      ${dev.hw_winlock != null ? `
        <div class="card expert-card">
          <div class="card-head"><h2 class="card-title">Raccourcis bloqués par le verrou Windows ${expertTag()}</h2></div>
          <div class="row">${[[1, "Touche Windows"], [2, "Alt + Tab"], [4, "Alt + F4"], [8, "Maj + Tab"]].map(([bit, lbl]) => `<label class="mini-check"><input type="checkbox" data-change="cs-wl-bit" data-dev="${i}" data-bit="${bit}" ${mask & bit ? "checked" : ""} />${esc(lbl)}</label>`).join("")}</div>
        </div>` : ""}
      ${dev.debounce_enabled != null ? `
        <div class="card expert-card">
          <div class="card-head"><div><h2 class="card-title">Anti-rebond matériel ${expertTag()}</h2><div class="card-desc">Délai pendant lequel un second appui est ignoré. Plus bas = plus réactif, mais risque de doubles frappes.</div></div></div>
          <div class="row">
            ${sw(!!dev.debounce_enabled, "Activé", "cs-deb", { dev: i }, { id: `cs-deb-en-${i}` })}
            <select id="cs-deb-ms-${i}" class="og-select auto" data-change="cs-deb" data-dev="${i}" aria-label="Délai anti-rebond">${[0, 1, 2, 3, 5, 10, 15, 20, 30, 60].map((v) => `<option value="${v}" ${deb === v ? "selected" : ""}>${v} ms</option>`).join("")}</select>
            ${deb > 5 ? `<button type="button" class="btn btn-sm btn-secondary" data-act="cs-deb-boost" data-dev="${i}">Passer à 2 ms</button>` : `<span class="pill emerald">Latence optimisée</span>`}
          </div>
        </div>` : ""}
      <div class="card expert-card">
        <div class="card-head"><h2 class="card-title">Couleurs des voyants ${expertTag()}</h2></div>
        <div class="row">
          <select id="cs-ind-${i}" class="og-select auto" aria-label="Voyant">
            <option value="esports">Rétroéclairage mode tournoi</option><option value="winlock_on">Voyant verrou Windows actif</option><option value="winlock_off">Voyant verrou Windows inactif</option><option value="profile">Voyant du bouton profil</option><option value="brightness">Voyant de luminosité</option>
          </select>
          <input type="color" id="cs-ind-col-${i}" class="og-color" value="#FF1848" aria-label="Couleur du voyant" />
          <button type="button" class="btn btn-sm btn-secondary" data-act="cs-ind" data-dev="${i}">Appliquer</button>
        </div>
      </div>
      ${hp ? `
        <div class="card expert-card">
          <div class="card-head">
            <div><h2 class="card-title">Effet enregistré dans le clavier ${expertTag()}</h2><div class="card-desc">Actif quand OpenGear est fermé. Fichier ${esc(hp.profile_file_id)} · éclairage ${esc((l && l.file_id) || "—")} (${num(l && l.led_count)} LEDs)</div></div>
            <div class="row">
              ${hwpList.length > 1 ? `<select id="hwp-prof-${i}" class="og-select auto" data-change="hwp-prof-sel" data-dev="${i}" aria-label="Profil matériel">${hwpList.map((item, pidx) => `<option value="${pidx}" ${pidx === hwpIdx ? "selected" : ""}>Profil ${num(item.slot_index, pidx + 1)} (${esc(item.name)})</option>`).join("")}</select>` : ""}
              <button type="button" class="btn btn-sm btn-secondary" data-act="cs-hwp-reload" data-dev="${i}">Relire le clavier</button>
            </div>
          </div>
          <div class="row">
            <input id="hwp-name-${i}" class="og-input" style="width:160px" value="${esc(hp.name)}" aria-label="Nom du profil" />
            <select id="hwp-kind-${i}" class="og-select auto" aria-label="Effet">${S.hwp_lighting_kinds.map((k) => `<option value="${esc(k.id)}" ${l && l.kind === k.id ? "selected" : ""}>${esc(k.label)}</option>`).join("")}</select>
            <input type="color" id="hwp-c1-${i}" class="og-color" value="${safeHex(l && arr(l.colors)[0], "#1ECFDF")}" aria-label="Couleur 1" />
            <input type="color" id="hwp-c2-${i}" class="og-color" value="${safeHex(l && arr(l.colors)[1], "#9B3CFF")}" aria-label="Couleur 2" />
            <select id="hwp-speed-${i}" class="og-select auto" aria-label="Vitesse">${[[0, "Lent"], [1, "Moyen"], [2, "Rapide"]].map(([v, t]) => `<option value="${v}" ${num(l && l.speed, 1) === v ? "selected" : ""}>${t}</option>`).join("")}</select>
            <select id="hwp-dir-${i}" class="og-select auto" aria-label="Direction">${[[0, "Gauche"], [1, "Droite"], [2, "Haut"], [3, "Bas"], [4, "Horaire"], [5, "Anti-horaire"]].map(([v, t]) => `<option value="${v}" ${num(l && l.direction, 1) === v ? "selected" : ""}>${t}</option>`).join("")}</select>
          </div>
          <div class="row mt-12"><button type="button" class="btn btn-primary" data-act="cs-hwp-flash" data-dev="${i}" data-prof="${hwpIdx}">Écrire l'effet dans le clavier</button></div>
        </div>` : ""}` : advancedHint("Anti-rebond matériel, voyants, raccourcis bloqués et écriture d'effets dans le clavier sont en mode Expert.")}
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
    <div class="card accent-purple">
      <div class="card-head"><div><h2 class="card-title">Retour micro (sidetone)</h2><div class="card-desc">T'entendre dans le casque quand tu parles, sans latence.</div></div></div>
      ${sliderRow("Niveau", { id: `hs-side-${i}`, min: 0, max: 100, value: num(h.sidetone_pct), input: "label", change: "logi-side", data: { dev: i, fmt: "pct100" }, display: `${num(h.sidetone_pct)} %` })}
      <div class="row mt-12"><button type="button" class="btn btn-sm btn-secondary" data-act="logi-side-quick" data-dev="${i}" data-pct="0">Couper</button></div>
    </div>
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
