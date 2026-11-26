import { ICONS, STATUS_LABEL, TAG_LABEL, arr, esc, fmtDate, fmtSize, num, plural } from "../utils.js";
import { S, isExpert, local, ui } from "../state.js";
import { advancedHint, emptyState, expertTag, pageHeader, seg, subtabs, sw } from "../components.js";
import { infoTable } from "./devices.js";

export function viewSystem() {
  const ex = isExpert();
  const checks = arr(local.diag.data && local.diag.data.checks);
  const errs = checks.filter((c) => c.status === "error").length;
  const items = [{ id: "prefs", label: "Préférences" }, { id: "diag", label: "Diagnostic", badge: errs ? String(errs) : "" }];
  if (ex) items.push({ id: "backups", label: "Sauvegardes" });
  items.push({ id: "about", label: "À propos" });
  if (local.sub.system === "backups" && !ex) local.sub.system = "prefs";
  const cur = local.sub.system;
  let body;
  if (cur === "diag") body = sysDiag();
  else if (cur === "backups") body = sysBackups();
  else if (cur === "about") body = sysAbout();
  else body = sysPrefs();
  return `${pageHeader("Système", "Préférences, diagnostic, sauvegardes et informations.", "", "purple")}${subtabs("system", items, cur)}${body}`;
}

function sysPrefs() {
  const sp = S.system_prefs;
  const mode = ui().mode;
  const lang = ui().language;
  const ex = isExpert();
  return `
    <div class="grid-2">
      <div class="card">
        <div class="card-head"><h2 class="card-title">Interface</h2></div>
        <div class="row-between">
          <div><div style="font-weight:600">Mode d'affichage</div><div class="muted">Simple masque les réglages avancés ou risqués. Expert affiche tout.</div></div>
          ${seg([["simple", "Simple"], ["expert", "Expert"]], mode, "set-mode-seg")}
        </div>
        <div class="divider"></div>
        <div class="row-between">
          <div><div style="font-weight:600">Langue de l'interface</div><div class="muted">Français ou English en direct.</div></div>
          ${seg([["fr", "Français"], ["en", "English"]], lang, "set-lang-seg")}
        </div>
        <div class="divider"></div>
        <div class="row-between"><span class="muted">Clavier affiché : disposition Windows <strong>${esc((S.keyboard.os_layout || {}).top_row || "?")}</strong>, format réglable dans Touches &amp; macros.</span><button type="button" class="btn btn-sm btn-secondary" data-act="nav" data-tab="input">Ouvrir</button></div>
        <div class="divider"></div>
        <div class="row-between"><span class="muted">Revoir la présentation de démarrage.</span><button type="button" class="btn btn-sm btn-secondary" data-act="wizard-open">Relancer l'assistant</button></div>
      </div>
      <div class="card">
        <div class="card-head"><h2 class="card-title">Démarrage et zone de notification</h2></div>
        <div class="stack" style="gap:12px">
          ${sw(sp.minimize_to_tray !== false, "Fermer la fenêtre garde OpenGear actif dans la zone de notification", "sys-pref", { key: "minimize_to_tray" })}
          ${sw(!!sp.autostart, "Démarrer avec Windows", "sys-pref", { key: "autostart" })}
          ${sw(!!sp.start_minimized, "Démarrer réduit dans la zone de notification", "sys-pref", { key: "start_minimized" })}
          ${sw(sp.auto_profile_switch !== false, "Changer de profil automatiquement selon le jeu", "sys-pref", { key: "auto_profile_switch" })}
        </div>
        <div class="divider"></div>
        <div class="row">
          <button type="button" class="btn btn-sm btn-secondary" data-act="shortcuts">Créer les raccourcis Bureau et menu Démarrer</button>
          <button type="button" class="btn btn-sm btn-secondary" data-act="rescan">Reconnecter tous les appareils</button>
        </div>
      </div>
      <div class="card">
        <div class="card-head"><div><h2 class="card-title">Éclairage automatique et alertes</h2><div class="card-desc">OpenGear coupe la lumière quand tu n'es pas là et la rallume exactement comme elle était.</div></div></div>
        <div class="stack" style="gap:12px">
          ${sw(sp.lights_off_on_lock !== false, "Éteindre l'éclairage quand la session Windows est verrouillée", "sys-pref", { key: "lights_off_on_lock" })}
          <div class="row-between">
            <label for="idle-off" style="font-weight:600">Éteindre après une période d'inactivité</label>
            <select id="idle-off" class="og-select auto" data-change="sys-pref-num" data-key="idle_lights_off_min">
              ${[[0, "Jamais"], [5, "5 minutes"], [10, "10 minutes"], [15, "15 minutes"], [30, "30 minutes"], [60, "1 heure"]]
                .map(([v, l]) => `<option value="${v}" ${num(sp.idle_lights_off_min) === v ? "selected" : ""}>${l}</option>`).join("")}
            </select>
          </div>
          ${sw(sp.battery_alerts !== false, "Prévenir quand la batterie d'une souris ou d'un clavier sans fil passe sous 15 % puis 5 %", "sys-pref", { key: "battery_alerts" })}
          ${sp.lights_blackout ? `<div class="muted">Éclairage actuellement coupé (verrouillage ou inactivité).</div>` : ""}
        </div>
      </div>
    </div>
    ${ex ? cardOpenRgbServer(sp) : ""}
    ${cardExperimentalDrivers()}`;
}

