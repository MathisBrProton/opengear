import { ICONS, LABEL_FMT, MACRO_PRESETS, arr, clamp, clone, devId, domId, downloadText, esc, inputVal, num, plural, safeHex, throttled, updateLabel } from "./utils.js";
import { call, doUndo, humanError, rpc, setBusy, setStatus, toast } from "./ipc.js";
import { modalStack, trapFocus } from "./modals.js";
import { S, applyState, curveDevice, deviceById, devicesList, getDraftFor, getEq, local, registerHooks, ui, updateChrome } from "./state.js";
import { curLang, t, translateChrome, translateDom } from "./i18n.js";
import { seg, updateTelemetryDom } from "./components.js";
import { startRgbPreviewLoop } from "./rgb-preview.js";
import { presetCard, viewHome } from "./views/home.js";
import { viewLighting, buildOverridePayload } from "./views/lighting.js";
import { renderFanCurve, renderSwFanCurve, getSwFanDraft, unifiedCoolingFans, viewPerformance } from "./views/performance.js";
import { startSocdPolling, viewInput } from "./views/input.js";
import { viewProfiles } from "./views/profiles.js";
import { renderEqSvg, viewDevices } from "./views/devices.js";
import { viewSystem } from "./views/system.js";

const MAX_PROFILE_IMPORT_BYTES = 2 * 1024 * 1024;

// ==================== ROUTER ====================
let lastRenderKey = "";
let pointerDownInMain = false;

function focusSignature(main) {
  const a = document.activeElement;
  if (!a || !main.contains(a) || a === main) return null;
  try {
    if (a.id) return "#" + CSS.escape(a.id);
    const parts = Object.keys(a.dataset)
      .filter((k) => k !== "busy")
      .map((k) => `[data-${k.replace(/[A-Z]/g, (m) => "-" + m.toLowerCase())}="${CSS.escape(a.dataset[k])}"]`);
    return parts.length ? a.tagName.toLowerCase() + parts.join("") : null;
  } catch (_) {
    return null;
  }
}

function render() {
  if (!S) return;
  const main = document.getElementById("main-content");
  const sig = focusSignature(main);
  const key = local.tab + "|" + (local.sub[local.tab] || "") + "|" + (local.tab === "devices" ? local.deviceKey : "");
  const scroll = main.scrollTop;
  let html = "";
  try {
    switch (local.tab) {
      case "home": html = viewHome(); break;
      case "lighting": html = viewLighting(); break;
      case "performance": html = viewPerformance(); break;
      case "input": html = viewInput(); break;
      case "profiles": html = viewProfiles(); break;
      case "devices": html = viewDevices(); break;
      case "system": html = viewSystem(); break;
      default: local.tab = "home"; html = viewHome();
    }
  } catch (e) {
    console.error(e);
    html = `<div class="alert err"><span class="alert-icon">${ICONS.warn}</span><div><div class="alert-title">Cette page n'a pas pu s'afficher</div><div class="muted">${esc(e && e.message)}</div></div></div>`;
  }
  main.innerHTML = html;
  translateChrome();
  translateDom(main);
  main.scrollTop = key === lastRenderKey ? scroll : 0;
  lastRenderKey = key;
  local.pendingRender = false;
  afterRender();
  if (sig) {
    try {
      const el = main.querySelector(sig);
      if (el) el.focus({ preventScroll: true });
    } catch (_) {
      /* ignore */
    }
  }
}

function afterRender() {
  local.lastPreviewSig = "";
  renderFanCurve();
  renderSwFanCurve();
  document.querySelectorAll("[data-eq-svg]").forEach((svg) => renderEqSvg(svg.dataset.eqSvg));
  updateTelemetryDom(S.telemetry);
}

function go(newTab, opts = {}) {
  local.tab = newTab;
  if (opts.sub && local.sub[newTab] !== undefined) local.sub[newTab] = opts.sub;
  if (opts.device) local.deviceKey = opts.device;
  updateChrome();
  render();
  if (newTab === "system" && local.sub.system === "diag" && !local.diag.loading) loadDiagnostics();
  if (newTab === "system" && local.sub.system === "backups" && !local.backups.loading && !local.backups.data) {
    loadBackups();
  }
  const main = document.getElementById("main-content");
  if (main && !opts.keepFocus) main.focus({ preventScroll: true });
}

function softRender() {
  if (isEditing() || pointerDownInMain) local.pendingRender = true;
  else render();
}

registerHooks({ onRender: render, onToast: toast });

// ==================== FIRST-RUN WIZARD ====================
let wiz = null;

function openWizard() {
  if (wiz || !S) return;
  const back = document.createElement("div");
  back.className = "modal-backdrop";
  back.id = "wizard";
  wiz = { step: 0, preset: null, mode: ui().onboarding_done ? ui().mode : "simple", el: back, prevFocus: document.activeElement, busy: false };
  back.addEventListener("click", onWizardClick);
  document.addEventListener("keydown", onWizardKey, true);
  document.getElementById("modal-root").appendChild(back);
  modalStack.push(back);
  renderWizard();
}

function closeWizard() {
  if (!wiz) return;
  document.removeEventListener("keydown", onWizardKey, true);
  const i = modalStack.indexOf(wiz.el);
  if (i >= 0) modalStack.splice(i, 1);
  wiz.el.remove();
  const pf = wiz.prevFocus;
  wiz = null;
  if (pf && pf.isConnected && typeof pf.focus === "function") pf.focus();
}

function onWizardKey(e) {
  if (!wiz || modalStack[modalStack.length - 1] !== wiz.el) return;
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    skipWizard();
  } else if (e.key === "Tab") {
    trapFocus(wiz.el.querySelector(".modal"), e);
  }
}

function renderWizard() {
  if (!wiz) return;
  const devs = S ? arr(S.device_list).length : 0;
  const presets = S.presets;
  const step = wiz.step;
  const chosen = presets.find((p) => p.id === wiz.preset);
  let body = "";
  if (step === 0) {
    const devRows = devicesList();
    const n = devRows.length;
    body = `
      <h2 class="wizard-title" id="wiz-title">Bienvenue dans OpenGear</h2>
      <p class="wizard-lead">${n ? `OpenGear a trouvé ${plural(n, "appareil", "appareils")} sur ce PC :` : "Aucun appareil compatible n'est branché pour l'instant."}</p>
      ${n ? `<div class="wiz-dev-list mb-16">${devRows.map((d) => `
        <div class="wiz-dev">
          <span class="glyph">${ICONS[d.icon] || ICONS.chip}</span>
          <div class="min-w-0"><div class="dev-item-name">${esc(d.name)}</div><div class="dev-item-sub">${esc(d.vendor)} · ${esc(d.type)}</div></div>
        </div>`).join("")}</div>` : ""}
      <div class="note">Tu pourras configurer l'éclairage, la ventilation et les macros hors ligne sans compte ni télémétrie.</div>`;
  } else if (step === 1) {
    body = `
      <h2 class="wizard-title" id="wiz-title">Que veux-tu faire ?</h2>
      <p class="wizard-lead">Choisis une ambiance de départ. Tu pourras tout ajuster ensuite, ou annuler ce choix.</p>
      <div class="preset-strip" role="radiogroup" aria-labelledby="wiz-title">
        ${presets.map((p) => presetCard(p, { act: "wiz-preset", pressed: wiz.preset === p.id })).join("")}
        <button type="button" class="preset-card" data-act="wiz-preset" data-preset="" aria-pressed="${!wiz.preset}">
          <span class="preset-icon">${ICONS.check}</span><span class="preset-label">Garder mes réglages</span><span class="preset-desc">Ne rien changer pour l'instant.</span>
        </button>
      </div>`;
  } else {
    body = `
      <h2 class="wizard-title" id="wiz-title">C'est prêt</h2>
      <p class="wizard-lead">Voici ce qui va être appliqué :</p>
      <div class="recap">
        <div class="recap-row"><span class="muted">Appareils détectés</span><strong>${devs}</strong></div>
        <div class="recap-row"><span class="muted">Préréglage</span><strong>${esc(chosen ? chosen.label : "Réglages actuels conservés")}</strong></div>
        <div class="recap-row"><span class="muted">Affichage</span>${seg([["simple", "Simple (recommandé)"], ["expert", "Expert"]], wiz.mode, "wiz-mode")}</div>
      </div>
      <div class="muted small mt-12">Le mode Simple masque les réglages risqués (écriture en mémoire des appareils, ventilation manuelle…). Tu peux changer à tout moment en bas de la barre latérale.</div>`;
  }
  wiz.el.innerHTML = `
    <div class="modal wizard" role="dialog" aria-modal="true" aria-labelledby="wiz-title">
      <div class="wizard-head">
        <span class="card-eyebrow">Étape ${step + 1} sur 3</span>
        <div class="wizard-steps" aria-hidden="true">${[0, 1, 2].map((s) => `<span class="wizard-step-dot ${s <= step ? "done" : ""}"></span>`).join("")}</div>
      </div>
      <div class="wizard-body">${body}</div>
      <div class="wizard-foot">
        <button type="button" class="btn btn-ghost" data-act="wiz-skip">Passer l'assistant</button>
        <div class="row">
          ${step > 0 ? `<button type="button" class="btn btn-secondary" data-act="wiz-back">Retour</button>` : ""}
          ${step < 2 ? `<button type="button" class="btn btn-primary" data-act="wiz-next" id="wiz-primary">Suivant</button>` : `<button type="button" class="btn btn-primary" data-act="wiz-finish" id="wiz-primary">Terminer</button>`}
        </div>
      </div>
    </div>`;
  const p = wiz.el.querySelector("#wiz-primary");
  translateDom(wiz.el);
  setTimeout(() => p && p.focus(), 0);
}

