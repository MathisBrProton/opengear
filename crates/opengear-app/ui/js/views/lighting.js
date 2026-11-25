import { arr, clamp, esc, num, plural, safeHex } from "../utils.js";
import { S, isExpert, local } from "../state.js";
import {
  advancedHint, batteryBadge, emptyState, expertTag, pageHeader, renderKeyboard, sliderRow,
  subtabs, sw, swatches,
} from "../components.js";

const isOmenRgb = (o) => !!(o && o.detected && (o.keyboard_type === "FourZoneRgb" || o.keyboard_type === "PerKeyRgb"));

export function viewLighting() {
  const hasOmenRgb = isOmenRgb(S.hp_omen);
  const items = [
    { id: "global", label: "Effet global" },
    ...(hasOmenRgb ? [{ id: "omen", label: "Clavier OMEN" }] : []),
    { id: "asus", label: "Carte mère" },
    { id: "ram", label: "Mémoire RAM" },
    { id: "aio", label: "Watercooling" },
    { id: "logi", label: "Logitech" },
  ];
  const cur = local.sub.lighting === "omen" && !hasOmenRgb ? "global" : local.sub.lighting;
  let body = "";
  if (cur === "omen") body = lightingOmen();
  else if (cur === "asus") body = lightingAsus();
  else if (cur === "ram") body = cardRam();
  else if (cur === "aio") body = cardSl3Argb();
  else if (cur === "logi") body = lightingLogi();
  else body = lightingGlobal();
  return `
    ${pageHeader("Éclairage", "Un effet pour tout le setup, puis des réglages par zone.", "", "")}
    ${subtabs("lighting", items, cur)}
    ${body}`;
}