function cardOpenRgbServer(sp) {
  return `
    <div class="card expert-card mt-16">
      <div class="card-head">
        <div><h2 class="card-title">Pont serveur OpenRGB SDK ${expertTag()}</h2><div class="card-desc">Expose les appareils lumineux d'OpenGear en local (127.0.0.1:6742) pour SignalRGB, Artemis ou Wallpaper Engine (désactivé par défaut).</div></div>
        ${sp.openrgb_server ? `<span class="pill cyan">${num(sp.openrgb_clients)} client(s) connecté(s)</span>` : ""}
      </div>
      ${sw(!!sp.openrgb_server, "Activer le serveur OpenRGB SDK (localhost uniquement)", "sys-pref", { key: "openrgb_server" })}
      ${sp.openrgb_server_error ? `<div class="muted small mt-8" style="color:var(--rog)">${esc(sp.openrgb_server_error)}</div>` : ""}
    </div>`;
}

/** Drivers written from protocol notes but never tested on real hardware: opt-in only. */
function cardExperimentalDrivers() {
  const list = arr(S.experimental_drivers);
  if (!list.length) return "";
  return `
    <div class="card mt-16">
      <div class="card-head"><div><h2 class="card-title">Pilotes expérimentaux <span class="pill amber">Non testés</span></h2>
        <div class="card-desc">Écrits à partir de fiches de protocole, sans avoir pu être essayés sur un vrai appareil. Ils ne pilotent que l'éclairage, sans rien enregistrer dans l'appareil. Active-les seulement si tu possèdes ce matériel, et signale tout comportement anormal.</div></div></div>
      <div class="stack" style="gap:10px">
        ${list.map((d) => sw(!!d.enabled, esc(d.label), "exp-driver", { name: d.name })).join("")}
      </div>
    </div>`;
}

