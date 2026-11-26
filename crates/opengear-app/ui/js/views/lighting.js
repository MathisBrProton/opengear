import { arr, clamp, esc, num, plural, safeHex } from "../utils.js";
import { S, isExpert, local } from "../state.js";
import {
  advancedHint, deviceSettings, emptyState, expertTag, pageHeader, renderKeyboard, seg, sliderRow,
  subtabs, sw, swatches,
} from "../components.js";

function lightingDevices() {
  return arr(S && S.device_list).filter((d) => !!d.lighting);
}

function deviceCfg(devId) {
  const map = (S && S.config && S.config.profile && S.config.profile.device_lighting) || {};
  return map[devId] || { enabled: false, mode: null, zones: {}, zone_led_counts: {} };
}

function overrideModeName(ov, isZone) {
  if (!ov) return isZone ? "inherit" : "global";
  return ov.mode || "global";
}

function getDraft(devId, zoneId, ov, hwEffects) {
  const k = `${devId}|${zoneId || ""}`;
  const isZone = zoneId != null;
  if (!local.lightingDrafts[k]) {
    const rgb = S.config.profile.rgb;
    const swEff = ov && ov.mode === "software" && ov.effect ? ov.effect : rgb;
    const hwReq = ov && ov.mode === "hardware" && ov.request ? ov.request : null;
    const firstHw = arr(hwEffects)[0] || null;
    local.lightingDrafts[k] = {
      mode: overrideModeName(ov, isZone),
      swKind: swEff.kind || "Static",
      swPrimary: safeHex(swEff.primary, "#00D4FF"),
      swSecondary: safeHex(swEff.secondary, "#9B3CFF"),
      swSpeed: num(swEff.speed, 1),
      swBrightness: num(swEff.brightness, 1),
      swReverse: !!swEff.reverse,
      hwEffect: hwReq ? hwReq.effect : (firstHw ? firstHw.id : ""),
      hwColor1: safeHex(hwReq && hwReq.colors && hwReq.colors[0], "#00D4FF"),
      hwColor2: safeHex(hwReq && hwReq.colors && hwReq.colors[1], "#9B3CFF"),
      hwSpeed: hwReq && hwReq.speed != null ? num(hwReq.speed) : (firstHw && firstHw.params && firstHw.params.speed ? Math.floor((firstHw.params.speed[0] + firstHw.params.speed[1]) / 2) : 1),
      hwReverse: !!(hwReq && hwReq.reverse),
    };
  }
  return local.lightingDrafts[k];
}

export function buildOverridePayload(devId, zoneId = null) {
  const dev = lightingDevices().find((d) => d.info.id === devId);
  const hwEffects = arr(dev && dev.lighting && dev.lighting.hardware_effects);
  const k = `${devId}|${zoneId || ""}`;
  const d = local.lightingDrafts[k] || {};
  if (d.mode === "inherit") return null;
  if (d.mode === "global") return { mode: "global" };
  if (d.mode === "off") return { mode: "off" };
  if (d.mode === "software") {
    return {
      mode: "software",
      effect: {
        kind: d.swKind || "Static",
        primary: safeHex(d.swPrimary, "#00D4FF"),
        secondary: safeHex(d.swSecondary, "#9B3CFF"),
        speed: num(d.swSpeed, 1),
        brightness: num(d.swBrightness, 1),
        reverse: !!d.swReverse,
      },
    };
  }
  if (d.mode === "hardware") {
    const def = hwEffects.find((e) => e.id === d.hwEffect) || hwEffects[0];
    if (!def) return null;
    const p = def.params || {};
    const cRange = arr(p.colors);
    const maxColors = num(cRange[1], 0);
    const colors = [];
    if (maxColors >= 1) colors.push(safeHex(d.hwColor1, "#00D4FF"));
    if (maxColors >= 2) colors.push(safeHex(d.hwColor2, "#9B3CFF"));
    return {
      mode: "hardware",
      request: {
        effect: def.id,
        zone: zoneId || null,
        colors,
        speed: p.speed ? clamp(Math.round(num(d.hwSpeed, p.speed[0])), p.speed[0], p.speed[1]) : null,
        brightness: null,
        reverse: !!(p.direction && d.hwReverse),
        save: false,
      },
    };
  }
  return null;
}