export function lightingOmen() {
  const o = S.hp_omen || {};
  if (!isOmenRgb(o)) return emptyState("Clavier OMEN RGB non détecté", "Cette section est réservée aux portables HP OMEN équipés d'un clavier RGB pilotable via BIOS WMI.");
  const zones = arr(o.zone_colors).length === 4 ? o.zone_colors : arr(o.saved_zones).length === 4 ? o.saved_zones : ["#00D4FF", "#00D4FF", "#00D4FF", "#00D4FF"];
  const labels = ["Zone 1 · Gauche", "Zone 2 · Centre-gauche", "Zone 3 · Centre-droite", "Zone 4 · Droite / Pavé"];
  return `
    ${!o.is_admin ? `
      <div class="alert warn mb-16">
        <span class="alert-icon">🔒</span>
        <div class="grow"><div class="alert-title">Droits administrateur requis pour le clavier HP OMEN</div><div class="muted">Le BIOS HP OMEN (hpqBIntM) exige l'exécution en administrateur pour modifier le rétroéclairage et les 4 zones RGB.</div></div>
        <button type="button" class="btn btn-warn btn-sm" data-act="relaunch-admin">Relancer en administrateur</button>
      </div>` : ""}
    <div class="grid-stage">
      <div class="hero-stage"><img class="stage-img" src="/assets/laptop.svg" alt="HP OMEN" style="max-height:240px" /></div>
      <div class="card accent-cyan">
        <div class="card-head">
          <div>
            <h2 class="card-title">Clavier ${esc(o.model || "HP OMEN")}</h2>
            <div class="card-desc">${esc(o.keyboard_label || "Rétroéclairage BIOS WMI")} · ${esc(o.board_id || "")}</div>
          </div>
          <span class="pill ${o.wmi_accessible ? "emerald" : "amber"}"><span class="dot"></span>${o.wmi_accessible ? (o.backlight_on ? "Allumé" : "Éteint") : (o.is_admin ? "WMI absent" : "Admin requis")}</span>
        </div>
        <div class="row mb-16">
          ${sw(!!o.backlight_on, "Rétroéclairage du clavier actif", "omen-kblight")}
          ${o.keyboard_type === "FourZoneRgb" ? sw(!!o.sync_rgb, "Synchroniser avec l'effet OpenGear", "omen-sync") : ""}
        </div>
        ${o.keyboard_type === "FourZoneRgb" ? `
          <div class="divider"></div>
          <div class="stack" style="gap:10px">
            ${zones.map((hex, i) => `
              <div class="color-row">
                <label class="field-label" for="omen-z-${i}" style="min-width:170px">${labels[i]}</label>
                <input type="color" id="omen-z-${i}" class="og-color" value="${safeHex(hex)}" />
              </div>`).join("")}
          </div>
          <div class="color-row mt-12">
            <span class="field-label">Tout peindre en :</span>
            ${swatches("omen-swatch", zones[0])}
          </div>
          <div class="row mt-16">
            <button type="button" class="btn btn-primary" data-act="omen-zones-apply">Appliquer les 4 zones</button>
            <button type="button" class="btn btn-secondary" data-act="omen-refresh">Relire le BIOS</button>
          </div>` : o.keyboard_type === "White"
            ? `<div class="note">Ce modèle dispose d'un clavier <strong>rétroéclairé blanc</strong> (1 seule zone). Certains portables HP OMEN à LED blanches ne sont pilotables que matériellement avec la touche <strong>Fn + F4</strong>.</div>`
            : `<div class="note">Type de clavier détecté : ${esc(o.keyboard_label || "inconnu")}. Utilise l'interrupteur ci-dessus pour allumer ou éteindre le rétroéclairage.</div>`}
      </div>
    </div>`;
}

function lightingGlobal() {
  const prof = S.config.profile;
  const rgb = prof.rgb;
  const syncAll = !!(prof.enable_rgb && prof.sync_asus_aura);
  const keyboards = S.corsair_devices.map((d, i) => ({ d, i })).filter((x) => x.d.kind === "Keyboard");
  const br = num(rgb.brightness, 1);
  const sp = num(rgb.speed, 1);
  return `
    <div class="card accent-cyan">
      <div class="card-head">
        <div>
          <h2 class="card-title">Effet OpenGear</h2>
          <div class="card-desc">Appliqué en direct aux claviers Corsair${S.asus ? " et, si la synchronisation est active, à la carte mère ASUS" : ""}.</div>
        </div>
        <div class="row">
          ${sw(!!prof.enable_rgb, "Éclairage actif", "rgb-enable-sw")}
          ${S.asus ? sw(syncAll, "Synchroniser toutes les marques", "sync-all") : ""}
        </div>
      </div>
      <div class="chip-grid" role="group" aria-label="Effets">
        ${S.effects.map((e) => `<button type="button" class="chip ${rgb.kind === e.id ? "active" : ""}" aria-pressed="${rgb.kind === e.id}" data-act="rgb-kind" data-kind="${esc(e.id)}">${esc(e.label)}</button>`).join("")}
      </div>
      <div class="divider"></div>
      <div class="color-row">
        <label class="field-label" for="rgb-primary">Couleur principale</label>
        <input type="color" class="og-color" id="rgb-primary" value="${safeHex(rgb.primary)}" data-input="rgb-live" data-change="rgb-commit" data-field="primary" />
        ${swatches("rgb-swatch", rgb.primary)}
      </div>
      <div class="color-row mt-12">
        <label class="field-label" for="rgb-secondary">Couleur secondaire</label>
        <input type="color" class="og-color" id="rgb-secondary" value="${safeHex(rgb.secondary, "#9B3CFF")}" data-input="rgb-live" data-change="rgb-commit" data-field="secondary" />
        <label class="mini-check"><input type="checkbox" ${rgb.reverse ? "checked" : ""} data-change="rgb-reverse" /> Inverser le sens de l'animation</label>
      </div>
      <div class="stack mt-16" style="gap:10px">
        ${sliderRow("Luminosité", { id: "rgb-brightness", min: 0, max: 1, step: 0.05, value: br, input: "rgb-live", change: "rgb-commit", data: { field: "brightness", fmt: "pct" }, display: `${Math.round(br * 100)} %` })}
        ${sliderRow("Vitesse", { id: "rgb-speed", min: 0.2, max: 4.5, step: 0.1, value: sp, input: "rgb-live", change: "rgb-commit", data: { field: "speed", fmt: "x" }, display: `${sp.toFixed(1)}×` })}
      </div>
    </div>

    ${keyboards.length ? keyboards.map(({ d, i }) => `
      <div class="card mt-16">
        <div class="card-head">
          <div>
            <h2 class="card-title">${esc(d.product_name)} — aperçu et peinture par touche</h2>
            <div class="card-desc">Active le pinceau puis clique sur les touches pour leur donner une couleur. L'effet passe automatiquement en « par touche ».</div>
          </div>
          <div class="row">
            <button type="button" class="btn btn-sm ${local.paintMode ? "btn-primary" : "btn-secondary"}" data-act="paint-toggle" aria-pressed="${local.paintMode}">${local.paintMode ? "Pinceau actif" : "Activer le pinceau"}</button>
            <input type="color" class="og-color" id="paint-color" value="${safeHex(local.paintColor)}" data-input="paint-color" aria-label="Couleur du pinceau" />
            <button type="button" class="btn btn-sm btn-secondary" data-act="clear-per-key">Effacer la peinture</button>
          </div>
        </div>
        ${renderKeyboard("paint", d)}
        <div class="stack mt-16" style="gap:10px">
          ${sliderRow("Luminosité matérielle", { id: `cs-bright-${i}`, min: 0, max: 100, value: num(d.hw_brightness_pct, 100), input: "label", change: "cs-bright", data: { dev: i, fmt: "pct100" }, display: `${num(d.hw_brightness_pct, 100)} %` })}
        </div>
      </div>`).join("") : ""}`;
}

function lightingAsus() {
  if (!S.asus) {
    return emptyState("Carte mère ASUS non détectée", `${S.asus_err ? S.asus_err + " — " : ""}Seules les cartes mères ASUS équipées d'un contrôleur Aura USB sont prises en charge. Si ta carte est compatible, vérifie qu'Armoury Crate n'est pas en train de l'utiliser puis réessaie.`,
      `<button type="button" class="btn btn-primary" data-act="rescan">Réessayer la connexion</button>`);
  }
  const a = S.asus;
  const direct = !!S.config.profile.sync_asus_aura;
  const mode = local.auraDraftMode || a.hw_mode;
  const color = safeHex(a.hw_color, "#FF1848");
  const hotspots = [
    { label: "Zone E/S", x: 18, y: 28 },
    { label: "RAM", x: 58, y: 34 },
    { label: "Chipset", x: 78, y: 69 },
    { label: "ARGB 1", x: 75, y: 12 },
    { label: "ARGB 2", x: 79, y: 55 },
    { label: "ARGB 3", x: 32, y: 92 },
  ];
  const argb = arr(a.channels).filter((c) => c.kind === "argb");
  return `
    <div class="grid-stage">
      <div class="stack">
        <div class="hero-stage rog-bg">
          <div class="stage-img-wrap">
            <img class="stage-img" src="/assets/motherboard.svg" alt="Carte mère" />
            ${hotspots.map((h) => `<span class="mb-hotspot" style="left:${h.x}%;top:${h.y}%;--zone-col:${color}"><span class="led-dot"></span>${esc(h.label)}</span>`).join("")}
          </div>
        </div>
        <div class="note">Les effets « matériels » tournent dans la carte mère elle-même, même quand OpenGear est fermé. La synchronisation, elle, nécessite qu'OpenGear tourne.</div>
      </div>
      <div class="stack">
        <div class="card accent-rog">
          <div class="card-head">
            <div>
              <h2 class="card-title">Éclairage de la carte mère</h2>
              <div class="card-desc">${esc(S.static_info.motherboard || a.product_name || "")}</div>
            </div>
            ${sw(direct, "Synchroniser avec l'effet OpenGear", "asus-direct-sw")}
          </div>
          ${direct ? `<div class="note">La carte mère suit l'effet global (onglet « Effet global »). Désactive la synchronisation pour choisir un effet propre à la carte mère.</div>` : `
            <div class="chip-grid" role="group" aria-label="Effets carte mère">
              ${S.aura_modes.map((m) => `<button type="button" class="chip rog ${mode === m.id ? "active" : ""}" aria-pressed="${mode === m.id}" data-act="aura-mode" data-mode="${esc(m.id)}">${esc(m.label)}</button>`).join("")}
            </div>
            <div class="color-row mt-12">
              <label class="field-label" for="asus-color">Couleur</label>
              <input type="color" class="og-color" id="asus-color" value="${color}" />
              ${swatches("aura-swatch", a.hw_color)}
            </div>
            <div class="row mt-16">
              <button type="button" class="btn btn-primary" data-act="aura-apply">Appliquer</button>
              ${isExpert() ? `<button type="button" class="btn btn-secondary" data-act="aura-commit" title="Écrit l'effet dans la mémoire de la carte mère">Enregistrer dans la carte mère</button>` : ""}
            </div>`}
        </div>
        ${isExpert() ? `
          <div class="card expert-card">
            <div class="card-head"><div><h2 class="card-title">Éclairage PC éteint ${expertTag()}</h2><div class="card-desc">Écrit de façon permanente dans la carte mère.</div></div></div>
            <div class="row">
              <select id="asus-sd-mode" class="og-select auto" aria-label="Effet PC éteint">
                ${[["Off", "Éteint"], ["Static", "Statique"], ["Breathing", "Respiration"], ["Rainbow", "Arc-en-ciel"]].map(([v, l]) => `<option value="${v}" ${a.shutdown_mode === v ? "selected" : ""}>${l}</option>`).join("")}
              </select>
              <input type="color" id="asus-sd-color" class="og-color" value="${safeHex(a.shutdown_color, "#000000")}" aria-label="Couleur PC éteint" />
              <button type="button" class="btn btn-secondary" data-act="aura-shutdown">Enregistrer</button>
            </div>
          </div>
          ${argb.length ? `
            <div class="card expert-card">
              <div class="card-head"><div><h2 class="card-title">Nombre de LEDs par connecteur ARGB ${expertTag()}</h2><div class="card-desc">Nécessaire pour la synchronisation en direct des bandeaux et ventilateurs ARGB.</div></div></div>
              <div class="stack" style="gap:10px">
                ${argb.map((c) => sliderRow(c.name, { id: `argb-${c.index}`, min: 4, max: 90, value: num(c.led_count, 16), input: "label", change: "argb-count", data: { ch: c.index, fmt: "leds" }, display: `${num(c.led_count, 16)} LEDs` })).join("")}
              </div>
            </div>` : ""}
          <div class="card expert-card">
            <table class="meta-table">
              <tr><td class="k">Contrôleur</td><td class="mono">${esc(a.vid)}:${esc(a.pid)} · ${esc(a.firmware)}</td></tr>
              <tr><td class="k">Topologie</td><td>${num(a.mainboard_fixed_leds)} LEDs carte · ${num(a.rgb_12v_headers)} RGB 12 V · ${num(a.argb_5v_headers)} ARGB 5 V</td></tr>
            </table>
          </div>` : advancedHint("Enregistrement permanent, éclairage PC éteint et calibration ARGB sont disponibles en mode Expert.")}
      </div>
    </div>`;
}

export function cardRam() {
  const ram = S.ram_rgb;
  if (!ram) return emptyState("Mémoire RGB indisponible", "Le moteur OpenGear n'a pas renvoyé d'informations sur la mémoire vive.");
  const dimms = arr(ram.physical_dimms);
  const ene = arr(ram.ene_modules);
  const total = dimms.reduce((a, d) => a + num(d.capacity_gb), 0);
  const speedLabels = ["Très rapide", "Rapide", "Normal", "Lent", "Très lent"];
  const spd = clamp(num(ram.last_speed, 2), 0, 4);
  return `
    <div class="card accent-cyan">
      <div class="card-head">
        <div>
          <h2 class="card-title">Mémoire RAM</h2>
          <div class="card-desc">${plural(dimms.length, "barrette", "barrettes")}${total ? ` · ${total} Go` : ""}${dimms[0] ? ` ${esc(dimms[0].memory_type)}` : ""}${dimms[0] && dimms[0].speed_mts ? ` à ${num(dimms[0].speed_mts)} MT/s` : ""}</div>
        </div>
        <span class="pill ${ene.length ? "emerald" : "amber"}"><span class="dot"></span>${ene.length ? plural(ene.length, "barrette RGB pilotable", "barrettes RGB pilotables") : ram.is_admin ? "Aucun RGB pilotable" : "Droits administrateur requis"}</span>
      </div>
      ${ram.message ? `<div class="note mb-16">${esc(ram.message)}</div>` : ""}
      ${!ram.is_admin ? `
        <div class="row">
          <button type="button" class="btn btn-warn" data-act="relaunch-admin">Relancer en administrateur</button>
          <button type="button" class="btn btn-secondary" data-act="ram-scan">Actualiser</button>
        </div>` : ene.length ? `
        <div class="chip-grid" role="group" aria-label="Effets RAM">
          ${S.ram_rgb_modes.map((m) => `<button type="button" class="chip ${ram.last_mode === m.id ? "active" : ""}" aria-pressed="${ram.last_mode === m.id}" data-act="ram-mode" data-mode="${esc(m.id)}">${esc(m.label)}</button>`).join("")}
        </div>
        <div class="color-row mt-12">
          <label class="field-label" for="ram-color">Couleur</label>
          <input type="color" id="ram-color" class="og-color" value="${safeHex(ram.last_color, "#00D2FF")}" />
          ${swatches("ram-swatch", ram.last_color)}
        </div>
        <div class="stack mt-12">
          ${sliderRow("Vitesse de l'effet", { id: "ram-speed", min: 0, max: 4, value: spd, input: "label", data: { fmt: "ramspeed" }, display: speedLabels[spd] })}
        </div>
        <div class="row mt-16">
          <button type="button" class="btn btn-primary" data-act="ram-apply">Appliquer</button>
          ${S.asus ? `<button type="button" class="btn btn-secondary" data-act="ram-sync-aura">Reprendre la couleur de la carte mère</button>` : ""}
          ${isExpert() ? `<button type="button" class="btn btn-secondary" data-act="ram-eeprom">Enregistrer dans les barrettes</button>` : ""}
          <button type="button" class="btn btn-ghost" data-act="ram-scan">Actualiser</button>
        </div>` : `
        <div class="row"><button type="button" class="btn btn-primary" data-act="ram-scan">Rechercher les barrettes RGB (lecture seule)</button></div>`}
      ${isExpert() && (dimms.length || ene.length) ? `
        <div class="divider"></div>
        <div class="card-eyebrow" style="margin-bottom:8px">Détails techniques ${expertTag()}</div>
        <table class="meta-table">
          ${dimms.map((d) => `<tr><td class="k">${esc(d.slot)}</td><td>${esc(d.manufacturer)} <span class="mono">${esc(d.part_number)}</span> · ${num(d.capacity_gb)} Go ${esc(d.memory_type)} · ${num(d.speed_mts)} MT/s</td></tr>`).join("")}
          ${ene.map((m) => `<tr><td class="k">Contrôleur 0x${num(m.address_7bit).toString(16).toUpperCase().padStart(2, "0")}</td><td class="mono">${esc(m.firmware)} · ${num(m.led_count)} LEDs · ${m.is_v2 ? "V2" : "V1"}</td></tr>`).join("")}
          <tr><td class="k">Pilote PawnIO</td><td>${esc(ram.pawnio_status || "inconnu")}</td></tr>
        </table>` : ""}
      ${advancedHint("L'enregistrement permanent dans les barrettes et les détails techniques sont en mode Expert.")}
    </div>`;
}

function cardSl3Argb() {
  const argb = arr(S.asus && S.asus.channels).filter((x) => x.kind === "argb");
  if (!argb.length) {
    return emptyState("Aucun connecteur ARGB disponible", "L'éclairage du bloc de watercooling passe par les connecteurs ARGB de la carte mère ASUS. Aucun contrôleur Aura compatible n'a été détecté.",
      `<button type="button" class="btn btn-secondary" data-act="rescan">Relancer la recherche</button>`);
  }
  const prof = S.config.profile;
  const ch = prof.silent_loop3_argb_channel != null ? prof.silent_loop3_argb_channel : argb[0].index;
  const tempSrc = S.telemetry.sensors && S.telemetry.sensors.cpu_temp_c != null ? "du processeur" : "de la carte graphique";
  return `
    <div class="grid-stage">
      <div class="hero-stage amber-bg"><img class="stage-img" src="/assets/aio.svg" alt="Watercooling" style="max-height:250px" /></div>
      <div class="card accent-amber">
        <div class="card-head"><div><h2 class="card-title">Éclairage du watercooling</h2><div class="card-desc">Bloc pompe relié à un connecteur ARGB de la carte mère.</div></div></div>
        <div class="row">
          <label class="field-label" for="sl3-ch">Connecteur</label>
          <select id="sl3-ch" class="og-select auto">
            ${argb.map((x) => `<option value="${num(x.index)}" ${x.index === ch ? "selected" : ""}>${esc(x.name)} (${num(x.led_count)} LEDs)</option>`).join("")}
          </select>
        </div>
        <div class="color-row mt-12">
          <label class="field-label" for="sl3-color">Couleur</label>
          <input type="color" id="sl3-color" class="og-color" value="${safeHex(local.sl3Color, "#FF8C1A")}" />
          <button type="button" class="btn btn-primary btn-sm" data-act="sl3-color">Appliquer</button>
        </div>
        <div class="divider"></div>
        ${sw(!!prof.silent_loop3_thermal_rgb, `Couleur selon la température ${esc(tempSrc)} (cyan → vert → ambre → rouge)`, "sl3-thermal")}
      </div>
    </div>`;
}

function lightingLogi() {
  const kbs = S.logi_keyboards;
  const mice = S.logi_mice.map((m, i) => ({ m, i })).filter((x) => arr(x.m.led_zones).length);
  if (!kbs.length && !mice.length) {
    return emptyState("Aucun appareil Logitech avec éclairage détecté", "Branche le récepteur LIGHTSPEED / Bolt ou le câble USB de ton clavier ou de ta souris Logitech, puis relance la recherche.",
      `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`);
  }
  return `<div class="grid-auto">${kbs.map((k, i) => cardLogiKbLighting(k, i)).join("")}${mice.map(({ m, i }) => cardLogiMouseLed(m, i)).join("")}</div>`;
}

export function cardLogiKbLighting(kb, i) {
  return `
    <div class="card accent-cyan">
      <div class="card-head"><div><h2 class="card-title">${esc(kb.name)}</h2><div class="card-desc">Éclairage du clavier</div></div>${kb.battery && kb.battery.percent != null ? batteryBadge(kb.battery.percent, !!kb.battery.is_charging) : ""}</div>
      <div class="row">
        <select id="lkb-mode-${i}" class="og-select auto" aria-label="Effet">
          <option value="Fixed">Statique</option><option value="Breathing">Respiration</option><option value="Cycle">Cycle de couleurs</option><option value="Wave">Vague arc-en-ciel</option><option value="Off">Éteint</option>
        </select>
        <input type="color" id="lkb-col-${i}" class="og-color" value="#00D4FF" aria-label="Couleur" />
        <button type="button" class="btn btn-primary btn-sm" data-act="lkb-apply" data-dev="${i}">Appliquer</button>
      </div>
      ${kb.supports_brightness ? `<div class="stack mt-12">${sliderRow("Luminosité", { id: `lkb-br-${i}`, min: 0, max: 100, value: num(kb.brightness_pct, 100), input: "label", change: "lkb-bright", data: { dev: i, fmt: "pct100" }, display: `${num(kb.brightness_pct, 100)} %` })}</div>` : ""}
    </div>`;
}

export function cardLogiMouseLed(m, i) {
  return `
    <div class="card accent-cyan">
      <div class="card-head"><div><h2 class="card-title">${esc(m.name)}</h2><div class="card-desc">Éclairage de la souris (${plural(arr(m.led_zones).length, "zone", "zones")})</div></div></div>
      <div class="row">
        <select id="lml-mode-${i}" class="og-select auto" aria-label="Effet">
          <option value="Fixed">Statique</option><option value="Breathing">Respiration</option><option value="Cycle">Cycle de couleurs</option><option value="Off">Éteint</option>
        </select>
        <input type="color" id="lml-col-${i}" class="og-color" value="#00D4FF" aria-label="Couleur" />
        <button type="button" class="btn btn-primary btn-sm" data-act="lml-apply" data-dev="${i}">Appliquer</button>
      </div>
    </div>`;
}
