# Protocole Logitech G (HID++ 2.0, récepteurs Lightspeed & Centurion)

> **Note salle blanche** : Fiche technique factuelle établie par rétro-ingénierie de l'architecture matérielle HID++/Centurion (sans reprise de code ni de binaire propriétaire tiers). Tout paramètre non vérifié sur matériel réel est signalé explicitement (**incertain**).

---

## 1. Transport USB HID et cadrage

- **VID USB** : `0x046D` (`Logitech`)
- **Interfaces HID++ 2.0** :
  - Usage Page `0xFF00` : rapports courts (`0x10`, 7 octets) et longs (`0x11`, 20 octets), avec usage `0x0001` (court uniquement, 3 octets max de paramètres) ou `0x0002` (long).
  - Usage Page `0xFF43` / usage `0x0202` : interface HID++ étendue sur plusieurs claviers (ex. G513 `046D:C33C`) et périphériques Lightspeed.
- **Format de trame HID++ 2.0** :
  - Octet 0 : ID de rapport (`0x10` court = 7 octets, `0x11` long = 20 octets)
  - Octet 1 : `device_index` (`0xFF` en filaire USB ou casque direct, `0x01..=0x06` derrière un récepteur USB sans fil)
  - Octet 2 : `feature_index` (résolu via Root `0x0000` / FeatureSet `0x0001`)
  - Octet 3 : `(function_id << 4) | software_id` (`function_id` sur les 4 bits de poids fort)
  - Octets 4+ : paramètres de requête ou charge utile de réponse (remplissage à `0x00`)
  - Réponse d'erreur HID++ 2.0 : octet 2 = `0xFF`, octet 3 = `feature_index`, octet 4 = `(fn << 4) | sw_id`, octet 5 = code d'erreur (`0x01..0x09`, `0x08 = Busy`).

### Récepteurs USB identifiés (Lightspeed / Unifying / ChargePad)

| PID | Rôle typique |
|---|---|
| `0xC539` | Récepteur souris/clavier Lightspeed 1re gén. |
| `0xC53A` | Tapis de charge sans fil POWERPLAY + récepteur intégré |
| `0xC53D` | Récepteur clavier G613 / clavier Lightspeed |
| `0xC53F` | Récepteur G305 / Lightspeed |
| `0xC541`, `0xC545`, `0xC547` | Récepteurs Lightspeed / 2K–8K multi-périphériques |

### Transport Centurion (nouvelle génération audio sans fil, ex. G735)

- Certaines gammes audio récentes utilisent un transport dédié (`CenturionTransport` / `CenturionPacketLayer`) au-dessus du bus HID, avec racine `0x0000`, table de features `0x0001`, et routage sous-périphérique `0x0003`.
- **Important** : le cadrage exact des rapports `CenturionPacketLayer` n'a pas encore été capturé sur un casque Centurion réel (**incertain**). La prise en charge sur matériel réel nécessitera une capture HID dédiée ; les identifiants de features Centurion (`0x0108`, `0x0600`..`0x0636`) ci-dessous décrivent les payloads applicatifs correspondants.

---

## 2. Features système, type d'appareil & batterie

### Type d'appareil (`0x0005` DeviceType / Name)

- `DeviceType` HID++ 2.0 : `0 = Keyboard`, `3 = Mouse`, `8 = Headset`.
- Certains claviers récents (ex. G513 `046D:C33C`) exposent un endpoint audio/HID secondaire ou une table de features spécifique : il est impératif de vérifier `DeviceType == 0` et l'absence de features DPI/Audio avant de classer un appareil.

### Batterie unifiée (`0x1004` Unified Battery)

| Fonction | Sens | Format octets | Détail |
|---|---|---|---|
| `fn 0` (`0x00`) | Réponse | `[flags_niveaux, flags, nb_niveaux]` | Capacités batterie |
| `fn 1` (`0x10`) | Réponse | `[soc_pct, level_0_3, status, ext_power, ...]` | `soc_pct` (0..100), `status`: `0 = Discharging`, `1 = Charging`, `2 = AlmostFull`, `3 = Full`, `4 = SlowCharging`, `5..=7 = Error` |
| `fn 2` (`0x20`) | Requête | `[]` | Demande l'affichage/notification batterie sur l'appareil |

