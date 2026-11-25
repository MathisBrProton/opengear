import { CONFIG_CMDS } from "./utils.js";
import { S, applyState, updateChrome } from "./state.js";
import { confirmDialog, riskOf } from "./modals.js";

const DEFAULT_RPC_TIMEOUT_MS = 15_000;
let nextReqId = 1;
const pending = new Map();

window.__ogReply = (id, res) => {
  const p = pending.get(id);
  if (!p) return;
  clearTimeout(p.timer);
  pending.delete(id);
  if (res && res.ok) p.resolve(res.data);
  else p.reject(new Error((res && res.error) || "Erreur IPC"));
};

export function rpc(cmd, payload = {}, timeoutMs = DEFAULT_RPC_TIMEOUT_MS) {
  return new Promise((resolve, reject) => {
    if (!window.ipc || typeof window.ipc.postMessage !== "function") {
      reject(new Error("Pont IPC indisponible"));
      return;
    }
    const id = nextReqId++;
    const effectiveTimeout = cmd === "pawnio_install" || cmd === "run_selftest"
      ? Math.max(timeoutMs, 120_000)
      : timeoutMs;
    const timer = setTimeout(() => {
      if (pending.delete(id)) {
        reject(new Error(`Délai dépassé (${Math.round(effectiveTimeout / 1000)} s) pour « ${cmd} »`));
      }
    }, effectiveTimeout);
    pending.set(id, { resolve, reject, timer });
    try {
      window.ipc.postMessage(JSON.stringify({ id, cmd, payload }));
    } catch (e) {
      clearTimeout(timer);
      pending.delete(id);
      reject(e);
    }
  });
}

export function setStatus(txt) {
  const el = document.getElementById("status-text");
  if (el) el.textContent = txt;
}

export function toast(msg, kind = "info", opts = {}) {
  if (!msg) return null;
  setStatus(msg);
  const root = document.getElementById("toast-root");
  if (!root) return null;
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

export function offerUndo(msg) {
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
    el._timer = setTimeout(() => el.remove(), 10_000);
    return;
  }
  undoToastEl = toast(label, "ok", { duration: 10_000, action: { label: "Annuler", run: (b) => doUndo(b) } });
}

export async function doUndo(btn) {
  if (undoToastEl) {
    undoToastEl.remove();
    undoToastEl = null;
  }
  const lbl = S && S.ui && S.ui.undo_label;
  await call("undo_last", {}, { btn, ok: lbl ? `Annulé : ${lbl}` : "Dernière modification annulée" });
}

export function humanError(raw) {
  const m = String(raw || "").trim();
  if (!m) return "Une erreur inconnue est survenue.";
  if (/Commande IPC inconnue/i.test(m)) return "Cette fonction n'est pas encore disponible dans le moteur OpenGear installé. Mets l'application à jour.";
  if (/Pont IPC/i.test(m)) return "Impossible de joindre le moteur OpenGear. Redémarre l'application.";
  if (/rien à annuler|nothing to undo/i.test(m)) return "Il n'y a plus rien à annuler.";
  if (/acc[eè]s refus|access is denied|access denied|os error 5|admin/i.test(m)) return `Windows a refusé l'accès au matériel : relance OpenGear en administrateur (Système › Diagnostic). Détail : ${m}`;
  if (!/Put param|ExecMethod|GetMethod/i.test(m) && /\b(timed out|timeout|d[ée]lai d[ée]pass[ée]|no response|pas r[ée]pondu)\b/i.test(m)) return `L'appareil n'a pas répondu à temps. Vérifie qu'il est branché et sorti de veille, puis réessaie. Détail : ${m}`;
  if (/non connect|not connected|introuvable|not found|disconnected|d[ée]branch|aucun|aucune/i.test(m)) return `Appareil introuvable : il a peut-être été débranché ou s'est mis en veille. Détail : ${m}`;
  if (/JSON invalide|invalid json/i.test(m)) return `Ce fichier n'est pas un profil OpenGear valide. Détail : ${m}`;
  if (/invalide|invalid/i.test(m)) return `Valeur refusée par l'appareil : ${m}`;
  return `L'opération a échoué : ${m}`;
}

export function setBusy(el, on) {
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

export async function call(cmd, payload = {}, opts = {}) {
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
    if (CONFIG_CMDS.has(cmd) && clean.commit !== false) offerUndo(opts.silent ? null : msg);
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