export function viewLighting() {
  const devs = lightingDevices();
  const items = [
    { id: "global", label: "Effet global" },
    { id: "devices", label: `Appareils & zones (${devs.length})` },
    { id: "canvas", label: "Disposition bureau" },
  ];
  const validIds = new Set(items.map((x) => x.id));
  const cur = validIds.has(local.sub.lighting) ? local.sub.lighting : "global";
  let body = "";
  if (cur === "devices") body = lightingDevicesView(devs);
  else if (cur === "canvas") body = lightingCanvas(devs);
  else body = lightingGlobal(devs);
  return `
    ${pageHeader("Éclairage", "Un effet commun synchronisé sur tout le bureau, puis des réglages fins par appareil et par zone.")}
    ${subtabs("lighting", items, cur)}
    ${body}`;
}

function lightingGlobal(devs) {
  if (!devs.length) {
    return emptyState(
      "Aucun appareil RGB détecté",
      "Branche un clavier, une souris, une carte mère ou des barrettes RAM compatibles puis relance la détection.",
      `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la détection</button>`,
    );
  }
  const prof = S.config.profile;
  const rgb = prof.rgb;
  const syncedCount = devs.filter((d) => deviceCfg(d.info.id).enabled).length;
  const allEnabled = devs.length > 0 && syncedCount === devs.length;
  const br = num(rgb.brightness, 1);
  const sp = num(rgb.speed, 1);
  const perKeyBoards = devs.filter((d) => arr(d.lighting.zones).some((z) => arr(z.leds).some((l) => l.key_code)));

  return `
    <div class="card accent-cyan">
      <div class="card-head">
        <div>
          <h2 class="card-title">Effet logiciel OpenGear</h2>
          <div class="card-desc">${syncedCount} / ${devs.length} ${syncedCount > 1 ? "appareils actifs" : "appareil actif"} en direct</div>
        </div>
        <div class="row">
          ${devs.length ? sw(allEnabled, "Tout synchroniser", "lighting-sync-all") : ""}
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
        <label class="mini-check"><input type="checkbox" ${rgb.reverse ? "checked" : ""} data-change="rgb-reverse" /> Inverser le sens</label>
      </div>
      <div class="stack mt-16 gap-10px">
        ${sliderRow("Luminosité", { id: "rgb-brightness", min: 0, max: 1, step: 0.05, value: br, input: "rgb-live", change: "rgb-commit", data: { field: "brightness", fmt: "pct" }, display: `${Math.round(br * 100)} %` })}
        ${sliderRow("Vitesse", { id: "rgb-speed", min: 0.2, max: 4.5, step: 0.1, value: sp, input: "rgb-live", change: "rgb-commit", data: { field: "speed", fmt: "x" }, display: `${sp.toFixed(1)}×` })}
      </div>
    </div>

    <div class="card mt-16">
      <div class="card-head">
        <div>
          <h2 class="card-title">Calques RGB (composition)</h2>
          <div class="card-desc">Superpose des effets par-dessus l'effet global (ex. ondulation réactive ou barre audio filtrée sur certaines touches).</div>
        </div>
        <button type="button" class="btn btn-sm btn-primary" data-act="rgb-layer-add">+ Ajouter un calque</button>
      </div>
      ${arr(prof.rgb_layers).length === 0 ? `<div class="muted">Aucun calque superposé. Ajoute un calque pour combiner plusieurs effets en temps réel.</div>` : `
        <div class="stack gap-10px">
          ${arr(prof.rgb_layers).map((layer, li) => {
            const ef = layer.effect || {};
            const opPct = Math.round(num(layer.opacity, 1) * 100);
            const blends = arr(S.blend_modes).length ? arr(S.blend_modes) : [
              { id: "Normal", label: "Normal" },
              { id: "Add", label: "Addition" },
              { id: "Screen", label: "Écran" },
              { id: "Max", label: "Maximum" },
            ];
            return `
              <div class="zone-item">
                <div class="row-between mb-8">
                  <div class="row">
                    ${sw(!!layer.enabled, layer.name || `Calque ${li + 1}`, "rgb-layer-en", { idx: li })}
                  </div>
                  <div class="row">
                    <select class="og-select auto" data-change="rgb-layer-blend" data-idx="${li}" aria-label="Mode de fusion">
                      ${blends.map((b) => `<option value="${esc(b.id)}" ${layer.blend === b.id ? "selected" : ""}>${esc(b.label)}</option>`).join("")}
                    </select>
                    <button type="button" class="btn btn-sm btn-ghost" data-act="rgb-layer-del" data-idx="${li}">Supprimer</button>
                  </div>
                </div>
                <div class="row gap-2 flex-wrap">
                  <select class="og-select auto" data-change="rgb-layer-kind" data-idx="${li}" aria-label="Effet du calque">
                    ${S.effects.map((e) => `<option value="${esc(e.id)}" ${ef.kind === e.id ? "selected" : ""}>${esc(e.label)}</option>`).join("")}
                  </select>
                  <input type="color" class="og-color" value="${safeHex(ef.primary, "#00D4FF")}" data-change="rgb-layer-color" data-idx="${li}" data-field="primary" aria-label="Couleur principale calque" />
                  <input type="color" class="og-color" value="${safeHex(ef.secondary, "#9B3CFF")}" data-change="rgb-layer-color" data-idx="${li}" data-field="secondary" aria-label="Couleur secondaire calque" />
                  <input class="og-input mono grow" value="${esc(arr(layer.key_filter || layer.keys).join(", "))}" placeholder="Toutes les touches (ou KeyQ,KeyW…)" data-change="rgb-layer-keys" data-idx="${li}" aria-label="Filtre de touches" />
                </div>
                <div class="mt-8">${sliderRow("Opacité", { min: 0, max: 1, step: 0.05, value: num(layer.opacity, 1), input: "label", change: "rgb-layer-opacity", data: { idx: li, fmt: "pct" }, display: `${opPct} %` })}</div>
              </div>`;
          }).join("")}
        </div>`}
    </div>

    ${perKeyBoards.map((d) => `
      <div class="card mt-16">
        <div class="card-head">
          <div>
            <h2 class="card-title">${esc(d.info.model)} — peinture par touche</h2>
            <div class="card-desc">Active le pinceau puis clique sur les touches (mémorisées par position physique).</div>
          </div>
          <div class="row">
            <button type="button" class="btn btn-sm ${local.paintMode ? "btn-primary" : "btn-secondary"}" data-act="paint-toggle" aria-pressed="${local.paintMode}">${local.paintMode ? "Pinceau actif" : "Activer le pinceau"}</button>
            <input type="color" class="og-color" id="paint-color" value="${safeHex(local.paintColor)}" data-input="paint-color" aria-label="Couleur du pinceau" />
            <button type="button" class="btn btn-sm btn-secondary" data-act="clear-per-key">Effacer la peinture</button>
          </div>
        </div>
        ${renderKeyboard("paint", { product_name: d.info.model })}
        <div class="stack mt-16 gap-10px">
          ${deviceSettings(d.info.id, { sections: ["lighting"], bare: true })}
        </div>
      </div>`).join("")}`;
}

