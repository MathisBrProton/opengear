import { ICONS, LABEL_FMT, arr, clamp, devIdx, downloadText, esc, inputVal, num, plural, safeHex, throttled, updateLabel } from "./utils.js";
import { call, doUndo, humanError, rpc, setBusy, setStatus, toast } from "./ipc.js";
import { modalStack, trapFocus } from "./modals.js";
import { S, applyState, getDraftFor, local, registerHooks, ui, updateChrome } from "./state.js";
import { seg, updateTelemetryDom } from "./components.js";
import { startRgbPreviewLoop } from "./rgb-preview.js";
import { presetCard, viewHome } from "./views/home.js";
import { viewLighting } from "./views/lighting.js";
import { renderFanCurve, viewPerformance } from "./views/performance.js";
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
  document.querySelectorAll("[data-eq-svg]").forEach((svg) => renderEqSvg(Number(svg.dataset.eqSvg)));
  updateTelemetryDom(S.telemetry);
}

function go(newTab, opts = {}) {
  local.tab = newTab;
  if (opts.sub && local.sub[newTab] !== undefined) local.sub[newTab] = opts.sub;
  if (opts.device) local.deviceKey = opts.device;
  updateChrome();
  render();
  if (newTab === "system" && local.sub.system === "diag" && !local.diag.loading) loadDiagnostics();
  if (newTab === "system" && local.sub.system === "backups" && !local.backups.loading && (!local.backups.data || !local.backups.corsairData)) {
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
  const devs = S ? (S.corsair_devices.length + S.logi_mice.length + S.logi_keyboards.length + S.logi_headsets.length + (S.asus ? 1 : 0)) : 0;
  const presets = S.presets;
  const step = wiz.step;
  const chosen = presets.find((p) => p.id === wiz.preset);
  let body = "";
  if (step === 0) {
    body = `
      <h2 class="wizard-title" id="wiz-title">Bienvenue dans OpenGear</h2>
      <p class="wizard-lead">${devs ? `OpenGear a trouvé ${plural(devs, "appareil", "appareils")} sur ce PC :` : "Aucun appareil compatible n'est branché pour l'instant."}</p>
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
    const [logiRes, corsairRes] = await Promise.all([
      rpc("list_backups", {}),
      rpc("corsair_list_backups", {}).catch(() => ({ backups: [] })),
    ]);
    local.backups.data = logiRes;
    local.backups.corsairData = corsairRes;
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
  undo: (el) => doUndo(el),
  rescan: (el) => call("reconnect_all", {}, { btn: el, ok: "Recherche terminée : liste des appareils actualisée." }).then(() => loadDiagnostics()),
  "open-device": (el) => go("devices", { device: el.dataset.key }),
  "select-device": (el) => { local.deviceKey = el.dataset.key; render(); },
  "apply-preset": (el) => call("apply_preset", { preset: el.dataset.preset }, { btn: el }),
  "wizard-open": () => openWizard(),

  // Lighting
  "rgb-enable": (el) => call("set_rgb_config", { enable_rgb: el.dataset.on === "1" }, { btn: el, ok: el.dataset.on === "1" ? "Éclairage allumé" : "Éclairage coupé" }),
  "rgb-kind": (el) => call("set_rgb_config", { kind: el.dataset.kind, enable_rgb: true }, { btn: el, ok: `Effet : ${el.textContent.trim()}` }),
  "rgb-swatch": (el) => call("set_rgb_config", { primary: el.dataset.color, commit: true }, { btn: el, ok: "Couleur appliquée" }),
  "paint-toggle": () => { local.paintMode = !local.paintMode; render(); },
  "clear-per-key": (el) => call("clear_per_key", {}, { btn: el, ok: "Peinture par touche effacée" }),
  "kb-view": (el) => { local.kbViewMode = el.dataset.val; render(); },
  // Shared device model: choice buttons (`data-val`) and quick actions (`data-json`).
  "dev-set": (el) => setDeviceSetting(el, el.dataset.json !== undefined ? JSON.parse(el.dataset.json) : el.dataset.val, el),
  "kb-key": (el) => {
    local.selectedKeyId = el.dataset.keyId;
    if (local.tab === "lighting" && local.paintMode) {
      call("set_per_key_color", { led_index: Number(el.dataset.led), color: safeHex(local.paintColor) }, { silent: true });
    } else {
      render();
    }
  },

  // Corsair
  "cm-stage": (el) => call("corsair_mouse_set_active_stage", { device_index: devIdx(el), stage: Number(el.dataset.stage) }, { btn: el, ok: `Palier DPI ${Number(el.dataset.stage) + 1} activé` }),
  "cm-dpi-save": (el) => {
    const v = Number(inputVal(`cm-dpi-${devIdx(el)}-${el.dataset.stage}`));
    if (!Number.isFinite(v) || v < 100 || v > 30000) { toast("Le DPI doit être compris entre 100 et 30 000.", "err"); return; }
    call("corsair_mouse_set_dpi_stage", { device_index: devIdx(el), stage: Number(el.dataset.stage), dpi: v }, { btn: el, ok: `Palier ${Number(el.dataset.stage) + 1} : ${v} DPI` });
  },
  "cs-mode": (el) => call("corsair_set_mode", { device_index: devIdx(el), mode: el.dataset.val }, { btn: el, ok: el.dataset.val === "sw" ? "Éclairage piloté par OpenGear" : "Le clavier gère son éclairage seul" }),
  "cs-ind": (el) => call("corsair_set_indicator_color", { device_index: devIdx(el), indicator: inputVal(`cs-ind-${devIdx(el)}`), color: safeHex(inputVal(`cs-ind-col-${devIdx(el)}`)) }, { btn: el, ok: "Couleur du voyant appliquée" }),
  "cs-hwp-reload": (el) => call("corsair_reload_hwp", { device_index: devIdx(el) }, { btn: el, ok: "Mémoire du clavier relue" }),
  "cs-hwp-flash": (el) => {
    const i = devIdx(el);
    const profIdx = Number(el.dataset.prof ?? inputVal(`hwp-prof-${i}`) ?? local.hwpProfSel[i] ?? 0);
    call("corsair_flash_hwp_lighting", {
      device_index: i, profile_index: profIdx, name: inputVal(`hwp-name-${i}`), kind: inputVal(`hwp-kind-${i}`),
      primary: safeHex(inputVal(`hwp-c1-${i}`)), secondary: safeHex(inputVal(`hwp-c2-${i}`)),
      speed: Number(inputVal(`hwp-speed-${i}`)), direction: Number(inputVal(`hwp-dir-${i}`)),
    }, { btn: el, ok: "Effet écrit dans le clavier et vérifié : il reste actif sans OpenGear." }).then((r) => {
      if (r) local.backups.corsairData = null;
    });
  },

  // Logitech
  "logi-dpi": (el) => call("logi_set_dpi", { device_index: devIdx(el), x: Number(el.dataset.dpi), y: Number(el.dataset.dpi) }, { btn: el, ok: `Sensibilité : ${el.dataset.dpi} DPI` }),
  "logi-dpi-apply": (el) => {
    const i = devIdx(el);
    const x = Number(inputVal(`ldpi-${i}`));
    const yv = inputVal(`ldpiy-${i}`);
    call("logi_set_dpi", { device_index: i, x, y: yv != null ? Number(yv) : x }, { btn: el, ok: `Sensibilité : ${x} DPI` });
  },
  "logi-lod": (el) => {
    const m = S.logi_mice[devIdx(el)];
    if (!m || !m.dpi) return;
    call("logi_set_dpi", { device_index: devIdx(el), x: m.dpi.dpi_x, y: m.dpi.dpi_y, lod: el.dataset.val }, { btn: el, ok: `Distance de décrochage : ${el.textContent.trim()}` });
  },
  "lkb-quick": (el) => call("logi_kb_set_rgb", { device_index: devIdx(el), mode: el.dataset.mode, color: safeHex(S.config.profile.rgb.primary) }, { btn: el, ok: "Éclairage du clavier appliqué" }),
  "lkb-apply": (el) => call("logi_kb_set_rgb", { device_index: devIdx(el), mode: inputVal(`lkb-mode-${devIdx(el)}`), color: safeHex(inputVal(`lkb-col-${devIdx(el)}`)) }, { btn: el, ok: "Éclairage du clavier appliqué" }),
  "lml-apply": (el) => call("logi_set_led", { device_index: devIdx(el), mode: inputVal(`lml-mode-${devIdx(el)}`), color: safeHex(inputVal(`lml-col-${devIdx(el)}`)) }, { btn: el, ok: "Éclairage de la souris appliqué" }),
  "eq-preset": (el) => {
    const i = devIdx(el);
    const h = S.logi_headsets[i];
    const pr = h && arr(h.presets)[Number(el.dataset.idx)];
    if (!pr) return;
    local.eqDrafts[i] = { gains: arr(pr.gains).map((x) => num(x)), dirty: false };
    call("logi_set_eq", { device_index: i, gains: local.eqDrafts[i].gains }, { btn: el, ok: `Égaliseur : ${pr.name}` });
  },
  "eq-apply": (el) => {
    const i = devIdx(el);
    const e = local.eqDrafts[i];
    if (!e) return;
    const g = e.gains.slice();
    call("logi_set_eq", { device_index: i, gains: g }, { btn: el, ok: "Égaliseur appliqué" }).then((r) => { if (r && local.eqDrafts[i]) local.eqDrafts[i].dirty = false; });
  },
  "eq-reset": (el) => {
    const i = devIdx(el);
    local.eqDrafts[i] = { gains: [0, 0, 0, 0, 0], dirty: false };
    call("logi_set_eq", { device_index: i, gains: [0, 0, 0, 0, 0] }, { btn: el, ok: "Égaliseur remis à plat" });
  },
  "mbtn-sel": (el) => { local.btnSel[devIdx(el)] = Number(el.dataset.btn); render(); },
  "mouse-variant": (el) => { local.selectedMouseVariant = Number(el.dataset.idx); render(); },
  "headset-variant": (el) => { local.selectedHeadsetVariant = Number(el.dataset.idx); render(); },
  "logi-prof-sel": (el) => {
    const i = devIdx(el);
    if (local.drafts[i] && local.drafts[i].dirty) toast("Brouillon non écrit abandonné pour ce profil.", "info");
    delete local.drafts[i];
    local.profSel[i] = Number(el.dataset.val);
    render();
  },
  "logi-activate": (el) => call("logi_set_active_profile", { device_index: devIdx(el), slot: Number(el.dataset.slot) }, { btn: el, ok: `Profil ${el.dataset.slot} activé sur la souris` }),
  "logi-activate-seg": (el) => call("logi_set_active_profile", { device_index: devIdx(el), slot: Number(el.dataset.val) }, { btn: el, ok: `Profil ${el.dataset.val} activé sur la souris` }),
  "logi-test-stage": (el) => call("logi_set_dpi_stage", { device_index: devIdx(el), stage: Number(el.dataset.stage) }, { btn: el, ok: `Palier ${Number(el.dataset.stage) + 1} activé pour essai` }),
  "draft-hz": (el) => {
    const d = getDraftFor(devIdx(el));
    if (!d) return;
    d.profile.report_rate_hz = Number(el.dataset.val);
    d.dirty = true;
    render();
  },
  "logi-reload": (el) => {
    delete local.drafts[devIdx(el)];
    call("logi_reload_profiles", { device_index: devIdx(el) }, { btn: el, ok: "Profils relus depuis la souris" });
  },
  "logi-flash": async (el) => {
    const i = devIdx(el);
    const d = getDraftFor(i);
    if (!d) return;
    const p = d.profile;
    const r = await call("logi_flash_profile", {
      device_index: i, index: d.idx, report_rate_hz: p.report_rate_hz, default_dpi_idx: p.default_dpi_idx,
      dpi_stages: p.dpi_stages, buttons: p.buttons.map((b) => b.hex),
    }, { btn: el, ok: `Profil ${p.slot_index} écrit dans la souris et vérifié.` });
    if (r) { delete local.drafts[i]; local.backups.data = null; render(); }
  },
  "logi-flash-macro": async (el) => {
    const i = devIdx(el);
    const d = getDraftFor(i);
    const preset = MACRO_PRESETS[inputVal(`macro-preset-${i}`)] || MACRO_PRESETS.double_click;
    const b = local.btnSel[i] || 0;
    const r = await call("logi_flash_macro", { device_index: i, profile_index: d ? d.idx : 0, button_index: b, steps: preset.steps }, { btn: el, ok: `Macro « ${preset.label} » écrite sur M${b + 1}.` });
    if (r) { delete local.drafts[i]; local.backups.data = null; render(); }
  },

  // ASUS / RAM / Watercooling
  "asus-direct": (el) => call("asus_set_direct", { enabled: el.dataset.on === "1" }, { btn: el, ok: el.dataset.on === "1" ? "Carte mère synchronisée avec OpenGear" : "Carte mère en mode autonome" }),
  "aura-quick": (el) => call("asus_set_hw_mode", { mode: el.dataset.mode, color: safeHex(S.asus && S.asus.hw_color, "#FF1848"), commit: false }, { btn: el, ok: `Carte mère : ${el.textContent.trim()}` }).then((r) => { if (r) local.auraDraftMode = el.dataset.mode; }),
  "aura-mode": (el) => {
    local.auraDraftMode = el.dataset.mode;
    call("asus_set_hw_mode", { mode: local.auraDraftMode, color: safeHex(inputVal("asus-color"), "#FF1848"), commit: false }, { btn: el, ok: `Effet carte mère : ${el.textContent.trim()}` });
  },
  "aura-swatch": (el) => {
    const inp = document.getElementById("asus-color");
    if (inp) inp.value = el.dataset.color;
    call("asus_set_hw_mode", { mode: local.auraDraftMode || S.asus.hw_mode, color: el.dataset.color, commit: false }, { btn: el, ok: "Couleur de la carte mère appliquée" });
  },
  "aura-apply": (el) => call("asus_set_hw_mode", { mode: local.auraDraftMode || S.asus.hw_mode, color: safeHex(inputVal("asus-color"), "#FF1848"), commit: false }, { btn: el, ok: "Effet appliqué à la carte mère" }),
  "aura-commit": (el) => call("asus_set_hw_mode", { mode: local.auraDraftMode || S.asus.hw_mode, color: safeHex(inputVal("asus-color"), "#FF1848"), commit: true }, { btn: el, ok: "Effet enregistré dans la carte mère" }),
  "aura-shutdown": (el) => call("asus_set_shutdown_mode", { mode: inputVal("asus-sd-mode"), color: safeHex(inputVal("asus-sd-color"), "#000000") }, { btn: el, ok: "Éclairage PC éteint enregistré" }),
  "ram-mode": (el) => call("ram_apply_rgb", { mode: el.dataset.mode, color: safeHex(inputVal("ram-color"), "#00D2FF"), speed: Number(inputVal("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: `RAM : ${el.textContent.trim()}` }),
  "ram-swatch": (el) => {
    const inp = document.getElementById("ram-color");
    if (inp) inp.value = el.dataset.color;
    call("ram_apply_rgb", { mode: (S.ram_rgb && S.ram_rgb.last_mode) || "Static", color: el.dataset.color, speed: Number(inputVal("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: "Couleur de la RAM appliquée" });
  },
  "ram-apply": (el) => call("ram_apply_rgb", { mode: (S.ram_rgb && S.ram_rgb.last_mode) || "Static", color: safeHex(inputVal("ram-color"), "#00D2FF"), speed: Number(inputVal("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: "Effet appliqué à la RAM" }),
  "ram-eeprom": (el) => call("ram_apply_rgb", { mode: (S.ram_rgb && S.ram_rgb.last_mode) || "Static", color: safeHex(inputVal("ram-color"), "#00D2FF"), speed: Number(inputVal("ram-speed") ?? 2), save_eeprom: true }, { btn: el, ok: "Effet enregistré dans les barrettes" }),
  "ram-sync-aura": (el) => {
    const col = safeHex(inputVal("asus-color") || (S.asus && S.asus.hw_color), "#FF1848");
    call("ram_apply_rgb", { mode: "Static", color: col, speed: Number(inputVal("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: "RAM alignée sur la couleur de la carte mère" });
  },
  "ram-off": (el) => call("ram_apply_rgb", { mode: "Off", color: "#000000", speed: 2, save_eeprom: false }, { btn: el, ok: "Éclairage de la RAM éteint" }),
  "ram-scan": (el) => call("ram_scan", {}, { btn: el, ok: "Recherche des barrettes terminée" }),
  "relaunch-admin": (el) => call("cooling_relaunch_admin", {}, { btn: el, ok: "Relance en administrateur…" }),
  "sl3-preset": (el) => { local.selectedCoolingIdx = 0; call("cooling_sl3_preset", { preset: el.dataset.preset }, { btn: el, ok: `Pompe : ${el.textContent.trim()}` }); },
  "sl3-color": (el) => {
    local.sl3Color = safeHex(inputVal("sl3-color"), "#FF8C1A");
    call("cooling_sl3_argb", { channel: Number(inputVal("sl3-ch")), color: local.sl3Color, thermal: false }, { btn: el, ok: "Couleur du watercooling appliquée" });
  },

  // Cooling
  "cool-ch": (el) => { local.selectedCoolingIdx = Number(el.dataset.val); render(); },
  "cool-pol": (el) => {
    const ch = S.cooling.channels[local.selectedCoolingIdx];
    if (ch) ch.policy = el.dataset.val;
    render();
  },
  "cool-read-bios": (el) => call("cooling_read_bios_curves", {}, { btn: el, ok: "Réglages relus depuis le BIOS" }),
  "cool-apply": (el) => {
    const ch = S.cooling.channels[local.selectedCoolingIdx];
    if (!ch) return;
    call("cooling_update_channel", { index: local.selectedCoolingIdx, policy: ch.policy, source: ch.source || "CpuBios", hysteresis_c: num(ch.hysteresis_c, 3), curve: ch.curve, apply: true }, { btn: el, ok: `${ch.short_label || ch.label} : réglage appliqué à la carte mère` });
  },
  "sio-refresh": (el) => call("superio_refresh", {}, { btn: el, ok: "Vitesses actualisées" }),
  "sio-apply": (el) => call("superio_set_pwm", { channel: Number(el.dataset.ch), duty_pct: Number(inputVal(`sio-${el.dataset.ch}`) ?? 50) }, { btn: el }),
  "sio-auto": (el) => call("superio_restore_auto", { channel: Number(el.dataset.ch) }, { btn: el }),
  "pawnio-install": (el) => call("pawnio_install", {}, { btn: el }),
  "power-plan": (el) => call("set_power_plan", { guid: el.dataset.guid }, { btn: el, ok: "Mode d'alimentation activé" }),

  // HP OMEN & Laptop
  "omen-refresh": (el) => call("omen_refresh", {}, { btn: el, ok: "État HP OMEN actualisé" }),
  "omen-thermal": (el) => call("omen_set_thermal_mode", { mode: el.dataset.mode || el.dataset.val }, { btn: el, ok: `Mode thermique : ${el.textContent.trim()}` }),
  "omen-gpupow": (el) => call("omen_set_gpu_power", { level: el.dataset.val }, { btn: el, ok: `Puissance GPU : ${el.textContent.trim()}` }),
  "omen-fan-apply": (el) => {
    const pct = Number(inputVal("omen-fan-pct") ?? 65);
    call("omen_set_manual_fan", { pct }, { btn: el, ok: `Ventilateurs forcés à ${pct} %` });
  },
  "omen-fan-auto": (el) => call("omen_set_manual_fan", { pct: null }, { btn: el, ok: "Ventilateurs rendus au BIOS" }),
  "omen-swatch": (el) => {
    const c = safeHex(el.dataset.color);
    for (let i = 0; i < 4; i++) {
      const inp = document.getElementById(`omen-z-${i}`);
      if (inp) inp.value = c;
    }
    call("omen_set_kb_zones", { colors: [c, c, c, c] }, { btn: el, ok: "Clavier OMEN mis à jour" });
  },
  "omen-zones-apply": (el) => {
    const colors = [0, 1, 2, 3].map((i) => safeHex(inputVal(`omen-z-${i}`)));
    call("omen_set_kb_zones", { colors }, { btn: el, ok: "Couleurs des 4 zones appliquées" });
  },
  "laptop-hz": (el) => call("laptop_set_refresh_rate", { hz: Number(el.dataset.val) }, { btn: el }),

  // Input / Macros
  "input-engine-on": (el) => call("input_set_enabled", { enabled: true }, { btn: el, ok: "Moteur d'entrée démarré" }),
  "bind-remap": (el) => call("input_set_binding", { key: local.selectedKeyId, kind: "remap", target: inputVal("mac-remap") }, { btn: el, ok: `${local.selectedKeyId} → ${inputVal("mac-remap")}` }),
  "bind-disable": (el) => call("input_set_binding", { key: local.selectedKeyId, kind: "disable" }, { btn: el, ok: `${local.selectedKeyId} désactivée` }),
  "bind-reset": (el) => call("input_remove_binding", { key: local.selectedKeyId }, { btn: el, ok: `${local.selectedKeyId} rétablie` }),
  "bind-text": (el) => {
    const t = inputVal("mac-text") || "";
    if (!t) { toast("Saisis d'abord le texte à taper.", "err"); return; }
    call("input_set_binding", { key: local.selectedKeyId, kind: "text", text: t, delay_ms: 15 }, { btn: el, ok: `Texte assigné à ${local.selectedKeyId}` });
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
    call("input_set_binding", { key: local.selectedKeyId, kind: "macro", name: inputVal("mac-name") || "Macro", steps: local.macroSteps }, { btn: el, ok: `Macro assignée à ${local.selectedKeyId}` });
  },
  "bind-remove": (el) => call("input_remove_binding", { key: el.dataset.key }, { btn: el, ok: `Règle ${el.dataset.key} supprimée` }),
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
  "diag-action": async (el) => {
    const c = local.diagSorted[Number(el.dataset.idx)];
    if (!c || !c.action || !c.action.cmd) return;
    const r = await call(c.action.cmd, c.action.payload || {}, { btn: el, ok: `${c.action.label || "Action"} : terminé` });
    if (r) loadDiagnostics();
  },
  selftest: (el) => call("run_selftest", {}, { btn: el, ok: "Auto-test terminé : rapport enregistré." }),
  "scan-hid": (el) => call("scan_hid", { all: el.dataset.all === "1" }, { btn: el, ok: "Bus HID analysé" }),
  "backups-refresh": () => loadBackups(),
  "backup-restore": async (el) => {
    const r = await call("logi_restore_backup", { device_index: local.backups.mouse, file: el.dataset.file, _label: el.dataset.label }, { btn: el });
    if (r) { Object.keys(local.drafts).forEach((k) => delete local.drafts[k]); loadBackups(); }
  },
  "corsair-backup-restore": async (el) => {
    const r = await call("corsair_restore_backup", { device_index: local.backups.corsairDev, file: el.dataset.file, _label: el.dataset.label }, { btn: el });
    if (r) loadBackups();
  },
};

/** Apply one setting of the shared device model. */
function setDeviceSetting(el, value, btn = null) {
  return call("device_set", { device: el.dataset.device, setting: el.dataset.setting, value }, { btn, ok: "Réglage appliqué" });
}

const CHANGE = {
  "dev-set": (el) => setDeviceSetting(el, el.type === "checkbox" ? el.checked : el.type === "range" ? Number(el.value) : el.value),
  "switch-profile": (el) => call("switch_profile", { name: el.value }, { ok: `Profil « ${el.value} » activé` }),
  "rgb-enable-sw": (el) => call("set_rgb_config", { enable_rgb: el.checked }, { ok: el.checked ? "Éclairage allumé" : "Éclairage coupé" }),
  "rgb-commit": (el) => {
    const field = el.dataset.field;
    const v = field === "primary" || field === "secondary" ? safeHex(el.value) : parseFloat(el.value);
    call("set_rgb_config", { [field]: v, commit: true }, { rerender: false, silent: true });
  },
  "sync-all": (el) => call("set_sync_all", { enabled: el.checked }, { ok: el.checked ? "Toutes les marques sont synchronisées" : "Synchronisation désactivée" }),
  "rgb-reverse": (el) => call("set_rgb_config", { reverse: el.checked, commit: true }, { silent: true }),
  "argb-count": (el) => call("asus_set_led_count", { channel: Number(el.dataset.ch), count: Number(el.value) }, { ok: `Connecteur réglé sur ${el.value} LEDs` }),
  "asus-direct-sw": (el) => call("asus_set_direct", { enabled: el.checked }, { ok: el.checked ? "Carte mère synchronisée avec OpenGear" : "Carte mère en mode autonome" }),
  "sl3-thermal": (el) => call("cooling_sl3_argb", { channel: Number(inputVal("sl3-ch")), thermal: el.checked }, { ok: el.checked ? "Couleur selon la température activée" : "Couleur selon la température désactivée" }),
  "cool-src": (el) => {
    const ch = S.cooling.channels[local.selectedCoolingIdx];
    if (!ch) return;
    ch.source = el.value;
    call("cooling_update_channel", { index: local.selectedCoolingIdx, source: ch.source, hysteresis_c: num(ch.hysteresis_c, 3), apply: false }, { ok: "Température de référence modifiée" });
  },
  "cool-hyst": (el) => {
    const ch = S.cooling.channels[local.selectedCoolingIdx];
    if (!ch) return;
    ch.hysteresis_c = Number(el.value);
    call("cooling_update_channel", { index: local.selectedCoolingIdx, source: ch.source || "CpuBios", hysteresis_c: ch.hysteresis_c, apply: false }, { ok: `Hystérésis : ${ch.hysteresis_c} °C` });
  },
  "curve-pt": (el) => {
    const ch = S.cooling.channels[local.selectedCoolingIdx];
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
      pt.duty_pct = clamp(v, ch.is_pump ? 40 : 0, 100);
    }
    ch.curve[i] = pt;
    ch.policy = "Manual";
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
    const d = getDraftFor(devIdx(el));
    if (!d) return;
    d.profile.default_dpi_idx = Number(el.value);
    d.dirty = true;
  },
  "draft-stage-dpi": (el) => {
    const d = getDraftFor(devIdx(el));
    if (!d) return;
    const v = clamp(Math.round(Number(el.value) / 50) * 50 || 800, 100, 32000);
    el.value = v;
    d.profile.dpi_stages[Number(el.dataset.stage)] = v;
    d.dirty = true;
  },
  "draft-btn": (el) => {
    const i = devIdx(el);
    const d = getDraftFor(i);
    if (!d) return;
    const b = Number(el.dataset.btn);
    d.profile.buttons[b] = { hex: el.value, label: el.options[el.selectedIndex].textContent };
    d.dirty = true;
    local.btnSel[i] = b;
    render();
  },
  "hwp-prof-sel": (el) => {
    local.hwpProfSel[devIdx(el)] = Number(el.value);
    render();
  },
  "wa-vol": (el) => call("audio_set_volume", { is_mic: el.dataset.mic === "1", percent: Number(el.value) }, { rerender: false, ok: `${el.dataset.mic === "1" ? "Micro" : "Volume"} : ${el.value} %` }),
  "sys-pref": (el) => call("sys_set_prefs", { [el.dataset.key]: el.checked }, { ok: "Préférence enregistrée" }),
  "sys-pref-num": (el) => call("sys_set_prefs", { [el.dataset.key]: Number(el.value) }, { ok: "Préférence enregistrée" }),
  "bk-mouse": (el) => { local.backups.mouse = Number(el.value); },
  "bk-corsair": (el) => { local.backups.corsairDev = Number(el.value); },
  "omen-maxfan": (el) => call("omen_set_max_fan", { enabled: el.checked }, { ok: el.checked ? "Max Fan activé" : "Max Fan désactivé" }),
  "omen-kblight": (el) => call("omen_set_kb_backlight", { enabled: el.checked }, { ok: el.checked ? "Rétroéclairage clavier allumé" : "Rétroéclairage clavier éteint" }),
  "omen-sync": (el) => call("omen_set_sync_rgb", { enabled: el.checked }, { ok: el.checked ? "Clavier OMEN synchronisé avec OpenGear" : "Synchronisation OMEN désactivée" }),
  "laptop-bright": (el) => call("laptop_set_brightness", { percent: Number(el.value) }, { rerender: false, ok: `Luminosité écran : ${el.value} %` }),
  "laptop-auto-bat": (el) => call("laptop_set_auto_battery", { enabled: el.checked }, { ok: el.checked ? "Mode économie sur batterie activé" : "Mode économie sur batterie désactivé" }),
};

const INPUT = {
  label: (el) => updateLabel(el),
  "rgb-live": (el) => {
    updateLabel(el);
    const field = el.dataset.field;
    const v = field === "primary" || field === "secondary" ? safeHex(el.value) : parseFloat(el.value);
    throttled("rgb-" + field, 120, () => call("set_rgb_config", { [field]: v, commit: false }, { rerender: false, silent: true }));
  },
  "paint-color": (el) => { local.paintColor = el.value; },
  "eq-band": (el) => {
    const i = devIdx(el);
    const h = S.logi_headsets[i];
    if (!h) return;
    const e = getEq(h, i);
    const b = Number(el.dataset.band);
    e.gains[b] = Number(el.value);
    e.dirty = true;
    const badge = document.getElementById(`eq-db-${i}-${b}`);
    if (badge) badge.textContent = `${e.gains[b] > 0 ? "+" : ""}${e.gains[b]} dB`;
    renderEqSvg(i);
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

// ==================== BACKEND PUSHES ====================
const deviceSignature = () => {
  if (!S) return "";
  return [
    ...S.corsair_devices.map((d, i) => `c:${i}:${d.product_name}`),
    ...S.logi_mice.map((m, i) => `lm:${i}:${m.name}`),
    ...S.logi_keyboards.map((k, i) => `lk:${i}:${k.name}`),
    ...S.logi_headsets.map((h, i) => `lh:${i}:${h.name}`),
    S.asus ? "asus" : "",
    S.hp_omen && S.hp_omen.detected ? "omen" : "",
  ].join("|");
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

rpc("get_state")
  .then((st) => {
    applyState(st);
    const n = S.corsair_devices.length + S.logi_mice.length + S.logi_keyboards.length + S.logi_headsets.length + (S.asus ? 1 : 0) + (S.hp_omen && S.hp_omen.detected ? 1 : 0);
    setStatus(n ? `Prêt : ${plural(n, "appareil actif", "appareils actifs")}.` : "Prêt : aucun appareil détecté pour l'instant.");
    loadDiagnostics();
    if (!ui().onboarding_done) openWizard();
  })
  .catch((e) => {
    document.getElementById("main-content").innerHTML = `
      <div class="alert err" role="alert"><span class="alert-icon">${ICONS.warn}</span>
        <div><div class="alert-title">OpenGear n'a pas pu démarrer l'interface</div><div class="muted">${esc(humanError(e && e.message))}</div></div>
      </div>`;
    setStatus("Erreur de démarrage");
  });
