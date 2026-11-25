import { BTN_NAMES, ICONS, MACRO_PRESETS, arr, esc, fmtHz, num, plural } from "../utils.js";
import { rpc } from "../ipc.js";
import { S, getDraft, isExpert, local, mouseImage } from "../state.js";
import { advancedHint, emptyState, expertTag, pageHeader, renderKeyboard, seg, sliderRow, subtabs, sw } from "../components.js";

const SOCD_PADS = [
  ["KeyA", "Gauche", "Q (AZERTY) / A (QWERTY)"],
  ["KeyD", "Droite", "D"],
  ["KeyW", "Avant", "Z (AZERTY) / W (QWERTY)"],
  ["KeyS", "Arrière", "S"],
];
const PAD_LIVE_KEYS = { KeyA: "left", KeyD: "right", KeyW: "forward", KeyS: "back" };

let socdLive = null;
let socdLiveBusy = false;

export function startSocdPolling() {
  setInterval(async () => {
    if (document.visibilityState !== "visible") return;
    if (local.tab !== "input" || local.sub.input !== "esports") return;
    if (socdLiveBusy || !document.querySelector("[data-socd-pad]")) return;
    socdLiveBusy = true;
    try {
      socdLive = await rpc("input_socd_live", {});
      const note = document.getElementById("socd-live-note");
      if (note && socdLive && socdLive.engine_running === false) {
        note.textContent = "Moteur clavier arrêté : active « Moteur d'entrée actif » en haut de la page.";
      }
      refreshSocdPads();
    } catch (_) {
      socdLive = null;
    } finally {
      socdLiveBusy = false;
    }
  }, 100);
}

function refreshSocdPads() {
  document.querySelectorAll("[data-socd-pad]").forEach((el) => {
    const on = !!(socdLive && socdLive[PAD_LIVE_KEYS[el.dataset.socdPad]]);
    el.classList.toggle("on", on);
    el.setAttribute("aria-pressed", String(on));
  });
}