function renderOverrideEditor(devId, zoneId, hwEffects, existingOverride) {
  const d = getDraft(devId, zoneId, existingOverride, hwEffects);
  const isZone = zoneId != null;
  const zoneHwEffects = isZone ? arr(hwEffects).filter((e) => arr(e.zones).includes(zoneId)) : arr(hwEffects);
  const modeOpts = [
    ...(isZone ? [["inherit", "Identique à l'appareil"]] : []),
    ["global", "Effet global OpenGear"],
    ["software", "Effet logiciel personnalisé"],
    ...(zoneHwEffects.length ? [["hardware", "Effet matériel interne"]] : []),
    ["off", "Éteint"],
  ];
  const curDef = zoneHwEffects.find((e) => e.id === d.hwEffect) || zoneHwEffects[0] || null;
  const cMax = curDef && curDef.params ? num(arr(curDef.params.colors)[1], 0) : 0;
  const spdRange = curDef && curDef.params ? curDef.params.speed : null;
  const dataAttr = `data-dev-id="${esc(devId)}" ${isZone ? `data-zone-id="${esc(zoneId)}"` : ""}`;

  return `
    <div class="stack gap-10px">
      <div class="row-between pair-row">
        <span class="field-label">${isZone ? "Source de la zone" : "Mode d'éclairage"}</span>
        ${seg(modeOpts, d.mode, "lgt-mode", isZone ? { devId, zoneId } : { devId })}
      </div>
      ${d.mode === "software" ? `
        <div class="chip-grid">
          ${S.effects.map((e) => `<button type="button" class="chip ${d.swKind === e.id ? "active" : ""}" data-act="lgt-sw-kind" ${dataAttr} data-kind="${esc(e.id)}">${esc(e.label)}</button>`).join("")}
        </div>
        <div class="color-row">
          <span class="field-label">Couleurs</span>
          <input type="color" class="og-color" value="${safeHex(d.swPrimary)}" data-input="lgt-draft" data-change="lgt-commit" ${dataAttr} data-field="swPrimary" />
          <input type="color" class="og-color" value="${safeHex(d.swSecondary)}" data-input="lgt-draft" data-change="lgt-commit" ${dataAttr} data-field="swSecondary" />
          <label class="mini-check"><input type="checkbox" ${d.swReverse ? "checked" : ""} data-change="lgt-draft-chk" ${dataAttr} data-field="swReverse" /> Inverser</label>
        </div>
        ${sliderRow("Luminosité", { min: 0, max: 1, step: 0.05, value: d.swBrightness, input: "lgt-draft", change: "lgt-commit", data: { devId, ...(isZone ? { zoneId } : {}), field: "swBrightness", fmt: "pct" }, display: `${Math.round(d.swBrightness * 100)} %` })}
        ${sliderRow("Vitesse", { min: 0.2, max: 4.5, step: 0.1, value: d.swSpeed, input: "lgt-draft", change: "lgt-commit", data: { devId, ...(isZone ? { zoneId } : {}), field: "swSpeed", fmt: "x" }, display: `${num(d.swSpeed, 1).toFixed(1)}×` })}` : ""}
      ${d.mode === "hardware" && curDef ? `
        <div class="chip-grid">
          ${zoneHwEffects.map((e) => `<button type="button" class="chip ${curDef.id === e.id ? "active" : ""}" data-act="lgt-hw-pick" ${dataAttr} data-effect="${esc(e.id)}">${esc(e.label)}</button>`).join("")}
        </div>
        ${cMax >= 1 ? `
          <div class="color-row">
            <span class="field-label">${cMax > 1 ? "Couleurs" : "Couleur"}</span>
            <input type="color" class="og-color" value="${safeHex(d.hwColor1)}" data-input="lgt-draft" data-change="lgt-commit" ${dataAttr} data-field="hwColor1" />
            ${cMax >= 2 ? `<input type="color" class="og-color" value="${safeHex(d.hwColor2)}" data-input="lgt-draft" data-change="lgt-commit" ${dataAttr} data-field="hwColor2" />` : ""}
            ${curDef.params.direction ? `<label class="mini-check"><input type="checkbox" ${d.hwReverse ? "checked" : ""} data-change="lgt-draft-chk" ${dataAttr} data-field="hwReverse" /> Inverser</label>` : ""}
          </div>` : (curDef.params.direction ? `<label class="mini-check"><input type="checkbox" ${d.hwReverse ? "checked" : ""} data-change="lgt-draft-chk" ${dataAttr} data-field="hwReverse" /> Inverser le sens</label>` : "")}
        ${spdRange ? sliderRow("Vitesse", { min: spdRange[0], max: spdRange[1], step: 1, value: clamp(num(d.hwSpeed, spdRange[0]), spdRange[0], spdRange[1]), input: "lgt-draft", change: "lgt-commit", data: { devId, ...(isZone ? { zoneId } : {}), field: "hwSpeed" }, display: String(clamp(num(d.hwSpeed, spdRange[0]), spdRange[0], spdRange[1])) }) : ""}
        ${curDef.params.savable && isExpert() ? `
          <div class="row">
            <button type="button" class="btn btn-secondary btn-sm" data-act="lgt-apply" ${dataAttr} data-save="1">Enregistrer en mémoire (flash) ${expertTag()}</button>
          </div>` : ""}` : ""}
    </div>`;
}

