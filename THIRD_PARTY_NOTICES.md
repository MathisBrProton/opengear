# Composants tiers

OpenGear est distribué sous licence **MIT OR Apache-2.0** (voir `LICENSE-MIT` et
`LICENSE-APACHE`). Les composants suivants, inclus dans le dépôt ou dans l'exécutable,
restent soumis à **leur propre licence**.

## PawnIO.Modules — LGPL-2.1

- Fichiers : `crates/opengear-asus/pawnio_modules/*.bin`
  (`SmbusPIIX4`, `SmbusI801`, `AMDFamily17`, `IntelMSR`, `LpcIO`)
- Projet : PawnIO.Modules (namazso) — modules de bytecode signés exécutés par le pilote PawnIO
- Version : v0.2.12
- Licence : GNU Lesser General Public License v2.1 — texte complet dans
  `crates/opengear-asus/pawnio_modules/COPYING`
- Le code source correspondant est publié par le projet amont (dépôt PawnIO.Modules de namazso).
- Ces modules sont actuellement incorporés dans `OpenGear.exe` (`include_bytes!`). Pour une
  distribution publique, joindre ce fichier et `COPYING`, et envisager de livrer les `.bin`
  comme fichiers séparés remplaçables (LGPL §6).

## Pilote PawnIO

Le pilote noyau signé PawnIO n'est **pas** inclus : il est installé séparément par
l'utilisateur (ou via le bouton « Installer PawnIO » du diagnostic) depuis sa source officielle.

## Microsoft WebView2 — WebView2Loader.dll

Copié à côté de l'exécutable par `crates/opengear-app/build.rs` depuis la crate
`webview2-com-sys`. Redistribué selon les conditions du SDK Microsoft Edge WebView2.

## Images produit (`crates/opengear-app/assets/*.jpg`)

Photos de produits de leurs fabricants respectifs (Corsair, Logitech, ASUS, be quiet!),
marques déposées de leurs propriétaires. Utilisées uniquement pour illustrer les appareils
détectés ; **à remplacer par des visuels libres de droits avant toute publication**.

## Dépendances Rust

Les crates listées dans `Cargo.lock` sont sous leurs licences respectives (majoritairement
MIT et/ou Apache-2.0).