async function onWizardClick(e) {
  const el = e.target.closest("[data-act]");
  if (!el || !wiz || wiz.busy) return;
  e.stopPropagation();
  const a = el.dataset.act;
  if (a === "wiz-next") { wiz.step = Math.min(2, wiz.step + 1); renderWizard(); }
  else if (a === "wiz-back") { wiz.step = Math.max(0, wiz.step - 1); renderWizard(); }
  else if (a === "wiz-skip") skipWizard();
  else if (a === "wiz-preset") {
    wiz.preset = el.dataset.preset || null;
    wiz.el.querySelectorAll("[data-act='wiz-preset']").forEach((b) => b.setAttribute("aria-pressed", String((b.dataset.preset || null) === wiz.preset)));
  } else if (a === "wiz-mode") {
    wiz.mode = el.dataset.val;
    renderWizard();
  } else if (a === "wiz-finish") {
    wiz.busy = true;
    setBusy(el, true);
    const preset = wiz.preset;
    const mode = wiz.mode;
    if (preset) await call("apply_preset", { preset }, { silent: true });
    const ok = await call("set_ui_prefs", { onboarding_done: true, ui_mode: mode }, { silent: true, quietError: !S.ui });
    if (!ok && !S.ui) local.uiModeOverride = mode;
    closeWizard();
    local.tab = "home";
    updateChrome();
    render();
    toast(preset ? "Tout est prêt : préréglage appliqué. Tu peux l'annuler depuis la barre d'état." : "Tout est prêt !", "ok");
  }
}

function skipWizard() {
  closeWizard();
  call("set_ui_prefs", { onboarding_done: true }, { silent: true, quietError: true });
}

// ==================== MODE & ASYNC LOADS ====================
async function setMode(mode, btn) {
  if ((mode !== "simple" && mode !== "expert") || mode === ui().mode) return;
  const prev = ui().mode;
  if (S.ui) S.ui.mode = mode;
  else local.uiModeOverride = mode;
  updateChrome();
  render();
  try {
    setBusy(btn, true);
    const data = await rpc("set_ui_prefs", { ui_mode: mode });
    if (data && data.config) applyState(data);
    if (S.ui) S.ui.mode = S.ui.mode || mode;
  } catch (e) {
    const msg = String(e && e.message);
    if (/Commande IPC inconnue/i.test(msg)) {
      local.uiModeOverride = mode;
    } else {
      if (S.ui) S.ui.mode = prev;
      else local.uiModeOverride = prev;
      toast(humanError(msg), "err");
      updateChrome();
      render();
      return;
    }
  } finally {
    setBusy(btn, false);
  }
  setStatus(mode === "expert" ? "Mode Expert : tous les réglages sont visibles." : "Mode Simple : réglages avancés masqués.");
}

async function loadDiagnostics() {
  local.diag.loading = true;
  if (local.tab === "system" && local.sub.system === "diag") softRender();
  try {
    local.diag.data = await rpc("get_diagnostics", {});
    local.diag.error = null;
  } catch (e) {
    local.diag.error = humanError(e && e.message);
  } finally {
    local.diag.loading = false;
    updateChrome();
    if (local.tab === "home" || (local.tab === "system" && local.sub.system === "diag")) softRender();
  }
}

async function loadBackups() {
  local.backups.loading = true;
  if (local.tab === "system") softRender();
  try {
    local.backups.data = await rpc("device_list_backups", {});
    local.backups.error = null;
  } catch (e) {
    local.backups.error = humanError(e && e.message);
  } finally {
    local.backups.loading = false;
    if (local.tab === "system") softRender();
  }
}

// ==================== EVENTS ====================
let lastClickTs = 0;

