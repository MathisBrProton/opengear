# OpenGear

Contrôleur matériel unifié, open-source et léger écrit en **Rust**, conçu pour remplacer les suites propriétaires lourdes (Corsair iCUE, ASUS Armoury Crate, Logitech G HUB, Alienware Command Center) depuis une seule application native sans télémétrie.

## Architecture du workspace

* `crates/opengear-core` : types partagés (RGB, effets, profils TOML, fréquences Hz) et scanner HID multi-marques.
* `crates/opengear-corsair` : driver natif **Corsair Bragi v2** (testé sur **Corsair K70 RGB TKL Champion Series `1B1C:1B73`**) :
  * Détection automatique de la taille de report HID Windows (`HidP_GetCaps` : 65B vs 1025B)
  * Lecture/écriture de la fréquence de polling USB (**125 Hz → 8000 Hz AXON**)
  * Luminosité matérielle, bascule Mode Matériel / Logiciel, keepalive Bragi
  * Contrôle RGB direct par touche (193 LEDs, auto-détection endpoint `0x22` Triplets / `0x01` Planar)
  * Effets temps réel : Statique, Par touche, Arc-en-ciel, Spectre, Respiration, Dégradé
* `crates/opengear-input` : moteur Windows basse latence (`WH_KEYBOARD_LL` + `SendInput`) pour remapping de touches, WinLock et macros multi-étapes / texte.
* `crates/opengear-app` : interface graphique native (`egui`/`eframe`) + CLI dans un seul exécutable.

## Utilisation

```powershell
# Lancer l'interface graphique
cargo run --release

# Scanner tous les périphériques HID détectés (Corsair, ASUS, Logitech, Alienware...)
cargo run --release -- scan

# Lire les infos complètes du clavier Corsair
cargo run --release -- corsair info

# Changer la fréquence de polling (125, 250, 500, 1000, 2000, 4000, 8000)
cargo run --release -- corsair pollrate 4000

# Régler la luminosité matérielle (0..100%)
cargo run --release -- corsair brightness 80

# Tester un effet RGB pendant 5 secondes puis restaurer le mode matériel
cargo run --release -- corsair rgb --effect rainbow --seconds 5
```

## Prochains drivers prévus (déjà détectés par le scanner)

* `opengear-asus` : ASUS AURA LED Controller (`0B05:19AF`, HID `UP:FF72` & LampArray `UP:0059`) + RAM/ventilos
* `opengear-logitech` : Souris LIGHTSPEED (`046D:C547`, HID++ 2.0 : batterie, DPI, polling rate) & Casque PRO X (`046D:0AAA`, EQ/sidetone)
* `opengear-alienfx` : Moniteur Alienware AW2725DF (`187C:1014`)