function sysDiag() {
  const ex = isExpert();
  const checks = arr(local.diag.data && local.diag.data.checks);
  const order = { error: 0, warn: 1, ok: 2 };
  const sorted = [...checks].sort((a, b) => (order[a.status] ?? 3) - (order[b.status] ?? 3));
  const count = (s) => checks.filter((c) => c.status === s).length;
  let list;
  if (local.diag.loading && !checks.length) list = `<div class="loading-inline"><div class="spinner sm"></div>Analyse en cours…</div>`;
  else if (local.diag.error) list = `<div class="alert warn"><span class="alert-icon">${ICONS.warn}</span><div><div class="alert-title">Diagnostic indisponible</div><div class="muted">${esc(local.diag.error)}</div></div></div>`;
  else if (!checks.length) list = `<div class="muted">Aucun résultat pour l'instant.</div>`;
  else list = `<div class="diag-list">${sorted.map((c, idx) => `
    <div class="diag-item ${esc(c.status)}">
      <span class="diag-dot ${esc(c.status)}" title="${esc(STATUS_LABEL[c.status] || c.status)}">${c.status === "ok" ? ICONS.check : c.status === "error" ? ICONS.x : ICONS.warn}</span>
      <div><div class="diag-label">${esc(c.label)} <span class="sr-only">(${esc(STATUS_LABEL[c.status] || c.status)})</span></div><div class="diag-detail">${esc(c.detail)}</div></div>
      <div>${c.action && c.action.cmd ? `<button type="button" class="btn btn-sm ${c.status === "error" ? "btn-primary" : "btn-secondary"}" data-act="diag-action" data-idx="${idx}">${esc(c.action.label || "Corriger")}</button>` : ""}</div>
    </div>`).join("")}</div>`;
  local.diagSorted = sorted;

  const st = S.selftest;
  const fws = S.firmware_inventory;
  const preferredOrder = ["ASUS", "Corsair", "Logitech", "Alienware"];
  const allVendors = [...new Set(S.devices.map((x) => x.vendor || "Autre"))].sort((a, b) => {
    const ia = preferredOrder.indexOf(a);
    const ib = preferredOrder.indexOf(b);
    if (ia >= 0 && ib >= 0) return ia - ib;
    if (ia >= 0) return -1;
    if (ib >= 0) return 1;
    return a.localeCompare(b);
  });

  return `
    <div class="card">
      <div class="card-head">
        <div><h2 class="card-title">État du système</h2><div class="card-desc">Vérifie que chaque composant d'OpenGear fonctionne et propose une correction quand c'est possible.</div></div>
        <div class="row">
          ${checks.length ? `<div class="diag-summary"><span class="pill emerald">${count("ok")} OK</span>${count("warn") ? `<span class="pill amber">${count("warn")} à vérifier</span>` : ""}${count("error") ? `<span class="pill rog">${plural(count("error"), "problème", "problèmes")}</span>` : ""}</div>` : ""}
          <button type="button" class="btn btn-sm btn-secondary" data-act="diag-refresh" ${local.diag.loading ? "disabled" : ""}>${ICONS.refresh}Relancer l'analyse</button>
          <button type="button" class="btn btn-sm btn-secondary" data-act="diag-report" title="Version, appareils, état et fin du journal — ton dossier utilisateur est masqué">Copier le rapport</button>
          <button type="button" class="btn btn-sm btn-secondary" data-act="diag-export">Exporter rapport anonyme (.txt)</button>
        </div>
      </div>
      ${list}
    </div>
    ${fws.length ? `
      <div class="card mt-16">
        <div class="card-head"><div><h2 class="card-title">Versions des firmwares</h2><div class="card-desc">Lecture seule : OpenGear ne met jamais à jour un firmware.</div></div></div>
        <div class="table-scroll"><table class="meta-table"><thead><tr><th>Appareil</th><th>Marque</th><th>Version</th><th>État</th></tr></thead><tbody>
          ${fws.map((f) => `<tr><td>${esc(f.device)}</td><td>${esc(f.vendor)}</td><td class="mono">${esc(f.version)}</td><td>${esc(f.status)}</td></tr>`).join("")}
        </tbody></table></div>
      </div>` : ""}
    ${ex ? `
      <div class="card expert-card mt-16">
        <div class="card-head">
          <div><h2 class="card-title">Auto-test matériel complet ${expertTag()}</h2><div class="card-desc">Interroge chaque contrôleur et écrit un rapport détaillé (%TEMP%\\opengear_selftest.txt). En administrateur, inclut aussi ventilateurs et RAM.</div></div>
          <div class="row">${st ? `<span class="pill ${num(st.failed) ? "rog" : "emerald"}">${num(st.passed)} réussis · ${num(st.warned)} avertissements · ${num(st.failed)} échecs</span>` : ""}<button type="button" class="btn btn-sm btn-primary" data-act="selftest">Lancer l'auto-test</button></div>
        </div>
        ${st ? `<div class="table-scroll"><table class="meta-table"><thead><tr><th>Résultat</th><th>Composant</th><th>Détail</th></tr></thead><tbody>
          ${arr(st.checks).map((c) => `<tr><td><span class="pill ${c.status === "PASS" ? "emerald" : c.status === "FAIL" ? "rog" : "amber"}">${esc(c.status)}</span></td><td style="font-weight:600">${esc(c.name)}</td><td class="mono small selectable">${esc(c.detail)}</td></tr>`).join("")}
        </tbody></table></div><div class="muted small mt-8 mono selectable">Rapport : ${esc(st.report_path)}</div>` : ""}
      </div>
      <div class="card expert-card mt-16">
        <div class="card-head">
          <div><h2 class="card-title">Bus USB / HID ${expertTag()}</h2><div class="card-desc">${plural(S.devices.length, "interface détectée", "interfaces détectées")}.</div></div>
          <div class="row"><button type="button" class="btn btn-sm btn-secondary" data-act="scan-hid" data-all="0">Périphériques de jeu</button><button type="button" class="btn btn-sm btn-secondary" data-act="scan-hid" data-all="1">Tout le bus HID</button></div>
        </div>
        ${allVendors.map((v) => {
          const list2 = S.devices.filter((x) => (x.vendor || "Autre") === v);
          if (!list2.length) return "";
          return `<div class="card-eyebrow mt-12" style="margin-bottom:6px">${esc(v)} · ${list2.length}</div><div class="table-scroll"><table class="meta-table"><thead><tr><th>VID:PID</th><th>IF</th><th>UsagePage</th><th>Usage</th><th>Produit</th><th>Statut</th></tr></thead><tbody>
            ${list2.map((x) => `<tr><td class="mono">${esc(x.vid_pid)}</td><td class="mono">${esc(x.interface)}</td><td class="mono">${esc(x.usage_page)}</td><td class="mono">${esc(x.usage)}</td><td>${esc(x.product)}</td><td style="color:${x.driver_supported ? "var(--emerald)" : "var(--text-2)"}">${esc(x.status)}</td></tr>`).join("")}
          </tbody></table></div>`;
        }).join("")}
      </div>` : advancedHint("L'auto-test complet et la liste brute des interfaces USB/HID sont en mode Expert.")}`;
}

