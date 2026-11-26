# OpenGear

**Un seul petit programme pour piloter tout ton matériel gaming** — éclairage RGB, souris,
clavier, casque, ventilateurs et watercooling — sans iCUE, G HUB ni Armoury Crate, sans
télémétrie et sans compte.

> ⚠️ Projet en développement actif, testé sur une configuration précise (voir
> [Matériel supporté](#matériel-supporté)). OpenGear écrit dans la mémoire flash de certains
> appareils : lis la section [Sécurité](#sécurité) avant d'utiliser le mode Expert.

> **English summary.** OpenGear is a free, open-source Windows app (Rust) that controls RGB
> lighting, mouse and keyboard settings, fans and pumps, macros and key remapping, from one
> lightweight program. No account, no telemetry. It is tested on a few specific devices;
> drivers marked *experimental* have not been tested on real hardware yet. Contributions are
> welcome: see [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## Démarrage rapide

### Télécharger

Le plus simple : [télécharge la dernière release](https://github.com/MathisBrProton/opengear/releases/latest)
et lance `OpenGear-Setup.exe`. Une version portable (`OpenGear-portable.zip`) est aussi disponible.
Vérifie l'intégrité avec `SHA256SUMS.txt`.

### Lancer l'application

### Lancer l'application

```powershell
cargo run --release
```

Au premier lancement, un assistant détecte tes appareils et te propose un préréglage
(Silencieux, Jeu, Bureau, Tout éteint). Tout est ensuite accessible depuis la barre latérale.

> Pour les ventilateurs, le RGB de la RAM et les capteurs avancés, OpenGear doit tourner
> **en administrateur** : utilise le bouton « Relancer en administrateur » de la page
> Système → Diagnostic, ou lance le terminal en administrateur.

### Installer comme une vraie application

```powershell
cargo run --release -- --install
```

Copie OpenGear dans `%LOCALAPPDATA%\Programs\OpenGear`, crée les raccourcis Bureau / Menu
Démarrer et l'entrée « Applications installées ». Désinstallation : `OpenGear.exe --uninstall`
(tes profils, dans `%APPDATA%\OpenGear`, sont conservés).

## Ce que fait OpenGear

| Domaine | Fonctions |
|---|---|
| **Éclairage** | Effets synchronisés sur tous les appareils (statique, arc-en-ciel, respiration, vague, par touche, ambilight écran, visualiseur audio, réactif à la frappe), effets matériels ASUS Aura, RAM DDR5 RGB |
| **Performances** | Fréquence de polling, DPI et paliers, hauteur de levée, ventilateurs et pompe (courbes BIOS, pilotage intelligent CPU/GPU avec hystérésis), plans d'alimentation Windows |
| **Touches & macros** | Remappage, désactivation, macros multi-étapes et texte, enregistreur, SOCD / Snap Tap, anti-rebond clavier et souris, verrou touche Windows |
| **Profils** | Profils multiples, bascule automatique selon le jeu au premier plan, import / export, profils embarqués dans la souris et le clavier |
| **Système** | Zone de notification ultra-légère (WebView2 libérée quand la fenêtre est fermée), démarrage automatique, diagnostic + rapport copiable, sauvegardes de la mémoire flash |
| **Au quotidien** | Reprise propre après la mise en veille, détection instantanée des branchements USB, extinction de l'éclairage au verrouillage de session ou après inactivité, alertes batterie faible (15 % / 5 %) |

Interface **simple par défaut, puissante à la demande** : le mode Simple masque les réglages
avancés ; le mode Expert donne accès à tout. Chaque changement de configuration peut être
**annulé** pendant quelques secondes.

## Matériel supporté

| Marque | Appareil | Protocole |
|---|---|---|
| Corsair | K70 RGB TKL Champion (`1B1C:1B73`) et périphériques Bragi v2 | Bragi v2 (HID) |
| Logitech G | Souris LIGHTSPEED (PRO X Superlight…), claviers RGB, casque PRO X | HID++ 2.0 |
| ASUS | Contrôleur AURA (`0B05:19AF`), ventilateurs via ASUS WMI, RAM ENE | USB HID, WMI, SMBus |
| Nuvoton | SuperIO NCT679x (PWM direct) | PawnIO LpcIO |
| HP | PC portables OMEN / Victus : modes thermiques, ventilateurs, puissance GPU, clavier 4 zones | BIOS WMI `hpqBIntM` |
| Tout clavier | Remappage, macros, Snap Tap, avec la vraie disposition Windows (AZERTY, QWERTZ, UK…) et le format choisi (100 %, TKL, 75 %, 65 %, 60 %, ANSI/ISO) | Hook Windows |

### Pilotes expérimentaux

Ces pilotes sont écrits à partir de documentation de protocole, mais **n'ont pas encore été testés
sur du matériel réel**. Ils sont désactivés par défaut : Système → Préférences → Pilotes expérimentaux.

| Marque | Appareils | Protocole |
|---|---|---|
| Razer | Périphériques à protocole 90 octets (177 modèles) | Razer HID (`opengear-razer`) |
| SteelSeries | Périphériques HID (voir `docs/protocols/steelseries.md`) | HID (`opengear-steelseries`) |
| HyperX | Périphériques HID (voir `docs/protocols/hyperx.md`) | HID (`opengear-hyperx`) |
| OpenRGB | Pont vers un serveur OpenRGB lancé séparément | SDK réseau local (`opengear-openrgb`) |

**Testeur ?** Un appareil de ces marques t'intéresse ? Ouvre une issue
« [Support d'appareil](https://github.com/MathisBrProton/opengear/issues/new/choose) » : les essais
se font en lecture seule et ton retour fait avancer la matrice de compatibilité.

Le scanner (`opengear scan`) liste aussi les appareils détectés mais pas encore pilotés.

## Sécurité

OpenGear touche à du matériel réel. Garde-fous en place :

- **Mémoire flash souris / clavier** : sauvegarde automatique avant chaque écriture
  (`%APPDATA%\OpenGear\backups`), vérification octet par octet, retour automatique à l'ancien
  contenu en cas d'échec, restauration en un clic (Système → Sauvegardes).
- **Ventilateurs et pompe** : planchers de sécurité (pompe ≥ 45 %, CPU ≥ 20 %, PWM direct
  ≥ 30 %, vitesse manuelle des PC portables ≥ 30 %), température de repli élevée si un capteur manque, et **retour au BIOS** à la
  fermeture — y compris après un plantage, grâce à un processus de surveillance.
- **Bus SMBus (RAM RGB)** : aucune écriture sans empreinte ENE vérifiée, adresses SPD jamais
  sondées, verrou partagé avec HWiNFO / OpenRGB / LibreHardwareMonitor.
- **Confirmation explicite** avant toute écriture permanente.

## Ligne de commande

```powershell
opengear scan                       # périphériques HID détectés
opengear telemetry                  # CPU / RAM / GPU / plans d'alimentation
opengear corsair info               # infos clavier Corsair
opengear corsair pollrate 4000      # 125 … 8000 Hz
opengear logitech info              # souris / casque Logitech
opengear asus info                  # contrôleur Aura
opengear cooling status             # ventilateurs & capteurs
opengear hp-probe                   # portable : GPU, batterie, écran, HP OMEN (lecture seule)
```

`opengear --help` et `opengear <commande> --help` donnent la liste complète.

## Développement

Prérequis : Windows 10/11 x64, [rustup](https://rustup.rs). La toolchain (`1.99.1`, cible
`x86_64-pc-windows-gnullvm`) est installée automatiquement via `rust-toolchain.toml`. Aucun
Visual Studio ni MinGW n'est nécessaire : l'édition de liens utilise le `rust-lld` fourni avec
Rust et produit un exécutable statique.

```powershell
cargo build --release
cargo test --workspace
cargo test --workspace -- --ignored   # tests qui nécessitent du vrai matériel
```

Compiler puis lancer la version du dépôt en un clic : `tools/build-and-run.ps1` (`-Admin` pour la
lancer en administrateur, `-BuildOnly` pour seulement compiler). Le script remplace l'instance
OpenGear en cours et exécute la nouvelle version depuis `target\dev-run`, ce qui laisse le
dossier de build libre pour la compilation suivante.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-and-run.ps1
```

### Architecture

| Crate | Rôle |
|---|---|
| `opengear-core` | Types partagés sans marque : modèle d'appareil (`Driver` / `Device` / réglages), couleurs et moteur d'effets, profils TOML, modèle de ventilation, géométrie clavier |
| `opengear-platform` | Services Windows sans marque : client WMI COM, pilote PawnIO, élévation, plans d'alimentation, télémétrie, batterie, écran intégré (appareil partagé), disposition clavier |
| `opengear-mainboard` | Puces de carte mère sans marque : SMBus, RGB de RAM ENE (multi-transport), SuperIO Nuvoton, température CPU, capteurs |
| `opengear-corsair` | Driver Corsair Bragi v2 (RGB par touche, polling, profils flash HWP) |
| `opengear-logitech` | HID++ 2.0 (DPI, batterie, profils et macros embarqués, égaliseur) |
| `opengear-asus` | Aura USB, ventilateurs ASUS WMI, transport SMBus ASUS WMI pour la RAM |
| `opengear-openrgb` | Client du SDK réseau d'OpenRGB (implémentation propre du protocole) |
| `opengear-hp` | HP OMEN / Victus via le BIOS WMI `hpqBIntM` |
| `opengear-razer` | Pilote Razer (expérimental) : protocole 90 octets, tables générées depuis `docs/protocols/` |
| `opengear-steelseries` | Pilote SteelSeries (expérimental, d'après `docs/protocols/steelseries.md`) |
| `opengear-hyperx` | Pilote HyperX (expérimental, d'après `docs/protocols/hyperx.md`) |
| `opengear-input` | Hook clavier bas niveau : remap, macros, SOCD, anti-rebond |
| `opengear-app` | Exécutable : CLI + interface WebView2 (`ui/` embarqué), tray, installeur |
| `tools/opengear-setup` | Installeur autonome (`OpenGear-Setup.exe`) |

Règles d'architecture (une marque = un crate, aucune disposition câblée dans l'UI…) : voir
[`AGENTS.md`](AGENTS.md), vérifiées par `crates/opengear-app/tests/architecture.rs`.

L'interface (`crates/opengear-app/ui/`) est en HTML / CSS / JavaScript sans dépendance,
embarquée dans l'exécutable et reliée au backend Rust par IPC.

### Données utilisateur

| Quoi | Où |
|---|---|
| Profils | `%APPDATA%\OpenGear\opengear_profiles.toml` |
| Sauvegardes flash | `%APPDATA%\OpenGear\backups\logitech\` |
| Marqueur ventilateurs (temporaire) | `%APPDATA%\OpenGear\fan_override.json` |
| Journaux | `%LOCALAPPDATA%\OpenGear\logs\opengear.log` (rotation, 3 fichiers max) |

## Contribuer

Toute aide est bienvenue, surtout les **tests sur matériel** et les **fiches de protocole**.

- [CONTRIBUTING.md](CONTRIBUTING.md) : mise en place, règles, workflow de PR.
- [SECURITY.md](SECURITY.md) : signaler une faille en privé.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) : participation respectueuse.
- Issues `good first issue` et `help wanted` : par où commencer.

## Licence

Code sous **MIT OR Apache-2.0** : tu choisis l'une ou l'autre (`LICENSE-MIT`, `LICENSE-APACHE`).
Les modules PawnIO embarqués sont sous LGPL-2.1 et les photos produit appartiennent à leurs
fabricants : voir [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
