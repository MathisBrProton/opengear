import { arr, esc, num } from "../utils.js";
import { S } from "../state.js";
import { pageHeader, seg, settingInline, sw } from "../components.js";

export function viewProfiles() {
  const prof = S.config.profile;
  const sp = S.system_prefs;
  const linked = arr(prof.linked_apps);
  const hw = [];
  arr(S.device_list).forEach((d) => {
    const id = d.info.id;
    const ctl = settingInline(id, "corsair.hw_profile", { label: d.info.model });
    if (ctl) hw.push(`<div class="list-row"><div class="grow">${ctl}<div class="muted small">Profils enregistrés dans l'appareil</div></div></div>`);
    const ob = d.onboard && d.onboard.info;
    const profiles = arr(d.onboard && d.onboard.profiles);
    if (ob && profiles.length) {
      hw.push(`<div class="list-row"><div><strong>${esc(d.info.model)}</strong><div class="muted small">Profils enregistrés dans l'appareil</div></div>${seg(profiles.map((p, idx) => [num(p.slot, idx + 1), `Profil ${num(p.slot, idx + 1)}`]), ob.active_profile, "ob-activate-seg", { id })}</div>`);
    }
  });
  return `
    ${pageHeader("Profils", "Un profil regroupe éclairage, touches et réglages de jeu. Bascule à la main ou automatiquement selon le jeu lancé.",
      `<button type="button" class="btn btn-primary btn-sm" data-act="save-config">Enregistrer sur le disque</button>
       <button type="button" class="btn btn-secondary btn-sm" data-act="export-profile">Exporter</button>
       <button type="button" class="btn btn-secondary btn-sm" data-act="import-profile">Importer…</button>`, "emerald")}
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
          <div class="muted small mt-12 mb-6">Programmes ouverts (clic pour les lier à ce profil) :</div>
          <div class="row gap-6px">${arr(sp.visible_apps).slice(0, 10).map((a) => `<button type="button" class="btn btn-sm btn-secondary mono" data-act="link-app" data-app="${esc(a)}">+ ${esc(a)}</button>`).join("")}</div>` : ""}
      </div>
    </div>
    <div class="card mt-16">
      <div class="card-head"><div><h2 class="card-title">Profils enregistrés dans les appareils</h2><div class="card-desc">Certains appareils gardent leurs propres profils en mémoire, utilisables même sans OpenGear.</div></div></div>
      ${hw.length ? `<div class="list">${hw.join("")}</div>` : `<div class="muted">Aucun appareil connecté ne propose de profils internes.</div>`}
    </div>`;
}