### Tension batterie (`0x1001` Battery Voltage)

| Fonction | Sens | Format octets |
|---|---|---|
| `fn 0` (`0x00`) | Réponse | `[mv_hi, mv_lo, flags]` (`bit 7` = en charge) |

### Modèles d'autonomie (`powermodel` / `battery_hours`)

Les courbes de décharge et bilans de puissance (`Wh` / `mAh` vs courant MCU + capteur + polling radio) permettent d'estimer les heures restantes à partir du SoC (%) et du polling rate actif. Exemples vérifiés sur modèles matériels :
- `PRO X SUPERLIGHT` (`046D:4093`) : `874 mWh`, capteur HERO `1.5456 mW`, MCU STM32 `2.3064 mW`, radio selon intervalle (`8ms: 2.2036 mW`, `4ms: 3.0738 mW`, `2ms: 4.8142 mW`, `1ms: 8.2951 mW`) (~70 h max à 1000 Hz).
- Autres souris/casques/claviers sans fil : estimation proportionnelle au pourcentage mesuré via les autonomies nominales constructeur (`G502 LIGHTSPEED ~60h`, `G903 HERO ~140h`, `G703 HERO ~35h`, `G305 ~250h`, `G915 ~30h`, `G915 TKL ~40h`, `G533 ~15h`, `G535 ~33h`, `G733 ~29h`, `G735 ~56h`, `PRO X WIRELESS ~20h`, `G933 / G935 ~12h`).

### Paramètres globaux & veille (`0x8103` Global Device Settings)

| Fonction | Requête | Réponse | Tags connus (BE16) |
|---|---|---|---|
| `fn 2` (`0x20`) | `[tag_hi, tag_lo]` | Données du tag | `0x0001` : délai d'économie d'énergie (`PowerSavingTimeout`), `0x0002` : délai d'extinction (`PowerOffTimeout`), `0x0003` : effet lumineux personnalisé, `0x0011` : luminosité écran |
| `fn 5` (`0x50`) | `[tag_hi, tag_lo, val...]` | Écho | Encodage en minutes (`0 = Jamais`, valeurs UI standard : `1, 2, 5, 10, 15, 30, 60`) |

---

## 3. Fonctionnalités Souris

| Feature ID | Nom | Fonctions principales (4 bits forts) | Format & valeurs |
|---|---|---|---|
| `0x2110` | SmartShift | `fn 0` lire, `fn 1` écrire | `[wheel_mode (1=FreeSpin, 2=Ratchet), auto_disengage (1..=100, 0xFF=off)]` |
| `0x2111` | SmartShift Tunable Torque | `fn 0` caps, `fn 1` lire, `fn 2` écrire | Caps: `[flags (bit0=tunable), def_disengage, def_torque, max_force]`. Get/Set: `[wheel_mode, auto_disengage, torque]` |
| `0x2130` | Ratchet Wheel | `fn 0` lire, `fn 1` écrire | `[divert_flag (0/1)]` |
| `0x2201` | Adjustable DPI | `fn 0..fn 3` | Sensor 0, BE16 DPI min/max/actif |
| `0x2202` | Extended Adjustable DPI | `fn 0` count, `fn 1` caps, `fn 2` plages, `fn 4` LOD list, `fn 5` lire, `fn 6` écrire | `flags bit0` = X/Y indépendant, `flags bit1` = LOD (`0=Low, 1=Med, 2=High`). Set `fn 6`: `[idx, x_hi, x_lo, (y_hi, y_lo)?, (lod)?]` |
| `0x2205` | Angle Snapping | `fn 0` lire, `fn 1` écrire | `[0/1]` |
| `0x2240` | Surface Tuning | `fn 0` lire état (9 octets), `fn 1` appliquer | Calibration surface capteur (**incertain** sur détail des modes surface avancés) |
| `0x8060` | Report Rate | `fn 0` caps (ms bitmask), `fn 1` lire, `fn 2` écrire | Intervalle en ms (`8=125 Hz, 4=250 Hz, 2=500 Hz, 1=1000 Hz`) |
| `0x8061` | Extended Report Rate | `fn 0` caps, `fn 1` list BE16, `fn 2` lire, `fn 3` écrire | Index `0..6` → `125, 250, 500, 1000, 2000, 4000, 8000 Hz` |
| `0x8100` | Onboard Profiles | `fn 0..fn C` | Mémoire embarquée flash/ROM (profils DPI/boutons/RGB, CRC-16 CCITT) |

