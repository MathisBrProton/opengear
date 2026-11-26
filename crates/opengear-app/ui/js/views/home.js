import { ICONS, PRESET_ICONS, arr, esc, plural } from "../utils.js";
import { S, devicesList, local } from "../state.js";
import { batteryBadge, deviceVisual, emptyState, pageHeader, quickBtn, telemetryTiles } from "../components.js";

export function alertBanner(alerts) {
  const errs = alerts.filter((a) => a.status === "error");
  const sorted = [...errs, ...alerts.filter((a) => a.status !== "error")];
  const cls = errs.length ? "err" : "warn";
  const warns = alerts.length - errs.length;
  const title = errs.length
    ? `${plural(errs.length, "problème détecté", "problèmes détectés")}${warns ? ` et ${plural(warns, "avertissement", "avertissements")}` : ""}`
    : plural(alerts.length, "point à vérifier", "points à vérifier");
  return `
    <div class="alert ${cls}" role="${errs.length ? "alert" : "status"}">
      <span class="alert-icon">${ICONS.warn}</span>
      <div class="grow">
        <div class="alert-title">${esc(title)}</div>
        <ul class="alert-list">${sorted.slice(0, 3).map((a) => `<li><strong>${esc(a.label)}</strong> — ${esc(a.detail)}</li>`).join("")}</ul>
      </div>
      <button type="button" class="btn btn-sm btn-secondary" data-act="nav" data-tab="system" data-sub="diag">Voir le diagnostic</button>
    </div>`;
}

export function presetCard(p, opts = {}) {
  return `
    <button type="button" class="preset-card" data-act="${opts.act || "apply-preset"}" data-preset="${esc(p.id)}" ${opts.pressed != null ? `aria-pressed="${opts.pressed}"` : ""}>
      <span class="preset-icon">${PRESET_ICONS[p.id] || ICONS.spark}</span>
      <span class="preset-label">${esc(p.label)}</span>
      <span class="preset-desc">${esc(p.description)}</span>
    </button>`;
}

export function deviceCard(d) {
  return `
    <article class="device-card" aria-label="${esc(d.name)}">
      <div class="dc-top">
        <span class="pill ${d.status.cls}"><span class="dot"></span>${esc(d.status.label)}</span>
        ${d.battery != null ? batteryBadge(d.battery, d.charging) : ""}
      </div>
      <button type="button" class="dc-thumb" data-act="open-device" data-key="${esc(d.key)}" data-kind="${esc(d.kind)}" tabindex="-1" aria-hidden="true">${deviceVisual(d)}</button>
      <div class="dc-name" title="${esc(d.name)}">${esc(d.name)}</div>
      <div class="dc-meta">${esc(d.type)}</div>
      ${d.quick.length ? `<div class="dc-quick">${d.quick.slice(0, 3).map(quickBtn).join("")}</div>` : ""}
      <button type="button" class="btn btn-sm btn-secondary dc-more" data-act="open-device" data-key="${esc(d.key)}">Plus de réglages ${ICONS.chev}</button>
    </article>`;
}

export function viewHome() {
  const si = S.static_info;
  const devs = devicesList();
  const checks = arr(local.diag.data && local.diag.data.checks);
  const alerts = checks.filter((c) => c.status === "error" || c.status === "warn");
  return `
    ${pageHeader("Accueil", esc([si.motherboard, si.cpu_name].filter(Boolean).join(" · ")),
      `<span class="pill ${devs.length ? "emerald" : ""}"><span class="dot"></span>${devs.length ? plural(devs.length, "appareil connecté", "appareils connectés") : "Aucun appareil"}</span>`)}
    ${alerts.length ? alertBanner(alerts) : ""}
    ${S.presets.length ? `
      <section class="section" aria-labelledby="h-presets">
        <h2 class="section-title" id="h-presets">Préréglages en un clic</h2>
        <div class="preset-strip">${S.presets.map((p) => presetCard(p)).join("")}</div>
      </section>` : ""}
    <section class="section" aria-labelledby="h-devs">
      <div class="section-head">
        <h2 class="section-title" id="h-devs">Mes appareils</h2>
        <button type="button" class="btn btn-sm btn-ghost" data-act="rescan">${ICONS.refresh}Rechercher des appareils</button>
      </div>
      ${devs.length
        ? `<div class="device-grid">${devs.map(deviceCard).join("")}</div>`
        : emptyState("Aucun appareil détecté", "Branche ton clavier, ta souris, ton casque ou le récepteur sans fil : OpenGear les détecte automatiquement. Si un appareil sans fil est en veille, bouge-le puis relance la recherche.", `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`)}
    </section>
    <section class="section" aria-labelledby="h-tel">
      <h2 class="section-title" id="h-tel">Système en direct</h2>
      ${telemetryTiles()}
    </section>`;
}