function lightingDevicesView(devs) {
  if (!devs.length) {
    return emptyState("Aucun appareil RGB détecté", "Branche un clavier, une souris, une carte mère ou des barrettes RAM compatibles puis relance la détection.", `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la détection</button>`);
  }
  return `
    <div class="stack gap-4">
      ${devs.map((d) => {
        const l = d.lighting;
        const cfg = deviceCfg(d.info.id);
        const totalLeds = arr(l.zones).reduce((s, z) => s + arr(z.leds).length, 0);
        const extraSettings = deviceSettings(d.info.id, { sections: ["lighting"], bare: true });
        return `
          <div class="card accent-cyan">
            <div class="card-head">
              <div>
                <h2 class="card-title">${esc(d.info.model)}</h2>
                <div class="card-desc">${esc(d.info.vendor)} · ${plural(arr(l.zones).length, "zone", "zones")} · ${plural(totalLeds, "LED", "LEDs")} · max ${num(l.max_fps, 30)} FPS</div>
              </div>
              <div class="row">
                ${sw(!!cfg.enabled, "Piloté par OpenGear", "lgt-dev-en", { devId: d.info.id })}
              </div>
            </div>
            ${renderOverrideEditor(d.info.id, null, l.hardware_effects, cfg.mode)}
            ${extraSettings ? `<div class="mt-16">${extraSettings}</div>` : ""}
            ${arr(l.zones).length > 1 || arr(l.zones).some((z) => z.resizable) ? `
              <div class="divider"></div>
              <div class="card-eyebrow mb-8">Zones de l'appareil</div>
              <div class="zone-list">
                ${l.zones.map((z) => {
                  const zOv = (cfg.zones && cfg.zones[z.id]) || null;
                  const count = num((cfg.zone_led_counts && cfg.zone_led_counts[z.id]) || arr(z.leds).length);
                  return `
                    <div class="zone-item">
                      <div class="row-between mb-8">
                        <strong>${esc(z.name)}</strong>
                        <span class="pill">${plural(count, "LED", "LEDs")}</span>
                      </div>
                      ${z.resizable ? sliderRow("Nombre de LEDs", { labelHtml: `Nombre de LEDs ${isExpert() ? expertTag() : ""}`, min: z.resizable[0], max: z.resizable[1], step: 1, value: count, input: "label", change: "lgt-resize-zone", data: { devId: d.info.id, zoneId: z.id, fmt: "leds" }, display: `${count} LEDs` }) : ""}
                      ${arr(l.zones).length > 1 ? renderOverrideEditor(d.info.id, z.id, l.hardware_effects, zOv) : ""}
                    </div>`;
                }).join("")}
              </div>` : ""}
          </div>`;
      }).join("")}
      ${!isExpert() ? advancedHint("L'enregistrement permanent en mémoire flash / EEPROM des effets matériels est disponible en mode Expert.") : ""}
    </div>`;
}

