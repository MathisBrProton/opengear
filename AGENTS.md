# Règles pour les agents (et les humains) qui modifient OpenGear

Ces règles sont **obligatoires**. Les règles d'architecture sont vérifiées automatiquement par
`crates/opengear-app/tests/architecture.rs` : un `cargo test` rouge sur ce fichier signifie que la
modification est mal placée, pas que le test est à assouplir.

## 1. Architecture des crates

```
opengear-core        types partagés, sans marque : modèle d'appareil (`driver`), couleurs/effets,
                     profils TOML, modèle de ventilation (`cooling`), géométrie clavier
opengear-platform    services Windows sans marque : client WMI COM, pilote PawnIO, mutex
                     inter-applications, élévation UAC, plans d'alimentation, télémétrie,
                     batterie/écran portable, disposition clavier OS
opengear-mainboard   puces de carte mère sans marque : SMBus (trait `SmbusBus`), contrôleurs RGB
                     de RAM ENE, SuperIO Nuvoton, température CPU, capteurs
opengear-input       hook clavier/souris : remap, macros, SOCD, anti-rebond
opengear-<marque>    UN crate par fabricant ou protocole : corsair, logitech, asus, hp, openrgb…
opengear-app         orchestration : CLI, backend de l'UI, tray, installeur. Aucun protocole.
                     `src/drivers.rs` est le SEUL endroit où les pilotes sont branchés.
```

Dépendances autorisées (vérifiées par le test) :

| Crate | Peut dépendre de |
|---|---|
| `opengear-core` | rien d'interne |
| `opengear-platform` | `opengear-core` |
| `opengear-mainboard` | `opengear-core`, `opengear-platform` |
| `opengear-input`, `opengear-<marque>` | `opengear-core`, `opengear-platform`, `opengear-mainboard` |
| `opengear-app` | tout |

Interdits absolus :

- **Un crate marque ne dépend jamais d'un autre crate marque.** Du code HP n'a rien à faire dans
  `opengear-asus` (c'est exactement l'erreur corrigée en novembre 2026).
- **Rien de propre à une marque dans `core`, `platform` ou `mainboard`** : classes WMI (`hpqBIntM`,
  `ASUSManagement`), noms de protocole (Bragi, HID++), valeurs « fil » d'un protocole. Si une brique
  générique manque (ex. appel WMI avec objet imbriqué), on l'ajoute **générique** dans `platform`
  et le wrapper spécifique reste dans le crate marque.
- **`opengear-app` ne parle jamais au matériel** : pas de HID, WMI, `DeviceIoControl`. Il appelle les
  crates pilotes.

### Où mettre du code ?