export function viewInput() {
  const prof = S.config.profile;
  const items = [
    { id: "keys", label: "Touches & macros" },
    { id: "esports", label: "Mode compétition" },
    { id: "mouse", label: "Boutons de souris" },
  ];
  const cur = local.sub.input;
  let body;
  if (cur === "esports") body = inputEsports();
  else if (cur === "mouse") body = inputMouse();
  else body = inputKeys();
  return `
    ${pageHeader("Touches & Macros", `Profil « ${esc(prof.name || S.config.active_profile || "")} »`,
      `${sw(!!S.input_running, "Moteur d'entrée actif", "input-engine")}
       ${sw(!!prof.win_lock, "Bloquer la touche Windows", "input-winlock")}
       ${sw(!!S.config.use_azerty_labels, "Clavier AZERTY", "azerty")}`, "purple")}
    ${!S.input_running && cur !== "mouse" ? `<div class="alert info"><span class="alert-icon">${ICONS.info}</span><div class="grow"><div class="alert-title">Le moteur d'entrée est arrêté</div><div class="muted">Les remappages, macros et le mode compétition ne s'appliquent que lorsque le moteur d'entrée est actif.</div></div><button type="button" class="btn btn-sm btn-primary" data-act="input-engine-on">Démarrer le moteur</button></div>` : ""}
    ${subtabs("input", items, cur)}
    ${body}`;
}

function inputKeys() {
  const prof = S.config.profile;
  const sp = S.system_prefs;
  const bindings = Object.entries(prof.bindings || {});
  const kb = S.corsair_devices.find((d) => d.kind === "Keyboard") || null;
  const keyOpts = (sel) => S.bindable_keys.map((k) => `<option value="${esc(k)}" ${k === sel ? "selected" : ""}>${esc(k)}</option>`).join("");
  return `
    ${renderKeyboard("macro", kb)}
    <div class="grid-2 mt-16">
      <div class="card">
        <div class="card-head"><h2 class="card-title">Touche sélectionnée</h2><span class="pill purple">${esc(local.selectedKeyId)}</span></div>
        <div class="row">
          <label class="field-label" for="mac-remap">Remplacer par</label>
          <select id="mac-remap" class="og-select grow">${keyOpts("F13")}</select>
          <button type="button" class="btn btn-primary btn-sm" data-act="bind-remap">Appliquer</button>
        </div>
        <div class="row mt-12">
          <button type="button" class="btn btn-secondary btn-sm" data-act="bind-disable">Désactiver la touche</button>
          <button type="button" class="btn btn-secondary btn-sm" data-act="bind-reset">Rétablir la touche d'origine</button>
        </div>
        <div class="divider"></div>
        <div class="row">
          <label class="field-label" for="mac-text">Taper un texte</label>
          <input id="mac-text" class="og-input grow" placeholder="Ex. : gg wp" />
          <button type="button" class="btn btn-secondary btn-sm" data-act="bind-text">Assigner</button>
        </div>
        <div class="divider"></div>
        <div class="row-between">
          <span class="field-label">Macro en plusieurs étapes</span>
          ${sp.macro_recording
            ? `<button type="button" class="btn btn-sm btn-danger" data-act="rec-stop">Arrêter l'enregistrement</button>`
            : `<button type="button" class="btn btn-sm btn-primary" data-act="rec-start">Enregistrer en direct</button>`}
        </div>
        <div class="row mt-8">
          <input id="mac-name" class="og-input" style="width:130px" value="Macro 1" aria-label="Nom de la macro" />
          <select id="mac-step-key" class="og-select auto" aria-label="Touche à ajouter">${keyOpts("")}</select>
          <input id="mac-step-ms" type="number" class="og-input" style="width:76px" value="15" min="0" aria-label="Délai en millisecondes" />
          <button type="button" class="btn btn-secondary btn-sm" data-act="macro-add">Ajouter un appui</button>
          <button type="button" class="btn btn-ghost btn-sm" data-act="macro-clear">Vider</button>
        </div>
        <div class="mono muted mt-12 scroll-y selectable" style="max-height:110px;overflow:auto;background:#0b0e16;padding:8px 10px;border-radius:8px" aria-live="polite">
          ${sp.macro_recording
            ? `<span style="color:#ff8aa2;font-weight:700">Enregistrement en cours… tape ta combinaison puis clique sur « Arrêter l'enregistrement ».</span>`
            : local.macroSteps.map((st, i) => `${i + 1}. ${st.down ? "Appui  " : "Relâche"} ${esc(st.key)} (${num(st.delay_ms)} ms)`).join("<br/>") || "Aucune étape pour l'instant."}
        </div>
        <button type="button" class="btn btn-primary btn-sm mt-12" data-act="bind-macro">Assigner la macro à ${esc(local.selectedKeyId)}</button>
      </div>
      <div class="card">
        <div class="card-head"><h2 class="card-title">Touches modifiées</h2><span class="pill">${plural(bindings.length, "règle", "règles")}</span></div>
        ${bindings.length === 0 ? `<div class="muted">Aucune touche modifiée sur ce profil. Clique sur une touche du clavier ci-dessus pour commencer.</div>` : `
          <table class="meta-table">
            <thead><tr><th>Touche</th><th>Action</th><th><span class="sr-only">Supprimer</span></th></tr></thead>
            <tbody>
              ${bindings.map(([k, act]) => {
                let desc = "";
                if (act.type === "Remap") desc = `→ ${act.target}`;
                else if (act.type === "Disable") desc = "Désactivée";
                else if (act.type === "Text") desc = `Texte « ${act.text} »`;
                else if (act.type === "Macro") desc = `Macro « ${act.name} » (${arr(act.steps).length} étapes)`;
                else desc = String(act.type || "Action");
                return `<tr><td class="mono" style="color:var(--cyan);font-weight:700">${esc(k)}</td><td>${esc(desc)}</td><td style="text-align:right"><button type="button" class="btn btn-sm btn-secondary" data-act="bind-remove" data-key="${esc(k)}" aria-label="Supprimer la règle ${esc(k)}">Supprimer</button></td></tr>`;
              }).join("")}
            </tbody>
          </table>`}
      </div>
    </div>`;
}

function flashtapCard() {
  const kbs = arr(S.corsair_devices).filter((d) => d.flashtap_supported);
  if (!kbs.length) {
    const corsairKb = arr(S.corsair_devices).find((d) => d.kind === "Keyboard");
    return corsairKb
      ? `<div class="card mb-16"><div class="muted">FlashTap matériel non détecté sur ${esc(corsairKb.product_name)} : ce firmware n'expose pas de réglage FlashTap reconnu. Le Snap Tap logiciel ci-dessous reste disponible.</div></div>`
      : "";
  }
  return `
    <div class="card accent-purple mb-16">
      <div class="card-head">
        <div>
          <h2 class="card-title">FlashTap matériel <span class="pill amber">Expérimental</span></h2>
          <div class="card-desc">La priorité entre touches opposées est gérée <strong>dans le clavier</strong> : rien n'est injecté dans Windows, donc invisible pour les anti-triche (contrairement au Snap Tap logiciel ci-dessous). Les touches concernées sont celles définies par le firmware du clavier.</div>
        </div>
      </div>
      <div class="stack" style="gap:10px">
        ${kbs.map((d) => `
          <div class="row-between">
            ${sw(!!d.flashtap_enabled, `${esc(d.product_name)} — FlashTap ${d.flashtap_enabled ? "actif" : "inactif"}`, "cs-flashtap", { dev: d.index })}
          </div>`).join("")}
        <div class="muted">Activer le FlashTap coupe automatiquement le Snap Tap logiciel. Vérifie le résultat avec le testeur juste en dessous.</div>
      </div>
    </div>`;
}