---

## 4. Fonctionnalités Clavier & Contrôles

| Feature ID | Nom | Fonctions principales | Format & valeurs |
|---|---|---|---|
| `0x40A2` | Fn Inversion | `fn 0` lire, `fn 1` écrire | Get: `[inverted != 0, gshift != 0]`. Set: `[enabled (0/1)]` |
| `0x40A3` | Multi-Host Fn Inversion | `fn 0` lire `[host_idx]`, `fn 1` écrire `[host_idx, inverted]` | Get: `[host, inverted, default, caps]` (`host_idx = 0` par défaut) |
| `0x4521` | Disable Keys (Game Mode) | `fn 0` caps, `fn 1` lire `[0]`, `fn 2` écrire `[mask]` | Masque 5 bits : `bit0 = Win Gauche (0x01)`, `bit1 = Win Droite (0x02)`, `bit2 = Menu/Contexte (0x04)`, `bit3 = CapsLock (0x08)`, `bit4 = Tab (0x10)`. Mode Jeu standard = `0x07` (Win G/D + Menu) ou `0x03` (Win G/D). |
| `0x4522` | Disable Keys By Usage | `fn 0` max count, `fn 1` désactiver `[usages...]`, `fn 2` réactiver, `fn 3` reset | Usages HID (ex. `0xE3` Win Gauche, `0xE7` Win Droite, `0x65` Menu). |
| `0x4523` | Disable Controls | `fn 0..fn 4` | Désactivation étendue par identifiants de contrôles |
| `0x8010` | G-Key | `fn 0` count, `fn 1` layout BE16, `fn 2` software control `[divert]` | Touches programmables G1..G9 |
| `0x1982` | Backlight | `fn 0` info, `fn 1` lire état, `fn 2/3` modifier | Rétroéclairage monochrome / niveau / extinction d'inactivité |
| `0x1983` | Keyboard Backlight | `fn 0..fn 3` | Rétroéclairage clavier (niveau, minuterie d'inactivité en s/min) |
| `0x1990` | Illumination Light | Get/Set illumination & luminosité | Éclairage fonctionnel / webcam |

---

## 5. Éclairage RGB & Touche-par-Touche (`0x8040`, `0x8070`, `0x8071`, `0x8080`, `0x8081`)

- **`0x8040` Brightness Control** : `fn 0` caps (`max_be16`), `fn 1` lire `val_be16`, `fn 2` écrire `[hi, lo]`, `fn 3` lire activation (`byte0 & 1`), `fn 4` activer/couper.
- **`0x8070` Color LED Effects** : `fn 0` nombre zones, `fn 1` info zone `[z]`, `fn 3` set effect `[zone, effect_id, R, G, B, sp_hi, sp_lo, ...]` (`0x00=Off, 0x01=Static, 0x03=Cycle, 0x0A=Breathing`).
- **`0x8071` RGB Effects** : `fn 0` discovery (`[0xFF, 0xFF, 0]` / `[zone, 0xFF, 0]`), `fn 1` set effect (`[cluster, effect_id, R, G, B, ...]`, `0x04=Wave`).
- **`0x8080` Per-Key Lighting (v0, ex. G910/G810)** :
  - `fn 0` info générale (comptes BE16), `fn 1` info groupe `[grp_hi, grp_lo]`
  - `fn 2` / `fn 3` écrire couleurs par touches (`[group_hi, group_lo, count_hi, count_lo, (idx, R, G, B)...]`)
  - `fn 4` SetAllKeysToColor (`[group_hi, group_lo, R, G, B, 0, ...]`)
  - `fn 5` Flush (`[persist]`) pour afficher le buffer LED.
- **`0x8081` Per-Key Lighting (v1, ex. G513/G815/G915)** :
  - `fn 0` init `[0, 0]`
  - `fn 1` SetIndividualRgbZones : jusqu'à 4 tuples `(key_id, R, G, B)` par rapport long de 16 octets (padding `0xFF`)
  - `fn 6` SetRangeRgbZones : `[cluster, idx_début, count, params...]`
  - `fn 7` FrameEnd : validation de trame `[b0, b1, b2, frame_hi, frame_lo]` pour synchroniser les LED.

---

## 6. Audio HID++ & Casques (`0x8300`, `0x8305`, `0x8310` & Centurion `0x0108`/`0x06xx`)

| Feature ID | Nom | Fonctions / Format | Notes |
|---|---|---|---|
| `0x8300` | Sidetone | `fn 0` lire `[pct]`, `fn 1` écrire `[pct]` (`0..100`), `fn 2` getMute, `fn 3` setMute | Retour microphone matériel |
| `0x8305` | Bass Tone / Acoustics | `fn 0` lire `[level]`, `fn 1` écrire `[level]` | Renforcement des basses matériel quand présent |
| `0x8310` | Equalizer | `fn 0` info (`[count, db_range, ..., min_i8, max_i8]`), `fn 1` freqs, `fn 2` lire `[i8...]`, `fn 3` écrire `[save, i8...]` | 10 bandes standard `[32, 64, 125, 250, 500, 1K, 2K, 4K, 8K, 16K]` ou 5 bandes PRO `[80, 240, 750, 2200, 6600]` |
| `0x0108` | Centurion AutoSleep | `fn 0` lire, `fn 1` écrire `[minutes_0, ...]`, `fn 2` état | Minuterie de veille sans fil |
| `0x0600` | Centurion RGB Effects | `fn 0..8` | Effets et contrôle SW/NV |
| `0x0601` | Centurion Mic Mute | `fn 0/3` lire, `fn 1` écrire bit0 | Souris/casque mute micro matériel |
| `0x0604` | Centurion Sidetone | `fn 0` lire, `fn 1` écrire (`[0xFF, level]` v2+, `[level]` v1) | Retour micro Centurion |
| `0x0605` | Centurion ANC | `fn 0` info, `fn 1` mode, `fn 2` écrire mode | Réduction de bruit active / transparence |
| `0x060B` | Centurion Headset Tones | `fn 0..5` | Invite vocale / bips système |
| `0x0611` | Centurion Mic Gain | `fn 0` info, `fn 1` lire, `fn 2` écrire `[gain]` | Gain entrée micro matériel |
| `0x0618` | Centurion Battery Saver | `fn 0` lire, `fn 1` écrire `[enabled]` | Mode éco batterie |
| `0x0636` | Centurion Onboard EQ | `fn 0` info (`num_bands`, `[freq_be16, min, max]` par bande), `fn 1` lire `[slot]`, `fn 2` écrire `[slot, ...]` | 10 bandes `[48, 64, 125, 250, 500, 1K, 2K, 4K, 8K, 16K]` ou 5 bandes `[125, 250, 1K, 4K, 8K]` |

> Les filtres logiciels Windows (**Blue VO!CE 1/2** et **DTS Headphone:X 2.0**) reposent sur des APO COM attachés au pilote audio Windows et sont documentés séparément dans [logitech-windows-apo.md](../a-faire/logitech-windows-apo.md).
