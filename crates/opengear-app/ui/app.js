// OpenGear — interface WebView2 « simple par défaut, puissant à la demande »
// JS vanilla, aucune dépendance. Tout texte issu du matériel ou de l'utilisateur passe par esc().
(() => {
  "use strict";

  // ==================== CONSTANTES ====================
  const SWATCHES = [
    ["Rouge ROG", "#FF1848"],
    ["Cyan", "#00D4FF"],
    ["Jaune", "#FFC400"],
    ["Violet", "#9B3CFF"],
    ["Émeraude", "#00EB91"],
    ["Orange", "#FF6A00"],
    ["Blanc", "#F0F6FF"],
    ["Bleu", "#195AFF"],
  ];

  // Commandes de configuration que le moteur sait annuler (`undo_last`) : proposent « Annuler ».
  // Les écritures purement matérielles (DPI, flash, EEPROM…) n'en font pas partie.
  const CONFIG_CMDS = new Set([
    "set_rgb_config", "set_sync_all", "set_per_key_color", "clear_per_key", "set_azerty",
    "input_set_binding", "input_remove_binding", "input_set_esports", "input_set_winlock",
    "apply_preset", "switch_profile", "create_profile", "profile_set_linked_apps", "profile_import_json",
    "asus_set_led_count", "cooling_sl3_argb",
  ]);

  const SVG = (inner, vb = "0 0 20 20") =>
    `<svg viewBox="${vb}" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${inner}</svg>`;
  const ICONS = {
    warn: SVG('<path d="M10 3L18 17H2L10 3z"/><path d="M10 8.5v3.5M10 14.6v.1"/>'),
    check: SVG('<path d="M4.5 10.5l3.5 3.5 7.5-8"/>'),
    x: SVG('<path d="M5.5 5.5l9 9M14.5 5.5l-9 9"/>'),
    info: SVG('<circle cx="10" cy="10" r="7.5"/><path d="M10 9v5M10 6.2v.1"/>'),
    chev: SVG('<path d="M8 5l5 5-5 5"/>'),
    plug: SVG('<path d="M7 2.5v4M13 2.5v4M5 6.5h10v3a5 5 0 0 1-10 0v-3zM10 14.5v3"/>'),
    lock: SVG('<rect x="4" y="9" width="12" height="8.5" rx="2"/><path d="M6.8 9V6.5a3.2 3.2 0 0 1 6.4 0V9"/>'),
    battery: SVG('<rect x="1.5" y="1.5" width="17" height="9" rx="2"/><path d="M20.5 4.5v3"/>', "0 0 22 12"),
    refresh: SVG('<path d="M16 10a6 6 0 1 1-1.8-4.3"/><path d="M16.5 3v3.5H13"/>'),
    keyboard: SVG('<rect x="2" y="5" width="16" height="10" rx="2"/><path d="M5 8.5h1M8 8.5h1M11 8.5h1M14 8.5h1M6 12h8"/>'),
    mouse: SVG('<rect x="5" y="2.5" width="10" height="15" rx="5"/><path d="M10 6v3"/>'),
    headset: SVG('<path d="M3.5 12.5V10a6.5 6.5 0 0 1 13 0v2.5"/><rect x="2.5" y="11.5" width="3.5" height="5.5" rx="1.2"/><rect x="14" y="11.5" width="3.5" height="5.5" rx="1.2"/>'),
    fan: SVG('<circle cx="10" cy="10" r="7.5"/><circle cx="10" cy="10" r="1.6"/><path d="M10 8.4C10 5.5 12.5 4 14 5.5M11.6 10C14.5 10 16 12.5 14.5 14M10 11.6C10 14.5 7.5 16 6 14.5M8.4 10C5.5 10 4 7.5 5.5 6"/>'),
    board: SVG('<rect x="3" y="3" width="14" height="14" rx="2"/><rect x="7" y="7" width="6" height="6" rx="1"/><path d="M7 3v2M13 3v2M7 15v2M13 15v2M3 7h2M3 13h2M15 7h2M15 13h2"/>'),
    ram: SVG('<rect x="2" y="6" width="16" height="8" rx="1.5"/><path d="M5 9v2M8 9v2M11 9v2M14 9v2M4 14v2.5M16 14v2.5"/>'),
    chip: SVG('<rect x="4.5" y="4.5" width="11" height="11" rx="2"/><path d="M7.5 2v2.5M12.5 2v2.5M7.5 15.5V18M12.5 15.5V18M2 7.5h2.5M2 12.5h2.5M15.5 7.5H18M15.5 12.5H18"/>'),
    undo: SVG('<path d="M7 5L3.5 8.5 7 12"/><path d="M3.5 8.5H12a4.5 4.5 0 0 1 0 9H9"/>'),
    spark: SVG('<path d="M10 2.5l1.8 5.7 5.7 1.8-5.7 1.8L10 17.5l-1.8-5.7L2.5 10l5.7-1.8L10 2.5z"/>'),
  };
  const PRESET_ICONS = {
    silent: SVG('<path d="M15.5 12.5A6.5 6.5 0 0 1 7.5 4.5a6.5 6.5 0 1 0 8 8z"/>'),
    gaming: SVG('<path d="M11 2.5L4.5 11h5l-1 6.5L15.5 9h-5l.5-6.5z"/>'),
    office: SVG('<rect x="2.5" y="6" width="15" height="10.5" rx="2"/><path d="M7 6V4.5A1.5 1.5 0 0 1 8.5 3h3A1.5 1.5 0 0 1 13 4.5V6M2.5 10.5h15"/>'),
    lights_off: SVG('<path d="M10 2.5a5 5 0 0 0-3 9v2h6v-2a5 5 0 0 0-3-9z"/><path d="M7.5 16h5M3 3l14 14"/>'),
  };

  // ==================== UTILITAIRES ====================
  function esc(s) {
    return String(s ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }
  const arr = (x) => (Array.isArray(x) ? x : []);
  const num = (x, d = 0) => (typeof x === "number" && Number.isFinite(x) ? x : d);
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const clone = (o) => JSON.parse(JSON.stringify(o));
  function safeHex(h, d = "#00D4FF") {
    const s = String(h || "").trim();
    return /^#[0-9a-fA-F]{6}$/.test(s) ? s.toUpperCase() : d;
  }
  const kebab = (k) => k.replace(/[A-Z]/g, (m) => "-" + m.toLowerCase());
  function dataAttrs(obj) {
    return Object.entries(obj || {})
      .filter(([, v]) => v !== undefined && v !== null)
      .map(([k, v]) => `data-${kebab(k)}="${esc(v)}"`)
      .join(" ");
  }
  const fmtHz = (hz) => (hz >= 1000 ? `${hz / 1000} kHz` : `${hz} Hz`);
  function fmtDate(ms) {
    const n = Number(ms);
    if (!Number.isFinite(n) || n <= 0) return "Date inconnue";
    try {
      return new Date(n).toLocaleString("fr-FR", { day: "2-digit", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit" });
    } catch (_) {
      return new Date(n).toISOString();
    }
  }
  function fmtSize(b) {
    const n = Number(b);
    if (!Number.isFinite(n)) return "—";
    if (n < 1024) return `${n} o`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} Ko`;
    return `${(n / 1024 / 1024).toFixed(1)} Mo`;
  }
  const plural = (n, one, many) => `${n} ${n > 1 ? many : one}`;

  // ==================== PONT IPC ====================
  let nextReqId = 1;
  const pending = new Map();
  window.__ogReply = (id, res) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    if (res && res.ok) p.resolve(res.data);
    else p.reject(new Error((res && res.error) || "Erreur IPC"));
  };

  function rpc(cmd, payload = {}) {
    return new Promise((resolve, reject) => {
      if (!window.ipc || typeof window.ipc.postMessage !== "function") {
        reject(new Error("Pont IPC indisponible"));
        return;
      }
      const id = nextReqId++;
      pending.set(id, { resolve, reject });
      try {
        window.ipc.postMessage(JSON.stringify({ id, cmd, payload }));
      } catch (e) {
        pending.delete(id);
        reject(e);
      }
    });
  }

  // ==================== NOTIFICATIONS ====================
  function setStatus(txt) {
    const el = document.getElementById("status-text");
    if (el) el.textContent = txt;
  }

  function toast(msg, kind = "info", opts = {}) {
    if (!msg) return null;
    setStatus(msg);
    const root = document.getElementById("toast-root");
    const el = document.createElement("div");
    el.className = "toast" + (kind === "err" ? " err" : kind === "ok" ? " ok" : "");
    el.setAttribute("role", kind === "err" ? "alert" : "status");
    const txt = document.createElement("span");
    txt.className = "toast-msg";
    txt.textContent = msg;
    el.appendChild(txt);
    if (opts.action) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "toast-btn";
      b.textContent = opts.action.label;
      b.onclick = () => {
        clearTimeout(el._timer);
        el.remove();
        opts.action.run(b);
      };
      el.appendChild(b);
    }
    const close = document.createElement("button");
    close.type = "button";
    close.className = "toast-close";
    close.setAttribute("aria-label", "Fermer la notification");
    close.textContent = "×";
    close.onclick = () => el.remove();
    el.appendChild(close);
    root.appendChild(el);
    el._timer = setTimeout(() => el.remove(), opts.duration || (kind === "err" ? 7000 : 3800));
    while (root.children.length > 4) root.firstElementChild.remove();
    return el;
  }

  let undoToastEl = null;
  function offerUndo(msg) {
    const u = S && S.ui;
    if (!u || !u.can_undo) {
      if (msg) toast(msg, "ok");
      return;
    }
    const label = msg || (u.undo_label ? `Modifié : ${u.undo_label}` : "Modification appliquée");
    if (undoToastEl && undoToastEl.isConnected) {
      undoToastEl.querySelector(".toast-msg").textContent = label;
      setStatus(label);
      clearTimeout(undoToastEl._timer);
      const el = undoToastEl;
      el._timer = setTimeout(() => el.remove(), 10000);
      return;
    }
    undoToastEl = toast(label, "ok", { duration: 10000, action: { label: "Annuler", run: () => doUndo() } });
  }

  async function doUndo(btn) {
    if (undoToastEl) {
      undoToastEl.remove();
      undoToastEl = null;
    }
    const lbl = S && S.ui && S.ui.undo_label;
    await call("undo_last", {}, { btn, ok: lbl ? `Annulé : ${lbl}` : "Dernière modification annulée" });
  }

  function humanError(raw) {
    const m = String(raw || "").trim();
    if (!m) return "Une erreur inconnue est survenue.";
    if (/Commande IPC inconnue/i.test(m)) return "Cette fonction n'est pas encore disponible dans le moteur OpenGear installé. Mets l'application à jour.";
    if (/Pont IPC/i.test(m)) return "Impossible de joindre le moteur OpenGear. Redémarre l'application.";
    if (/rien à annuler|nothing to undo/i.test(m)) return "Il n'y a plus rien à annuler.";
    if (/acc[eè]s refus|access is denied|access denied|os error 5|admin/i.test(m)) return `Windows a refusé l'accès au matériel : relance OpenGear en administrateur (Système › Diagnostic). Détail : ${m}`;
    if (/timeout|timed out|d[ée]lai|no response|pas r[ée]pondu/i.test(m)) return `L'appareil n'a pas répondu à temps. Vérifie qu'il est branché et sorti de veille, puis réessaie. Détail : ${m}`;
    if (/non connect|not connected|introuvable|not found|disconnected|d[ée]branch|aucun|aucune/i.test(m)) return `Appareil introuvable : il a peut-être été débranché ou s'est mis en veille. Détail : ${m}`;
    if (/JSON invalide|invalid json/i.test(m)) return `Ce fichier n'est pas un profil OpenGear valide. Détail : ${m}`;
    if (/invalide|invalid/i.test(m)) return `Valeur refusée par l'appareil : ${m}`;
    return `L'opération a échoué : ${m}`;
  }

  // ==================== FENÊTRES MODALES ====================
  const modalStack = [];
  const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  function trapFocus(container, e) {
    const f = [...container.querySelectorAll(FOCUSABLE)].filter((x) => x.getClientRects().length);
    if (!f.length) return;
    const first = f[0];
    const last = f[f.length - 1];
    if (!container.contains(document.activeElement)) {
      e.preventDefault();
      first.focus();
    } else if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  // Ouvre une modale générique. Échap / clic hors du cadre = valeur `dismiss`.
  function openModal({ title, html, actions, dismiss = null, wide = false, danger = false }) {
    return new Promise((resolve) => {
      const root = document.getElementById("modal-root");
      const prevFocus = document.activeElement;
      const back = document.createElement("div");
      back.className = "modal-backdrop";
      const id = "mdl-" + Math.random().toString(36).slice(2, 8);
      back.innerHTML = `
        <div class="modal ${wide ? "modal-wide" : ""} ${danger ? "modal-danger" : ""}" role="dialog" aria-modal="true" aria-labelledby="${id}-t" aria-describedby="${id}-b">
          <div class="modal-head">
            ${danger ? `<span class="modal-icon">${ICONS.warn}</span>` : ""}
            <h2 class="modal-title" id="${id}-t">${esc(title)}</h2>
          </div>
          <div class="modal-body" id="${id}-b">${html}</div>
          <div class="modal-actions">
            ${actions.map((a, i) => `<button type="button" class="btn ${a.cls || "btn-secondary"}" data-mi="${i}">${esc(a.label)}</button>`).join("")}
          </div>
        </div>`;
      let closed = false;
      const done = (v) => {
        if (closed) return;
        closed = true;
        document.removeEventListener("keydown", onKey, true);
        back.remove();
        const i = modalStack.indexOf(back);
        if (i >= 0) modalStack.splice(i, 1);
        if (prevFocus && prevFocus.isConnected && typeof prevFocus.focus === "function") prevFocus.focus();
        resolve(v);
      };
      const onKey = (e) => {
        if (modalStack[modalStack.length - 1] !== back) return;
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          done(dismiss);
        } else if (e.key === "Tab") {
          trapFocus(back.querySelector(".modal"), e);
        }
      };
      back.querySelectorAll("[data-mi]").forEach((b) => {
        b.onclick = () => done(actions[Number(b.dataset.mi)].value);
      });
      back.addEventListener("mousedown", (e) => {
        if (e.target === back) done(dismiss);
      });
      document.addEventListener("keydown", onKey, true);
      root.appendChild(back);
      modalStack.push(back);
      const afIdx = actions.findIndex((a) => a.autofocus);
      const af = back.querySelector(`[data-mi="${afIdx >= 0 ? afIdx : 0}"]`);
      setTimeout(() => af && af.focus(), 0);
    });
  }

  function confirmDialog(r) {
    return openModal({
      title: r.title,
      danger: true,
      html: `
        <p class="modal-text">${esc(r.body)}</p>
        ${r.details && r.details.length ? `<ul class="modal-list">${r.details.map((d) => `<li>${esc(d)}</li>`).join("")}</ul>` : ""}
        <p class="modal-note">${esc(r.note || "Cette opération écrit de façon durable dans le matériel.")}</p>`,
      actions: [
        { label: "Annuler", value: false, autofocus: true },
        { label: r.confirm || "Confirmer", cls: "btn-danger", value: true },
      ],
      dismiss: false,
    });
  }

  // Description claire du risque pour chaque commande permanente ou risquée (null = pas de confirmation).
  function riskOf(cmd, p = {}) {
    switch (cmd) {
      case "logi_flash_profile":
        return {
          title: "Écrire ce profil dans la souris ?",
          body: "Le profil sera écrit dans la mémoire flash interne de la souris et remplacera le contenu actuel de cet emplacement. Une copie de sauvegarde est faite avant l'écriture, puis le résultat est vérifié (CRC-16).",
          details: ["Ne débranche pas la souris et ne la laisse pas se mettre en veille pendant l'écriture.", "Une sauvegarde restaurable apparaîtra dans Système › Sauvegardes."],
          confirm: "Écrire dans la souris",
        };
      case "logi_flash_macro":
        return {
          title: "Écrire la macro dans la souris ?",
          body: `La macro sera écrite dans la mémoire flash de la souris et assignée au bouton M${num(p.button_index) + 1} du profil embarqué sélectionné. Le bouton perdra son action actuelle.`,
          details: ["Ne débranche pas la souris pendant l'écriture.", "Une sauvegarde du secteur est conservée (Système › Sauvegardes)."],
          confirm: "Écrire la macro",
        };
      case "logi_restore_backup":
        return {
          title: "Restaurer cette sauvegarde ?",
          body: "Le contenu de la sauvegarde va remplacer le secteur correspondant dans la mémoire flash de la souris. Les réglages actuellement stockés dans ce secteur seront perdus.",
          details: p._label ? [p._label] : [],
          confirm: "Restaurer",
        };
      case "corsair_flash_hwp_lighting":
        return {
          title: "Flasher l'effet dans le clavier ?",
          body: "L'effet sera écrit dans la mémoire interne du clavier (profil matériel) et restera actif même sans OpenGear. L'ancien effet est sauvegardé puis l'écriture est vérifiée. L'éclairage logiciel en direct sera coupé pour afficher l'effet flashé.",
          details: ["Ne débranche pas le clavier pendant l'opération."],
          confirm: "Flasher le clavier",
        };
      case "asus_set_hw_mode":
        if (!p.commit) return null;
        return {
          title: "Enregistrer dans la carte mère ?",
          body: "L'effet sera enregistré dans la mémoire du contrôleur ASUS Aura et réappliqué à chaque démarrage du PC, même sans OpenGear.",
          confirm: "Enregistrer",
        };
      case "asus_set_shutdown_mode":
        return {
          title: "Modifier l'éclairage PC éteint ?",
          body: "Ce réglage est écrit de façon permanente dans la mémoire de la carte mère. Il s'applique lorsque le PC est éteint ou en veille.",
          confirm: "Écrire dans la carte mère",
        };
      case "ram_apply_rgb":
        if (!p.save_eeprom) return null;
        return {
          title: "Écrire dans la mémoire des barrettes RAM ?",
          body: "L'effet sera stocké de façon permanente dans les barrettes via le bus SMBus. Une écriture interrompue peut laisser l'éclairage des barrettes dans un état incorrect.",
          details: ["Évite de lancer cette opération pendant une forte charge système."],
          confirm: "Écrire dans les barrettes",
        };
      case "superio_set_pwm":
        return {
          title: "Forcer la vitesse de ce ventilateur ?",
          body: `OpenGear va fixer ce ventilateur à ${num(p.duty_pct)} % en écrivant directement dans la puce de contrôle de la carte mère. La régulation automatique du BIOS sera désactivée pour ce ventilateur jusqu'à « Auto BIOS » ou au redémarrage.`,
          details: ["Une vitesse trop basse peut faire chauffer le composant refroidi (processeur, pompe…)."],
          confirm: "Forcer la vitesse",
        };
      case "cooling_update_channel":
        if (!p.apply) return null;
        return {
          title: "Appliquer ce réglage à la carte mère ?",
          body: "Le mode de ventilation de ce canal sera écrit dans le firmware de la carte mère (BIOS) et restera actif sans OpenGear.",
          details: ["Vérifie que la pompe et les ventilateurs gardent une vitesse suffisante en charge."],
          confirm: "Appliquer au BIOS",
        };
      case "cooling_sl3_preset":
        return {
          title: "Changer le réglage de la pompe ?",
          body: "Ce préréglage remplace la courbe actuelle de la pompe du watercooling. Si OpenGear tourne en administrateur, il est aussi écrit dans la carte mère.",
          details: ["Un plancher de sécurité anti-cavitation est toujours conservé."],
          confirm: "Changer le réglage",
        };
      case "pawnio_install":
        return {
          title: "Installer le pilote PawnIO ?",
          body: "OpenGear va télécharger puis lancer l'installateur officiel signé de PawnIO, un pilote système qui donne un accès bas niveau au matériel (capteurs, ventilateurs, RAM). Windows demandera une autorisation administrateur.",
          confirm: "Installer le pilote",
          note: "Un pilote système s'installe au niveau de Windows : n'accepte que si tu fais confiance à cette source.",
        };
      case "cooling_relaunch_admin":
        return {
          title: "Relancer OpenGear en administrateur ?",
          body: "OpenGear va se fermer puis redémarrer avec les droits administrateur (fenêtre d'autorisation Windows). Les réglages non enregistrés de la session pourraient être perdus.",
          confirm: "Relancer",
          note: "Les droits administrateur donnent à OpenGear un accès complet au matériel.",
        };
      case "profile_import_json": {
        let name = "";
        try {
          name = JSON.parse(p.json || "{}").name || "";
        } catch (_) {
          /* l'erreur sera signalée par le moteur */
        }
        return {
          title: "Importer ce profil ?",
          body: `Le profil${name ? ` « ${name} »` : ""} sera importé puis activé. S'il existe déjà un profil portant ce nom, il sera remplacé.`,
          confirm: "Importer",
          note: "Le profil existant du même nom sera écrasé.",
        };
      }
      default:
        return null;
    }
  }

  // ==================== ÉTAT ====================
  let S = null;
  let uiModeOverride = null; // utilisé seulement si le moteur ne connaît pas encore `S.ui`
  let configWarningShown = false;
  let diag = { data: null, loading: false, error: null };
  let backups = { data: null, loading: false, error: null, mouse: 0 };

  function ui() {
    const u = (S && S.ui) || {};
    return {
      mode: u.mode === "simple" || u.mode === "expert" ? u.mode : uiModeOverride || "expert",
      onboarding_done: S && S.ui ? u.onboarding_done !== false : true,
      can_undo: !!u.can_undo,
      undo_label: u.undo_label || null,
    };
  }
  const isExpert = () => ui().mode === "expert";

  function normalizeState() {
    if (!S) return;
    [
      "corsair_devices", "logi_mice", "logi_keyboards", "logi_headsets", "power_plans", "effects",
      "aura_modes", "ram_rgb_modes", "button_presets", "keys_layout", "bindable_keys", "devices",
      "firmware_inventory", "hwp_lighting_kinds",
    ].forEach((k) => {
      if (!Array.isArray(S[k])) S[k] = [];
    });
    S.presets = arr(S.presets);
    S.telemetry = S.telemetry || {};
    S.cooling = S.cooling || {};
    S.cooling.channels = arr(S.cooling.channels);
    S.static_info = S.static_info || {};
    S.system_prefs = S.system_prefs || {};
    S.local_assets = S.local_assets || {};
    S.config = S.config || {};
    S.config.profiles = arr(S.config.profiles);
    const p = (S.config.profile = S.config.profile || {});
    p.rgb = p.rgb || { kind: "Static", primary: "#00D4FF", secondary: "#9B3CFF", speed: 1, brightness: 1, reverse: false, per_key: {} };
    p.rgb.per_key = p.rgb.per_key || {};
    p.bindings = p.bindings || {};
    p.linked_apps = arr(p.linked_apps);
    if (S.ui && uiModeOverride && !S.ui.mode) S.ui.mode = uiModeOverride;
  }

  function applyState(st, rerender = true) {
    S = st;
    normalizeState();
    syncDrafts();
    updateChrome();
    if (S.config_warning && !configWarningShown) {
      configWarningShown = true;
      toast(S.config_warning, "err");
    }
    if (rerender) render();
  }

  function setBusy(el, on) {
    if (!el) return;
    if (on) {
      el.classList.add("is-busy");
      el.setAttribute("aria-busy", "true");
      if (el.tagName === "BUTTON") el.disabled = true;
    } else if (el.isConnected) {
      el.classList.remove("is-busy");
      el.removeAttribute("aria-busy");
      if (el.tagName === "BUTTON") el.disabled = false;
    }
  }

  // Appel IPC unifié : confirmation si risqué, état de chargement, erreurs lisibles, toast « Annuler ».
  async function call(cmd, payload = {}, opts = {}) {
    if (!opts.skipConfirm) {
      const risk = riskOf(cmd, payload);
      if (risk && !(await confirmDialog(risk))) return null;
    }
    const clean = {};
    Object.keys(payload || {}).forEach((k) => {
      if (k[0] !== "_") clean[k] = payload[k];
    });
    const btn = opts.btn || null;
    setBusy(btn, true);
    try {
      const data = await rpc(cmd, clean);
      if (data && data.config) applyState(data, opts.rerender !== false);
      else if (data && data.state && data.state.config) applyState(data.state, opts.rerender !== false);
      const msg = (data && data.last_action_msg) || opts.ok || (data && typeof data.message === "string" ? data.message : null);
      if (CONFIG_CMDS.has(cmd)) offerUndo(opts.silent ? null : msg);
      else if (msg && !opts.silent) toast(msg, "ok");
      updateChrome();
      return data || {};
    } catch (e) {
      if (!opts.quietError) toast(humanError(e && e.message), "err");
      return null;
    } finally {
      setBusy(btn, false);
    }
  }

  // ==================== ÉTAT LOCAL DE L'INTERFACE ====================
  let tab = "home";
  const sub = { lighting: "global", performance: "mouse", input: "keys", system: "prefs" };
  let deviceKey = null;
  let selectedKeyId = "ESC";
  let paintMode = false;
  let paintColor = "#00D4FF";
  let kbViewMode = "interactive";
  let auraDraftMode = null;
  let selectedCoolingIdx = 0;
  let sl3Color = "#FF8C1A";
  let selectedMouseVariant = 0;
  let selectedHeadsetVariant = 0;
  let macroSteps = [
    { key: "LCTRL", down: true, delay_ms: 10 },
    { key: "C", down: true, delay_ms: 10 },
    { key: "C", down: false, delay_ms: 10 },
    { key: "LCTRL", down: false, delay_ms: 10 },
  ];
  const profSel = {}; // souris Logitech -> index du profil embarqué affiché
  const btnSel = {}; // souris Logitech -> bouton sélectionné
  const drafts = {}; // souris Logitech -> { idx, profile, dirty }
  const eqDrafts = {}; // casque Logitech -> { gains, dirty }
  let pendingRender = false;
  let curveDragging = false;
  const animStart = performance.now();

  function syncDrafts() {
    Object.keys(drafts).forEach((k) => {
      if (!drafts[k].dirty) delete drafts[k];
    });
    Object.keys(eqDrafts).forEach((k) => {
      if (!eqDrafts[k].dirty) delete eqDrafts[k];
    });
    if (S && S.asus && !auraDraftMode) auraDraftMode = S.asus.hw_mode;
    if (selectedCoolingIdx >= S.cooling.channels.length) selectedCoolingIdx = 0;
  }

  function getDraft(m, i) {
    const profiles = arr(m.onboard_profiles);
    if (!profiles.length) return null;
    let idx = profSel[i] || 0;
    if (idx >= profiles.length) idx = 0;
    profSel[i] = idx;
    const d = drafts[i];
    if (!d || d.idx !== idx) drafts[i] = { idx, profile: clone(profiles[idx]), dirty: false };
    const p = drafts[i].profile;
    p.buttons = arr(p.buttons);
    p.dpi_stages = arr(p.dpi_stages);
    return drafts[i];
  }

  function getEq(h, i) {
    if (!eqDrafts[i]) {
      const g = arr(h.equalizer && h.equalizer.gains_db);
      eqDrafts[i] = { gains: g.length ? g.map((x) => num(x)) : [0, 0, 0, 0, 0], dirty: false };
    }
    return eqDrafts[i];
  }

  // ==================== IMAGES ====================
  function mouseImage() {
    const vars = arr(S.local_assets.mouse_variants);
    if (vars.length) {
      const idx = Math.min(selectedMouseVariant, vars.length - 1);
      return { url: vars[idx].asset_url, official: true, variants: vars };
    }
    const builtin = [
      { label: "Noir", asset_url: "/assets/mouse_superlight.jpg" },
      { label: "Blanc", asset_url: "/assets/mouse_superlight_white.jpg" },
    ];
    const idx = Math.min(selectedMouseVariant, builtin.length - 1);
    return { url: builtin[idx].asset_url, official: false, variants: builtin };
  }
  function headsetImage() {
    const vars = arr(S.local_assets.headset_variants);
    if (vars.length) {
      const idx = Math.min(selectedHeadsetVariant, vars.length - 1);
      return { url: vars[idx].asset_url, official: true, variants: vars };
    }
    return { url: "/assets/headset_prox.jpg", official: false, variants: [] };
  }
  function k70Image() {
    const imgs = arr(S.local_assets.k70_images);
    if (imgs.length) {
      const wanted = S.config.use_azerty_labels ? "fr" : "na";
      const f = imgs.find((x) => x.id === wanted) || imgs.find((x) => x.id === "thumb") || imgs[0];
      return { url: f.asset_url, official: true };
    }
    return { url: "/assets/keyboard_k70_tkl.jpg", official: false };
  }

  // ==================== CATALOGUE DES APPAREILS ====================
  const qa = (label, act, data = {}, title = "") => ({ label, act, data, title });

  function devicesList() {
    if (!S) return [];
    const out = [];
    const prof = S.config.profile;
    S.corsair_devices.forEach((d, i) => {
      const kind = d.kind;
      const base = { key: `corsair:${i}`, vendor: "Corsair", kind, name: d.product_name || "Appareil Corsair", type: d.kind_label || "Corsair", battery: d.battery_pct, status: { label: "Connecté", cls: "emerald" }, quick: [] };
      if (kind === "Mouse") {
        Object.assign(base, { img: mouseImage().url, icon: "mouse" });
        if (d.mouse) {
          const st = num(d.mouse.active_stage);
          const stages = arr(d.mouse.dpi_stages);
          base.status = { label: `${num(d.mouse.current_dpi_x, stages[st] || 0)} DPI`, cls: "cyan" };
          if (stages.length) base.quick.push(qa("Palier DPI suivant", "cm-stage", { dev: i, stage: (st + 1) % stages.length }));
        }
      } else if (kind === "Headset") {
        Object.assign(base, { img: headsetImage().url, icon: "headset" });
        const side = num(d.headset && d.headset.sidetone_pct);
        base.quick.push(qa(side > 0 ? "Couper le retour micro" : "Activer le retour micro", "chs-side-quick", { dev: i, pct: side > 0 ? 0 : 50 }));
      } else if (kind === "CoolerHub") {
        Object.assign(base, { img: "/assets/aio_silentloop3.jpg", icon: "fan" });
      } else if (kind === "Keyboard") {
        const k = k70Image();
        Object.assign(base, { img: k.url, imgCls: k.official ? "cutout" : "", icon: "keyboard" });
        base.status = d.pollrate_hz ? { label: fmtHz(d.pollrate_hz), cls: "emerald" } : base.status;
        base.quick.push(qa(prof.enable_rgb ? "Couper l'éclairage" : "Allumer l'éclairage", "rgb-enable", { on: prof.enable_rgb ? "0" : "1" }));
        const br = num(d.hw_brightness_pct, 100);
        const next = br >= 100 ? 50 : br >= 50 ? 0 : 100;
        base.quick.push(qa(`Luminosité ${next} %`, "cs-bright-quick", { dev: i, pct: next }));
        if (d.hw_winlock != null) base.quick.push(qa(d.hw_winlock ? "Débloquer Windows" : "Bloquer Windows", "cs-winlock-quick", { dev: i, on: d.hw_winlock ? "0" : "1" }, "Bloque la touche Windows pendant le jeu"));
      } else {
        Object.assign(base, { icon: "chip" });
      }
      out.push(base);
    });
    S.logi_mice.forEach((m, i) => {
      const mi = mouseImage();
      const bat = m.battery || null;
      const dpi = m.dpi;
      const item = {
        key: `logi-mouse:${i}`, vendor: "Logitech", kind: "Mouse", name: m.name || "Souris Logitech", type: "Souris Logitech G",
        img: mi.url, imgCls: mi.official ? "cutout" : "", icon: "mouse",
        battery: bat ? bat.percent : null, charging: bat && /charge/i.test(bat.state || "") && !/sur batterie/i.test(bat.state || ""),
        status: { label: dpi ? `${dpi.dpi_x} DPI` : "Connectée", cls: "cyan" }, quick: [],
      };
      if (dpi) {
        [800, 1600, 3200].filter((v) => v >= num(dpi.min_dpi, 100) && v <= num(dpi.max_dpi, 25600)).forEach((v) => item.quick.push(qa(`${v} DPI`, "logi-dpi", { dev: i, dpi: v })));
      }
      out.push(item);
    });
    S.logi_keyboards.forEach((k, i) => {
      out.push({
        key: `logi-kb:${i}`, vendor: "Logitech", kind: "Keyboard", name: k.name || "Clavier Logitech", type: "Clavier Logitech G", icon: "keyboard",
        battery: k.battery ? k.battery.percent : null, status: { label: "Connecté", cls: "emerald" },
        quick: [qa("Couleur fixe", "lkb-quick", { dev: i, mode: "Fixed" }), qa("Vague", "lkb-quick", { dev: i, mode: "Wave" }), qa("Éteindre", "lkb-quick", { dev: i, mode: "Off" })],
      });
    });
    S.logi_headsets.forEach((h, i) => {
      const hi = headsetImage();
      const presets = arr(h.presets);
      const flat = presets.findIndex((p) => /plat/i.test(p.name));
      const fps = presets.findIndex((p) => /fps/i.test(p.name));
      const quick = [];
      if (h.equalizer && flat >= 0) quick.push(qa("Son neutre", "eq-preset", { dev: i, idx: flat }));
      if (h.equalizer && fps >= 0) quick.push(qa("Son FPS", "eq-preset", { dev: i, idx: fps }));
      quick.push(qa(num(h.sidetone_pct) > 0 ? "Couper le retour micro" : "Retour micro 50 %", "logi-side-quick", { dev: i, pct: num(h.sidetone_pct) > 0 ? 0 : 50 }));
      out.push({
        key: `logi-hs:${i}`, vendor: "Logitech", kind: "Headset", name: h.name || "Casque Logitech", type: "Casque Logitech G",
        img: hi.url, imgCls: hi.official ? "cutout" : "", icon: "headset", status: { label: "Connecté", cls: "purple" }, quick,
      });
    });
    if (S.asus) {
      const direct = !!prof.sync_asus_aura;
      out.push({
        key: "asus", vendor: "ASUS", kind: "Motherboard", name: (S.static_info.motherboard || S.asus.product_name || "Carte mère ASUS").replace(" GAMING WIFI", ""), type: "Carte mère · Aura",
        img: "/assets/motherboard_b650a.jpg", icon: "board", status: { label: direct ? "Synchronisée" : "Mode autonome", cls: direct ? "cyan" : "rog" },
        quick: [
          qa(direct ? "Arrêter la synchro" : "Synchroniser", "asus-direct", { on: direct ? "0" : "1" }, "Pilote l'éclairage de la carte mère avec l'effet OpenGear"),
          qa("Arc-en-ciel", "aura-quick", { mode: "Rainbow" }),
          qa("Éteindre", "aura-quick", { mode: "Off" }),
        ],
      });
    }
    if (S.cooling.channels.length) {
      const c = S.cooling;
      out.push({
        key: "cooling", vendor: "Refroidissement", kind: "Cooling", name: "Ventilation & watercooling", type: `${c.channels.length} canaux`,
        img: "/assets/aio_silentloop3.jpg", icon: "fan",
        status: c.is_admin ? { label: c.wmi_available ? "Pilotable" : "Lecture seule", cls: c.wmi_available ? "emerald" : "amber" } : { label: "Admin requis", cls: "amber" },
        quick: [qa("Silence", "sl3-preset", { preset: "ultra_silent" }), qa("Équilibré", "sl3-preset", { preset: "default" }), qa("Performance", "sl3-preset", { preset: "max_perf" })],
      });
    }
    const ram = S.ram_rgb;
    if (ram && (arr(ram.physical_dimms).length || arr(ram.ene_modules).length)) {
      const ene = arr(ram.ene_modules).length;
      const total = arr(ram.physical_dimms).reduce((a, d) => a + num(d.capacity_gb), 0);
      out.push({
        key: "ram", vendor: "Mémoire", kind: "Ram", name: "Mémoire RAM", type: `${plural(arr(ram.physical_dimms).length, "barrette", "barrettes")}${total ? ` · ${total} Go` : ""}`, icon: "ram",
        status: ene ? { label: "RGB détecté", cls: "emerald" } : { label: ram.is_admin ? "Sans RGB pilotable" : "Admin requis", cls: "amber" },
        quick: ene ? [qa("Couleur de la carte mère", "ram-sync-aura", {}), qa("Éteindre", "ram-off", {})] : [],
      });
    }
    return out;
  }

  // ==================== CHROME (BARRE LATÉRALE, STATUT) ====================
  function updateChrome() {
    if (!S) return;
    const verEl = document.getElementById("app-version");
    if (verEl && S.app_version) verEl.textContent = `OpenGear v${S.app_version} · aucune télémétrie`;
    document.querySelectorAll(".nav-item").forEach((b) => {
      const on = b.dataset.tab === tab;
      b.classList.toggle("active", on);
      if (on) b.setAttribute("aria-current", "page");
      else b.removeAttribute("aria-current");
    });
    const devCount = devicesList().length;
    const bd = document.getElementById("badge-devices");
    if (bd) {
      bd.textContent = devCount ? String(devCount) : "";
      bd.className = "nav-badge" + (devCount ? " ok" : "");
    }
    const bs = document.getElementById("badge-system");
    if (bs) {
      const checks = arr(diag.data && diag.data.checks);
      const errs = checks.filter((c) => c.status === "error").length;
      const warns = checks.filter((c) => c.status === "warn").length;
      bs.textContent = errs ? String(errs) : warns ? String(warns) : "";
      bs.className = "nav-badge" + (errs ? " err" : warns ? " warn" : "");
      bs.title = errs ? `${errs} erreur(s) au diagnostic` : warns ? `${warns} avertissement(s) au diagnostic` : "";
    }
    const sel = document.getElementById("profile-select");
    if (sel && document.activeElement !== sel) {
      sel.innerHTML = S.config.profiles
        .map((n) => `<option value="${esc(n)}" ${n === S.config.active_profile ? "selected" : ""}>${esc(n)}</option>`)
        .join("");
    }
    const mode = ui().mode;
    const ms = document.getElementById("mode-simple");
    const me = document.getElementById("mode-expert");
    if (ms) ms.setAttribute("aria-pressed", String(mode === "simple"));
    if (me) me.setAttribute("aria-pressed", String(mode === "expert"));
    const mh = document.getElementById("mode-hint");
    if (mh) mh.textContent = mode === "simple" ? "Réglages avancés masqués." : "Tous les réglages sont visibles.";
    const u = ui();
    const ub = document.getElementById("btn-undo");
    if (ub) {
      ub.hidden = !u.can_undo;
      ub.textContent = u.undo_label ? `Annuler : ${u.undo_label}` : "Annuler la dernière modification";
      ub.title = ub.textContent;
    }
  }

  // ==================== ROUTEUR ====================
  let lastRenderKey = "";
  function focusSignature(main) {
    const a = document.activeElement;
    if (!a || !main.contains(a) || a === main) return null;
    try {
      if (a.id) return "#" + CSS.escape(a.id);
      const parts = Object.keys(a.dataset)
        .filter((k) => k !== "busy")
        .map((k) => `[data-${kebab(k)}="${CSS.escape(a.dataset[k])}"]`);
      return parts.length ? a.tagName.toLowerCase() + parts.join("") : null;
    } catch (_) {
      return null;
    }
  }

  function render() {
    if (!S) return;
    const main = document.getElementById("main-content");
    const sig = focusSignature(main);
    const key = tab + "|" + (sub[tab] || "") + "|" + (tab === "devices" ? deviceKey : "");
    const scroll = main.scrollTop;
    let html = "";
    try {
      switch (tab) {
        case "home": html = viewHome(); break;
        case "lighting": html = viewLighting(); break;
        case "performance": html = viewPerformance(); break;
        case "input": html = viewInput(); break;
        case "profiles": html = viewProfiles(); break;
        case "devices": html = viewDevices(); break;
        case "system": html = viewSystem(); break;
        default: tab = "home"; html = viewHome();
      }
    } catch (e) {
      console.error(e);
      html = `<div class="alert err"><span class="alert-icon">${ICONS.warn}</span><div><div class="alert-title">Cette page n'a pas pu s'afficher</div><div class="muted">${esc(e && e.message)}</div></div></div>`;
    }
    main.innerHTML = html;
    main.scrollTop = key === lastRenderKey ? scroll : 0;
    lastRenderKey = key;
    pendingRender = false;
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
    lastPreviewSig = "";
    renderFanCurve();
    document.querySelectorAll("[data-eq-svg]").forEach((svg) => renderEqSvg(Number(svg.dataset.eqSvg)));
    updateTelemetryDom(S.telemetry);
  }

  function go(newTab, opts = {}) {
    tab = newTab;
    if (opts.sub && sub[newTab] !== undefined) sub[newTab] = opts.sub;
    if (opts.device) deviceKey = opts.device;
    updateChrome();
    render();
    if (newTab === "system" && sub.system === "diag" && !diag.loading) loadDiagnostics();
    if (newTab === "system" && sub.system === "backups" && !backups.loading && !backups.data) loadBackups();
    const main = document.getElementById("main-content");
    if (main && !opts.keepFocus) main.focus({ preventScroll: true });
  }

  // ==================== FRAGMENTS HTML PARTAGÉS ====================
  function pageHeader(title, subHtml, actionsHtml = "", accent = "") {
    return `
      <header class="page-header">
        <div class="page-title-wrap">
          <div class="page-bar ${accent}"></div>
          <div><h1 class="page-title">${esc(title)}</h1>${subHtml ? `<div class="page-sub">${subHtml}</div>` : ""}</div>
        </div>
        ${actionsHtml ? `<div class="header-actions">${actionsHtml}</div>` : ""}
      </header>`;
  }

  function subtabs(group, items, current) {
    return `<div class="subtabs" role="tablist" aria-label="Sections">${items
      .map((it) => `<button type="button" role="tab" class="subtab ${it.id === current ? "active" : ""}" aria-selected="${it.id === current}" data-act="subtab" data-group="${group}" data-id="${it.id}">${esc(it.label)}${it.badge ? `<span class="subtab-badge">${esc(it.badge)}</span>` : ""}</button>`)
      .join("")}</div>`;
  }

  function emptyState(title, text, actionHtml = "") {
    return `
      <div class="empty">
        <div class="empty-icon">${ICONS.plug}</div>
        <div class="empty-title">${esc(title)}</div>
        <div class="empty-text">${esc(text)}</div>
        ${actionHtml ? `<div class="row mt-8" style="justify-content:center">${actionHtml}</div>` : ""}
      </div>`;
  }

  function advancedHint(text) {
    if (isExpert()) return "";
    return `
      <div class="adv-hint">${ICONS.lock}
        <span>${esc(text || "Des réglages avancés sont masqués en mode Simple.")}</span>
        <button type="button" class="btn btn-sm btn-ghost" data-act="set-mode" data-mode="expert">Afficher le mode Expert</button>
      </div>`;
  }

  const expertTag = () => `<span class="expert-tag">Expert</span>`;

  function sw(checked, labelHtml, change, data = {}, opts = {}) {
    return `
      <label class="switch">
        <input type="checkbox" role="switch" ${checked ? "checked" : ""} ${opts.disabled ? "disabled" : ""} data-change="${change}" ${dataAttrs(data)} ${opts.id ? `id="${opts.id}"` : ""} />
        <span class="switch-track"></span>
        <span>${labelHtml}</span>
      </label>`;
  }

  function seg(items, current, act, data = {}, cls = "") {
    return `<div class="seg-group" role="group">${items
      .map(([val, label]) => `<button type="button" class="seg-btn ${cls} ${String(val) === String(current) ? "active" : ""}" aria-pressed="${String(val) === String(current)}" data-act="${act}" ${dataAttrs(data)} data-val="${esc(val)}">${esc(label)}</button>`)
      .join("")}</div>`;
  }

  function swatches(act, current, data = {}) {
    const cur = String(current || "").toUpperCase();
    return `<div class="swatch-row" role="group" aria-label="Couleurs rapides">${SWATCHES.map(
      ([n, hex]) => `<button type="button" class="swatch ${cur === hex ? "active" : ""}" style="background:${hex}" title="${esc(n)}" aria-label="${esc(n)}" data-act="${act}" ${dataAttrs(data)} data-color="${hex}"></button>`
    ).join("")}</div>`;
  }

  function sliderRow(label, { id, min, max, step = 1, value, input, change, data = {}, display }) {
    return `
      <div class="slider-row">
        <label class="field-label" for="${id}">${esc(label)}</label>
        <input type="range" class="og-range" id="${id}" min="${min}" max="${max}" step="${step}" value="${esc(value)}" ${input ? `data-input="${input}"` : ""} ${change ? `data-change="${change}"` : ""} ${dataAttrs(data)} data-label="${id}-val" />
        <span class="val" id="${id}-val">${esc(display != null ? display : value)}</span>
      </div>`;
  }

  function batteryBadge(pct, charging) {
    const p = num(pct, -1);
    if (p < 0) return "";
    const cls = p <= 15 ? "low" : p <= 35 ? "mid" : "";
    return `<span class="battery ${cls}" title="Batterie ${p} %${charging ? " (en charge)" : ""}">${ICONS.battery}<span>${p} %${charging ? " · en charge" : ""}</span></span>`;
  }

  function deviceVisual(d, cls = "") {
    if (d.img) return `<img src="${esc(d.img)}" class="${cls} ${d.imgCls || ""}" alt="" />`;
    return `<span class="glyph">${ICONS[d.icon] || ICONS.chip}</span>`;
  }

  function quickBtn(q) {
    return `<button type="button" class="btn btn-sm btn-secondary" data-act="${q.act}" ${dataAttrs(q.data)} ${q.title ? `title="${esc(q.title)}"` : ""}>${esc(q.label)}</button>`;
  }

  // ==================== TÉLÉMÉTRIE ====================
  function telemetryTiles() {
    return `
      <div class="tel-grid">
        <div class="tel-tile"><div class="card-eyebrow">Processeur</div><div class="tel-val" id="tel-cpu-val">—</div><div class="tel-sub" id="tel-cpu-sub">${esc(S.static_info.cpu_name || "")}</div><div class="bar-track"><div class="bar-fill" id="tel-cpu-bar" style="width:0;background:var(--cyan)"></div></div></div>
        <div class="tel-tile"><div class="card-eyebrow">Carte graphique</div><div class="tel-val" id="tel-gpu-val" style="color:#ff8aa2">—</div><div class="tel-sub" id="tel-gpu-sub">${esc(S.static_info.gpu_name || "")}</div><div class="bar-track"><div class="bar-fill" id="tel-gpu-bar" style="width:0;background:var(--rog)"></div></div></div>
        <div class="tel-tile"><div class="card-eyebrow">Mémoire vive</div><div class="tel-val" id="tel-ram-val">—</div><div class="tel-sub" id="tel-ram-sub"></div><div class="bar-track"><div class="bar-fill" id="tel-ram-bar" style="width:0;background:var(--purple)"></div></div></div>
        <div class="tel-tile"><div class="card-eyebrow">Températures</div><div class="tel-val" id="tel-temp-val" style="color:var(--amber)">—</div><div class="tel-sub" id="tel-temp-sub"></div><div class="bar-track"><div class="bar-fill" id="tel-temp-bar" style="width:0;background:var(--amber)"></div></div></div>
      </div>`;
  }

  function setTxt(id, txt) {
    const el = document.getElementById(id);
    if (el) el.textContent = txt;
  }
  function setBar(id, pct) {
    const el = document.getElementById(id);
    if (el) el.style.width = `${clamp(num(pct), 0, 100)}%`;
  }

  function updateTelemetryDom(tel) {
    if (!tel) return;
    const sensors = tel.sensors || {};
    const gpu = tel.gpu || {};
    const cpuPct = num(tel.cpu_usage_pct);
    const cpuT = sensors.cpu_temp_c;
    const gpuT = gpu.temp_c;
    const ramUsed = num(tel.ram_used_gb);
    const ramTot = num(tel.ram_total_gb);
    const mini = document.getElementById("footer-tel-mini");
    if (mini) {
      const parts = [`CPU ${Math.round(cpuPct)} %${cpuT != null ? ` · ${Math.round(cpuT)} °C` : ""}`];
      if (gpuT != null) parts.push(`GPU ${Math.round(gpuT)} °C`);
      if (sensors.aio_pump_rpm != null) parts.push(`Pompe ${sensors.aio_pump_rpm} tr/min`);
      parts.push(`RAM ${ramUsed.toFixed(1)} Go`);
      mini.textContent = parts.join("  ·  ");
    }
    if (document.getElementById("tel-cpu-val")) {
      setTxt("tel-cpu-val", `${Math.round(cpuPct)} %`);
      setTxt("tel-cpu-sub", `${cpuT != null ? `${Math.round(cpuT)} °C · ` : ""}${S.static_info.cpu_name || "Charge processeur"}`);
      setBar("tel-cpu-bar", cpuPct);
      if (gpu.available && gpuT != null) {
        setTxt("tel-gpu-val", `${Math.round(gpuT)} °C`);
        const extra = gpu.power_watts != null ? ` · ${Math.round(gpu.power_watts)} W` : gpu.core_clock_mhz ? ` · ${gpu.core_clock_mhz} MHz` : "";
        setTxt("tel-gpu-sub", `Charge ${num(gpu.activity_pct)} %${extra}`);
        setBar("tel-gpu-bar", (gpuT / 95) * 100);
      } else {
        setTxt("tel-gpu-val", "—");
        setTxt("tel-gpu-sub", "Capteur GPU indisponible");
      }
      setTxt("tel-ram-val", `${ramUsed.toFixed(1)} Go`);
      setTxt("tel-ram-sub", ramTot ? `sur ${Math.round(ramTot)} Go (${Math.round(num(tel.ram_usage_pct))} %)` : "");
      setBar("tel-ram-bar", tel.ram_usage_pct);
      const hot = [cpuT, sensors.motherboard_temp_c, gpuT].filter((x) => x != null);
      if (hot.length) {
        const mx = Math.max(...hot);
        setTxt("tel-temp-val", `${Math.round(mx)} °C`);
        const sub2 = [];
        if (sensors.motherboard_temp_c != null) sub2.push(`Carte mère ${Math.round(sensors.motherboard_temp_c)} °C`);
        if (sensors.aio_pump_rpm != null) sub2.push(`Pompe ${sensors.aio_pump_rpm} tr/min`);
        setTxt("tel-temp-sub", sub2.join(" · ") || "Composant le plus chaud");
        setBar("tel-temp-bar", (mx / 95) * 100);
      } else {
        setTxt("tel-temp-val", "—");
        setTxt("tel-temp-sub", "Capteurs de température indisponibles");
      }
    }
    const live = document.getElementById("cool-live");
    if (live) live.innerHTML = sensorChips(tel);
    const audioOut = document.getElementById("wa-out-state");
    if (audioOut && tel.audio) audioOut.textContent = tel.audio.output_muted ? "Muet" : `${num(tel.audio.output_volume_pct)} %`;
  }

  function sensorChips(tel) {
    const s = (tel && tel.sensors) || {};
    const chips = [];
    if (s.cpu_temp_c != null) chips.push(`<div class="sensor-chip">Processeur <strong>${num(s.cpu_temp_c).toFixed(1)} °C</strong></div>`);
    if (tel && tel.gpu && tel.gpu.temp_c != null) chips.push(`<div class="sensor-chip">Carte graphique <strong>${Math.round(tel.gpu.temp_c)} °C</strong></div>`);
    if (s.motherboard_temp_c != null) chips.push(`<div class="sensor-chip">Carte mère <strong>${num(s.motherboard_temp_c).toFixed(1)} °C</strong></div>`);
    if (s.aio_pump_rpm != null) chips.push(`<div class="sensor-chip">Pompe <strong>${s.aio_pump_rpm} tr/min</strong></div>`);
    arr(s.fans).forEach((f) => chips.push(`<div class="sensor-chip">${esc(f.name)} <strong>${num(f.rpm)} tr/min</strong></div>`));
    return chips.length ? chips.join("") : `<span class="muted">Aucun capteur de vitesse disponible. Lance LibreHardwareMonitor en arrière-plan ou installe PawnIO (mode Expert) pour voir les vitesses ici.</span>`;
  }

  // ==================== 1. ACCUEIL ====================
  function viewHome() {
    const si = S.static_info;
    const devs = devicesList();
    const checks = arr(diag.data && diag.data.checks);
    const alerts = checks.filter((c) => c.status === "error" || c.status === "warn");
    return `
      ${pageHeader("Accueil", esc([si.motherboard, si.cpu_name].filter(Boolean).join(" · ")),
        `<span class="pill emerald"><span class="dot"></span>${devs.length ? plural(devs.length, "appareil connecté", "appareils connectés") : "Aucun appareil"}</span>`)}
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

  function alertBanner(alerts) {
    const errs = alerts.filter((a) => a.status === "error");
    const sorted = [...errs, ...alerts.filter((a) => a.status !== "error")];
    const cls = errs.length ? "err" : "warn";
    const title = errs.length
      ? `${plural(errs.length, "problème détecté", "problèmes détectés")}${alerts.length > errs.length ? ` et ${plural(alerts.length - errs.length, "avertissement", "avertissements")}` : ""}`
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

  function presetCard(p, opts = {}) {
    return `
      <button type="button" class="preset-card" data-act="${opts.act || "apply-preset"}" data-preset="${esc(p.id)}" ${opts.pressed != null ? `aria-pressed="${opts.pressed}"` : ""}>
        <span class="preset-icon">${PRESET_ICONS[p.id] || ICONS.spark}</span>
        <span class="preset-label">${esc(p.label)}</span>
        <span class="preset-desc">${esc(p.description)}</span>
      </button>`;
  }

  function deviceCard(d) {
    return `
      <article class="device-card" aria-label="${esc(d.name)}">
        <div class="dc-top">
          <span class="pill ${d.status.cls}"><span class="dot"></span>${esc(d.status.label)}</span>
          ${d.battery != null ? batteryBadge(d.battery, d.charging) : ""}
        </div>
        <button type="button" class="dc-thumb" data-act="open-device" data-key="${esc(d.key)}" tabindex="-1" aria-hidden="true">${deviceVisual(d)}</button>
        <div class="dc-name" title="${esc(d.name)}">${esc(d.name)}</div>
        <div class="dc-meta">${esc(d.type)}</div>
        ${d.quick.length ? `<div class="dc-quick">${d.quick.slice(0, 3).map(quickBtn).join("")}</div>` : ""}
        <button type="button" class="btn btn-sm btn-secondary dc-more" data-act="open-device" data-key="${esc(d.key)}">Plus de réglages ${ICONS.chev}</button>
      </article>`;
  }

  // ==================== 2. ÉCLAIRAGE ====================
  function viewLighting() {
    const items = [
      { id: "global", label: "Effet global" },
      { id: "asus", label: "Carte mère" },
      { id: "ram", label: "Mémoire RAM" },
      { id: "aio", label: "Watercooling" },
      { id: "logi", label: "Logitech" },
    ];
    const cur = sub.lighting;
    let body = "";
    if (cur === "asus") body = lightingAsus();
    else if (cur === "ram") body = cardRam();
    else if (cur === "aio") body = cardSl3Argb();
    else if (cur === "logi") body = lightingLogi();
    else body = lightingGlobal();
    return `
      ${pageHeader("Éclairage", "Un effet pour tout le setup, puis des réglages par zone.", "", "")}
      ${subtabs("lighting", items, cur)}
      ${body}`;
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
          <input type="color" class="og-color" id="rgb-primary" value="${safeHex(rgb.primary)}" data-input="rgb-live" data-field="primary" />
          ${swatches("rgb-swatch", rgb.primary)}
        </div>
        <div class="color-row mt-12">
          <label class="field-label" for="rgb-secondary">Couleur secondaire</label>
          <input type="color" class="og-color" id="rgb-secondary" value="${safeHex(rgb.secondary, "#9B3CFF")}" data-input="rgb-live" data-field="secondary" />
          <label class="mini-check"><input type="checkbox" ${rgb.reverse ? "checked" : ""} data-change="rgb-reverse" /> Inverser le sens de l'animation</label>
        </div>
        <div class="stack mt-16" style="gap:10px">
          ${sliderRow("Luminosité", { id: "rgb-brightness", min: 0, max: 1, step: 0.05, value: br, input: "rgb-live", data: { field: "brightness", fmt: "pct" }, display: `${Math.round(br * 100)} %` })}
          ${sliderRow("Vitesse", { id: "rgb-speed", min: 0.2, max: 4.5, step: 0.1, value: sp, input: "rgb-live", data: { field: "speed", fmt: "x" }, display: `${sp.toFixed(1)}×` })}
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
              <button type="button" class="btn btn-sm ${paintMode ? "btn-primary" : "btn-secondary"}" data-act="paint-toggle" aria-pressed="${paintMode}">${paintMode ? "Pinceau actif" : "Activer le pinceau"}</button>
              <input type="color" class="og-color" id="paint-color" value="${safeHex(paintColor)}" data-input="paint-color" aria-label="Couleur du pinceau" />
              <button type="button" class="btn btn-sm btn-secondary" data-act="clear-per-key">Effacer la peinture</button>
            </div>
          </div>
          ${renderKeyboard("paint", d)}
          <div class="stack mt-16" style="gap:10px">
            ${sliderRow("Luminosité matérielle", { id: `cs-bright-${i}`, min: 0, max: 100, value: num(d.hw_brightness_pct, 100), input: "label", change: "cs-bright", data: { dev: i, fmt: "pct100" }, display: `${num(d.hw_brightness_pct, 100)} %` })}
          </div>
        </div>`).join("") : ""}`;
  }

  // Rendu du clavier K70 TKL (mode "paint" = éclairage, "macro" = remappage).
  function renderKeyboard(mode, kb) {
    const az = !!S.config.use_azerty_labels;
    const bindings = S.config.profile.bindings || {};
    const maxX = 18.8;
    const maxY = 7.6;
    const photo = k70Image();
    const showPhoto = mode === "paint" && kbViewMode === "photo";
    if (!S.keys_layout.length) return `<div class="note">Disposition du clavier indisponible.</div>`;
    return `
      <div class="kb-chassis">
        <div class="kb-topbar">
          <div class="kb-brand-strip">
            <span>${esc(((kb && kb.product_name) || "Clavier").toUpperCase())}</span>
            ${kb && kb.pollrate_hz ? `<span class="pill emerald"><span class="dot"></span>${fmtHz(kb.pollrate_hz)}</span>` : ""}
            ${kb && kb.esports_switch_on ? `<span class="pill rog"><span class="dot"></span>Mode tournoi</span>` : ""}
          </div>
          ${mode === "paint"
            ? seg([["interactive", "Vue interactive"], ["photo", "Photo"]], kbViewMode, "kb-view")
            : `<span class="muted">Clique sur une touche pour la configurer. Point orange = touche modifiée.</span>`}
        </div>
        ${showPhoto
          ? `<div style="padding:8px 0"><img class="kb-real-photo ${photo.official ? "cutout" : ""}" src="${esc(photo.url)}" alt="Photo du clavier" /></div>`
          : `<div class="kb-canvas-wrap" role="group" aria-label="Touches du clavier">
              ${S.keys_layout.map((k) => {
                const left = (num(k.x) / maxX) * 100;
                const top = (num(k.y) / maxY) * 100;
                const width = ((num(k.w, 1) - 0.1) / maxX) * 100;
                const height = ((num(k.h, 1) - 0.12) / maxY) * 100;
                const label = az ? k.azerty : k.qwerty;
                const isSel = selectedKeyId === k.id;
                const isBound = !!bindings[k.id];
                return `<button type="button" class="kb-key ${isSel ? "selected" : ""} ${isBound ? "bound" : ""}" data-act="kb-key" data-key-id="${esc(k.id)}" data-led="${num(k.led_index)}" data-nx="${num(k.norm_x)}" data-ny="${num(k.norm_y)}" style="left:${left.toFixed(2)}%;top:${top.toFixed(2)}%;width:${width.toFixed(2)}%;height:${height.toFixed(2)}%" aria-label="Touche ${esc(label)}${isBound ? " (modifiée)" : ""}" ${mode === "macro" ? `aria-pressed="${isSel}"` : ""}>${esc(label)}</button>`;
              }).join("")}
            </div>`}
      </div>`;
  }

  function lightingAsus() {
    if (!S.asus) {
      return emptyState("Carte mère ASUS non détectée", `${S.asus_err ? S.asus_err + " — " : ""}Seules les cartes mères ASUS équipées d'un contrôleur Aura USB sont prises en charge. Si ta carte est compatible, vérifie qu'Armoury Crate n'est pas en train de l'utiliser puis réessaie.`,
        `<button type="button" class="btn btn-primary" data-act="rescan">Réessayer la connexion</button>`);
    }
    const a = S.asus;
    const direct = !!S.config.profile.sync_asus_aura;
    const mode = auraDraftMode || a.hw_mode;
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
              <img class="stage-img" src="/assets/motherboard_b650a.jpg" alt="Carte mère" />
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

  function cardRam() {
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
        <div class="hero-stage amber-bg"><img class="stage-img" src="/assets/aio_silentloop3.jpg" alt="Watercooling" style="max-height:250px" /></div>
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
            <input type="color" id="sl3-color" class="og-color" value="${safeHex(sl3Color, "#FF8C1A")}" />
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

  function cardLogiKbLighting(kb, i) {
    return `
      <div class="card accent-cyan">
        <div class="card-head"><div><h2 class="card-title">${esc(kb.name)}</h2><div class="card-desc">Éclairage du clavier</div></div>${kb.battery && kb.battery.percent != null ? batteryBadge(kb.battery.percent) : ""}</div>
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

  function cardLogiMouseLed(m, i) {
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

  // ==================== 3. PERFORMANCES ====================
  function viewPerformance() {
    const items = [
      { id: "mouse", label: "Souris & clavier" },
      { id: "cooling", label: "Ventilation" },
      { id: "power", label: "Alimentation" },
    ];
    const cur = sub.performance;
    let body;
    if (cur === "cooling") body = viewCooling();
    else if (cur === "power") body = cardPowerPlans();
    else body = perfInputs();
    return `
      ${pageHeader("Performances", "Précision de la souris, réactivité du clavier, ventilation et énergie.", "", "amber")}
      ${subtabs("performance", items, cur)}
      ${body}`;
  }

  function perfInputs() {
    const cards = [];
    S.logi_mice.forEach((m, i) => cards.push(cardLogiDpi(m, i)));
    S.corsair_devices.forEach((d, i) => {
      if (d.kind === "Mouse" && d.mouse) cards.push(cardCorsairMouse(d, i));
      else if (d.kind === "Keyboard" && d.pollrate_hz) cards.push(cardCorsairPoll(d, i));
    });
    if (!cards.length) {
      return emptyState("Aucune souris ni clavier réglable détecté", "Branche une souris Logitech G (récepteur LIGHTSPEED / Bolt ou câble) ou un périphérique Corsair compatible, puis relance la recherche.",
        `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`);
    }
    return `<div class="grid-2">${cards.join("")}</div>${advancedHint("Angle snapping et sensibilité X/Y séparée sont disponibles en mode Expert.")}`;
  }

  function cardLogiDpi(m, i) {
    const dpi = m.dpi;
    const rr = m.report_rate;
    const ex = isExpert();
    if (!dpi && !rr) return `<div class="card"><h2 class="card-title">${esc(m.name)}</h2><div class="muted mt-8">Cette souris n'expose pas de réglage de sensibilité.</div></div>`;
    const presets = dpi ? [400, 800, 1200, 1600, 3200, 6400].filter((v) => v >= num(dpi.min_dpi, 100) && v <= num(dpi.max_dpi, 25600)) : [];
    return `
      <div class="card accent-cyan">
        <div class="card-head">
          <div><h2 class="card-title">${esc(m.name)}</h2><div class="card-desc">Sensibilité et fréquence d'envoi</div></div>
          <span class="pill cyan">${dpi ? `${num(dpi.dpi_x)} DPI` : ""}${dpi && rr ? " · " : ""}${rr ? fmtHz(num(rr.current_hz, 1000)) : ""}</span>
        </div>
        ${dpi ? `
          <div class="row" role="group" aria-label="DPI rapides">
            ${presets.map((v) => `<button type="button" class="btn btn-sm ${num(dpi.dpi_x) === v ? "btn-primary" : "btn-secondary"}" data-act="logi-dpi" data-dev="${i}" data-dpi="${v}" aria-pressed="${num(dpi.dpi_x) === v}">${v}</button>`).join("")}
          </div>
          <div class="stack mt-12" style="gap:10px">
            ${sliderRow(ex && dpi.supports_independent_xy ? "DPI horizontal (X)" : "DPI précis", { id: `ldpi-${i}`, min: num(dpi.min_dpi, 100), max: num(dpi.max_dpi, 25600), step: num(dpi.step_dpi, 50) || 50, value: num(dpi.dpi_x, 800), input: "label", data: { fmt: "dpi" }, display: `${num(dpi.dpi_x, 800)} DPI` })}
            ${ex && dpi.supports_independent_xy ? sliderRow("DPI vertical (Y)", { id: `ldpiy-${i}`, min: num(dpi.min_dpi, 100), max: num(dpi.max_dpi, 25600), step: num(dpi.step_dpi, 50) || 50, value: num(dpi.dpi_y, dpi.dpi_x), input: "label", data: { fmt: "dpi" }, display: `${num(dpi.dpi_y, dpi.dpi_x)} DPI` }) : ""}
          </div>
          <div class="row mt-12"><button type="button" class="btn btn-sm btn-primary" data-act="logi-dpi-apply" data-dev="${i}">Appliquer le DPI précis</button></div>
          ${dpi.supports_lod ? `
            <div class="row-between mt-12">
              <span class="field-label">Distance de décrochage</span>
              ${seg([["Low", "Basse"], ["Medium", "Moyenne"], ["High", "Haute"]], dpi.lod, "logi-lod", { dev: i })}
            </div>` : ""}` : ""}
        ${rr && arr(rr.supported_hz).length ? `
          <div class="row-between mt-12">
            <span class="field-label">Fréquence d'envoi</span>
            ${seg(arr(rr.supported_hz).map((hz) => [hz, fmtHz(hz)]), rr.current_hz, "logi-hz", { dev: i })}
          </div>` : ""}
        ${ex && m.angle_snapping != null ? `<div class="mt-12">${sw(!!m.angle_snapping, `Correction d'angle (angle snapping) ${expertTag()}`, "logi-asnap", { dev: i })}</div>` : ""}
      </div>`;
  }

  function cardCorsairMouse(dev, i) {
    const m = dev.mouse;
    const stages = arr(m.dpi_stages);
    const active = num(m.active_stage);
    return `
      <div class="card accent-cyan">
        <div class="card-head">
          <div><h2 class="card-title">${esc(dev.product_name)}</h2><div class="card-desc">Paliers de sensibilité (bouton DPI de la souris)</div></div>
          <span class="pill cyan">${num(m.current_dpi_x, stages[active] || 0)} DPI</span>
        </div>
        <div class="stack" style="gap:8px">
          ${stages.map((v, s) => `
            <div class="row-nowrap">
              <button type="button" class="btn btn-sm ${active === s ? "btn-primary" : "btn-secondary"}" data-act="cm-stage" data-dev="${i}" data-stage="${s}" aria-pressed="${active === s}" style="min-width:96px">Palier ${s + 1}${active === s ? " (actif)" : ""}</button>
              <input type="number" class="og-input narrow" min="100" max="30000" step="50" value="${num(v)}" id="cm-dpi-${i}-${s}" aria-label="DPI du palier ${s + 1}" />
              <button type="button" class="btn btn-sm btn-secondary" data-act="cm-dpi-save" data-dev="${i}" data-stage="${s}">Enregistrer</button>
            </div>`).join("")}
        </div>
        ${dev.pollrate_hz ? `<div class="row-between mt-12"><span class="field-label">Fréquence d'envoi</span>${seg([125, 250, 500, 1000, 2000].map((hz) => [hz, fmtHz(hz)]), dev.pollrate_hz, "cs-hz", { dev: i })}</div>` : ""}
        ${m.lift_height != null ? `<div class="row-between mt-12"><span class="field-label">Distance de décrochage</span>${seg([[0, "Basse"], [1, "Moyenne"], [2, "Haute"]], m.lift_height, "cm-lod", { dev: i })}</div>` : ""}
      </div>`;
  }

  function cardCorsairPoll(dev, i) {
    const maxHz = num(dev.max_pollrate_hz, 8000) || 8000;
    const rates = [125, 250, 500, 1000, 2000, 4000, 8000].filter((hz) => hz <= maxHz);
    return `
      <div class="card accent-cyan">
        <div class="card-head">
          <div><h2 class="card-title">${esc(dev.product_name)}</h2><div class="card-desc">Fréquence d'envoi du clavier : plus elle est haute, plus la latence est faible (consommation CPU légèrement supérieure).</div></div>
          <span class="pill emerald">${fmtHz(dev.pollrate_hz)}</span>
        </div>
        ${seg(rates.map((hz) => [hz, fmtHz(hz)]), dev.pollrate_hz, "cs-hz", { dev: i })}
      </div>`;
  }

  function cardPowerPlans() {
    if (!S.power_plans.length) return emptyState("Aucun mode d'alimentation trouvé", "Windows n'a renvoyé aucun mode d'alimentation.");
    return `
      <div class="card">
        <div class="card-head"><div><h2 class="card-title">Mode d'alimentation Windows</h2><div class="card-desc">Choisis le compromis entre performances et consommation.</div></div></div>
        <div class="plan-grid">
          ${S.power_plans.map((p) => `
            <div class="plan-card ${p.active ? "active" : ""}">
              <div>
                <div style="font-weight:650;color:#fff">${esc(p.name)}</div>
                ${isExpert() ? `<div class="mono muted small" style="margin-top:3px">${esc(p.guid)}</div>` : ""}
              </div>
              <div>${p.active ? `<span class="pill cyan"><span class="dot"></span>Actif</span>` : `<button type="button" class="btn btn-sm btn-secondary" data-act="power-plan" data-guid="${esc(p.guid)}">Activer</button>`}</div>
            </div>`).join("")}
        </div>
      </div>`;
  }

  const POLICIES = [["Silent", "Silencieux"], ["Standard", "Standard"], ["Turbo", "Turbo"], ["FullSpeed", "Pleine vitesse"], ["Manual", "Courbe perso."]];

  function viewCooling() {
    const c = S.cooling;
    const ex = isExpert();
    if (!c.channels.length) return emptyState("Ventilation non pilotable", c.message || "Aucun canal de ventilation n'a été trouvé sur cette carte mère.");
    const ch = c.channels[selectedCoolingIdx] || c.channels[0];
    const pols = ex ? POLICIES : POLICIES.filter(([id]) => id !== "Manual");
    const sio = c.superio || { detected: false, fans: [], diagnostic: "" };
    return `
      ${!c.is_admin ? `
        <div class="alert warn">
          <span class="alert-icon">${ICONS.lock}</span>
          <div class="grow"><div class="alert-title">Droits administrateur nécessaires pour modifier la ventilation</div><div class="muted">Windows protège l'accès aux réglages de ventilation de la carte mère. Tu peux préparer tes réglages ici, puis relancer OpenGear en administrateur pour les appliquer.</div></div>
          <button type="button" class="btn btn-warn btn-sm" data-act="relaunch-admin">Relancer en administrateur</button>
        </div>` : ""}
      <div class="card mb-16">
        <div class="card-head"><div><h2 class="card-title">Capteurs en direct</h2>${ex ? `<div class="card-desc">${esc((S.telemetry.sensors && S.telemetry.sensors.source) || "")}</div>` : ""}</div></div>
        <div class="sensor-chip-row" id="cool-live">${sensorChips(S.telemetry)}</div>
      </div>
      <div class="grid-stage">
        <div class="stack">
          <div class="card accent-amber">
            <div class="card-head"><div><h2 class="card-title">Pompe du watercooling</h2><div class="card-desc">Un plancher de sécurité est toujours conservé.</div></div></div>
            <div class="stack" style="gap:8px">
              <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="ultra_silent">Silence (45–95 %)</button>
              <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="default">Équilibré (55–100 %)</button>
              <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="max_perf">Performance (75–100 %)</button>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="card-head">
            <div><h2 class="card-title">Ventilateurs</h2><div class="card-desc">Choisis un canal puis un mode.</div></div>
            ${ex && c.wmi_available ? `<button type="button" class="btn btn-sm btn-secondary" data-act="cool-read-bios">Relire le BIOS</button>` : ""}
          </div>
          ${seg(c.channels.map((x) => [x.index, x.short_label || x.label]), selectedCoolingIdx, "cool-ch", {}, "amber")}
          <div class="row-between mt-12">
            <div style="font-weight:650">${esc(ch.label)}</div>
            ${!ex && ch.policy === "Manual" ? `<span class="pill purple">Courbe personnalisée</span>` : ""}
          </div>
          <div class="mt-8">${seg(pols, ch.policy, "cool-pol")}</div>
          ${ex ? `
            <div class="row mt-12">
              <label class="field-label" for="cool-src">Température de référence</label>
              <select id="cool-src" class="og-select auto" data-change="cool-src">
                <option value="CpuBios" ${(ch.source || "CpuBios") === "CpuBios" ? "selected" : ""}>Processeur (régulation BIOS)</option>
                <option value="GpuLive" ${ch.source === "GpuLive" ? "selected" : ""}>Carte graphique (en direct)</option>
                <option value="MixCpuGpu" ${ch.source === "MixCpuGpu" ? "selected" : ""}>Le plus chaud des deux</option>
              </select>
            </div>
            <div class="stack mt-12">${sliderRow("Hystérésis anti-yoyo", { id: "cool-hyst", min: 0, max: 10, value: num(ch.hysteresis_c, 3), input: "label", change: "cool-hyst", data: { fmt: "deg" }, display: `${num(ch.hysteresis_c, 3)} °C` })}</div>
            <div class="curve-wrap mt-12"><svg id="fan-curve-svg" class="curve-svg" viewBox="0 0 680 250" role="img" aria-label="Courbe de ventilation"></svg></div>
            <div class="curve-pts" id="curve-pts">${curvePointsHtml(ch)}</div>
            <div class="row mt-16">
              <button type="button" class="btn btn-ok" data-act="cool-apply">Appliquer à la carte mère</button>
              <button type="button" class="btn btn-secondary" data-act="sl3-preset" data-preset="default">Réinitialiser la pompe</button>
            </div>` : `
            <div class="row mt-16">
              <button type="button" class="btn btn-ok" data-act="cool-apply">Appliquer à la carte mère</button>
            </div>`}
        </div>
      </div>
      ${ex ? `
        <div class="card expert-card mt-16">
          <div class="card-head">
            <div><h2 class="card-title">Contrôle direct des ventilateurs ${expertTag()}</h2><div class="card-desc">${esc(sio.diagnostic || "Pilote directement la puce de contrôle de la carte mère (SuperIO).")}</div></div>
            <div class="row">
              <span class="pill ${sio.detected ? "emerald" : "amber"}"><span class="dot"></span>${sio.detected ? esc(sio.chip_name) : c.is_admin ? "Pilote PawnIO absent" : "Administrateur requis"}</span>
              ${!sio.detected ? `<button type="button" class="btn btn-sm btn-warn" data-act="pawnio-install">Installer / réparer PawnIO</button>` : ""}
              <button type="button" class="btn btn-sm btn-secondary" data-act="sio-refresh">Actualiser</button>
            </div>
          </div>
          ${sio.detected && arr(sio.fans).length ? `
            <div class="stack" style="gap:8px">
              ${arr(sio.fans).map((f) => `
                <div class="list-row">
                  <div style="min-width:140px"><strong>${esc(f.label)}</strong><div class="muted small">${f.is_manual ? "Vitesse forcée" : "Automatique (BIOS)"}</div></div>
                  <span class="pill ${num(f.rpm) > 0 ? "amber" : ""}">${num(f.rpm)} tr/min</span>
                  <input type="range" class="og-range grow" min="0" max="100" value="${num(f.pwm_pct)}" id="sio-${num(f.index)}" data-input="label" data-label="sio-${num(f.index)}-val" data-fmt="pct100" aria-label="Vitesse ${esc(f.label)}" />
                  <span class="mono" id="sio-${num(f.index)}-val" style="width:44px;text-align:right">${num(f.pwm_pct)} %</span>
                  <button type="button" class="btn btn-sm btn-primary" data-act="sio-apply" data-ch="${num(f.index)}">Forcer</button>
                  <button type="button" class="btn btn-sm btn-secondary" data-act="sio-auto" data-ch="${num(f.index)}">Auto BIOS</button>
                </div>`).join("")}
            </div>` : ""}
        </div>` : advancedHint("Courbes 8 points, source de température et contrôle direct des ventilateurs sont en mode Expert.")}`;
  }

  function curvePointsHtml(ch) {
    return arr(ch.curve).map((pt, i) => `
      <div class="curve-pt">
        <span>P${i + 1}</span>
        <input type="number" min="20" max="100" value="${num(pt.temp_c)}" data-change="curve-pt" data-pt="${i}" data-field="temp_c" aria-label="Point ${i + 1} : température °C" />
        <input type="number" min="0" max="100" value="${num(pt.duty_pct)}" data-change="curve-pt" data-pt="${i}" data-field="duty_pct" aria-label="Point ${i + 1} : vitesse %" />
      </div>`).join("");
  }

  function renderFanCurve() {
    const svg = document.getElementById("fan-curve-svg");
    if (!svg || !S) return;
    const ch = S.cooling.channels[selectedCoolingIdx];
    if (!ch || arr(ch.curve).length < 2) return;
    const accent = ch.is_pump ? "#FFA53D" : "#00D4FF";
    const padL = 44, padR = 18, padT = 18, padB = 30, W = 680, H = 250;
    const pw = W - padL - padR, ph = H - padT - padB;
    const tx = (t) => padL + ((t - 20) / 80) * pw;
    const dy = (d) => padT + (1 - d / 100) * ph;
    let html = `<defs><linearGradient id="cg" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="${accent}" stop-opacity="0.32"/><stop offset="100%" stop-color="${accent}" stop-opacity="0.02"/></linearGradient></defs>`;
    for (const d of [0, 25, 50, 75, 100]) {
      const y = dy(d);
      html += `<line x1="${padL}" y1="${y}" x2="${W - padR}" y2="${y}" stroke="rgba(255,255,255,0.06)"/><text x="${padL - 8}" y="${y + 4}" fill="#8592ab" font-size="10.5" text-anchor="end" font-family="Consolas,monospace">${d}%</text>`;
    }
    for (let t = 20; t <= 100; t += 10) {
      const x = tx(t);
      html += `<line x1="${x}" y1="${padT}" x2="${x}" y2="${H - padB}" stroke="rgba(255,255,255,0.06)"/><text x="${x}" y="${H - 10}" fill="#8592ab" font-size="10.5" text-anchor="middle" font-family="Consolas,monospace">${t}°C</text>`;
    }
    if (ch.is_pump) {
      const fy = dy(40);
      html += `<rect x="${padL}" y="${fy}" width="${pw}" height="${H - padB - fy}" fill="rgba(255,61,99,0.08)"/><line x1="${padL}" y1="${fy}" x2="${W - padR}" y2="${fy}" stroke="rgba(255,61,99,0.45)" stroke-dasharray="4 4"/><text x="${padL + 8}" y="${fy + 15}" fill="rgba(255,150,170,0.9)" font-size="10.5">Plancher de sécurité de la pompe (40 % min.)</text>`;
    }
    const sensors = S.telemetry.sensors || {};
    const liveT = sensors.cpu_temp_c != null ? sensors.cpu_temp_c : S.telemetry.gpu && S.telemetry.gpu.temp_c;
    if (liveT != null) {
      const lx = tx(clamp(liveT, 20, 100));
      html += `<line x1="${lx}" y1="${padT}" x2="${lx}" y2="${H - padB}" stroke="#FF3D63" stroke-width="1.4" stroke-dasharray="3 3"/><text x="${lx + 5}" y="${padT + 12}" fill="#FF7a93" font-size="10.5" font-family="Consolas,monospace">${sensors.cpu_temp_c != null ? "CPU" : "GPU"} ${Math.round(liveT)}°C</text>`;
    }
    const pts = ch.curve.map((p) => [tx(num(p.temp_c, 50)), dy(num(p.duty_pct, 50))]);
    const poly = [[padL, pts[0][1]], ...pts, [W - padR, pts[pts.length - 1][1]]];
    const lineD = poly.map((p, i) => `${i === 0 ? "M" : "L"}${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(" ");
    html += `<path d="${lineD} L${W - padR},${H - padB} L${padL},${H - padB} Z" fill="url(#cg)"/><path d="${lineD}" fill="none" stroke="${accent}" stroke-width="2.6" stroke-linejoin="round"/>`;
    pts.forEach(([x, y], i) => {
      const p = ch.curve[i];
      html += `<g class="curve-node" data-pt="${i}" style="cursor:grab"><circle cx="${x}" cy="${y}" r="12" fill="transparent"/><circle cx="${x}" cy="${y}" r="6.5" fill="${accent}" stroke="#090b10" stroke-width="2"/><title>Point ${i + 1} : ${num(p.temp_c)} °C → ${num(p.duty_pct)} %</title></g>`;
    });
    svg.innerHTML = html;

    let dragIdx = -1;
    const n = ch.curve.length;
    const onMove = (e) => {
      if (dragIdx < 0) return;
      const r = svg.getBoundingClientRect();
      const sx = ((e.clientX - r.left) / r.width) * W;
      const sy = ((e.clientY - r.top) / r.height) * H;
      const minT = dragIdx === 0 ? 20 : num(ch.curve[dragIdx - 1].temp_c) + 2;
      const maxT = dragIdx === n - 1 ? 98 : num(ch.curve[dragIdx + 1].temp_c) - 2;
      const nt = clamp(Math.round(20 + ((sx - padL) / pw) * 80), minT, maxT);
      const nd = clamp(Math.round((1 - (sy - padT) / ph) * 100), ch.is_pump ? 40 : 0, 100);
      ch.curve[dragIdx] = { temp_c: nt, duty_pct: nd };
      ch.policy = "Manual";
      renderFanCurve();
      const strip = document.getElementById("curve-pts");
      if (strip) strip.innerHTML = curvePointsHtml(ch);
    };
    const onUp = () => {
      dragIdx = -1;
      curveDragging = false;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.querySelectorAll("[data-act='cool-pol']").forEach((b) => {
        const on = b.dataset.val === "Manual";
        b.classList.toggle("active", on);
        b.setAttribute("aria-pressed", String(on));
      });
    };
    svg.querySelectorAll(".curve-node").forEach((g) => {
      g.onpointerdown = (e) => {
        e.preventDefault();
        dragIdx = Number(g.dataset.pt);
        curveDragging = true;
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
      };
    });
  }

  // ==================== 4. TOUCHES & MACROS ====================
  function viewInput() {
    const prof = S.config.profile;
    const items = [
      { id: "keys", label: "Touches & macros" },
      { id: "esports", label: "Mode compétition" },
      { id: "mouse", label: "Boutons de souris" },
    ];
    const cur = sub.input;
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
          <div class="card-head"><h2 class="card-title">Touche sélectionnée</h2><span class="pill purple">${esc(selectedKeyId)}</span></div>
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
              : macroSteps.map((st, i) => `${i + 1}. ${st.down ? "Appui  " : "Relâche"} ${esc(st.key)} (${num(st.delay_ms)} ms)`).join("<br/>") || "Aucune étape pour l'instant."}
          </div>
          <button type="button" class="btn btn-primary btn-sm mt-12" data-act="bind-macro">Assigner la macro à ${esc(selectedKeyId)}</button>
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

  // ---- FlashTap (hardware SOCD inside Corsair keyboards) ----
  function flashtapCard() {
    const kbs = arr(S.corsair_devices).filter((d) => d.flashtap_supported);
    if (!kbs.length) return "";
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

  // ---- Opposing-keys tester: shows what Windows actually receives (after hardware / software SOCD) ----
  const SOCD_PADS = [["KeyA", "Gauche", "Q (AZERTY) / A (QWERTY)"], ["KeyD", "Droite", "D"], ["KeyW", "Avant", "Z (AZERTY) / W (QWERTY)"], ["KeyS", "Arrière", "S"]];
  const heldCodes = new Set();
  function refreshSocdPads() {
    document.querySelectorAll("[data-socd-pad]").forEach((el) => {
      const on = heldCodes.has(el.dataset.socdPad);
      el.classList.toggle("on", on);
      el.setAttribute("aria-pressed", String(on));
    });
  }
  let socdTesterBound = false;
  function bindSocdTester() {
    if (socdTesterBound) return;
    socdTesterBound = true;
    document.addEventListener("keydown", (e) => { if (SOCD_PADS.some(([c]) => c === e.code)) { heldCodes.add(e.code); refreshSocdPads(); } });
    document.addEventListener("keyup", (e) => { if (heldCodes.delete(e.code)) refreshSocdPads(); });
    window.addEventListener("blur", () => { heldCodes.clear(); refreshSocdPads(); });
  }
  function socdTesterCard() {
    bindSocdTester();
    return `
      <div class="card mb-16">
        <div class="card-head">
          <div>
            <h2 class="card-title">Testeur de touches opposées</h2>
            <div class="card-desc">Maintiens la touche Gauche, puis appuie sur Droite sans relâcher. Avec FlashTap ou Snap Tap actif, <strong>seule la dernière touche doit rester allumée</strong> ; relâche-la et la première doit se rallumer.</div>
          </div>
        </div>
        <div class="socd-pads" role="group" aria-label="Touches reçues par Windows">
          ${SOCD_PADS.map(([code, label, hint]) => `<div class="socd-pad ${heldCodes.has(code) ? "on" : ""}" data-socd-pad="${code}" aria-pressed="${heldCodes.has(code)}"><strong>${label}</strong><span class="muted">${hint}</span></div>`).join("")}
        </div>
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

  // Mémoire embarquée Logitech : profils, boutons, macros (écriture flash = mode Expert + confirmation).
  const MACRO_PRESETS = {
    double_click: { label: "Double clic gauche", steps: [
      { kind: "mouse_down", value: 1 }, { kind: "delay", value: 15 }, { kind: "mouse_up", value: 1 },
      { kind: "delay", value: 25 },
      { kind: "mouse_down", value: 1 }, { kind: "delay", value: 15 }, { kind: "mouse_up", value: 1 },
    ] },
    rapid_e: { label: "Double appui sur E", steps: [
      { kind: "key_down", value: 0x08 }, { kind: "delay", value: 20 }, { kind: "key_up", value: 0x08 },
      { kind: "delay", value: 30 },
      { kind: "key_down", value: 0x08 }, { kind: "delay", value: 20 }, { kind: "key_up", value: 0x08 },
    ] },
    jump_crouch: { label: "Saut + accroupi (Espace + Ctrl)", steps: [
      { kind: "key_down", value: 0x2c }, { kind: "delay", value: 25 }, { kind: "key_down", value: 0xe0 },
      { kind: "delay", value: 60 }, { kind: "key_up", value: 0xe0 }, { kind: "key_up", value: 0x2c },
    ] },
  };
  const BTN_NAMES = ["M1 · Clic gauche", "M2 · Clic droit", "M3 · Molette", "M4 · Arrière", "M5 · Avant"];

  function mouseStage(m, i, buttons, btnCount) {
    const imgInfo = mouseImage();
    const markers = arr(S.local_assets.mouse_markers);
    const sel = btnSel[i] || 0;
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
          ${imgInfo.variants.length > 1 ? `<div class="variant-bar">${imgInfo.variants.map((v, vi) => `<button type="button" class="variant-chip ${vi === selectedMouseVariant ? "active" : ""}" data-act="mouse-variant" data-idx="${vi}">${esc(String(v.label).replace(" (Officiel)", ""))}</button>`).join("")}</div>` : ""}
        </div>
      </div>`;
  }

  function cardLogiOnboard(m, i) {
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
    const sel = btnSel[i] || 0;
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

  // ==================== 5. PROFILS ====================
  function viewProfiles() {
    const prof = S.config.profile;
    const sp = S.system_prefs;
    const linked = arr(prof.linked_apps);
    const hw = [];
    S.corsair_devices.forEach((d, i) => {
      if (d.active_hw_profile != null) hw.push(`<div class="list-row"><div><strong>${esc(d.product_name)}</strong><div class="muted small">Profils enregistrés dans le clavier</div></div>${seg([0, 1, 2].map((n) => [n, `Profil ${n + 1}`]), d.active_hw_profile, "cs-hwprof", { dev: i })}</div>`);
    });
    S.logi_mice.forEach((m, i) => {
      const ob = m.onboard;
      const profiles = arr(m.onboard_profiles);
      if (ob && profiles.length) hw.push(`<div class="list-row"><div><strong>${esc(m.name)}</strong><div class="muted small">Profils enregistrés dans la souris</div></div>${seg(profiles.map((p, idx) => [num(p.slot_index, idx + 1), `Profil ${num(p.slot_index, idx + 1)}`]), ob.active_profile, "logi-activate-seg", { dev: i })}</div>`);
    });
    return `
      ${pageHeader("Profils", "Un profil regroupe éclairage, touches et réglages de jeu. Bascule à la main ou automatiquement selon le jeu lancé.",
        `<button type="button" class="btn btn-secondary btn-sm" data-act="save-config">Enregistrer sur le disque</button>
         <button type="button" class="btn btn-secondary btn-sm" data-act="export-profile">Exporter</button>
         <button type="button" class="btn btn-secondary btn-sm" data-act="import-profile">Importer…</button>`, "emerald")}
      ${S.presets.length ? `<section class="section"><h2 class="section-title">Préréglages en un clic</h2><div class="preset-strip">${S.presets.map((p) => presetCard(p)).join("")}</div></section>` : ""}
      <div class="grid-2">
        <div class="card">
          <div class="card-head"><h2 class="card-title">Mes profils</h2><span class="pill">${S.config.profiles.length}</span></div>
          <div class="list">
            ${S.config.profiles.map((n) => `
              <div class="list-row ${n === S.config.active_profile ? "active" : ""}">
                <strong>${esc(n)}</strong>
                ${n === S.config.active_profile ? `<span class="pill cyan"><span class="dot"></span>Actif</span>` : `<button type="button" class="btn btn-sm btn-secondary" data-act="switch-profile" data-name="${esc(n)}">Activer</button>`}
              </div>`).join("")}
          </div>
          <div class="row mt-12">
            <input id="new-profile" class="og-input grow" placeholder="Nom du nouveau profil" aria-label="Nom du nouveau profil" maxlength="60" />
            <button type="button" class="btn btn-primary btn-sm" data-act="create-profile">Créer</button>
          </div>
          <div class="muted small mt-8">Le nouveau profil reprend les réglages du profil actif.</div>
        </div>
        <div class="card accent-emerald">
          <div class="card-head">
            <div><h2 class="card-title">Bascule automatique par jeu</h2><div class="card-desc">Active « ${esc(prof.name || S.config.active_profile || "")} » dès qu'un de ces programmes est au premier plan.</div></div>
            ${sp.foreground_app ? `<span class="pill cyan">Au premier plan : ${esc(sp.foreground_app)}</span>` : ""}
          </div>
          ${sw(sp.auto_profile_switch !== false, "Bascule automatique activée", "sys-pref", { key: "auto_profile_switch" })}
          <div class="row mt-12">
            <input id="linked-apps" class="og-input mono grow" value="${esc(linked.join(", "))}" placeholder="ex. valorant.exe, r5apex.exe" aria-label="Programmes liés" />
            <button type="button" class="btn btn-sm btn-primary" data-act="linked-save">Enregistrer</button>
          </div>
          ${arr(sp.visible_apps).length ? `
            <div class="muted small mt-12" style="margin-bottom:6px">Programmes ouverts (clic pour les lier à ce profil) :</div>
            <div class="row" style="gap:6px">${arr(sp.visible_apps).slice(0, 10).map((a) => `<button type="button" class="btn btn-sm btn-secondary mono" data-act="link-app" data-app="${esc(a)}">+ ${esc(a)}</button>`).join("")}</div>` : ""}
        </div>
      </div>
      <div class="card mt-16">
        <div class="card-head"><div><h2 class="card-title">Profils enregistrés dans les appareils</h2><div class="card-desc">Certains appareils gardent leurs propres profils en mémoire, utilisables même sans OpenGear.</div></div></div>
        ${hw.length ? `<div class="list">${hw.join("")}</div>` : `<div class="muted">Aucun appareil connecté ne propose de profils internes.</div>`}
      </div>`;
  }

  // ==================== 6. APPAREILS ====================
  function viewDevices() {
    const devs = devicesList();
    if (!devs.length) {
      return `${pageHeader("Appareils", "Fiche détaillée de chaque appareil.")}
        ${emptyState("Aucun appareil détecté", "Branche ton clavier, ta souris, ton casque ou le récepteur sans fil : OpenGear les détecte automatiquement.", `<button type="button" class="btn btn-primary" data-act="rescan">Relancer la recherche</button>`)}`;
    }
    if (!devs.some((d) => d.key === deviceKey)) deviceKey = devs[0].key;
    const d = devs.find((x) => x.key === deviceKey);
    return `
      ${pageHeader("Appareils", "Fiche détaillée de chaque appareil et réglages propres à chaque marque.", `<button type="button" class="btn btn-sm btn-ghost" data-act="rescan">${ICONS.refresh}Rechercher des appareils</button>`)}
      <div class="dev-layout">
        <nav class="dev-list" aria-label="Liste des appareils">
          ${devs.map((x) => `
            <button type="button" class="dev-item ${x.key === deviceKey ? "active" : ""}" data-act="select-device" data-key="${esc(x.key)}" ${x.key === deviceKey ? 'aria-current="true"' : ""}>
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

  function infoTable(rows) {
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
    const hp = arr(dev.hwp_profiles)[0];
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
              <button type="button" class="btn btn-sm btn-secondary" data-act="cs-hwp-reload" data-dev="${i}">Relire le clavier</button>
            </div>
            <div class="row">
              <input id="hwp-name-${i}" class="og-input" style="width:160px" value="${esc(hp.name)}" aria-label="Nom du profil" />
              <select id="hwp-kind-${i}" class="og-select auto" aria-label="Effet">${S.hwp_lighting_kinds.map((k) => `<option value="${esc(k.id)}" ${l && l.kind === k.id ? "selected" : ""}>${esc(k.label)}</option>`).join("")}</select>
              <input type="color" id="hwp-c1-${i}" class="og-color" value="${safeHex(l && arr(l.colors)[0], "#1ECFDF")}" aria-label="Couleur 1" />
              <input type="color" id="hwp-c2-${i}" class="og-color" value="${safeHex(l && arr(l.colors)[1], "#9B3CFF")}" aria-label="Couleur 2" />
              <select id="hwp-speed-${i}" class="og-select auto" aria-label="Vitesse">${[[0, "Lent"], [1, "Moyen"], [2, "Rapide"]].map(([v, t]) => `<option value="${v}" ${num(l && l.speed, 1) === v ? "selected" : ""}>${t}</option>`).join("")}</select>
              <select id="hwp-dir-${i}" class="og-select auto" aria-label="Direction">${[[0, "Gauche"], [1, "Droite"], [2, "Haut"], [3, "Bas"], [4, "Horaire"], [5, "Anti-horaire"]].map(([v, t]) => `<option value="${v}" ${num(l && l.direction, 1) === v ? "selected" : ""}>${t}</option>`).join("")}</select>
            </div>
            <div class="row mt-12"><button type="button" class="btn btn-primary" data-act="cs-hwp-flash" data-dev="${i}">Écrire l'effet dans le clavier</button></div>
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
      ${hi.variants.length > 1 ? `<div class="row">${hi.variants.map((v, vi) => `<button type="button" class="variant-chip ${vi === selectedHeadsetVariant ? "active" : ""}" data-act="headset-variant" data-idx="${vi}">${esc(String(v.label).replace("PRO X ", ""))}</button>`).join("")}</div>` : ""}
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

  function renderEqSvg(i) {
    const svg = document.querySelector(`[data-eq-svg="${i}"]`);
    const e = eqDrafts[i];
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

  // ==================== 7. SYSTÈME ====================
  function viewSystem() {
    const ex = isExpert();
    const checks = arr(diag.data && diag.data.checks);
    const errs = checks.filter((c) => c.status === "error").length;
    const items = [{ id: "prefs", label: "Préférences" }, { id: "diag", label: "Diagnostic", badge: errs ? String(errs) : "" }];
    if (ex) items.push({ id: "backups", label: "Sauvegardes" });
    items.push({ id: "about", label: "À propos" });
    if (sub.system === "backups" && !ex) sub.system = "prefs";
    const cur = sub.system;
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
    return `
      <div class="grid-2">
        <div class="card">
          <div class="card-head"><h2 class="card-title">Interface</h2></div>
          <div class="row-between">
            <div><div style="font-weight:600">Mode d'affichage</div><div class="muted">Simple masque les réglages avancés ou risqués. Expert affiche tout.</div></div>
            ${seg([["simple", "Simple"], ["expert", "Expert"]], mode, "set-mode-seg")}
          </div>
          <div class="divider"></div>
          ${sw(!!S.config.use_azerty_labels, "Afficher les touches en AZERTY", "azerty")}
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
      </div>`;
  }

  const STATUS_LABEL = { ok: "OK", warn: "À vérifier", error: "Problème" };
  function sysDiag() {
    const ex = isExpert();
    const checks = arr(diag.data && diag.data.checks);
    const order = { error: 0, warn: 1, ok: 2 };
    const sorted = [...checks].sort((a, b) => (order[a.status] ?? 3) - (order[b.status] ?? 3));
    const count = (s) => checks.filter((c) => c.status === s).length;
    let list;
    if (diag.loading && !checks.length) list = `<div class="loading-inline"><div class="spinner sm"></div>Analyse en cours…</div>`;
    else if (diag.error) list = `<div class="alert warn"><span class="alert-icon">${ICONS.warn}</span><div><div class="alert-title">Diagnostic indisponible</div><div class="muted">${esc(diag.error)}</div></div></div>`;
    else if (!checks.length) list = `<div class="muted">Aucun résultat pour l'instant.</div>`;
    else list = `<div class="diag-list">${sorted.map((c, idx) => `
      <div class="diag-item ${esc(c.status)}">
        <span class="diag-dot ${esc(c.status)}" title="${esc(STATUS_LABEL[c.status] || c.status)}">${c.status === "ok" ? ICONS.check : c.status === "error" ? ICONS.x : ICONS.warn}</span>
        <div><div class="diag-label">${esc(c.label)} <span class="sr-only">(${esc(STATUS_LABEL[c.status] || c.status)})</span></div><div class="diag-detail">${esc(c.detail)}</div></div>
        <div>${c.action && c.action.cmd ? `<button type="button" class="btn btn-sm ${c.status === "error" ? "btn-primary" : "btn-secondary"}" data-act="diag-action" data-idx="${idx}">${esc(c.action.label || "Corriger")}</button>` : ""}</div>
      </div>`).join("")}</div>`;
    diagSorted = sorted;
    const st = S.selftest;
    const fws = S.firmware_inventory;
    const groups = ["ASUS", "Corsair", "Logitech", "Alienware"];
    return `
      <div class="card">
        <div class="card-head">
          <div><h2 class="card-title">État du système</h2><div class="card-desc">Vérifie que chaque composant d'OpenGear fonctionne et propose une correction quand c'est possible.</div></div>
          <div class="row">
            ${checks.length ? `<div class="diag-summary"><span class="pill emerald">${count("ok")} OK</span>${count("warn") ? `<span class="pill amber">${count("warn")} à vérifier</span>` : ""}${count("error") ? `<span class="pill rog">${plural(count("error"), "problème", "problèmes")}</span>` : ""}</div>` : ""}
            <button type="button" class="btn btn-sm btn-secondary" data-act="diag-refresh" ${diag.loading ? "disabled" : ""}>${ICONS.refresh}Relancer l'analyse</button>
            <button type="button" class="btn btn-sm btn-secondary" data-act="diag-report" title="Version, appareils, état et fin du journal — ton dossier utilisateur est masqué">Copier le rapport</button>
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
          ${groups.map((v) => {
            const list2 = S.devices.filter((x) => x.vendor === v);
            if (!list2.length) return "";
            return `<div class="card-eyebrow mt-12" style="margin-bottom:6px">${esc(v)} · ${list2.length}</div><div class="table-scroll"><table class="meta-table"><thead><tr><th>VID:PID</th><th>IF</th><th>UsagePage</th><th>Usage</th><th>Produit</th><th>Statut</th></tr></thead><tbody>
              ${list2.map((x) => `<tr><td class="mono">${esc(x.vid_pid)}</td><td class="mono">${esc(x.interface)}</td><td class="mono">${esc(x.usage_page)}</td><td class="mono">${esc(x.usage)}</td><td>${esc(x.product)}</td><td style="color:${x.active ? "var(--emerald)" : "var(--text-2)"}">${esc(x.status)}</td></tr>`).join("")}
            </tbody></table></div>`;
          }).join("")}
        </div>` : advancedHint("L'auto-test complet et la liste brute des interfaces USB/HID sont en mode Expert.")}`;
  }
  let diagSorted = [];

  const TAG_LABEL = { profil: "Profil", repertoire: "Répertoire", macro: "Macro" };
  function sysBackups() {
    const mice = S.logi_mice;
    const list = arr(backups.data && backups.data.backups).slice().sort((a, b) => num(b.created_ms) - num(a.created_ms));
    if (backups.mouse >= mice.length) backups.mouse = 0;
    let body;
    if (backups.loading && !backups.data) body = `<div class="loading-inline"><div class="spinner sm"></div>Lecture des sauvegardes…</div>`;
    else if (backups.error) body = `<div class="alert warn"><span class="alert-icon">${ICONS.warn}</span><div><div class="alert-title">Sauvegardes indisponibles</div><div class="muted">${esc(backups.error)}</div></div></div>`;
    else if (!list.length) body = emptyState("Aucune sauvegarde", "Une sauvegarde est créée automatiquement avant chaque écriture dans la mémoire d'une souris Logitech. Il n'y en a pas encore.");
    else body = `
      <div class="table-scroll"><table class="meta-table">
        <thead><tr><th>Date</th><th>Type</th><th>Secteur</th><th>Taille</th><th><span class="sr-only">Action</span></th></tr></thead>
        <tbody>${list.map((b) => `
          <tr>
            <td>${esc(fmtDate(b.created_ms))}</td>
            <td>${esc(TAG_LABEL[b.tag] || b.tag || "—")}</td>
            <td class="mono">${esc(b.sector_hex)}</td>
            <td>${esc(fmtSize(b.size))}</td>
            <td style="text-align:right"><button type="button" class="btn btn-sm btn-secondary" data-act="backup-restore" data-file="${esc(b.file)}" data-label="${esc(`${TAG_LABEL[b.tag] || b.tag || "Sauvegarde"} · secteur ${b.sector_hex} · ${fmtDate(b.created_ms)}`)}" ${mice.length ? "" : "disabled"}>Restaurer</button></td>
          </tr>`).join("")}</tbody>
      </table></div>`;
    return `
      <div class="card expert-card">
        <div class="card-head">
          <div><h2 class="card-title">Sauvegardes de la mémoire des souris ${expertTag()}</h2><div class="card-desc">Copies faites automatiquement avant chaque écriture de profil ou de macro.</div></div>
          <div class="row">
            ${mice.length > 1 ? `<label class="field-label" for="bk-mouse" style="min-width:0">Souris cible</label><select id="bk-mouse" class="og-select auto" data-change="bk-mouse">${mice.map((m, i) => `<option value="${i}" ${i === backups.mouse ? "selected" : ""}>${esc(m.name)}</option>`).join("")}</select>` : ""}
            <button type="button" class="btn btn-sm btn-secondary" data-act="backups-refresh">${ICONS.refresh}Actualiser</button>
          </div>
        </div>
        ${!mice.length ? `<div class="alert info"><span class="alert-icon">${ICONS.info}</span><div><div class="alert-title">Aucune souris Logitech détectée</div><div class="muted">Branche le récepteur LIGHTSPEED / Bolt ou le câble de la souris pour pouvoir restaurer une sauvegarde.</div></div></div>` : mice.length === 1 ? `<div class="muted mb-16">Restauration vers : <strong>${esc(mice[0].name)}</strong></div>` : ""}
        ${body}
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
          ${infoTable([["Version", "0.8"], ["Données personnelles", "Aucune télémétrie, rien n'est envoyé sur Internet"], ["Images officielles locales", [la.lghub_depot_found ? "G HUB" : "", la.icue_devices_found ? "iCUE" : ""].filter(Boolean).join(" + ") || "Non (images intégrées)"]])}
        </div>
        <div class="card">
          <div class="card-head"><h2 class="card-title">Ce PC</h2></div>
          ${infoTable([["Carte mère", si.motherboard], ["BIOS", si.bios_version], ["Processeur", si.cpu_name], ["Carte graphique", si.gpu_name]])}
        </div>
      </div>`;
  }

  // ==================== ASSISTANT DE PREMIER LANCEMENT ====================
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
    const devs = devicesList();
    const presets = S.presets;
    const step = wiz.step;
    const chosen = presets.find((p) => p.id === wiz.preset);
    let body = "";
    if (step === 0) {
      body = `
        <h2 class="wizard-title" id="wiz-title">Bienvenue dans OpenGear</h2>
        <p class="wizard-lead">${devs.length ? `OpenGear a trouvé ${plural(devs.length, "appareil", "appareils")} sur ce PC :` : "Aucun appareil compatible n'est branché pour l'instant."}</p>
        ${devs.length ? `<div class="wiz-dev-list">${devs.map((d) => `<div class="wiz-dev"><span class="glyph">${ICONS[d.icon] || ICONS.chip}</span><div style="min-width:0"><div class="dev-item-name">${esc(d.name)}</div><div class="dev-item-sub">${esc(d.vendor)} · ${esc(d.type)}</div></div></div>`).join("")}</div>`
          : `<div class="note">Pas d'inquiétude : branche ton matériel quand tu veux, il sera détecté automatiquement et apparaîtra sur l'accueil.</div>`}`;
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
          <div class="recap-row"><span class="muted">Appareils détectés</span><strong>${devs.length}</strong></div>
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
      if (!ok && !S.ui) uiModeOverride = mode;
      closeWizard();
      tab = "home";
      updateChrome();
      render();
      toast(preset ? "Tout est prêt : préréglage appliqué. Tu peux l'annuler depuis la barre d'état." : "Tout est prêt !", "ok");
    }
  }

  function skipWizard() {
    closeWizard();
    call("set_ui_prefs", { onboarding_done: true }, { silent: true, quietError: true });
  }

  // ==================== MODE SIMPLE / EXPERT ====================
  async function setMode(mode, btn) {
    if (mode !== "simple" && mode !== "expert" || mode === ui().mode) return;
    const prev = ui().mode;
    if (S.ui) S.ui.mode = mode;
    else uiModeOverride = mode;
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
        uiModeOverride = mode; // ancien moteur : préférence gardée pour cette session seulement
      } else {
        if (S.ui) S.ui.mode = prev;
        else uiModeOverride = prev;
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

  // ==================== DIAGNOSTIC & SAUVEGARDES ====================
  function softRender() {
    if (isEditing()) pendingRender = true;
    else render();
  }

  async function loadDiagnostics() {
    diag.loading = true;
    if (tab === "system" && sub.system === "diag") softRender();
    try {
      diag.data = await rpc("get_diagnostics", {});
      diag.error = null;
    } catch (e) {
      diag.error = humanError(e && e.message);
    } finally {
      diag.loading = false;
      updateChrome();
      if (tab === "home" || (tab === "system" && sub.system === "diag")) softRender();
    }
  }

  async function loadBackups() {
    backups.loading = true;
    if (tab === "system") softRender();
    try {
      backups.data = await rpc("list_backups", {});
      backups.error = null;
    } catch (e) {
      backups.error = humanError(e && e.message);
    } finally {
      backups.loading = false;
      if (tab === "system") softRender();
    }
  }

  // ==================== GESTIONNAIRES D'ÉVÉNEMENTS ====================
  const dev = (el) => Number(el.dataset.dev || 0);
  const val = (id) => {
    const el = document.getElementById(id);
    return el ? el.value : null;
  };
  const LABEL_FMT = {
    pct: (v) => `${Math.round(Number(v) * 100)} %`,
    x: (v) => `${Number(v).toFixed(1)}×`,
    pct100: (v) => `${v} %`,
    leds: (v) => `${v} LEDs`,
    dpi: (v) => `${v} DPI`,
    deg: (v) => `${v} °C`,
    ms: (v) => (Number(v) ? `${v} ms` : "Désactivé"),
    ramspeed: (v) => ["Très rapide", "Rapide", "Normal", "Lent", "Très lent"][Number(v)] || "Normal",
  };
  function updateLabel(el) {
    const lbl = el.dataset.label && document.getElementById(el.dataset.label);
    if (!lbl) return;
    const f = LABEL_FMT[el.dataset.fmt];
    lbl.textContent = f ? f(el.value) : el.value;
  }

  const throttles = {};
  function throttled(key, ms, fn) {
    const t = throttles[key] || (throttles[key] = { last: 0, timer: null, fn: null });
    t.fn = fn;
    const run = () => {
      t.last = Date.now();
      t.timer = null;
      const f = t.fn;
      t.fn = null;
      if (f) f();
    };
    const wait = ms - (Date.now() - t.last);
    if (wait <= 0) run();
    else if (!t.timer) t.timer = setTimeout(run, wait);
  }

  function getDraftFor(i) {
    const m = S.logi_mice[i];
    return m ? getDraft(m, i) : null;
  }

  function downloadText(filename, text) {
    const blob = new Blob([text], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  let lastClickTs = 0;

  const ACT = {
    // Navigation
    nav: (el) => go(el.dataset.tab, { sub: el.dataset.sub }),
    subtab: (el) => go(tab, { sub: el.dataset.id, keepFocus: true }),
    "set-mode": (el) => setMode(el.dataset.mode, el),
    "set-mode-seg": (el) => setMode(el.dataset.val, el),
    undo: (el) => doUndo(el),
    rescan: (el) => call("reconnect_all", {}, { btn: el, ok: "Recherche terminée : liste des appareils actualisée." }).then(() => loadDiagnostics()),
    "open-device": (el) => go("devices", { device: el.dataset.key }),
    "select-device": (el) => { deviceKey = el.dataset.key; render(); },
    "apply-preset": (el) => call("apply_preset", { preset: el.dataset.preset }, { btn: el }),
    "wizard-open": () => openWizard(),

    // Éclairage global
    "rgb-enable": (el) => call("set_rgb_config", { enable_rgb: el.dataset.on === "1" }, { btn: el, ok: el.dataset.on === "1" ? "Éclairage allumé" : "Éclairage coupé" }),
    "rgb-kind": (el) => call("set_rgb_config", { kind: el.dataset.kind, enable_rgb: true }, { btn: el, ok: `Effet : ${el.textContent.trim()}` }),
    "rgb-swatch": (el) => call("set_rgb_config", { primary: el.dataset.color }, { btn: el, ok: "Couleur appliquée" }),
    "paint-toggle": () => { paintMode = !paintMode; render(); },
    "clear-per-key": (el) => call("clear_per_key", {}, { btn: el, ok: "Peinture par touche effacée" }),
    "kb-view": (el) => { kbViewMode = el.dataset.val; render(); },
    "kb-key": (el) => {
      selectedKeyId = el.dataset.keyId;
      if (tab === "lighting" && paintMode) {
        call("set_per_key_color", { led_index: Number(el.dataset.led), color: safeHex(paintColor) }, { silent: true });
      } else {
        render();
      }
    },

    // Corsair
    "cs-bright-quick": (el) => call("corsair_set_brightness", { device_index: dev(el), percent: Number(el.dataset.pct) }, { btn: el, ok: `Luminosité : ${el.dataset.pct} %` }),
    "cs-winlock-quick": (el) => call("corsair_set_hw_winlock", { device_index: dev(el), enabled: el.dataset.on === "1" }, { btn: el, ok: el.dataset.on === "1" ? "Touche Windows bloquée" : "Touche Windows débloquée" }),
    "cm-stage": (el) => call("corsair_mouse_set_active_stage", { device_index: dev(el), stage: Number(el.dataset.stage) }, { btn: el, ok: `Palier DPI ${Number(el.dataset.stage) + 1} activé` }),
    "cm-dpi-save": (el) => {
      const v = Number(val(`cm-dpi-${dev(el)}-${el.dataset.stage}`));
      if (!Number.isFinite(v) || v < 100 || v > 30000) { toast("Le DPI doit être compris entre 100 et 30 000.", "err"); return; }
      call("corsair_mouse_set_dpi_stage", { device_index: dev(el), stage: Number(el.dataset.stage), dpi: v }, { btn: el, ok: `Palier ${Number(el.dataset.stage) + 1} : ${v} DPI` });
    },
    "cm-lod": (el) => call("corsair_mouse_set_lift_height", { device_index: dev(el), level: Number(el.dataset.val) }, { btn: el, ok: `Distance de décrochage : ${el.textContent.trim()}` }),
    "cs-hz": (el) => call("corsair_set_pollrate", { device_index: dev(el), hz: Number(el.dataset.val) }, { btn: el, ok: `Fréquence d'envoi : ${fmtHz(Number(el.dataset.val))}` }),
    "chs-side-quick": (el) => call("corsair_headset_set_sidetone", { device_index: dev(el), percent: Number(el.dataset.pct), muted: Number(el.dataset.pct) === 0 }, { btn: el, ok: Number(el.dataset.pct) ? `Retour micro : ${el.dataset.pct} %` : "Retour micro coupé" }),
    "cs-hwprof": (el) => call("corsair_set_hw_profile", { device_index: dev(el), index: Number(el.dataset.val) }, { btn: el, ok: `Profil interne ${Number(el.dataset.val) + 1} activé` }),
    "cs-mode": (el) => call("corsair_set_mode", { device_index: dev(el), mode: el.dataset.val }, { btn: el, ok: el.dataset.val === "sw" ? "Éclairage piloté par OpenGear" : "Le clavier gère son éclairage seul" }),
    "cs-deb-boost": (el) => call("corsair_set_debounce", { device_index: dev(el), enabled: true, timeout_ms: 2 }, { btn: el, ok: "Anti-rebond matériel réglé à 2 ms" }),
    "cs-ind": (el) => call("corsair_set_indicator_color", { device_index: dev(el), indicator: val(`cs-ind-${dev(el)}`), color: safeHex(val(`cs-ind-col-${dev(el)}`)) }, { btn: el, ok: "Couleur du voyant appliquée" }),
    "cs-hwp-reload": (el) => call("corsair_reload_hwp", { device_index: dev(el) }, { btn: el, ok: "Mémoire du clavier relue" }),
    "cs-hwp-flash": (el) => {
      const i = dev(el);
      call("corsair_flash_hwp_lighting", {
        device_index: i, profile_index: 0, name: val(`hwp-name-${i}`), kind: val(`hwp-kind-${i}`),
        primary: safeHex(val(`hwp-c1-${i}`)), secondary: safeHex(val(`hwp-c2-${i}`)),
        speed: Number(val(`hwp-speed-${i}`)), direction: Number(val(`hwp-dir-${i}`)),
      }, { btn: el, ok: "Effet écrit dans le clavier et vérifié : il reste actif sans OpenGear." });
    },

    // Logitech
    "logi-dpi": (el) => call("logi_set_dpi", { device_index: dev(el), x: Number(el.dataset.dpi), y: Number(el.dataset.dpi) }, { btn: el, ok: `Sensibilité : ${el.dataset.dpi} DPI` }),
    "logi-dpi-apply": (el) => {
      const i = dev(el);
      const x = Number(val(`ldpi-${i}`));
      const yv = val(`ldpiy-${i}`);
      call("logi_set_dpi", { device_index: i, x, y: yv != null ? Number(yv) : x }, { btn: el, ok: `Sensibilité : ${x} DPI` });
    },
    "logi-lod": (el) => {
      const m = S.logi_mice[dev(el)];
      if (!m || !m.dpi) return;
      call("logi_set_dpi", { device_index: dev(el), x: m.dpi.dpi_x, y: m.dpi.dpi_y, lod: el.dataset.val }, { btn: el, ok: `Distance de décrochage : ${el.textContent.trim()}` });
    },
    "logi-hz": (el) => call("logi_set_pollrate", { device_index: dev(el), hz: Number(el.dataset.val) }, { btn: el, ok: `Fréquence d'envoi : ${fmtHz(Number(el.dataset.val))}` }),
    "lkb-quick": (el) => call("logi_kb_set_rgb", { device_index: dev(el), mode: el.dataset.mode, color: safeHex(S.config.profile.rgb.primary) }, { btn: el, ok: "Éclairage du clavier appliqué" }),
    "lkb-apply": (el) => call("logi_kb_set_rgb", { device_index: dev(el), mode: val(`lkb-mode-${dev(el)}`), color: safeHex(val(`lkb-col-${dev(el)}`)) }, { btn: el, ok: "Éclairage du clavier appliqué" }),
    "lml-apply": (el) => call("logi_set_led", { device_index: dev(el), mode: val(`lml-mode-${dev(el)}`), color: safeHex(val(`lml-col-${dev(el)}`)) }, { btn: el, ok: "Éclairage de la souris appliqué" }),
    "logi-side-quick": (el) => call("logi_set_sidetone", { device_index: dev(el), percent: Number(el.dataset.pct) }, { btn: el, ok: Number(el.dataset.pct) ? `Retour micro : ${el.dataset.pct} %` : "Retour micro coupé" }),
    "eq-preset": (el) => {
      const i = dev(el);
      const h = S.logi_headsets[i];
      const pr = h && arr(h.presets)[Number(el.dataset.idx)];
      if (!pr) return;
      eqDrafts[i] = { gains: arr(pr.gains).map((x) => num(x)), dirty: false };
      call("logi_set_eq", { device_index: i, gains: eqDrafts[i].gains }, { btn: el, ok: `Égaliseur : ${pr.name}` });
    },
    "eq-apply": (el) => {
      const i = dev(el);
      const e = eqDrafts[i];
      if (!e) return;
      const g = e.gains.slice();
      call("logi_set_eq", { device_index: i, gains: g }, { btn: el, ok: "Égaliseur appliqué" }).then((r) => { if (r && eqDrafts[i]) eqDrafts[i].dirty = false; });
    },
    "eq-reset": (el) => {
      const i = dev(el);
      eqDrafts[i] = { gains: [0, 0, 0, 0, 0], dirty: false };
      call("logi_set_eq", { device_index: i, gains: [0, 0, 0, 0, 0] }, { btn: el, ok: "Égaliseur remis à plat" });
    },
    "mbtn-sel": (el) => { btnSel[dev(el)] = Number(el.dataset.btn); render(); },
    "mouse-variant": (el) => { selectedMouseVariant = Number(el.dataset.idx); render(); },
    "headset-variant": (el) => { selectedHeadsetVariant = Number(el.dataset.idx); render(); },
    "logi-mode": (el) => call("logi_set_mode", { device_index: dev(el), mode: el.dataset.val }, { btn: el, ok: el.dataset.val === "Host" ? "Souris pilotée par OpenGear" : "La souris utilise ses profils internes" }),
    "logi-prof-sel": (el) => {
      const i = dev(el);
      if (drafts[i] && drafts[i].dirty) toast("Brouillon non écrit abandonné pour ce profil.", "info");
      delete drafts[i];
      profSel[i] = Number(el.dataset.val);
      render();
    },
    "logi-activate": (el) => call("logi_set_active_profile", { device_index: dev(el), slot: Number(el.dataset.slot) }, { btn: el, ok: `Profil ${el.dataset.slot} activé sur la souris` }),
    "logi-activate-seg": (el) => call("logi_set_active_profile", { device_index: dev(el), slot: Number(el.dataset.val) }, { btn: el, ok: `Profil ${el.dataset.val} activé sur la souris` }),
    "logi-test-stage": (el) => call("logi_set_dpi_stage", { device_index: dev(el), stage: Number(el.dataset.stage) }, { btn: el, ok: `Palier ${Number(el.dataset.stage) + 1} activé pour essai` }),
    "draft-hz": (el) => {
      const d = getDraftFor(dev(el));
      if (!d) return;
      d.profile.report_rate_hz = Number(el.dataset.val);
      d.dirty = true;
      render();
    },
    "logi-reload": (el) => {
      delete drafts[dev(el)];
      call("logi_reload_profiles", { device_index: dev(el) }, { btn: el, ok: "Profils relus depuis la souris" });
    },
    "logi-flash": async (el) => {
      const i = dev(el);
      const d = getDraftFor(i);
      if (!d) return;
      const p = d.profile;
      const r = await call("logi_flash_profile", {
        device_index: i, index: d.idx, report_rate_hz: p.report_rate_hz, default_dpi_idx: p.default_dpi_idx,
        dpi_stages: p.dpi_stages, buttons: p.buttons.map((b) => b.hex),
      }, { btn: el, ok: `Profil ${p.slot_index} écrit dans la souris et vérifié.` });
      if (r) { delete drafts[i]; backups.data = null; render(); }
    },
    "logi-flash-macro": async (el) => {
      const i = dev(el);
      const d = getDraftFor(i);
      const preset = MACRO_PRESETS[val(`macro-preset-${i}`)] || MACRO_PRESETS.double_click;
      const b = btnSel[i] || 0;
      const r = await call("logi_flash_macro", { device_index: i, profile_index: d ? d.idx : 0, button_index: b, steps: preset.steps }, { btn: el, ok: `Macro « ${preset.label} » écrite sur M${b + 1}.` });
      if (r) { delete drafts[i]; backups.data = null; render(); }
    },

    // ASUS / RAM / watercooling
    "asus-direct": (el) => call("asus_set_direct", { enabled: el.dataset.on === "1" }, { btn: el, ok: el.dataset.on === "1" ? "Carte mère synchronisée avec OpenGear" : "Carte mère en mode autonome" }),
    "aura-quick": (el) => call("asus_set_hw_mode", { mode: el.dataset.mode, color: safeHex(S.asus && S.asus.hw_color, "#FF1848"), commit: false }, { btn: el, ok: `Carte mère : ${el.textContent.trim()}` }).then((r) => { if (r) auraDraftMode = el.dataset.mode; }),
    "aura-mode": (el) => {
      auraDraftMode = el.dataset.mode;
      call("asus_set_hw_mode", { mode: auraDraftMode, color: safeHex(val("asus-color"), "#FF1848"), commit: false }, { btn: el, ok: `Effet carte mère : ${el.textContent.trim()}` });
    },
    "aura-swatch": (el) => {
      const inp = document.getElementById("asus-color");
      if (inp) inp.value = el.dataset.color;
      call("asus_set_hw_mode", { mode: auraDraftMode || S.asus.hw_mode, color: el.dataset.color, commit: false }, { btn: el, ok: "Couleur de la carte mère appliquée" });
    },
    "aura-apply": (el) => call("asus_set_hw_mode", { mode: auraDraftMode || S.asus.hw_mode, color: safeHex(val("asus-color"), "#FF1848"), commit: false }, { btn: el, ok: "Effet appliqué à la carte mère" }),
    "aura-commit": (el) => call("asus_set_hw_mode", { mode: auraDraftMode || S.asus.hw_mode, color: safeHex(val("asus-color"), "#FF1848"), commit: true }, { btn: el, ok: "Effet enregistré dans la carte mère" }),
    "aura-shutdown": (el) => call("asus_set_shutdown_mode", { mode: val("asus-sd-mode"), color: safeHex(val("asus-sd-color"), "#000000") }, { btn: el, ok: "Éclairage PC éteint enregistré" }),
    "ram-mode": (el) => call("ram_apply_rgb", { mode: el.dataset.mode, color: safeHex(val("ram-color"), "#00D2FF"), speed: Number(val("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: `RAM : ${el.textContent.trim()}` }),
    "ram-swatch": (el) => {
      const inp = document.getElementById("ram-color");
      if (inp) inp.value = el.dataset.color;
      call("ram_apply_rgb", { mode: (S.ram_rgb && S.ram_rgb.last_mode) || "Static", color: el.dataset.color, speed: Number(val("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: "Couleur de la RAM appliquée" });
    },
    "ram-apply": (el) => call("ram_apply_rgb", { mode: (S.ram_rgb && S.ram_rgb.last_mode) || "Static", color: safeHex(val("ram-color"), "#00D2FF"), speed: Number(val("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: "Effet appliqué à la RAM" }),
    "ram-eeprom": (el) => call("ram_apply_rgb", { mode: (S.ram_rgb && S.ram_rgb.last_mode) || "Static", color: safeHex(val("ram-color"), "#00D2FF"), speed: Number(val("ram-speed") ?? 2), save_eeprom: true }, { btn: el, ok: "Effet enregistré dans les barrettes" }),
    "ram-sync-aura": (el) => {
      const col = safeHex(val("asus-color") || (S.asus && S.asus.hw_color), "#FF1848");
      call("ram_apply_rgb", { mode: "Static", color: col, speed: Number(val("ram-speed") ?? 2), save_eeprom: false }, { btn: el, ok: "RAM alignée sur la couleur de la carte mère" });
    },
    "ram-off": (el) => call("ram_apply_rgb", { mode: "Off", color: "#000000", speed: 2, save_eeprom: false }, { btn: el, ok: "Éclairage de la RAM éteint" }),
    "ram-scan": (el) => call("ram_scan", {}, { btn: el, ok: "Recherche des barrettes terminée" }),
    "relaunch-admin": (el) => call("cooling_relaunch_admin", {}, { btn: el, ok: "Relance en administrateur…" }),
    "sl3-preset": (el) => { selectedCoolingIdx = 0; call("cooling_sl3_preset", { preset: el.dataset.preset }, { btn: el, ok: `Pompe : ${el.textContent.trim()}` }); },
    "sl3-color": (el) => {
      sl3Color = safeHex(val("sl3-color"), "#FF8C1A");
      call("cooling_sl3_argb", { channel: Number(val("sl3-ch")), color: sl3Color, thermal: false }, { btn: el, ok: "Couleur du watercooling appliquée" });
    },

    // Ventilation
    "cool-ch": (el) => { selectedCoolingIdx = Number(el.dataset.val); render(); },
    "cool-pol": (el) => {
      const ch = S.cooling.channels[selectedCoolingIdx];
      if (ch) ch.policy = el.dataset.val;
      render();
    },
    "cool-read-bios": (el) => call("cooling_read_bios_curves", {}, { btn: el, ok: "Réglages relus depuis le BIOS" }),
    "cool-apply": (el) => {
      const ch = S.cooling.channels[selectedCoolingIdx];
      if (!ch) return;
      call("cooling_update_channel", { index: selectedCoolingIdx, policy: ch.policy, source: ch.source || "CpuBios", hysteresis_c: num(ch.hysteresis_c, 3), curve: ch.curve, apply: true }, { btn: el, ok: `${ch.short_label || ch.label} : réglage appliqué à la carte mère` });
    },
    "sio-refresh": (el) => call("superio_refresh", {}, { btn: el, ok: "Vitesses actualisées" }),
    "sio-apply": (el) => call("superio_set_pwm", { channel: Number(el.dataset.ch), duty_pct: Number(val(`sio-${el.dataset.ch}`) ?? 50) }, { btn: el }),
    "sio-auto": (el) => call("superio_restore_auto", { channel: Number(el.dataset.ch) }, { btn: el }),
    "pawnio-install": (el) => call("pawnio_install", {}, { btn: el }),
    "power-plan": (el) => call("set_power_plan", { guid: el.dataset.guid }, { btn: el, ok: "Mode d'alimentation activé" }),

    // Touches & macros
    "input-engine-on": (el) => call("input_set_enabled", { enabled: true }, { btn: el, ok: "Moteur d'entrée démarré" }),
    "bind-remap": (el) => call("input_set_binding", { key: selectedKeyId, kind: "remap", target: val("mac-remap") }, { btn: el, ok: `${selectedKeyId} → ${val("mac-remap")}` }),
    "bind-disable": (el) => call("input_set_binding", { key: selectedKeyId, kind: "disable" }, { btn: el, ok: `${selectedKeyId} désactivée` }),
    "bind-reset": (el) => call("input_remove_binding", { key: selectedKeyId }, { btn: el, ok: `${selectedKeyId} rétablie` }),
    "bind-text": (el) => {
      const t = val("mac-text") || "";
      if (!t) { toast("Saisis d'abord le texte à taper.", "err"); return; }
      call("input_set_binding", { key: selectedKeyId, kind: "text", text: t, delay_ms: 15 }, { btn: el, ok: `Texte assigné à ${selectedKeyId}` });
    },
    "rec-start": (el) => call("input_macro_record_start", {}, { btn: el, ok: "Enregistrement démarré : tape ta combinaison." }),
    "rec-stop": async (el) => {
      setBusy(el, true);
      try {
        const resp = await rpc("input_macro_record_stop", {});
        if (resp && arr(resp.recorded_steps).length) {
          macroSteps = resp.recorded_steps;
          toast(`${plural(macroSteps.length, "étape capturée", "étapes capturées")}.`, "ok");
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
      const k = val("mac-step-key");
      const ms = Math.max(0, Number(val("mac-step-ms")) || 15);
      if (!k) return;
      macroSteps.push({ key: k, down: true, delay_ms: ms }, { key: k, down: false, delay_ms: ms });
      render();
    },
    "macro-clear": () => { macroSteps = []; render(); },
    "bind-macro": (el) => {
      if (!macroSteps.length) { toast("La macro est vide : ajoute ou enregistre des étapes.", "err"); return; }
      call("input_set_binding", { key: selectedKeyId, kind: "macro", name: val("mac-name") || "Macro", steps: macroSteps }, { btn: el, ok: `Macro assignée à ${selectedKeyId}` });
    },
    "bind-remove": (el) => call("input_remove_binding", { key: el.dataset.key }, { btn: el, ok: `Règle ${el.dataset.key} supprimée` }),
    "socd-blocked": (el) => {
      const list = String(val("socd-blocked") || "").split(",").map((s) => s.trim()).filter(Boolean);
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

    // Profils
    "save-config": (el) => call("save_config", {}, { btn: el, ok: "Configuration enregistrée" }),
    "switch-profile": (el) => call("switch_profile", { name: el.dataset.name }, { btn: el, ok: `Profil « ${el.dataset.name} » activé` }),
    "create-profile": (el) => {
      const name = String(val("new-profile") || "").trim();
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
        let text;
        try {
          text = await f.text();
        } catch (e) {
          toast("Impossible de lire ce fichier.", "err");
          return;
        }
        call("profile_import_json", { json: text }, { btn: el, ok: `Profil importé depuis ${f.name}` });
      };
      inp.click();
    },
    "linked-save": (el) => {
      const apps = String(val("linked-apps") || "").split(",").map((s) => s.trim()).filter(Boolean);
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

    // Système
    shortcuts: (el) => call("sys_create_shortcuts", {}, { btn: el }),
    "diag-refresh": () => loadDiagnostics(),
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
          // Fallback for contexts without the async clipboard API
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
        toast(copied ? "Rapport de diagnostic copié : colle-le dans ton signalement." : "Copie impossible : le presse-papiers est indisponible.", copied ? "info" : "error");
      } catch (e) {
        toast(String((e && e.message) || e), "error");
      } finally {
        el.disabled = false;
      }
    },
    "diag-action": async (el) => {
      const c = diagSorted[Number(el.dataset.idx)];
      if (!c || !c.action || !c.action.cmd) return;
      const r = await call(c.action.cmd, c.action.payload || {}, { btn: el, ok: `${c.action.label || "Action"} : terminé` });
      if (r) loadDiagnostics();
    },
    selftest: (el) => call("run_selftest", {}, { btn: el, ok: "Auto-test terminé : rapport enregistré." }),
    "scan-hid": (el) => call("scan_hid", { all: el.dataset.all === "1" }, { btn: el, ok: "Bus HID analysé" }),
    "backups-refresh": () => loadBackups(),
    "backup-restore": async (el) => {
      const r = await call("logi_restore_backup", { device_index: backups.mouse, file: el.dataset.file, _label: el.dataset.label }, { btn: el });
      if (r) { Object.keys(drafts).forEach((k) => delete drafts[k]); loadBackups(); }
    },
  };

  const CHANGE = {
    "switch-profile": (el) => call("switch_profile", { name: el.value }, { ok: `Profil « ${el.value} » activé` }),
    "rgb-enable-sw": (el) => call("set_rgb_config", { enable_rgb: el.checked }, { ok: el.checked ? "Éclairage allumé" : "Éclairage coupé" }),
    "sync-all": (el) => call("set_sync_all", { enabled: el.checked }, { ok: el.checked ? "Toutes les marques sont synchronisées" : "Synchronisation désactivée" }),
    "rgb-reverse": (el) => call("set_rgb_config", { reverse: el.checked }, { silent: true }),
    "cs-bright": (el) => call("corsair_set_brightness", { device_index: dev(el), percent: Number(el.value) }, { ok: `Luminosité : ${el.value} %` }),
    "argb-count": (el) => call("asus_set_led_count", { channel: Number(el.dataset.ch), count: Number(el.value) }, { ok: `Connecteur réglé sur ${el.value} LEDs` }),
    "asus-direct-sw": (el) => call("asus_set_direct", { enabled: el.checked }, { ok: el.checked ? "Carte mère synchronisée avec OpenGear" : "Carte mère en mode autonome" }),
    "sl3-thermal": (el) => call("cooling_sl3_argb", { channel: Number(val("sl3-ch")), thermal: el.checked }, { ok: el.checked ? "Couleur selon la température activée" : "Couleur selon la température désactivée" }),
    "lkb-bright": (el) => call("logi_kb_set_brightness", { device_index: dev(el), percent: Number(el.value) }, { ok: `Luminosité : ${el.value} %` }),
    "logi-asnap": (el) => call("logi_set_angle_snapping", { device_index: dev(el), enabled: el.checked }, { ok: `Correction d'angle ${el.checked ? "activée" : "désactivée"}` }),
    "cool-src": (el) => {
      const ch = S.cooling.channels[selectedCoolingIdx];
      if (!ch) return;
      ch.source = el.value;
      call("cooling_update_channel", { index: selectedCoolingIdx, source: ch.source, hysteresis_c: num(ch.hysteresis_c, 3), apply: false }, { ok: "Température de référence modifiée" });
    },
    "cool-hyst": (el) => {
      const ch = S.cooling.channels[selectedCoolingIdx];
      if (!ch) return;
      ch.hysteresis_c = Number(el.value);
      call("cooling_update_channel", { index: selectedCoolingIdx, source: ch.source || "CpuBios", hysteresis_c: ch.hysteresis_c, apply: false }, { ok: `Hystérésis : ${ch.hysteresis_c} °C` });
    },
    "curve-pt": (el) => {
      const ch = S.cooling.channels[selectedCoolingIdx];
      if (!ch) return;
      const i = Number(el.dataset.pt);
      const pt = Object.assign({}, ch.curve[i]);
      let v = Math.round(Number(el.value));
      if (!Number.isFinite(v)) v = el.dataset.field === "temp_c" ? num(pt.temp_c, 50) : num(pt.duty_pct, 50);
      if (el.dataset.field === "temp_c") {
        const lo = i === 0 ? 20 : num(ch.curve[i - 1].temp_c) + 2;
        const hi = i === ch.curve.length - 1 ? 98 : num(ch.curve[i + 1].temp_c) - 2;
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
    azerty: (el) => call("set_azerty", { enabled: el.checked }, { ok: el.checked ? "Disposition AZERTY" : "Disposition QWERTY" }),
    "cs-flashtap": (el) => call("corsair_set_flashtap", { device_index: Number(el.dataset.dev), enabled: el.checked }),
    "socd-en": (el) => call("input_set_esports", { socd_enabled: el.checked }, { ok: el.checked ? "Snap Tap activé" : "Snap Tap désactivé" }),
    "socd-preset": (el) => call("input_set_esports", { socd_preset: el.value }, { ok: "Touches Snap Tap mises à jour" }),
    "es-chatter": (el) => call("input_set_esports", { key_chatter_ms: Number(el.value) }, { ok: `Anti-rebond clavier : ${LABEL_FMT.ms(el.value)}` }),
    "es-mdeb": (el) => call("input_set_esports", { mouse_debounce_ms: Number(el.value) }, { ok: `Anti-rebond clic : ${LABEL_FMT.ms(el.value)}` }),
    "draft-def-dpi": (el) => {
      const d = getDraftFor(dev(el));
      if (!d) return;
      d.profile.default_dpi_idx = Number(el.value);
      d.dirty = true;
    },
    "draft-stage-dpi": (el) => {
      const d = getDraftFor(dev(el));
      if (!d) return;
      const v = clamp(Math.round(Number(el.value) / 50) * 50 || 800, 100, 32000);
      el.value = v;
      d.profile.dpi_stages[Number(el.dataset.stage)] = v;
      d.dirty = true;
    },
    "draft-btn": (el) => {
      const i = dev(el);
      const d = getDraftFor(i);
      if (!d) return;
      const b = Number(el.dataset.btn);
      d.profile.buttons[b] = { hex: el.value, label: el.options[el.selectedIndex].textContent };
      d.dirty = true;
      btnSel[i] = b;
      render();
    },
    "cs-winlock": (el) => call("corsair_set_hw_winlock", { device_index: dev(el), enabled: el.checked }, { ok: el.checked ? "Touche Windows bloquée" : "Touche Windows débloquée" }),
    "cs-wl-bit": (el) => {
      let mask = 0;
      document.querySelectorAll(`[data-change="cs-wl-bit"][data-dev="${dev(el)}"]:checked`).forEach((x) => { mask |= Number(x.dataset.bit); });
      call("corsair_set_winlock_shortcuts", { device_index: dev(el), mask }, { ok: "Raccourcis bloqués mis à jour" });
    },
    "cs-deb": (el) => {
      const i = dev(el);
      const en = document.getElementById(`cs-deb-en-${i}`);
      call("corsair_set_debounce", { device_index: i, enabled: en ? en.checked : true, timeout_ms: Number(val(`cs-deb-ms-${i}`) ?? 30) }, { ok: "Anti-rebond matériel mis à jour" });
    },
    "chs-side": (el) => call("corsair_headset_set_sidetone", { device_index: dev(el), percent: Number(el.value), muted: Number(el.value) === 0 }, { ok: `Retour micro : ${el.value} %` }),
    "logi-side": (el) => call("logi_set_sidetone", { device_index: dev(el), percent: Number(el.value) }, { ok: `Retour micro : ${el.value} %` }),
    "wa-vol": (el) => call("audio_set_volume", { is_mic: el.dataset.mic === "1", percent: Number(el.value) }, { rerender: false, ok: `${el.dataset.mic === "1" ? "Micro" : "Volume"} : ${el.value} %` }),
    "sys-pref": (el) => call("sys_set_prefs", { [el.dataset.key]: el.checked }, { ok: "Préférence enregistrée" }),
    "sys-pref-num": (el) => call("sys_set_prefs", { [el.dataset.key]: Number(el.value) }, { ok: "Préférence enregistrée" }),
    "bk-mouse": (el) => { backups.mouse = Number(el.value); },
  };

  const INPUT = {
    label: (el) => updateLabel(el),
    "rgb-live": (el) => {
      updateLabel(el);
      const field = el.dataset.field;
      const v = field === "primary" || field === "secondary" ? safeHex(el.value) : parseFloat(el.value);
      throttled("rgb-" + field, 150, () => call("set_rgb_config", { [field]: v }, { rerender: false, silent: true }));
    },
    "paint-color": (el) => { paintColor = el.value; },
    "eq-band": (el) => {
      const i = dev(el);
      const h = S.logi_headsets[i];
      if (!h) return;
      const e = getEq(h, i);
      const b = Number(el.dataset.band);
      e.gains[b] = Number(el.value);
      e.dirty = true;
      setTxt(`eq-db-${i}-${b}`, `${e.gains[b] > 0 ? "+" : ""}${e.gains[b]} dB`);
      renderEqSvg(i);
    },
  };

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
  // Entrée dans les champs texte : valide l'action voisine.
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
    if (curveDragging) return true;
    const a = document.activeElement;
    const main = document.getElementById("main-content");
    if (!a || !main || !main.contains(a)) return false;
    if (a.tagName === "TEXTAREA" || a.tagName === "SELECT") return true;
    return a.tagName === "INPUT" && ["text", "number", "search", ""].includes(a.type || "");
  }
  document.addEventListener("focusout", () => {
    setTimeout(() => {
      if (pendingRender && !isEditing()) render();
    }, 0);
  });

  // ==================== ANIMATION DES APERÇUS RGB ====================
  function hexToRgb(hex) {
    const h = safeHex(hex).slice(1);
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }
  function hsvToRgb(h, s, v) {
    h = ((h % 360) + 360) % 360;
    const c = v * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = v - c;
    let r = 0, g = 0, b = 0;
    if (h < 60) [r, g, b] = [c, x, 0];
    else if (h < 120) [r, g, b] = [x, c, 0];
    else if (h < 180) [r, g, b] = [0, c, x];
    else if (h < 240) [r, g, b] = [0, x, c];
    else if (h < 300) [r, g, b] = [x, 0, c];
    else [r, g, b] = [c, 0, x];
    return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
  }
  function lerpRgb(a, b, t) {
    t = clamp(t, 0, 1);
    return [0, 1, 2].map((k) => Math.round(a[k] + (b[k] - a[k]) * t));
  }
  function sampleEffect(rgb, ledIdx, nx, ny, t) {
    const p1 = hexToRgb(rgb.primary), p2 = hexToRgb(rgb.secondary);
    const sp = num(rgb.speed, 1), dir = rgb.reverse ? -1 : 1;
    let c = p1;
    switch (rgb.kind) {
      case "Off": c = [12, 15, 22]; break;
      case "PerKey": c = rgb.per_key && rgb.per_key[String(ledIdx)] ? hexToRgb(rgb.per_key[String(ledIdx)]) : p1; break;
      case "RainbowWave": c = hsvToRgb(nx * 360 - dir * t * 120 * sp, 1, 1); break;
      case "SpiralRainbow": {
        const dx = nx - 0.5, dy = (ny - 0.5) * 0.45;
        c = hsvToRgb((Math.atan2(dy, dx) * 180) / Math.PI + Math.hypot(dx, dy) * 240 - dir * t * 140 * sp, 1, 1);
        break;
      }
      case "SpectrumCycle": c = hsvToRgb(t * 45 * sp, 1, 1); break;
      case "Breathing": {
        const ph = Math.sin(t * 2 * sp) * 0.5 + 0.5;
        const mix = lerpRgb(p1, p2, ph);
        const f = 0.18 + 0.82 * ph;
        c = mix.map((x) => Math.round(x * f));
        break;
      }
      case "ColorWave": c = lerpRgb(p1, p2, Math.sin(nx * Math.PI * 2 - dir * t * 3 * sp) * 0.5 + 0.5); break;
      case "Visor": {
        const scan = Math.sin(t * 1.8 * sp) * 0.5 + 0.5;
        const g = Math.max(0, 1 - Math.abs(nx - scan) * 5);
        c = lerpRgb(p2.map((x) => x * 0.15), p1, g * g);
        break;
      }
      case "Rain": {
        const seed = Math.abs(Math.sin(Math.floor(nx * 23) * 37.7 + 11.3));
        const d = (((t * 0.85 * sp + seed * 7) % 1) - ny + 1) % 1;
        const tail = Math.max(0, 1 - d * 3.2);
        c = lerpRgb(p2.map((x) => x * 0.12), p1, tail * tail);
        break;
      }
      case "Gradient": c = lerpRgb(p1, p2, rgb.reverse ? 1 - nx : nx); break;
      default: c = p1;
    }
    const br = num(rgb.brightness, 1);
    return `rgb(${Math.round(c[0] * br)},${Math.round(c[1] * br)},${Math.round(c[2] * br)})`;
  }

  let lastPreviewSig = "";
  setInterval(() => {
    if (!S || document.hidden) return;
    const keys = document.querySelectorAll(".kb-key");
    const spots = document.querySelectorAll(".mb-hotspot");
    if (!keys.length && !spots.length) return;
    const prof = S.config.profile;
    const rgb = prof.rgb;
    const t = (performance.now() - animStart) / 1000;
    const staticEff = ["Static", "PerKey", "Off", "Gradient"].includes(rgb.kind);
    if (keys.length) {
      const sig = `k|${rgb.kind}|${rgb.primary}|${rgb.secondary}|${rgb.brightness}|${rgb.reverse}|${JSON.stringify(rgb.per_key || {})}|${prof.enable_rgb}`;
      if (!(staticEff && sig === lastPreviewSig)) {
        lastPreviewSig = sig;
        keys.forEach((el) => {
          el.style.borderColor = prof.enable_rgb === false ? "rgba(255,255,255,0.14)" : sampleEffect(rgb, Number(el.dataset.led), Number(el.dataset.nx), Number(el.dataset.ny), t);
        });
      }
    }
    if (spots.length && S.asus) {
      const sync = !!prof.sync_asus_aura;
      const hwAnim = ["Rainbow", "SpectrumCycle", "Wave"].includes(S.asus.hw_mode);
      spots.forEach((el, idx) => {
        let col = safeHex(S.asus.hw_color, "#FF1848");
        if (sync) col = sampleEffect(rgb, idx, idx / 5, 0.5, t);
        else if (hwAnim) {
          const [r, g, b] = hsvToRgb(t * 85 + idx * 55, 1, 1);
          col = `rgb(${r},${g},${b})`;
        } else if (S.asus.hw_mode === "Off") col = "#364158";
        el.style.setProperty("--zone-col", col);
      });
    }
  }, 100);

  // ==================== PUSH DU MOTEUR ====================
  const deviceSignature = () => devicesList().map((d) => d.key + "=" + d.name).join("|");

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

  // ==================== DÉMARRAGE ====================
  rpc("get_state")
    .then((st) => {
      applyState(st);
      const n = devicesList().length;
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
})();