function socdTesterCard() {
  return `
    <div class="card mb-16">
      <div class="card-head">
        <div>
          <h2 class="card-title">Testeur de touches opposées</h2>
          <div class="card-desc">Les pastilles montrent ce que <strong>tes jeux et applications reçoivent</strong>, vu par le moteur clavier. Ouvre le Bloc-notes de test, garde OpenGear visible à côté, puis tape <strong>dans le Bloc-notes</strong> : maintiens Gauche, appuie sur Droite. Avec le Snap Tap actif, seule la dernière doit rester allumée ; relâche-la et la première doit se rallumer.</div>
        </div>
        <button type="button" class="btn btn-sm btn-primary" data-act="open-test-window">Ouvrir le Bloc-notes de test</button>
      </div>
      <div class="socd-pads" role="group" aria-label="Touches reçues par les applications">
        ${SOCD_PADS.map(([code, label, hint]) => `<div class="socd-pad" data-socd-pad="${code}" aria-pressed="false"><strong>${label}</strong><span class="muted">${hint}</span></div>`).join("")}
      </div>
      <div id="socd-live-note" class="muted mt-16">Les frappes faites dans la fenêtre d'OpenGear elle-même ne passent pas par le moteur clavier : teste dans une autre fenêtre.</div>
    </div>`;
}