const ACT = {
  nav: (el) => go(el.dataset.tab, { sub: el.dataset.sub }),
  subtab: (el) => go(local.tab, { sub: el.dataset.id, keepFocus: true }),
  "set-mode": (el) => setMode(el.dataset.mode, el),
  "set-mode-seg": (el) => setMode(el.dataset.val, el),
  "set-lang-seg": (el) => call("set_ui_prefs", { language: el.dataset.val }, { btn: el, ok: el.dataset.val === "en" ? "Language: English" : "Langue : Français" }),
  undo: (el) => doUndo(el),
  rescan: (el) => call("reconnect_all", {}, { btn: el, ok: "Recherche terminée : liste des appareils actualisée." }).then(() => loadDiagnostics()),
  "open-device": (el) => go("devices", { device: el.dataset.key }),
  "select-device": (el) => { local.deviceKey = el.dataset.key; render(); },
  "apply-preset": (el) => call("apply_preset", { preset: el.dataset.preset }, { btn: el }),
  "wizard-open": () => openWizard(),

  // Lighting
  "rgb-enable": (el) => {
    const on = el.dataset.on === "1";
    const id = el.dataset.id;
    if (!id) {
      call("lighting_sync_all", { enabled: on }, { btn: el, ok: on ? "Éclairage allumé" : "Éclairage coupé" });
      return;
    }
    delete local.lightingDrafts[`${id}|`];
    call("lighting_set_device", {
      device_id: id,
      enabled: true,
      override: { mode: on ? "global" : "off" },
    }, { btn: el, ok: on ? "Éclairage allumé" : "Éclairage coupé" });
  },
  "rgb-kind": (el) => call("set_rgb_config", { kind: el.dataset.kind, enable_rgb: true }, { btn: el, ok: `Effet : ${el.textContent.trim()}` }),
  "rgb-swatch": (el) => call("set_rgb_config", { primary: el.dataset.color, commit: true }, { btn: el, ok: "Couleur appliquée" }),
  "paint-toggle": () => { local.paintMode = !local.paintMode; render(); },
  "clear-per-key": (el) => call("clear_per_key", {}, { btn: el, ok: "Peinture par touche effacée" }),
  "kb-view": (el) => { local.kbViewMode = el.dataset.val; render(); },
  "lgt-mode": (el) => {
    const devId = el.dataset.devId;
    const zoneId = el.dataset.zoneId || null;
    const k = `${devId}|${zoneId || ""}`;
    local.lightingDrafts[k] = Object.assign(local.lightingDrafts[k] || {}, { mode: el.dataset.val });
    const ov = buildOverridePayload(devId, zoneId);
    call("lighting_set_device", {
      device_id: devId,
      ...(zoneId ? { zone_id: zoneId } : {}),
      override: ov,
    }, { btn: el, ok: "Éclairage mis à jour" });
  },
  "lgt-sw-kind": (el) => {
    const devId = el.dataset.devId;
    const zoneId = el.dataset.zoneId || null;
    const k = `${devId}|${zoneId || ""}`;
    local.lightingDrafts[k] = Object.assign(local.lightingDrafts[k] || {}, { swKind: el.dataset.kind });
    const ov = buildOverridePayload(devId, zoneId);
    call("lighting_set_device", {
      device_id: devId,
      ...(zoneId ? { zone_id: zoneId } : {}),
      override: ov,
    }, { btn: el, ok: `Effet : ${el.textContent.trim()}` });
  },
  "lgt-hw-pick": (el) => {
    const devId = el.dataset.devId;
    const zoneId = el.dataset.zoneId || null;
    const k = `${devId}|${zoneId || ""}`;
    local.lightingDrafts[k] = Object.assign(local.lightingDrafts[k] || {}, { hwEffect: el.dataset.effect });
    const ov = buildOverridePayload(devId, zoneId);
    call("lighting_set_device", {
      device_id: devId,
      ...(zoneId ? { zone_id: zoneId } : {}),
      override: ov,
    }, { btn: el, ok: `Effet : ${el.textContent.trim()}` });
  },
  "lgt-apply": (el) => {
    const devId = el.dataset.devId;
    const zoneId = el.dataset.zoneId || null;
    const ov = buildOverridePayload(devId, zoneId);
    const saveFlash = el.dataset.save === "1";
    if (ov && ov.mode === "hardware" && ov.request) {
      ov.request.save = saveFlash;
    }
    call("lighting_set_device", {
      device_id: devId,
      ...(zoneId ? { zone_id: zoneId } : {}),
      override: ov,
    }, { btn: el, ok: saveFlash ? "Effet enregistré dans l'appareil" : "Éclairage mis à jour" });
  },
  "canvas-reset": (el) => call("lighting_set_placement", { device_id: el.dataset.devId, x: 0, y: 0, width: 1, height: 1, rotation_deg: 0 }, { btn: el, ok: "Placement réinitialisé" }),
  "rgb-layer-add": (el) => {
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    cur.push({
      name: `Calque ${cur.length + 1}`,
      enabled: true,
      blend: "Add",
      opacity: 0.85,
      key_filter: [],
      effect: { kind: "ReactiveTyping", primary: "#FF3D63", secondary: "#00D4FF", speed: 1.2, brightness: 1.0, reverse: false },
    });
    call("set_rgb_layers", { layers: cur }, { btn: el, ok: "Calque RGB ajouté" });
  },
  "rgb-layer-del": (el) => {
    const idx = Number(el.dataset.idx);
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    cur.splice(idx, 1);
    call("set_rgb_layers", { layers: cur }, { btn: el, ok: "Calque RGB supprimé" });
  },
  // Shared device model: choice buttons (`data-val`) and quick actions (`data-json`).
  "dev-set": (el) => setDeviceSetting(el, el.dataset.json !== undefined ? JSON.parse(el.dataset.json) : el.dataset.val, el),
  "kb-key": (el) => {
    local.selectedKeyId = el.dataset.keyId;
    if (local.tab === "lighting" && local.paintMode && el.dataset.code) {
      call("set_per_key_color", { key_code: el.dataset.code, color: safeHex(local.paintColor) }, { silent: true });
    } else {
      render();
    }
  },

  // Pointer DPI & Stages
  "ptr-stage": (el) => call("device_set_active_dpi_stage", { device_id: devId(el), stage: Number(el.dataset.stage) }, { btn: el, ok: `Palier DPI ${Number(el.dataset.stage) + 1} activé` }),
  "ptr-stage-save": (el) => {
    const id = devId(el);
    const v = Number(inputVal(`pstage-${domId(id)}-${el.dataset.stage}`));
    if (!Number.isFinite(v) || v < 100 || v > 30000) { toast("Le DPI doit être compris entre 100 et 30 000.", "err"); return; }
    call("device_set_dpi_stage", { device_id: id, stage: Number(el.dataset.stage), dpi: v }, { btn: el, ok: `Palier ${Number(el.dataset.stage) + 1} : ${v} DPI` });
  },
  "ptr-dpi": (el) => call("device_set_dpi", { device_id: devId(el), x: Number(el.dataset.dpi), y: Number(el.dataset.dpi) }, { btn: el, ok: `Sensibilité : ${el.dataset.dpi} DPI` }),
  "ptr-dpi-apply": (el) => {
    const id = devId(el);
    const did = domId(id);
    const x = Number(inputVal(`pdpi-${did}`));
    const yv = inputVal(`pdpiy-${did}`);
    call("device_set_dpi", { device_id: id, x, y: yv != null ? Number(yv) : x }, { btn: el, ok: `Sensibilité : ${x} DPI` });
  },
  "ptr-lod": (el) => {
    const id = devId(el);
    const dev = deviceById(id);
    if (!dev || !dev.pointer) return;
    call("device_set_dpi", { device_id: id, x: dev.pointer.dpi_x, y: dev.pointer.dpi_y, lod: el.dataset.val }, { btn: el, ok: `Distance de décrochage : ${el.textContent.trim()}` });
  },

  // Audio Equalizer
  "eq-preset": (el) => {
    const id = devId(el);
    const dev = deviceById(id);
    const pr = dev && dev.equalizer && arr(dev.equalizer.presets).find((p) => p.id === el.dataset.preset);
    if (!pr) return;
    local.eqDrafts[id] = { gains: arr(pr.gains_db).map((x) => num(x)), dirty: false };
    call("device_set_eq", { device_id: id, preset: pr.id }, { btn: el, ok: `Égaliseur : ${pr.name}` });
  },
  "eq-apply": (el) => {
    const id = devId(el);
    const e = local.eqDrafts[id];
    if (!e) return;
    const g = e.gains.slice();
    call("device_set_eq", { device_id: id, gains: g }, { btn: el, ok: "Égaliseur appliqué" }).then((r) => { if (r && local.eqDrafts[id]) local.eqDrafts[id].dirty = false; });
  },
  "eq-reset": (el) => {
    const id = devId(el);
    local.eqDrafts[id] = { gains: [0, 0, 0, 0, 0], dirty: false };
    call("device_set_eq", { device_id: id, gains: [0, 0, 0, 0, 0] }, { btn: el, ok: "Égaliseur remis à plat" });
  },

  // Onboard Memory & Backups
  "mbtn-sel": (el) => { local.btnSel[devId(el)] = Number(el.dataset.btn); render(); },
  "mouse-variant": (el) => { local.selectedMouseVariant = Number(el.dataset.idx); render(); },
  "headset-variant": (el) => { local.selectedHeadsetVariant = Number(el.dataset.idx); render(); },
  "ob-prof-sel": (el) => {
    const id = devId(el);
    if (local.drafts[id] && local.drafts[id].dirty) toast("Brouillon non écrit abandonné pour ce profil.", "info");
    delete local.drafts[id];
    local.profSel[id] = Number(el.dataset.val);
    render();
  },
  "ob-activate": (el) => call("device_onboard_activate", { device_id: devId(el), slot: Number(el.dataset.slot) }, { btn: el, ok: `Profil ${el.dataset.slot} activé sur l'appareil` }),
  "ob-activate-seg": (el) => call("device_onboard_activate", { device_id: devId(el), slot: Number(el.dataset.val) }, { btn: el, ok: `Profil ${el.dataset.val} activé sur l'appareil` }),
  "ob-test-stage": (el) => call("device_set_active_dpi_stage", { device_id: devId(el), stage: Number(el.dataset.stage) }, { btn: el, ok: `Palier ${Number(el.dataset.stage) + 1} activé pour essai` }),
  "draft-hz": (el) => {
    const d = getDraftFor(devId(el));
    if (!d) return;
    d.profile.report_rate_hz = Number(el.dataset.val);
    d.dirty = true;
    render();
  },
  "ob-reload": (el) => {
    const id = devId(el);
    delete local.drafts[id];
    call("device_onboard_reload", { device_id: id }, { btn: el, ok: "Profils relus depuis l'appareil" });
  },
  "ob-flash": async (el) => {
    const id = devId(el);
    const d = getDraftFor(id);
    if (!d) return;
    const p = d.profile;
    const r = await call("device_onboard_flash_profile", {
      device_id: id, index: d.idx, report_rate_hz: p.report_rate_hz, default_dpi_idx: p.default_dpi_idx,
      dpi_stages: p.dpi_stages, buttons: p.buttons.map((b) => b.hex),
    }, { btn: el, ok: `Profil ${p.slot} écrit et vérifié.` });
    if (r) { delete local.drafts[id]; local.backups.data = null; render(); }
  },
  "ob-flash-macro": async (el) => {
    const id = devId(el);
    const d = getDraftFor(id);
    const preset = MACRO_PRESETS[inputVal(`macro-preset-${domId(id)}`)] || MACRO_PRESETS.double_click;
    const b = local.btnSel[id] || 0;
    const r = await call("device_onboard_flash_macro", { device_id: id, profile_index: d ? d.idx : 0, button_index: b, steps: preset.steps }, { btn: el, ok: `Macro « ${preset.label} » écrite sur M${b + 1}.` });
    if (r) { delete local.drafts[id]; local.backups.data = null; render(); }
  },

  "relaunch-admin": (el) => call("relaunch_admin", {}, { btn: el, ok: "Relance en administrateur…" }),
  // The backend writes the pump channel, wherever it sits: show that one, not channel 0.
  "sl3-preset": (el) => {
    const cd = curveDevice();
    if (!cd) return;
    const idx = arr(cd.curves && cd.curves.channels).findIndex((c) => c.is_pump);
    if (idx >= 0) local.selectedCoolingIdx = idx;
    call("device_pump_preset", { device: cd.info.id, preset: el.dataset.preset }, { btn: el, ok: `Pompe : ${el.textContent.trim()}` });
  },

  // Cooling
  "cool-ch": (el) => { local.selectedCoolingIdx = Number(el.dataset.val); render(); },
  "cool-pol": (el) => {
    const cd = curveDevice();
    const ch = cd && cd.curves.channels[local.selectedCoolingIdx];
    if (ch) ch.policy = el.dataset.val;
    render();
  },
  "cool-read-bios": (el) => {
    const cd = curveDevice();
    const id = el.dataset.device || (cd && cd.info.id);
    if (!id) return;
    call("device_curve_read", { device: id }, { btn: el, ok: "Réglages relus depuis le BIOS" });
  },
  "cool-apply": (el) => {
    const cd = curveDevice();
    const id = el.dataset.device || (cd && cd.info.id);
    const ch = cd && cd.curves.channels[local.selectedCoolingIdx];
    if (!id || !ch) return;
    call("device_curve_set", { device: id, index: local.selectedCoolingIdx, policy: ch.policy, source: ch.source || "CpuBios", hysteresis_c: num(ch.hysteresis_c, 3), smoothing_secs: num(ch.smoothing_secs, 0), curve: ch.curve, apply: true }, { btn: el, ok: `${ch.short_label || ch.label} : réglage appliqué à la carte mère` });
  },
  "sio-refresh": (el) => call("device_fan_refresh", { device: el.dataset.device }, { btn: el, ok: "Vitesses actualisées" }),
  "sio-apply": (el) => {
    const pct = Number(inputVal(el.dataset.target) ?? 50);
    call("device_fan_set", { device: el.dataset.device, fan: el.dataset.fan, target: pct }, { btn: el, ok: `Ventilateurs forcés à ${pct} %` });
  },
  "sio-auto": (el) => call("device_fan_set", { device: el.dataset.device, fan: el.dataset.fan, target: "auto" }, { btn: el, ok: "Ventilateurs rendus au BIOS" }),
  "sw-fan-ch": (el) => { local.selectedFanKey = el.dataset.val; render(); },
  "sw-fan-sub": (el) => { local.selectedSwCurveIdx = Number(el.dataset.val) || 0; render(); },
  "sw-fan-mode": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (!fan) return;
    const draft = getSwFanDraft(fan);
    draft.mode = el.dataset.val;
    if (draft.mode === "auto") {
      call("device_fan_rule_set", { device: fan.deviceId, fan: fan.fan.id, mode: "auto" }, { btn: el, ok: "Ventilateur rendu au BIOS" }).then((r) => { if (r) delete local.fanRuleDrafts[fan.key]; });
    } else {
      render();
    }
  },
  "sw-fan-mix-op": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (!fan) return;
    getSwFanDraft(fan).mix_op = el.dataset.val;
    render();
  },
  "sw-fan-apply": async (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (!fan) return;
    const d = getSwFanDraft(fan);
    const r = await call("device_fan_rule_set", {
      device: fan.deviceId,
      fan: fan.fan.id,
      mode: d.mode,
      fixed_pct: d.fixed_pct,
      source: d.source,
      custom_mix: d.custom_mix,
      curve: d.curve,
      hysteresis_c: d.hysteresis_c,
      smoothing_secs: d.smoothing_secs,
      ramp_pct_per_sec: d.ramp_pct_per_sec,
      mix_source: d.mix_source,
      mix_custom: d.mix_custom,
      mix_curve: d.mix_curve,
      mix_op: d.mix_op,
    }, { btn: el, ok: `${fan.fan.name} : règle appliquée` });
    if (r) delete local.fanRuleDrafts[fan.key];
  },
  "sw-fan-auto": async (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (!fan) return;
    const r = await call("device_fan_rule_set", { device: fan.deviceId, fan: fan.fan.id, mode: "auto" }, { btn: el, ok: `${fan.fan.name} rendu au BIOS` });
    if (r) delete local.fanRuleDrafts[fan.key];
  },
  "sw-fan-calibrate": (el) => call("device_fan_calibrate", { device: el.dataset.device, fan: el.dataset.fan }, { btn: el, ok: "Calibration lancée…" }),
  "sw-fan-cal-cancel": (el) => call("device_fan_calibrate", { cancel: true }, { btn: el, ok: "Calibration annulée" }),
  "pawnio-install": (el) => call("pawnio_install", {}, { btn: el }),
  "power-plan": (el) => call("set_power_plan", { guid: el.dataset.guid }, { btn: el, ok: "Mode d'alimentation activé" }),

  // Shared Cooling capability (device fans)
  "dev-fan-apply": (el) => {
    const pct = Number(inputVal(el.dataset.target) ?? 65);
    call("device_fan_set", { device: el.dataset.device, target: pct }, { btn: el, ok: `Ventilateurs forcés à ${pct} %` });
  },
  "dev-fan-auto": (el) => call("device_fan_set", { device: el.dataset.device, target: "auto" }, { btn: el, ok: "Ventilateurs rendus au BIOS" }),

  // Input / Macros
  "input-engine-on": (el) => call("input_set_enabled", { enabled: true }, { btn: el, ok: "Moteur d'entrée démarré" }),
  "input-layer": (el) => { local.activeLayer = el.dataset.val || "base"; render(); },
  "bind-remap": (el) => {
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_binding" : "input_set_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), key: local.selectedKeyId, kind: "remap", target: inputVal("mac-remap") }, { btn: el, ok: `${local.selectedKeyId} → ${inputVal("mac-remap")}` });
  },
  "bind-disable": (el) => {
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_binding" : "input_set_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), key: local.selectedKeyId, kind: "disable" }, { btn: el, ok: `${local.selectedKeyId} désactivée` });
  },
  "bind-reset": (el) => {
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_remove_binding" : "input_remove_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), key: local.selectedKeyId }, { btn: el, ok: `${local.selectedKeyId} rétablie` });
  },
  "bind-layer-hold": (el) => call("input_set_binding", { key: local.selectedKeyId, kind: "layer_hold", layer: el.dataset.layer || "fn" }, { btn: el, ok: `${local.selectedKeyId} → Maintenir Fn` }),
  "bind-layer-toggle": (el) => call("input_set_binding", { key: local.selectedKeyId, kind: "layer_toggle", layer: el.dataset.layer || "fn" }, { btn: el, ok: `${local.selectedKeyId} → Basculer Fn` }),
  "bind-taphold": (el) => {
    const tap = inputVal("mac-th-tap");
    const hold = inputVal("mac-th-hold");
    const tap_ms = Math.min(1000, Math.max(50, Number(inputVal("mac-th-ms")) || 200));
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_binding" : "input_set_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), key: local.selectedKeyId, kind: "tap_hold", tap, hold, tap_ms }, { btn: el, ok: `${local.selectedKeyId} : ${tap} / ${hold}` });
  },
  "bind-text": (el) => {
    const t = inputVal("mac-text") || "";
    if (!t) { toast("Saisis d'abord le texte à taper.", "err"); return; }
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_binding" : "input_set_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), key: local.selectedKeyId, kind: "text", text: t, delay_ms: 15 }, { btn: el, ok: `Texte assigné à ${local.selectedKeyId}` });
  },
  "rec-start": (el) => call("input_macro_record_start", {}, { btn: el, ok: "Enregistrement démarré : tape ta combinaison." }),
  "rec-stop": async (el) => {
    setBusy(el, true);
    try {
      const resp = await rpc("input_macro_record_stop", {});
      if (resp && arr(resp.recorded_steps).length) {
        local.macroSteps = resp.recorded_steps;
        toast(`${plural(local.macroSteps.length, "étape capturée", "étapes capturées")}.`, "ok");
      } else toast("Aucune touche n'a été capturée.", "err");
      if (resp && resp.state && resp.state.config) applyState(resp.state, false);
      render();
    } catch (e) {
      toast(humanError(e && e.message), "err");
    } finally {
      setBusy(el, false);
    }
  },
  "macro-add": () => {
    const k = inputVal("mac-step-key");
    const ms = Math.max(0, Number(inputVal("mac-step-ms")) || 15);
    if (!k) return;
    local.macroSteps.push({ key: k, down: true, delay_ms: ms }, { key: k, down: false, delay_ms: ms });
    render();
  },
  "macro-clear": () => { local.macroSteps = []; render(); },
  "bind-macro": (el) => {
    if (!local.macroSteps.length) { toast("La macro est vide : ajoute ou enregistre des étapes.", "err"); return; }
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_binding" : "input_set_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), key: local.selectedKeyId, kind: "macro", name: inputVal("mac-name") || "Macro", mode: inputVal("mac-mode") || "once", steps: local.macroSteps }, { btn: el, ok: `Macro assignée à ${local.selectedKeyId}` });
  },
  "bind-remove": (el) => {
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_remove_binding" : "input_remove_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), key: el.dataset.key }, { btn: el, ok: `Règle ${el.dataset.key} supprimée` });
  },
  "m-bind-remap": (el) => {
    const btn = inputVal("m-src-btn");
    const target = inputVal("m-remap-target");
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_mouse_binding" : "input_set_mouse_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), button: btn, kind: "remap", target }, { btn: el, ok: `${btn} → ${target}` });
  },
  "m-bind-disable": (el) => {
    const btn = inputVal("m-src-btn");
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_mouse_binding" : "input_set_mouse_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), button: btn, kind: "disable" }, { btn: el, ok: `${btn} désactivé` });
  },
  "m-bind-layer-hold": (el) => {
    const btn = inputVal("m-src-btn");
    call("input_set_mouse_binding", { button: btn, kind: "layer_hold", layer: el.dataset.layer || "fn" }, { btn: el, ok: `${btn} → Maintenir Fn` });
  },
  "m-bind-text": (el) => {
    const btn = inputVal("m-src-btn");
    const t = inputVal("m-bind-text") || "";
    if (!t) { toast("Saisis d'abord le texte à taper.", "err"); return; }
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_mouse_binding" : "input_set_mouse_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), button: btn, kind: "text", text: t, delay_ms: 15 }, { btn: el, ok: `Texte assigné à ${btn}` });
  },
  "m-bind-macro": (el) => {
    if (!local.macroSteps.length) { toast("La macro est vide : ajoute ou enregistre des étapes dans l'onglet Touches & macros.", "err"); return; }
    const btn = inputVal("m-src-btn");
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_set_mouse_binding" : "input_set_mouse_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), button: btn, kind: "macro", name: inputVal("mac-name") || "Macro souris", mode: inputVal("m-macro-mode") || "once", steps: local.macroSteps }, { btn: el, ok: `Macro assignée à ${btn}` });
  },
  "m-bind-remove": (el) => {
    const layer = local.activeLayer || "base";
    const cmd = layer === "base" ? "input_remove_mouse_binding" : "input_remove_mouse_layer_binding";
    call(cmd, { ...(layer !== "base" ? { layer } : {}), button: el.dataset.button }, { btn: el, ok: `Règle ${el.dataset.button} supprimée` });
  },
  "socd-blocked": (el) => {
    const list = String(inputVal("socd-blocked") || "").split(",").map((s) => s.trim()).filter(Boolean);
    call("input_set_esports", { blocked_apps: list }, { btn: el, ok: `Jeux protégés : ${list.join(", ") || "aucun"}` });
  },
  "click-test": () => {
    const res = document.getElementById("es-click-res");
    const now = performance.now();
    if (res) {
      if (lastClickTs > 0 && now - lastClickTs < 2000) {
        const dt = Math.round(now - lastClickTs);
        res.textContent = `${dt} ms${dt < 45 ? " · très rapide" : ""}`;
        res.className = `pill mono ${dt < 45 ? "rog" : "emerald"}`;
      } else {
        res.textContent = "1er clic";
        res.className = "pill mono";
      }
    }
    lastClickTs = now;
  },

  // Profiles
  "save-config": (el) => call("save_config", {}, { btn: el, ok: "Configuration enregistrée" }),
  "switch-profile": (el) => call("switch_profile", { name: el.dataset.name }, { btn: el, ok: `Profil « ${el.dataset.name} » activé` }),
  "create-profile": (el) => {
    const name = String(inputVal("new-profile") || "").trim();
    if (!name) { toast("Donne un nom au nouveau profil.", "err"); return; }
    if (S.config.profiles.includes(name)) { toast("Un profil porte déjà ce nom.", "err"); return; }
    call("create_profile", { name }, { btn: el, ok: `Profil « ${name} » créé` });
  },
  "export-profile": async (el) => {
    setBusy(el, true);
    try {
      const res = await rpc("profile_export_json", {});
      downloadText(`opengear-profil-${String(res.name || "profil").toLowerCase().replace(/[^a-z0-9_-]/g, "_")}.json`, res.json || "{}");
      toast(`Profil « ${res.name} » exporté`, "ok");
    } catch (e) {
      toast(humanError(e && e.message), "err");
    } finally {
      setBusy(el, false);
    }
  },
  "import-profile": (el) => {
    const inp = document.createElement("input");
    inp.type = "file";
    inp.accept = ".json,application/json";
    inp.onchange = async () => {
      const f = inp.files && inp.files[0];
      if (!f) return;
      if (f.size > MAX_PROFILE_IMPORT_BYTES) {
        toast("Fichier trop volumineux : limite de 2 Mo pour un profil OpenGear.", "err");
        return;
      }
      let text;
      try {
        text = await f.text();
      } catch (_) {
        toast("Impossible de lire ce fichier.", "err");
        return;
      }
      call("profile_import_json", { json: text }, { btn: el, ok: `Profil importé depuis ${f.name}` });
    };
    inp.click();
  },
  "linked-save": (el) => {
    const apps = String(inputVal("linked-apps") || "").split(",").map((s) => s.trim()).filter(Boolean);
    call("profile_set_linked_apps", { apps }, { btn: el, ok: `${plural(apps.length, "programme lié", "programmes liés")} au profil` });
  },
  "link-app": (el) => {
    const cur = arr(S.config.profile.linked_apps).slice();
    if (!cur.includes(el.dataset.app)) cur.push(el.dataset.app);
    call("profile_set_linked_apps", { apps: cur }, { btn: el, ok: `${el.dataset.app} lié au profil` });
  },

  // Audio
  "wa-mute": (el) => {
    const mic = el.dataset.mic === "1";
    const au = S.telemetry.audio || {};
    call("audio_set_mute", { is_mic: mic, muted: !(mic ? au.mic_muted : au.output_muted) }, { btn: el });
  },

  // System
  shortcuts: (el) => call("sys_create_shortcuts", {}, { btn: el }),
  "diag-refresh": () => loadDiagnostics(),
  "open-test-window": (el) => call("open_test_window", {}, { btn: el, rerender: false }),
  "diag-report": async (el) => {
    el.disabled = true;
    try {
      const res = await rpc("get_diagnostic_report", {});
      const text = (res && res.text) || "";
      let copied = false;
      try {
        await navigator.clipboard.writeText(text);
        copied = true;
      } catch (_) {
        const ta = document.createElement("textarea");
        ta.value = text;
        ta.setAttribute("readonly", "");
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.select();
        copied = document.execCommand("copy");
        ta.remove();
      }
      toast(copied ? "Rapport de diagnostic copié : colle-le dans ton signalement." : "Copie impossible : le presse-papiers est indisponible.", copied ? "info" : "err");
    } catch (e) {
      toast(String((e && e.message) || e), "err");
    } finally {
      el.disabled = false;
    }
  },
  "diag-export": async (el) => {
    setBusy(el, true);
    try {
      const res = await rpc("export_anonymous_report", {});
      downloadText("opengear-anonymous-diagnostic.txt", (res && res.text) || "");
      toast(curLang() === "en" ? "Anonymous report downloaded." : "Rapport anonyme téléchargé.", "ok");
    } catch (e) {
      toast(humanError(e && e.message), "err");
    } finally {
      setBusy(el, false);
    }
  },
  "diag-action": async (el) => {
    const c = local.diagSorted[Number(el.dataset.idx)];
    if (!c || !c.action || !c.action.cmd) return;
    const r = await call(c.action.cmd, c.action.payload || {}, { btn: el, ok: `${c.action.label || "Action"} : terminé` });
    if (r) loadDiagnostics();
  },
  selftest: (el) => call("run_selftest", {}, { btn: el, ok: "Auto-test terminé : rapport enregistré." }),
  "scan-hid": (el) => call("scan_hid", { all: el.dataset.all === "1" }, { btn: el, ok: "Bus HID analysé" }),
  "backups-refresh": () => loadBackups(),
  "ob-backup-restore": async (el) => {
    const r = await call("device_restore_backup", { device_id: devId(el), file: el.dataset.file, _label: el.dataset.label }, { btn: el });
    if (r) { Object.keys(local.drafts).forEach((k) => delete local.drafts[k]); loadBackups(); }
  },
};