function lightingCanvas(devs) {
  if (!devs.length) {
    return emptyState("Aucun appareil à placer", "Branche un périphérique ou une carte mère compatible pour disposer son emplacement sur le bureau.");
  }
  const placements = (S.config && S.config.device_placements) || {};
  const selectedId = devs.some((d) => d.info.id === local.canvasSelectedId) ? local.canvasSelectedId : devs[0].info.id;
  local.canvasSelectedId = selectedId;
  const selDev = devs.find((d) => d.info.id === selectedId);
  const selPl = placements[selectedId] || selDev.lighting.placement || { x: 0, y: 0, width: 1, height: 1, rotation_deg: 0 };

  return `
    <div class="grid-stage canvas-layout">
      <div class="card accent-cyan">
        <div class="card-head">
          <div>
            <h2 class="card-title">Canevas du bureau</h2>
            <div class="card-desc">Glisse et redimensionne chaque appareil pour que les vagues RGB traversent ton installation dans l'ordre réel.</div>
          </div>
          <button type="button" class="btn btn-sm btn-secondary" data-act="canvas-reset" data-dev-id="${esc(selectedId)}">Plein écran</button>
        </div>
        <div class="desk-canvas" id="desk-canvas">
          ${devs.map((d) => {
            const pl = placements[d.info.id] || d.lighting.placement || { x: 0, y: 0, width: 1, height: 1, rotation_deg: 0 };
            const rot = num(pl.rotation_deg, 0);
            const isSel = d.info.id === selectedId;
            const dots = [];
            arr(d.lighting.zones).forEach((z) => {
              const step = Math.max(1, Math.floor(arr(z.leds).length / 40));
              for (let i = 0; i < arr(z.leds).length; i += step) {
                const led = z.leds[i];
                const cx = clamp(pl.x + led.x * pl.width, 0, 1);
                const cy = clamp(pl.y + led.y * pl.height, 0, 1);
                dots.push(`<span class="canvas-led" data-code="${esc(led.key_code || "")}" data-cx="${cx.toFixed(3)}" data-cy="${cy.toFixed(3)}" style="left:${(led.x * 100).toFixed(1)}%;top:${(led.y * 100).toFixed(1)}%"></span>`);
              }
            });
            return `
              <div class="canvas-dev ${isSel ? "selected" : ""}" data-canvas-dev="${esc(d.info.id)}" style="left:${(pl.x * 100).toFixed(1)}%;top:${(pl.y * 100).toFixed(1)}%;width:${(pl.width * 100).toFixed(1)}%;height:${(pl.height * 100).toFixed(1)}%;transform:rotate(${rot}deg)">
                <div class="canvas-dev-title">${esc(d.info.model)}</div>
                <div class="canvas-dev-sub">${esc(d.info.vendor)}</div>
                ${dots.join("")}
                <div class="canvas-handle" data-canvas-resize="${esc(d.info.id)}" title="Redimensionner"></div>
              </div>`;
          }).join("")}
        </div>
      </div>
      <div class="card">
        <div class="card-head">
          <div>
            <h2 class="card-title">${esc(selDev.info.model)}</h2>
            <div class="card-desc">Position normalisée sur le bureau (0 à 100 %).</div>
          </div>
        </div>
        <div class="stack gap-10px">
          ${sliderRow("Position X", { min: 0, max: 95, step: 1, value: Math.round(selPl.x * 100), input: "label", change: "canvas-coord", data: { devId: selectedId, field: "x", fmt: "pct100" }, display: `${Math.round(selPl.x * 100)} %` })}
          ${sliderRow("Position Y", { min: 0, max: 95, step: 1, value: Math.round(selPl.y * 100), input: "label", change: "canvas-coord", data: { devId: selectedId, field: "y", fmt: "pct100" }, display: `${Math.round(selPl.y * 100)} %` })}
          ${sliderRow("Largeur", { min: 5, max: 100, step: 1, value: Math.round(selPl.width * 100), input: "label", change: "canvas-coord", data: { devId: selectedId, field: "width", fmt: "pct100" }, display: `${Math.round(selPl.width * 100)} %` })}
          ${sliderRow("Hauteur", { min: 5, max: 100, step: 1, value: Math.round(selPl.height * 100), input: "label", change: "canvas-coord", data: { devId: selectedId, field: "height", fmt: "pct100" }, display: `${Math.round(selPl.height * 100)} %` })}
          ${sliderRow("Rotation", { min: -180, max: 180, step: 5, value: Math.round(num(selPl.rotation_deg, 0)), input: "label", change: "canvas-coord", data: { devId: selectedId, field: "rotation_deg", fmt: "rot" }, display: `${Math.round(num(selPl.rotation_deg, 0))} °` })}
        </div>
      </div>
    </div>`;
}