function inputEsports() {
  const prof = S.config.profile;
  const sp = S.system_prefs;
  const socd = prof.socd || { enabled: false, pairs: [], blocked_apps: [] };
  const pairs = arr(socd.pairs);
  const pairLabel = pairs.map((p) => `${p.key_a}+${p.key_b}`).join(" · ") || "Q+D · Z+S";
  const guess = pairLabel.includes("A+D") ? (pairs.length === 1 ? "ad_only" : "qwerty") : pairs.length === 1 ? "qd_only" : "azerty";
  const chatter = num(prof.key_chatter_ms);
  const mdeb = num(prof.mouse_debounce_ms);
  return `
    ${flashtapCard()}
    ${socdTesterCard()}
    <div class="grid-2">
      <div class="card accent-purple">
        <div class="card-head">
          <div><h2 class="card-title">Snap Tap (SOCD)</h2><div class="card-desc">Quand deux touches opposées sont enfoncées, seule la dernière compte : contre-déplacements instantanés (${esc(pairLabel)}).</div></div>
          <span class="pill ${socd.enabled ? (sp.socd_suspended ? "amber" : "emerald") : ""}"><span class="dot"></span>${socd.enabled ? (sp.socd_suspended ? "En pause (jeu protégé)" : "Actif") : "Désactivé"}</span>
        </div>
        <div class="row-between">
          ${sw(!!socd.enabled, "Activer Snap Tap", "socd-en")}
          <select class="og-select auto" data-change="socd-preset" aria-label="Touches concernées">
            <option value="azerty" ${guess === "azerty" ? "selected" : ""}>AZERTY (Q+D et Z+S)</option>
            <option value="qd_only" ${guess === "qd_only" ? "selected" : ""}>AZERTY, gauche/droite (Q+D)</option>
            <option value="qwerty" ${guess === "qwerty" ? "selected" : ""}>QWERTY (A+D et W+S)</option>
            <option value="ad_only" ${guess === "ad_only" ? "selected" : ""}>QWERTY, gauche/droite (A+D)</option>
          </select>
        </div>
        <div class="divider"></div>
        <div class="muted" style="margin-bottom:6px">Mis en pause automatiquement dans ces jeux (anti-triche) :</div>
        <div class="row">
          <input id="socd-blocked" class="og-input mono grow" value="${esc(arr(socd.blocked_apps).join(", "))}" placeholder="cs2.exe" aria-label="Jeux protégés" />
          <button type="button" class="btn btn-sm btn-secondary" data-act="socd-blocked">Enregistrer</button>
        </div>
      </div>
      <div class="card accent-cyan">
        <div class="card-head">
          <div><h2 class="card-title">Anti-rebond</h2><div class="card-desc">Filtre les doubles appuis involontaires (touches ou clics usés).</div></div>
          <span class="pill cyan">Filtrés : ${num(sp.chatter_blocked)} touches · ${num(sp.mouse_dblclick_blocked)} clics</span>
        </div>
        <div class="stack" style="gap:10px">
          ${sliderRow("Clavier", { id: "es-chatter", min: 0, max: 60, value: chatter, input: "label", change: "es-chatter", data: { fmt: "ms" }, display: chatter ? `${chatter} ms` : "Désactivé" })}
          ${sliderRow("Clic souris", { id: "es-mdeb", min: 0, max: 60, value: mdeb, input: "label", change: "es-mdeb", data: { fmt: "ms" }, display: mdeb ? `${mdeb} ms` : "Désactivé" })}
        </div>
        <div class="divider"></div>
        <div class="row-nowrap">
          <button type="button" class="btn btn-secondary grow" data-act="click-test" style="padding:12px">Clique ici rapidement pour mesurer l'intervalle entre deux clics</button>
          <span class="pill mono" id="es-click-res" style="min-width:110px;justify-content:center" aria-live="polite">Prêt</span>
        </div>
      </div>
    </div>`;
}

function inputMouse() {
  if (!S.logi_mice.length) {
    return emptyState("Aucune souris Logitech détectée", "Branche le récepteur LIGHTSPEED / Bolt ou le câble USB de ta souris Logitech G, puis bouge-la pour la sortir de veille.",
      `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`);
  }
  return `<div class="stack">${S.logi_mice.map((m, i) => cardLogiOnboard(m, i)).join("")}</div>`;
}