/** Apply one setting of the shared device model. */
function setDeviceSetting(el, value, btn = null) {
  return call("device_set", { device: el.dataset.device, setting: el.dataset.setting, value }, { btn, ok: "Réglage appliqué" });
}

const CHANGE = {
  "exp-driver": (el) => call("set_experimental_driver", { name: el.dataset.name, enabled: el.checked }, { ok: el.checked ? "Pilote expérimental activé" : "Pilote expérimental désactivé" }).then((r) => { if (r === null) el.checked = !el.checked; }),
  "ptr-dpi-slider": (el) => {
    const id = el.dataset.device || devId(el);
    const did = domId(id);
    const x = Number(inputVal(`pdpi-${did}`));
    const yv = inputVal(`pdpiy-${did}`);
    call("device_set_dpi", { device_id: id, x, y: yv != null ? Number(yv) : x }, { ok: `Sensibilité : ${x} DPI` });
  },
  "dev-set": (el) => setDeviceSetting(el, el.type === "checkbox" ? el.checked : el.type === "range" ? Number(el.value) : el.value),
  "switch-profile": (el) => call("switch_profile", { name: el.value }, { ok: `Profil « ${el.value} » activé` }),
  "rgb-commit": (el) => {
    const field = el.dataset.field;
    const v = field === "primary" || field === "secondary" ? safeHex(el.value) : parseFloat(el.value);
    call("set_rgb_config", { [field]: v, commit: true }, { rerender: false, silent: true });
  },
  "lighting-sync-all": (el) => call("lighting_sync_all", { enabled: el.checked }, { ok: el.checked ? "Tous les appareils sont synchronisés" : "Synchronisation globale coupée" }),
  "lgt-dev-en": (el) => call("lighting_set_device", { device_id: el.dataset.devId, enabled: el.checked }, { ok: el.checked ? "Appareil synchronisé" : "Appareil en mode autonome" }),
  "lgt-resize-zone": (el) => call("lighting_resize_zone", { device_id: el.dataset.devId, zone_id: el.dataset.zoneId, led_count: Number(el.value) }, { ok: `Zone réglée sur ${el.value} LEDs` }),
  "lgt-draft-chk": (el) => {
    const devId = el.dataset.devId;
    const zoneId = el.dataset.zoneId || null;
    const k = `${devId}|${zoneId || ""}`;
    local.lightingDrafts[k] = Object.assign(local.lightingDrafts[k] || {}, { [el.dataset.field]: el.checked });
    call("lighting_set_device", {
      device_id: devId,
      ...(zoneId ? { zone_id: zoneId } : {}),
      override: buildOverridePayload(devId, zoneId),
    }, { silent: true });
  },
  "lgt-commit": (el) => {
    const devId = el.dataset.devId;
    const zoneId = el.dataset.zoneId || null;
    const k = `${devId}|${zoneId || ""}`;
    const v = el.type === "range" ? parseFloat(el.value) : el.value;
    local.lightingDrafts[k] = Object.assign(local.lightingDrafts[k] || {}, { [el.dataset.field]: v });
    call("lighting_set_device", {
      device_id: devId,
      ...(zoneId ? { zone_id: zoneId } : {}),
      override: buildOverridePayload(devId, zoneId),
    }, { silent: true });
  },
  "canvas-coord": (el) => {
    const devId = el.dataset.devId;
    const pl = Object.assign({ x: 0, y: 0, width: 1, height: 1, rotation_deg: 0 }, (S.config.device_placements || {})[devId] || {});
    if (el.dataset.field === "rotation_deg") {
      pl.rotation_deg = clamp(Number(el.value) || 0, -180, 180);
    } else {
      pl[el.dataset.field] = clamp(Number(el.value) / 100, 0, 1);
    }
    call("lighting_set_placement", { device_id: devId, ...pl }, { silent: true });
  },
  "rgb-layer-en": (el) => {
    const idx = Number(el.dataset.idx);
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    if (cur[idx]) { cur[idx].enabled = el.checked; call("set_rgb_layers", { layers: cur }, { silent: true }); }
  },
  "rgb-layer-blend": (el) => {
    const idx = Number(el.dataset.idx);
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    if (cur[idx]) { cur[idx].blend = el.value; call("set_rgb_layers", { layers: cur }, { silent: true }); }
  },
  "rgb-layer-kind": (el) => {
    const idx = Number(el.dataset.idx);
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    if (cur[idx]) { cur[idx].effect.kind = el.value; call("set_rgb_layers", { layers: cur }, { silent: true }); }
  },
  "rgb-layer-color": (el) => {
    const idx = Number(el.dataset.idx);
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    if (cur[idx]) { cur[idx].effect[el.dataset.field] = safeHex(el.value); call("set_rgb_layers", { layers: cur }, { silent: true }); }
  },
  "rgb-layer-keys": (el) => {
    const idx = Number(el.dataset.idx);
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    if (cur[idx]) {
      cur[idx].key_filter = String(el.value || "").split(",").map((s) => s.trim()).filter(Boolean);
      call("set_rgb_layers", { layers: cur }, { silent: true });
    }
  },
  "rgb-layer-opacity": (el) => {
    const idx = Number(el.dataset.idx);
    const cur = clone(arr(S && S.config && S.config.profile && S.config.profile.rgb_layers));
    if (cur[idx]) { cur[idx].opacity = clamp(Number(el.value), 0, 1); call("set_rgb_layers", { layers: cur }, { silent: true }); }
  },
  "rgb-reverse": (el) => call("set_rgb_config", { reverse: el.checked, commit: true }, { silent: true }),
  "cool-src": (el) => {
    const cd = curveDevice();
    const id = el.dataset.device || (cd && cd.info.id);
    const ch = cd && cd.curves.channels[local.selectedCoolingIdx];
    if (!id || !ch) return;
    ch.source = el.value;
    call("device_curve_set", { device: id, index: local.selectedCoolingIdx, source: ch.source, ...(ch.custom_mix ? { custom_mix: ch.custom_mix } : {}), hysteresis_c: num(ch.hysteresis_c, 3), smoothing_secs: num(ch.smoothing_secs, 0), apply: false }, { ok: "Température de référence modifiée" });
  },
  "cool-mix": (el) => {
    const cd = curveDevice();
    const id = el.dataset.device || (cd && cd.info.id);
    const ch = cd && cd.curves.channels[local.selectedCoolingIdx];
    if (!id || !ch) return;
    ch.custom_mix = {
      op: inputVal("cool-mix-op") || "Max",
      a: inputVal("cool-mix-a") || "Cpu",
      b: inputVal("cool-mix-b") || "GpuEdge",
    };
    call("device_curve_set", { device: id, index: local.selectedCoolingIdx, source: "CustomMix", custom_mix: ch.custom_mix, hysteresis_c: num(ch.hysteresis_c, 3), smoothing_secs: num(ch.smoothing_secs, 0), apply: false }, { ok: "Source composée mise à jour" });
  },
  "cool-hyst": (el) => {
    const cd = curveDevice();
    const id = el.dataset.device || (cd && cd.info.id);
    const ch = cd && cd.curves.channels[local.selectedCoolingIdx];
    if (!id || !ch) return;
    ch.hysteresis_c = Number(el.value);
    call("device_curve_set", { device: id, index: local.selectedCoolingIdx, source: ch.source || "CpuBios", hysteresis_c: ch.hysteresis_c, smoothing_secs: num(ch.smoothing_secs, 0), apply: false }, { ok: `Hystérésis : ${ch.hysteresis_c} °C` });
  },
  "cool-smooth": (el) => {
    const cd = curveDevice();
    const id = el.dataset.device || (cd && cd.info.id);
    const ch = cd && cd.curves.channels[local.selectedCoolingIdx];
    if (!id || !ch) return;
    ch.smoothing_secs = Number(el.value);
    call("device_curve_set", { device: id, index: local.selectedCoolingIdx, source: ch.source || "CpuBios", hysteresis_c: num(ch.hysteresis_c, 3), smoothing_secs: ch.smoothing_secs, apply: false }, { ok: ch.smoothing_secs ? `Lissage : ${ch.smoothing_secs} s` : "Lissage désactivé" });
  },
  "curve-pt": (el) => {
    const cd = curveDevice();
    const ch = cd && cd.curves.channels[local.selectedCoolingIdx];
    if (!ch) return;
    const i = Number(el.dataset.pt);
    const pt = Object.assign({}, ch.curve[i]);
    let v = Math.round(Number(el.value));
    if (!Number.isFinite(v)) v = el.dataset.field === "temp_c" ? num(pt.temp_c, 50) : num(pt.duty_pct, 50);
    if (el.dataset.field === "temp_c") {
      const lo = i === 0 ? 20 : num(ch.curve[i - 1].temp_c) + 1;
      const hi = i === ch.curve.length - 1 ? 100 - (ch.curve.length - 1 - i) : num(ch.curve[i + 1].temp_c) - 1;
      pt.temp_c = clamp(v, lo, Math.max(lo, hi));
    } else {
      pt.duty_pct = clamp(v, ch.is_pump ? 45 : 0, 100);
    }
    ch.curve[i] = pt;
    ch.policy = "Manual";
    render();
  },
  "sw-fan-fixed": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (fan) getSwFanDraft(fan).fixed_pct = Number(el.value);
  },
  "sw-fan-src": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (!fan) return;
    const d = getSwFanDraft(fan);
    if (Number(el.dataset.sub) === 1) d.mix_source = el.value;
    else d.source = el.value;
    render();
  },
  "sw-fan-mix": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (!fan) return;
    const d = getSwFanDraft(fan);
    const target = Number(el.dataset.sub) === 1 ? (d.mix_custom = d.mix_custom || { op: "max", a: "cpu", b: "gpu_edge" }) : (d.custom_mix = d.custom_mix || { op: "max", a: "cpu", b: "gpu_edge" });
    target[el.dataset.part] = el.value;
  },
  "sw-fan-hyst": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (fan) getSwFanDraft(fan).hysteresis_c = Number(el.value);
  },
  "sw-fan-smooth": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (fan) getSwFanDraft(fan).smoothing_secs = Number(el.value);
  },
  "sw-fan-ramp": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (fan) getSwFanDraft(fan).ramp_pct_per_sec = Number(el.value);
  },
  "sw-curve-pt": (el) => {
    const fan = unifiedCoolingFans().find((x) => x.key === el.dataset.key);
    if (!fan) return;
    const d = getSwFanDraft(fan);
    const curve = Number(el.dataset.sub) === 1 ? d.mix_curve : d.curve;
    const i = Number(el.dataset.pt);
    const pt = Object.assign({}, curve[i]);
    let v = Math.round(Number(el.value));
    if (!Number.isFinite(v)) v = el.dataset.field === "temp_c" ? num(pt.temp_c, 50) : num(pt.duty_pct, 50);
    if (el.dataset.field === "temp_c") {
      const lo = i === 0 ? 20 : num(curve[i - 1].temp_c) + 1;
      const hi = i === curve.length - 1 ? 100 - (curve.length - 1 - i) : num(curve[i + 1].temp_c) - 1;
      pt.temp_c = clamp(v, lo, Math.max(lo, hi));
    } else {
      pt.duty_pct = clamp(v, num(fan.fan.min_pct, 30), 100);
    }
    curve[i] = pt;
    render();
  },
  "input-engine": (el) => call("input_set_enabled", { enabled: el.checked }, { ok: el.checked ? "Moteur d'entrée démarré" : "Moteur d'entrée arrêté" }),
  "input-winlock": (el) => call("input_set_winlock", { enabled: el.checked }, { ok: el.checked ? "Touche Windows bloquée" : "Touche Windows débloquée" }),
  "kb-form": (el) => call("set_keyboard_layout", { form_factor: el.value }, { ok: "Format du clavier mis à jour" }),
  "kb-std": (el) => call("set_keyboard_layout", { standard: el.value }, { ok: "Norme du clavier mise à jour" }),
  "socd-en": (el) => call("input_set_esports", { socd_enabled: el.checked }, { ok: el.checked ? "Snap Tap activé" : "Snap Tap désactivé" }),
  "socd-preset": (el) => call("input_set_esports", { socd_preset: el.value }, { ok: "Touches Snap Tap mises à jour" }),
  "es-chatter": (el) => call("input_set_esports", { key_chatter_ms: Number(el.value) }, { ok: `Anti-rebond clavier : ${LABEL_FMT.ms(el.value)}` }),
  "es-mdeb": (el) => call("input_set_esports", { mouse_debounce_ms: Number(el.value) }, { ok: `Anti-rebond clic : ${LABEL_FMT.ms(el.value)}` }),
  "draft-def-dpi": (el) => {
    const d = getDraftFor(devId(el));
    if (!d) return;
    d.profile.default_dpi_idx = Number(el.value);
    d.dirty = true;
  },
  "draft-stage-dpi": (el) => {
    const d = getDraftFor(devId(el));
    if (!d) return;
    const v = clamp(Math.round(Number(el.value) / 50) * 50 || 800, 100, 32000);
    el.value = v;
    d.profile.dpi_stages[Number(el.dataset.stage)] = v;
    d.dirty = true;
  },
  "draft-btn": (el) => {
    const id = devId(el);
    const d = getDraftFor(id);
    if (!d) return;
    const b = Number(el.dataset.btn);
    d.profile.buttons[b] = { hex: el.value, label: el.options[el.selectedIndex].textContent };
    d.dirty = true;
    local.btnSel[id] = b;
    render();
  },
  "bk-dev": (el) => { local.backups.deviceId = el.value; render(); },
  "wa-vol": (el) => call("audio_set_volume", { is_mic: el.dataset.mic === "1", percent: Number(el.value) }, { rerender: false, ok: `${el.dataset.mic === "1" ? "Micro" : "Volume"} : ${el.value} %` }),
  "sys-pref": (el) => call("sys_set_prefs", { [el.dataset.key]: el.checked }, { ok: "Préférence enregistrée" }).then((r) => { if (r === null) el.checked = !el.checked; }),
  "sys-pref-num": (el) => call("sys_set_prefs", { [el.dataset.key]: Number(el.value) }, { ok: "Préférence enregistrée" }).then((r) => { if (r === null) render(); }),
  "laptop-auto-bat": (el) => call("laptop_set_auto_battery", { enabled: el.checked }, { ok: el.checked ? "Mode économie sur batterie activé" : "Mode économie sur batterie désactivé" }),
};