1. Ça dépend d'une marque / d'un modèle précis ? → `opengear-<marque>` (le créer s'il n'existe pas,
   l'ajouter au workspace **et** à `VENDOR_MARKERS` dans `tests/architecture.rs`).
2. C'est un service Windows utile à plusieurs marques ? → `opengear-platform`.
   Une puce standard de carte mère (bus, contrôleur générique) ? → `opengear-mainboard`.
3. C'est un type de données ou un calcul pur partagé ? → `opengear-core`.
4. C'est de l'assemblage pour l'UI ou la CLI ? → `opengear-app`.

### Ajouter un appareil ou une marque (ex. Razer)

1. Crate `opengear-razer` : le protocole, et `impl Driver` + `impl Device` (`opengear_core::driver`).
2. `Device::settings()` décrit chaque réglage comme **donnée** (`Setting::toggle/slider/choice/color`,
   `.expert()`, `.stored_in_device()`, `.help()`). Réglage partagé → id de `setting_ids`
   (polling, luminosité, verrou Windows, SOCD matériel…) ; réglage propre à la marque → id préfixé
   (`"razer.hyperpolling"`). `apply_setting` ne reçoit que des valeurs déjà validées.
3. Une ligne dans `device_drivers()` de `crates/opengear-app/src/drivers.rs`.
4. C'est tout : l'appareil apparaît dans Appareils, ses réglages dans chaque onglet (filtrés par
   section et par mode Simple / Expert), sa batterie dans les alertes. **Aucune commande IPC ni
   aucun écran par marque à écrire** : la commande générique est `device_set`.
5. Un transport RGB de RAM : `impl RamRgbBackend` (`opengear_mainboard::ram`) + une ligne dans
   `ram_rgb()`. Un bus SMBus constructeur : `impl SmbusBus`, et le protocole ENE est réutilisé tel
   quel.

Capacités riches (éclairage synchronisé, égaliseur, paliers DPI, mémoire embarquée, courbes de
ventilateur) : chacune devient un trait à côté de `Device` avec **une** vue générique. Tant
qu'une capacité n'est pas migrée, elle reste servie par l'ancien code de la marque — ne pas en
ajouter de nouvelle sous cette forme.

## 2. Matériel « dynamique », jamais câblé sur un appareil

- L'UI ne code **aucune** disposition, liste de touches ou image propre à un appareil. Elle affiche ce
  que le backend décrit (`S.keyboard`, listes d'appareils, capacités).
- Clavier : une touche est identifiée par sa **position physique** (`code` W3C : `KeyQ`, `Digit1`,
  `IntlBackslash`…). Son libellé et son identifiant d'assignation viennent de la **disposition
  Windows active** (`opengear_platform::KeyResolver`), jamais d'une table AZERTY/QWERTY.
  - géométrie générique : `opengear_core::keyboard::physical_layout(format, norme)` ;
  - carte exacte d'un appareil (LED par touche) : fournie par son crate (ex.
    `opengear_corsair::keys_for_layout`), avec un `code` physique par touche ;
  - assemblage : `opengear-app/src/ui/keyboard_view.rs`.
- Les noms de `opengear_input::keys::KEY_TABLE` sont des identifiants **persistés dans les profils** :
  ne jamais les renommer ; l'UI ne les affiche pas (elle affiche le libellé Windows).
- Un nouvel appareil ne doit demander **aucune** modification de l'UI pour apparaître correctement.

## 3. Sécurité matérielle (ne jamais affaiblir)

- Écriture en flash (souris, clavier) : sauvegarde avant, vérification octet par octet, retour
  arrière automatique, confirmation explicite dans l'UI.
- Ventilateurs/pompe : planchers (pompe ≥ 45 %, CPU ≥ 20 %, PWM direct ≥ 30 %), repli sur température
  haute si capteur absent, retour BIOS à la fermeture **et** via le processus de surveillance.
- SMBus : pas d'écriture sans empreinte vérifiée, adresses SPD jamais sondées, verrou partagé.
- Toute nouvelle écriture matérielle : lecture préalable, garde-fous équivalents, test unitaire de la
  logique de sécurité, et chemin en lecture seule (`probe`) pour le diagnostic.

## 4. Avant de rendre la main

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

La CI exécute exactement ces commandes. Si `cargo fmt` ou `cargo clippy` est introuvable :
`rustup component add --toolchain 1.99.1-x86_64-pc-windows-gnu rustfmt clippy`.

- Pas de `#[allow(...)]` pour faire taire clippy sans raison écrite à côté.
- Ce qui ne peut être vérifié qu'avec le vrai matériel doit être **dit** (test `#[ignore]` + mention
  dans le compte rendu), jamais présenté comme vérifié.
- Interface en français ; code, identifiants et commentaires techniques en anglais.

## 5. Dette connue (à résorber, ne pas aggraver)

- Corsair et Logitech sont encore ouverts par le moteur d'éclairage via des listes typées
  (`corsair_devices`, `logi_*`) au lieu du registre `device_drivers()` : à migrer quand le trait
  d'éclairage existera. Leurs réglages simples passent déjà par le modèle commun.
- Commandes encore propres à une marque, à remplacer par des capacités : éclairage (`corsair_set_mode`,
  `asus_*`, `logi_set_led`, `omen_set_kb_*`), couleurs de voyants Corsair (écriture seule),
  paliers DPI, égaliseur, mémoire embarquée Logitech / Corsair, ventilateurs (ASUS, SuperIO, OMEN),
  réglages OMEN (mode thermique, puissance GPU) et écran du portable.
- `opengear-core::profile` stocke des réglages par marque (Aura, OMEN…) dans le profil global.
- JIS / ABNT utilisent la géométrie ISO dans `physical_layout`.