function mouseStage(m, i, buttons, btnCount) {
  const imgInfo = mouseImage();
  const markers = arr(S.local_assets.mouse_markers);
  const sel = local.btnSel[i] || 0;
  const proj = (idx, fx, fy) => {
    if (!imgInfo.official || !markers[idx]) return { px: fx, py: fy };
    const mk = markers[idx];
    return { px: Number((31 + (num(mk.x_pct) / 100) * 38).toFixed(1)), py: Number((8 + (num(mk.y_pct) / 100) * 84).toFixed(1)) };
  };
  const callouts = [
    { idx: 0, id: "M1 · Gauche", ...proj(0, 42, 28), lx: 2, ly: 12 },
    { idx: 1, id: "M2 · Droit", ...proj(1, 58, 28), lx: 72, ly: 12 },
    { idx: 2, id: "M3 · Molette", ...proj(2, 50, 24), lx: 72, ly: 38 },
    { idx: 4, id: "M5 · Avant", ...proj(4, 34, 41), lx: 2, ly: 40 },
    { idx: 3, id: "M4 · Arrière", ...proj(3, 34, 53), lx: 2, ly: 66 },
  ].filter((c) => c.idx < btnCount);
  return `
    <div class="hero-stage" style="padding:0">
      <div class="mouse-stage-inner">
        <img src="${esc(imgInfo.url)}" class="${imgInfo.official ? "cutout" : ""}" alt="${esc(m.name)}" />
        <svg class="mouse-svg-overlay" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          ${callouts.map((c) => {
            const on = sel === c.idx;
            const endX = c.lx < 50 ? c.lx + 24 : c.lx;
            return `<line x1="${c.px}" y1="${c.py}" x2="${endX}" y2="${c.ly + 6}" stroke="${on ? "#00D4FF" : "rgba(255,255,255,0.3)"}" stroke-width="${on ? 0.6 : 0.32}" vector-effect="non-scaling-stroke"/><circle cx="${c.px}" cy="${c.py}" r="1.25" fill="${on ? "#00D4FF" : "#cfd8ea"}"/>`;
          }).join("")}
        </svg>
        ${callouts.map((c) => `
          <button type="button" class="callout-pin ${sel === c.idx ? "active" : ""}" data-act="mbtn-sel" data-dev="${i}" data-btn="${c.idx}" style="left:${c.lx}%;top:${c.ly}%" aria-pressed="${sel === c.idx}">
            <div class="pin-id">${esc(c.id)}</div>
            <div class="pin-act">${esc((buttons[c.idx] && buttons[c.idx].label) || "Standard")}</div>
          </button>`).join("")}
        ${imgInfo.variants.length > 1 ? `<div class="variant-bar">${imgInfo.variants.map((v, vi) => `<button type="button" class="variant-chip ${vi === local.selectedMouseVariant ? "active" : ""}" data-act="mouse-variant" data-idx="${vi}">${esc(String(v.label).replace(" (Officiel)", ""))}</button>`).join("")}</div>` : ""}
      </div>
    </div>`;
}

export function cardLogiOnboard(m, i) {
  const ob = m.onboard;
  const profiles = arr(m.onboard_profiles);
  if (!ob || !profiles.length) {
    return `<div class="card"><h2 class="card-title">${esc(m.name)}</h2><div class="muted mt-8">Cette souris n'expose pas de profils embarqués modifiables : ses boutons gardent leur fonction d'usine.</div></div>`;
  }
  const d = getDraft(m, i);
  const p = d.profile;
  const btns = p.buttons;
  const btnCount = Math.max(btns.length, num(ob.button_count, 5));
  const ex = isExpert();
  const selIdx = d.idx;
  const sel = local.btnSel[i] || 0;
  const profSeg = profiles.map((pr, idx) => [idx, `Profil ${num(pr.slot_index, idx + 1)}${pr.slot_index === ob.active_profile ? " (actif)" : ""}`]);
  if (!ex) {
    return `
      <div class="card">
        <div class="card-head"><div><h2 class="card-title">${esc(m.name)}</h2><div class="card-desc">Profils enregistrés dans la souris</div></div></div>
        <div class="grid-stage">
          ${mouseStage(m, i, btns, btnCount)}
          <div class="stack">
            <div class="list">
              ${profiles.map((pr, idx) => `
                <div class="list-row ${pr.slot_index === ob.active_profile ? "active" : ""}">
                  <div><strong>Profil ${num(pr.slot_index, idx + 1)}</strong><div class="muted small">${arr(pr.dpi_stages).map((v) => num(v)).join(" / ")} DPI · ${fmtHz(num(pr.report_rate_hz, 1000))}</div></div>
                  ${pr.slot_index === ob.active_profile ? `<span class="pill cyan"><span class="dot"></span>Actif</span>` : `<button type="button" class="btn btn-sm btn-secondary" data-act="logi-activate" data-dev="${i}" data-slot="${num(pr.slot_index)}">Activer</button>`}
                </div>`).join("")}
            </div>
            ${advancedHint("La modification des boutons et l'écriture des profils et macros dans la souris se font en mode Expert.")}
          </div>
        </div>
      </div>`;
  }
  return `
    <div class="card expert-card">
      <div class="card-head">
        <div><h2 class="card-title">${esc(m.name)} — mémoire embarquée ${expertTag()}</h2><div class="card-desc">Les modifications restent en brouillon tant que tu ne les écris pas dans la souris.${d.dirty ? " <strong style='color:var(--amber)'>Brouillon modifié.</strong>" : ""}</div></div>
        ${seg([["Onboard", "Profils de la souris"], ["Host", "Piloté par OpenGear"]], ob.mode, "logi-mode", { dev: i })}
      </div>
      <div class="grid-stage">
        ${mouseStage(m, i, btns, btnCount)}
        <div class="stack">
          <div class="row-between">
            ${seg(profSeg, selIdx, "logi-prof-sel", { dev: i })}
            <button type="button" class="btn btn-sm btn-secondary" data-act="logi-activate" data-dev="${i}" data-slot="${num(p.slot_index)}">Activer ce profil</button>
          </div>
          <div class="muted small">Emplacement ${esc(p.sector_addr)} · ${p.from_rom ? "ROM d'usine" : "Flash"} · ${p.crc_valid ? "intégrité vérifiée" : "intégrité à vérifier"}</div>
          <div>
            <div class="field-label" style="margin-bottom:6px">Paliers DPI (le point indique le palier par défaut)</div>
            <div class="stack" style="gap:6px">
              ${p.dpi_stages.map((v, st) => `
                <div class="row-nowrap">
                  <input type="radio" name="def-dpi-${i}" value="${st}" ${num(p.default_dpi_idx) === st ? "checked" : ""} data-change="draft-def-dpi" data-dev="${i}" aria-label="Palier ${st + 1} par défaut" style="accent-color:var(--cyan)" />
                  <span class="muted" style="width:60px">Palier ${st + 1}</span>
                  <input type="number" class="og-input narrow" min="100" max="32000" step="50" value="${num(v)}" data-change="draft-stage-dpi" data-dev="${i}" data-stage="${st}" aria-label="DPI du palier ${st + 1}" />
                  <button type="button" class="btn btn-sm btn-secondary" data-act="logi-test-stage" data-dev="${i}" data-stage="${st}">Tester</button>
                </div>`).join("")}
            </div>
          </div>
          <div>
            <div class="field-label" style="margin-bottom:6px">Boutons</div>
            <div class="stack" style="gap:6px">
              ${Array.from({ length: btnCount }, (_, b) => {
                const cur = btns[b] || { hex: "", label: "Standard" };
                const known = S.button_presets.some((bp) => bp.hex === cur.hex);
                return `
                  <div class="row-nowrap" style="${sel === b ? "background:rgba(0,212,255,0.08);border-radius:6px;padding:2px 4px" : ""}">
                    <label class="muted" style="width:110px;font-size:12px" for="btn-${i}-${b}">${esc(BTN_NAMES[b] || `Bouton M${b + 1}`)}</label>
                    <select id="btn-${i}-${b}" class="og-select grow" data-change="draft-btn" data-dev="${i}" data-btn="${b}">
                      ${!known ? `<option value="${esc(cur.hex)}" selected>${esc(cur.label)}</option>` : ""}
                      ${S.button_presets.map((bp) => `<option value="${esc(bp.hex)}" ${bp.hex === cur.hex ? "selected" : ""}>${esc(bp.label)}</option>`).join("")}
                    </select>
                  </div>`;
              }).join("")}
            </div>
          </div>
          <div class="row-between">
            <span class="field-label">Fréquence du profil</span>
            ${seg([125, 250, 500, 1000].map((hz) => [hz, fmtHz(hz)]), p.report_rate_hz, "draft-hz", { dev: i })}
          </div>
          <div class="row">
            <button type="button" class="btn btn-primary" data-act="logi-flash" data-dev="${i}">Écrire le profil dans la souris</button>
            <button type="button" class="btn btn-secondary btn-sm" data-act="logi-reload" data-dev="${i}">Relire la souris</button>
          </div>
          <div class="divider" style="margin:4px 0"></div>
          <div class="row">
            <label class="field-label" for="macro-preset-${i}">Macro matérielle sur M${sel + 1}</label>
            <select id="macro-preset-${i}" class="og-select auto">${Object.entries(MACRO_PRESETS).map(([k, v]) => `<option value="${k}">${esc(v.label)}</option>`).join("")}</select>
            <button type="button" class="btn btn-secondary btn-sm" data-act="logi-flash-macro" data-dev="${i}">Écrire la macro</button>
          </div>
        </div>
      </div>
    </div>`;
}
