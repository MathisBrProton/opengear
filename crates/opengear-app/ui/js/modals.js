import { ICONS, esc, num } from "./utils.js";

export const modalStack = [];
const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function trapFocus(container, e) {
  if (!container) return;
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

export function openModal({ title, html, actions, dismiss = null, wide = false, danger = false }) {
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

export function confirmDialog(r) {
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

export function riskOf(cmd, p = {}) {
  switch (cmd) {
    case "set_experimental_driver":
      if (!p.enabled) return null;
      return {
        title: "Activer un pilote expérimental ?",
        body: "Ce pilote n'a jamais été testé sur du vrai matériel. Il n'écrit rien dans la mémoire de l'appareil (éclairage seulement, état non conservé), mais un appareil peut mal réagir ou ignorer les commandes.",
        details: ["Les appareils concernés sont recherchés immédiatement après l'activation.", "Tu peux le désactiver à tout moment ici."],
        confirm: "Activer",
      };
    case "device_onboard_flash_profile":
      return {
        title: "Écrire ce profil dans l'appareil ?",
        body: "Le profil sera écrit dans la mémoire flash interne de l'appareil et remplacera le contenu actuel de cet emplacement. Une copie de sauvegarde est faite avant l'écriture, puis le résultat est vérifié.",
        details: ["Ne débranche pas l'appareil et ne le laisse pas se mettre en veille pendant l'écriture.", "Une sauvegarde restaurable apparaîtra dans Système › Sauvegardes."],
        confirm: "Écrire dans l'appareil",
      };
    case "device_onboard_flash_macro":
      return {
        title: "Écrire la macro dans l'appareil ?",
        body: `La macro sera écrite dans la mémoire flash de l'appareil et assignée au bouton M${num(p.button_index) + 1} du profil embarqué sélectionné. Le bouton perdra son action actuelle.`,
        details: ["Ne débranche pas l'appareil pendant l'écriture.", "Une sauvegarde du secteur est conservée (Système › Sauvegardes)."],
        confirm: "Écrire la macro",
      };
    case "device_restore_backup":
      return {
        title: "Restaurer cette sauvegarde ?",
        body: "Le contenu de la sauvegarde va remplacer les données correspondantes dans la mémoire flash de l'appareil. Les réglages actuellement stockés à cet emplacement seront perdus.",
        details: p._label ? [p._label] : [],
        confirm: "Restaurer",
      };
    case "lighting_set_device":
      if (p.override && p.override.mode === "hardware" && p.override.request && p.override.request.save) {
        return {
          title: "Enregistrer cet effet dans la mémoire de l'appareil ?",
          body: "L'effet sélectionné sera écrit de façon permanente dans la mémoire non volatile de l'appareil et restera actif sans OpenGear.",
          details: [
            `Effet : ${p.override.request.effect || "?"}`,
            "Ne débranche pas l'appareil pendant l'écriture.",
          ],
          confirm: "Enregistrer dans l'appareil",
        };
      }
      return null;
    case "device_fan_set":
      if (p.device !== "superio:mainboard" || p.target === "auto") return null;
      return {
        title: "Forcer la vitesse de ce ventilateur ?",
        body: `OpenGear va fixer ce ventilateur à ${num(p.target)} % en écrivant directement dans la puce de contrôle de la carte mère. La régulation automatique du BIOS sera désactivée pour ce ventilateur jusqu'à « Auto BIOS » ou au redémarrage.`,
        details: ["Une vitesse trop basse peut faire chauffer le composant refroidi (processeur, pompe…)."],
        confirm: "Forcer la vitesse",
      };
    case "device_fan_calibrate":
      if (p.cancel) return null;
      return {
        title: "Calibrer ce ventilateur ?",
        body: "OpenGear va faire varier ce ventilateur de 100 % jusqu'à son plancher de sécurité (~10 secondes) pour mesurer ses vitesses réelles et son seuil de démarrage, puis le remettre en mode automatique.",
        details: ["La calibration est sauvegardée pour affiner la courbe et repérer le seuil 0 tr/min."],
        confirm: "Lancer la calibration",
      };
    case "device_curve_set":
      if (!p.apply) return null;
      return {
        title: "Appliquer ce réglage à la carte mère ?",
        body: "Le mode de ventilation de ce canal sera écrit dans le firmware de la carte mère (BIOS) et restera actif sans OpenGear.",
        details: ["Vérifie que la pompe et les ventilateurs gardent une vitesse suffisante en charge."],
        confirm: "Appliquer au BIOS",
      };
    case "device_pump_preset":
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
    case "relaunch_admin":
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
        /* reported by backend */
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
