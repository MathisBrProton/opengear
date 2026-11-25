# Logitech G — Traitements audio Windows (Blue VO!CE & DTS Headphone:X 2.0)

## Constat technique

Comme sur les casques HyperX APO, une partie du traitement audio de certains casques et microphones Logitech G (PRO X, PRO X Wireless, G733, G735, G935, G635, Yeti X) ne s'exécute **pas** dans le firmware USB du périphérique mais **côté Windows via des Audio Processing Objects (APO) COM** intégrés au pilote audio Logitech :

- **Blue VO!CE 1** (ex. `G733`, `PRO X`, `PRO X Wireless`) : filtres micro (HPF, de-esser, gate, compresseur, limiteur, égaliseur micro, préréglages broadcast).
- **Blue VO!CE 2** (ex. `G735`, `Yeti X`) : chaîne vocale avancée temps réel avec profils multi-effets côté hôte.
- **DTS Headphone:X 2.0 / Surround Sound** (`hasDtsHx2e` / `hasDts` sur `G935`, `G733`, `G735`, `G533`, `G431`, `G432`, `PRO X`) : spatialisation binaurale multi-canaux côté APO Windows.

Il n'existe aucune commande USB HID/HID++ pour ces traitements sur les firmwares concernés ; `opengear-logitech` pilote en revanche nativement tous les réglages matériels embarqués (Sidetone `0x8300`, Bass Tone `0x8305`, Equalizer matériel `0x8310`, batterie, extinction automatique et RGB).

## Deux approches possibles

### Piste A — Piloter l'APO Logitech déjà installé sur Windows

- **Principe** : dialoguer avec l'interface COM / IPC locale du pilote APO Logitech lorsque celui-ci est déjà installé sur la machine hôte, sans inclure ni redistribuer aucun binaire propriétaire Logitech/Blue/DTS.
- **Limites** : dépend d'une installation tierce préalable et reste fragile face aux mises à jour de schéma IPC.

### Piste B — Chaîne audio clean-room 100 % OpenGear (`opengear-platform`)

- **Principe** : implémenter un APO Windows COM `MFX`/`EFX` open-source dans `crates/opengear-platform` (avec enregistrement administrateur lors de l'installation via `OpenGear-Setup.exe`) :
  - **Chaîne micro open-source** : filtre passe-haut, noise gate, compresseur/limiteur, égaliseur paramétrique et débruiteur léger libre (type **RNNoise**) en remplacement fonctionnel de Blue VO!CE.
  - **Spatialisation libre** : moteur binaural HRTF à base de tables SOFA libres ou bascule vers Windows Sonic en remplacement fonctionnel de DTS:X.

## Contraintes d'architecture à respecter

- Conserver la frontière vérifiée par `crates/opengear-app/tests/architecture.rs` : aucune dépendance Win32/COM dans `opengear-app` ni `opengear-logitech` (toute couche WASAPI/APO appartient exclusivement à `crates/opengear-platform`).
- OpenGear ne doit jamais embarquer, copier ou charger de DLL, poids IA ou licences propriétaires Blue / DTS / Logitech G HUB.