const INPUT = {
  label: (el) => updateLabel(el),
  "lgt-draft": (el) => {
    updateLabel(el);
    const k = `${el.dataset.devId}|${el.dataset.zoneId || ""}`;
    const v = el.type === "range" ? parseFloat(el.value) : el.value;
    local.lightingDrafts[k] = Object.assign(local.lightingDrafts[k] || {}, { [el.dataset.field]: v });
  },
  "rgb-live": (el) => {
    updateLabel(el);
    const field = el.dataset.field;
    const v = field === "primary" || field === "secondary" ? safeHex(el.value) : parseFloat(el.value);
    throttled("rgb-" + field, 120, () => call("set_rgb_config", { [field]: v, commit: false }, { rerender: false, silent: true }));
  },
  "paint-color": (el) => { local.paintColor = el.value; },
  "eq-band": (el) => {
    const id = devId(el);
    const dev = deviceById(id);
    if (!dev || !dev.equalizer) return;
    const e = getEq(dev);
    const b = Number(el.dataset.band);
    e.gains[b] = Number(el.value);
    e.dirty = true;
    const badge = document.getElementById(`eq-db-${domId(id)}-${b}`);
    if (badge) badge.textContent = `${e.gains[b] > 0 ? "+" : ""}${e.gains[b]} dB`;
    renderEqSvg(id);
  },
};