function sysBackups() {
  const mice = S.logi_mice;
  const corsairDevs = S.corsair_devices;
  const logiList = arr(local.backups.data && local.backups.data.backups).slice().sort((a, b) => num(b.created_ms) - num(a.created_ms));
  const corsairList = arr(local.backups.corsairData && local.backups.corsairData.backups).slice().sort((a, b) => num(b.created_ms) - num(a.created_ms));
  if (local.backups.mouse >= mice.length) local.backups.mouse = 0;
  if (local.backups.corsairDev >= corsairDevs.length) local.backups.corsairDev = 0;

  let logiBody;
  if (local.backups.loading && !local.backups.data) logiBody = `<div class="loading-inline"><div class="spinner sm"></div>Lecture des sauvegardes…</div>`;
  else if (local.backups.error) logiBody = `<div class="alert warn"><span class="alert-icon">${ICONS.warn}</span><div><div class="alert-title">Sauvegardes indisponibles</div><div class="muted">${esc(local.backups.error)}</div></div></div>`;
  else if (!logiList.length) logiBody = emptyState("Aucune sauvegarde Logitech", "Une sauvegarde est créée automatiquement avant chaque écriture dans la mémoire d'une souris Logitech.");
  else logiBody = `
    <div class="table-scroll"><table class="meta-table">
      <thead><tr><th>Date</th><th>Type</th><th>Secteur</th><th>Taille</th><th><span class="sr-only">Action</span></th></tr></thead>
      <tbody>${logiList.map((b) => `
        <tr>
          <td>${esc(fmtDate(b.created_ms))}</td>
          <td>${esc(TAG_LABEL[b.tag] || b.tag || "—")}</td>
          <td class="mono">${esc(b.sector_hex)}</td>
          <td>${esc(fmtSize(b.size))}</td>
          <td style="text-align:right"><button type="button" class="btn btn-sm btn-secondary" data-act="backup-restore" data-file="${esc(b.file)}" data-label="${esc(`${TAG_LABEL[b.tag] || b.tag || "Sauvegarde"} · secteur ${b.sector_hex} · ${fmtDate(b.created_ms)}`)}" ${mice.length ? "" : "disabled"}>Restaurer</button></td>
        </tr>`).join("")}</tbody>
    </table></div>`;

  const corsairBody = !corsairList.length
    ? `<div class="muted">Aucune sauvegarde Corsair HWP enregistrée pour l'instant.</div>`
    : `<div class="table-scroll"><table class="meta-table">
        <thead><tr><th>Date</th><th>Fichier HWP</th><th>Type</th><th>Taille</th><th><span class="sr-only">Action</span></th></tr></thead>
        <tbody>${corsairList.map((b) => `
          <tr>
            <td>${esc(fmtDate(b.created_ms))}</td>
            <td class="mono">${esc(b.file_id_hex)}</td>
            <td>${esc(TAG_LABEL[b.tag] || b.tag || "HWP")}</td>
            <td>${esc(fmtSize(b.size))}</td>
            <td style="text-align:right"><button type="button" class="btn btn-sm btn-secondary" data-act="corsair-backup-restore" data-file="${esc(b.file)}" data-label="${esc(`Corsair ${b.file_id_hex} · ${fmtDate(b.created_ms)}`)}" ${corsairDevs.length ? "" : "disabled"}>Restaurer</button></td>
          </tr>`).join("")}</tbody>
      </table></div>`;

  return `
    <div class="stack">
      <div class="card expert-card">
        <div class="card-head">
          <div><h2 class="card-title">Sauvegardes de la mémoire des souris Logitech ${expertTag()}</h2><div class="card-desc">Copies faites automatiquement avant chaque écriture de profil ou de macro.</div></div>
          <div class="row">
            ${mice.length > 1 ? `<label class="field-label" for="bk-mouse" style="min-width:0">Souris cible</label><select id="bk-mouse" class="og-select auto" data-change="bk-mouse">${mice.map((m, i) => `<option value="${i}" ${i === local.backups.mouse ? "selected" : ""}>${esc(m.name)}</option>`).join("")}</select>` : ""}
            <button type="button" class="btn btn-sm btn-secondary" data-act="backups-refresh">${ICONS.refresh}Actualiser</button>
          </div>
        </div>
        ${!mice.length ? `<div class="alert info"><span class="alert-icon">${ICONS.info}</span><div><div class="alert-title">Aucune souris Logitech détectée</div><div class="muted">Branche le récepteur LIGHTSPEED / Bolt ou le câble de la souris pour pouvoir restaurer une sauvegarde.</div></div></div>` : mice.length === 1 ? `<div class="muted mb-16">Restauration vers : <strong>${esc(mice[0].name)}</strong></div>` : ""}
        ${logiBody}
      </div>
      <div class="card expert-card">
        <div class="card-head">
          <div><h2 class="card-title">Sauvegardes flash Corsair HWP ${expertTag()}</h2><div class="card-desc">Copies créées automatiquement avant chaque flash d'éclairage matériel Corsair.</div></div>
          ${corsairDevs.length > 1 ? `<select id="bk-corsair" class="og-select auto" data-change="bk-corsair">${corsairDevs.map((d, i) => `<option value="${i}" ${i === local.backups.corsairDev ? "selected" : ""}>${esc(d.product_name)}</option>`).join("")}</select>` : ""}
        </div>
        ${corsairBody}
      </div>
    </div>`;
}

function sysAbout() {
  const si = S.static_info;
  const la = S.local_assets;
  return `
    <div class="grid-2">
      <div class="card">
        <div class="card-head"><h2 class="card-title">OpenGear</h2></div>
        <p class="muted">Contrôleur matériel unifié, léger et local : remplace iCUE, G HUB et Armoury Crate pour l'éclairage, les souris, claviers, casques et la ventilation.</p>
        <div class="divider"></div>
        ${infoTable([["Version", S.app_version || "0.4.0"], ["Données personnelles", "Aucune télémétrie, rien n'est envoyé sur Internet"], ["Images officielles locales", [la.lghub_depot_found ? "G HUB" : "", la.icue_devices_found ? "iCUE" : ""].filter(Boolean).join(" + ") || "Non (images vectorielles intégrées)"]])}
      </div>
      <div class="card">
        <div class="card-head"><h2 class="card-title">Ce PC</h2></div>
        ${infoTable([["Carte mère", si.motherboard], ["BIOS", si.bios_version], ["Processeur", si.cpu_name], ["Carte graphique", si.gpu_name]])}
      </div>
    </div>`;
}
