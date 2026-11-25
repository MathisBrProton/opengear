# Composants tiers

OpenGear est distribué sous licence **MIT OR Apache-2.0** (voir `LICENSE-MIT` et
`LICENSE-APACHE`). Les composants suivants, inclus dans le dépôt ou distribués avec
l'exécutable, restent soumis à **leur propre licence**.

## PawnIO.Modules — LGPL-2.1

- Fichiers : `crates/opengear-platform/pawnio_modules/*.bin`
  (`SmbusPIIX4`, `SmbusI801`, `AMDFamily17`, `IntelMSR`, `LpcIO`)
- Projet : PawnIO.Modules (namazso) — modules de bytecode signés exécutés par le pilote PawnIO
- Version : v0.2.12
- Licence : GNU Lesser General Public License v2.1 — texte complet dans
  `crates/opengear-platform/pawnio_modules/COPYING`
- Le code source correspondant est publié par le projet amont (dépôt PawnIO.Modules de namazso).
- Ces modules ne sont **pas** incorporés statiquement dans `OpenGear.exe` : ils sont copiés
  comme fichiers séparés et remplaçables dans le dossier `pawnio_modules/` à côté de
  l'exécutable (conformément à la LGPL-2.1 §6).

## Pilote PawnIO

Le pilote noyau signé PawnIO n'est **pas** inclus : il est installé séparément par
l'utilisateur (ou via le bouton « Installer PawnIO » du diagnostic après vérification SHA-256 et
signature Authenticode) depuis sa source officielle.

## Microsoft WebView2 — WebView2Loader.dll

Copié à côté de l'exécutable par `crates/opengear-app/build.rs` depuis la crate
`webview2-com-sys`. Redistribué selon les conditions du SDK Microsoft Edge WebView2.

## Visuels matériels (`crates/opengear-app/assets/*.svg`)

Schémas vectoriels SVG originaux produits pour OpenGear et distribués sous la même licence
(`MIT OR Apache-2.0`). Si G HUB ou iCUE 5 sont déjà installés localement sur la machine de
l'utilisateur, OpenGear peut lire leurs visuels locaux à la volée sans jamais les redistribuer.

## Dépendances Rust

Les crates listées dans `Cargo.lock` sont sous leurs licences respectives (majoritairement
MIT et/ou Apache-2.0).