document.addEventListener("pointerdown", (ev) => {
  const main = document.getElementById("main-content");
  if (main && main.contains(ev.target)) pointerDownInMain = true;
});
const clearPointerDown = () => {
  if (!pointerDownInMain) return;
  pointerDownInMain = false;
  if (local.pendingRender && !isEditing()) {
    setTimeout(() => {
      if (local.pendingRender && !isEditing() && !pointerDownInMain) render();
    }, 80);
  }
};
window.addEventListener("pointerup", clearPointerDown);
window.addEventListener("pointercancel", clearPointerDown);

document.addEventListener("click", (ev) => {
  const el = ev.target.closest("[data-act]");
  if (!el || el.closest("#wizard")) return;
  if (el.closest(".modal-backdrop")) return;
  if (el.disabled || el.getAttribute("aria-busy") === "true") return;
  const fn = ACT[el.dataset.act];
  if (fn) {
    ev.preventDefault();
    if (!S && el.dataset.act !== "nav") return;
    fn(el, ev);
  }
});

document.addEventListener("change", (ev) => {
  const el = ev.target.closest("[data-change]");
  if (!el || !S) return;
  const fn = CHANGE[el.dataset.change];
  if (fn) fn(el, ev);
});

document.addEventListener("input", (ev) => {
  const el = ev.target.closest("[data-input]");
  if (!el || !S) return;
  const fn = INPUT[el.dataset.input];
  if (fn) fn(el, ev);
});

document.addEventListener("keydown", (ev) => {
  if (ev.key !== "Enter" || !ev.target || ev.target.tagName !== "INPUT") return;
  const map = { "new-profile": "create-profile", "linked-apps": "linked-save", "socd-blocked": "socd-blocked", "mac-text": "bind-text" };
  const act = map[ev.target.id];
  if (act) {
    const b = document.querySelector(`[data-act="${act}"]`);
    if (b) { ev.preventDefault(); b.click(); }
  }
});

function isEditing() {
  if (local.curveDragging) return true;
  const a = document.activeElement;
  const main = document.getElementById("main-content");
  if (!a || !main || !main.contains(a)) return false;
  if (a.tagName === "TEXTAREA" || a.tagName === "SELECT") return true;
  return a.tagName === "INPUT" && ["text", "number", "search", ""].includes(a.type || "");
}

document.addEventListener("focusout", () => {
  setTimeout(() => {
    if (local.pendingRender && !isEditing() && !pointerDownInMain) render();
  }, 80);
});

// Desk canvas drag & resize
let canvasDrag = null;
document.addEventListener("pointerdown", (ev) => {
  const resizeGrip = ev.target.closest("[data-canvas-resize]");
  const devBox = ev.target.closest("[data-canvas-dev]");
  const canvas = document.getElementById("desk-canvas");
  if (!canvas || (!resizeGrip && !devBox)) return;
  const devId = resizeGrip ? resizeGrip.dataset.canvasResize : devBox.dataset.canvasDev;
  local.canvasSelectedId = devId;
  const box = canvas.querySelector(`[data-canvas-dev="${CSS.escape(devId)}"]`);
  const pl = Object.assign({ x: 0, y: 0, width: 1, height: 1 }, (S && S.config && S.config.device_placements && S.config.device_placements[devId]) || {});
  const rect = canvas.getBoundingClientRect();
  canvasDrag = { devId, box, rect, mode: resizeGrip ? "resize" : "move", startX: ev.clientX, startY: ev.clientY, init: { ...pl }, cur: { ...pl }, moved: false };
});
document.addEventListener("pointermove", (ev) => {
  if (!canvasDrag) return;
  const dx = (ev.clientX - canvasDrag.startX) / Math.max(1, canvasDrag.rect.width);
  const dy = (ev.clientY - canvasDrag.startY) / Math.max(1, canvasDrag.rect.height);
  if (Math.abs(dx) > 0.004 || Math.abs(dy) > 0.004) canvasDrag.moved = true;
  const cur = canvasDrag.cur;
  if (canvasDrag.mode === "move") {
    cur.x = clamp(canvasDrag.init.x + dx, 0, 1 - cur.width);
    cur.y = clamp(canvasDrag.init.y + dy, 0, 1 - cur.height);
  } else {
    cur.width = clamp(canvasDrag.init.width + dx, 0.08, 1 - cur.x);
    cur.height = clamp(canvasDrag.init.height + dy, 0.08, 1 - cur.y);
  }
  canvasDrag.box.style.left = `${(cur.x * 100).toFixed(1)}%`;
  canvasDrag.box.style.top = `${(cur.y * 100).toFixed(1)}%`;
  canvasDrag.box.style.width = `${(cur.width * 100).toFixed(1)}%`;
  canvasDrag.box.style.height = `${(cur.height * 100).toFixed(1)}%`;
});
document.addEventListener("pointerup", () => {
  if (!canvasDrag) return;
  const drag = canvasDrag;
  canvasDrag = null;
  if (drag.moved) {
    call("lighting_set_placement", { device_id: drag.devId, ...drag.cur }, { silent: true });
  } else {
    render();
  }
});

// ==================== BACKEND PUSHES ====================
const deviceSignature = () => {
  if (!S) return "";
  return arr(S.device_list).map((d) => d.info.id).join("|");
};

window.__ogTelemetry = (tel) => {
  if (!S || !tel) return;
  S.telemetry = tel;
  updateTelemetryDom(tel);
};

window.__ogHotplug = (st) => {
  if (!st || !st.config) return;
  const before = S ? deviceSignature() : "";
  applyState(st, false);
  softRender();
  const after = deviceSignature();
  if (before && before !== after) toast("Appareils mis à jour : la liste a été actualisée automatiquement.", "info");
  if (wiz && wiz.step === 0) renderWizard();
};

// ==================== BOOT ====================
startSocdPolling();
startRgbPreviewLoop();

async function bootWithRetry(retriesLeft = 2) {
  const shell = document.querySelector(".app-shell");
  if (shell) shell.classList.add("is-offline");
  try {
    const st = await rpc("get_state", {}, 25_000);
    applyState(st);
    if (shell) shell.classList.remove("is-offline");
    const n = devicesList().length;
    setStatus(n ? `Prêt : ${plural(n, "appareil actif", "appareils actifs")}.` : "Prêt : aucun appareil détecté pour l'instant.", "ok");
    loadDiagnostics();
    if (!ui().onboarding_done) openWizard();
  } catch (e) {
    if (retriesLeft > 0) {
      setStatus("Reconnexion au moteur OpenGear…");
      setTimeout(() => bootWithRetry(retriesLeft - 1), 1500);
      return;
    }
    const main = document.getElementById("main-content");
    main.innerHTML = `
      <div class="alert err" role="alert">
        <span class="alert-icon">${ICONS.warn}</span>
        <div class="grow">
          <div class="alert-title">OpenGear n'a pas pu démarrer l'interface</div>
          <div class="muted">${esc(humanError(e && e.message))}</div>
        </div>
        <button type="button" class="btn btn-primary btn-sm" id="btn-retry-boot">Réessayer</button>
      </div>`;
    translateDom(main);
    const retryBtn = document.getElementById("btn-retry-boot");
    if (retryBtn) {
      retryBtn.onclick = () => {
        setStatus("Reconnexion au moteur OpenGear…", "ok");
        bootWithRetry(1);
      };
    }
    setStatus("Erreur de démarrage", "err");
  }
}

bootWithRetry();
