# Protocoles USB HID HyperX complets (Universel, Casques, Souris, Claviers, Micros)

> Document clean-room : tables d'interopérabilité factuelles relevées pour les périphériques HyperX
> (VID `0x0951` Kingston / `0x03F0` HP). Pour le détail des cartes de LED par touche des claviers et
> tapis hérités, voir [hyperx.md](hyperx.md). Tout paramètre non vérifié sur matériel physique reste
> marqué **(incertain)**.

---

## 1. Catalogue USB (VID : PID) et interfaces HID d'exécution

Les PID indiqués comme DFU / bootloader ne doivent **jamais** être ouverts par l'énumération normale.

### 1.1 Souris héritées (HID 264 / 65 octets)

| Modèle | VID | PID (filaire / dongle) | DFU (à exclure) | Interface / Collection HID | Protocole |
|---|---|---|---|---|---|
| Pulsefire Surge | `0x0951` / `0x03F0` | `0x16D3` / `0x0490` | — | `MI_01&Col05` | Feature 264 B `0x07` + RGB `0x14` |
| Pulsefire Raid | `0x0951` / `0x03F0` | `0x16E4` / `0x0290` | `0x16F9` / `0x0390` | `MI_01&Col05` | Feature 264 B `0x07` |
| Pulsefire FPS Pro | `0x0951` / `0x03F0` | `0x16D7` / `0x0E8F` | — | `MI_01&Col05` | Feature 264 B `0x07` |
| Pulsefire FPS Pro 2 (CD) | `0x0951` / `0x03F0` | `0x1729` / `0x0693` | `0x172A` / `0x0793` | `MI_02` | Output 65 B `0xD0` / `0xD3` |
| Pulsefire Core | `0x0951` / `0x03F0` | `0x16DE` / `0x0D8F` | — | `MI_01&Col05` | Feature 264 B `0x07` |
| Pulsefire Dart (filaire) | `0x0951` / `0x03F0` | `0x16E2` / `0x088E` | `0x16F8` / `0x098E` | `MI_01` | Output 65 B `0x51` / `0xD0` / `0xD2` / `0xD3` / `0xDE` |
| Pulsefire Dart (dongle) | `0x0951` / `0x03F0` | `0x16E1` / `0x068E` | `0x16F7` / `0x078E` | `MI_02` | idem sans fil |
| Pulsefire Haste (filaire) | `0x0951` / `0x03F0` | `0x1727` / `0x0F8F` | `0x1728` / `0x0190` | `MI_03` (`Col04` notif `MI_01`) | Feature 65 B `0x04` (profils, DPI, RGB) |
| Pulsefire Haste Wireless (souris) | `0x03F0` | `0x048E` | `0x058E` | `MI_02` | Output/Input 65 B `0x51` / `0xD0` / `0xD3` / `0xDE` |
| Pulsefire Haste Wireless (dongle) | `0x03F0` | `0x028E` | `0x038E` | `MI_02` | idem sans fil |

### 1.2 Souris Universelles HP (`0x03F0`)

Protocole Universel 64 octets (`0x10..0x71`, section 2).

| Modèle | PID principal / filaire | PID dongle | DFU (à exclure) | Interface HID |
|---|---|---|---|---|
| Pulsefire Haste 2 | `0x0B97` | — | `0x0C97` | `MI_02` |
| Pulsefire Haste 2 Wireless | `0x0D97` | `0x0F98` | `0x0E97` / `0x0199` | `MI_02` |
| Pulsefire Haste 2 Mini Wireless | `0x0BA0` | `0x0DA0` | `0x0CA0` / `0x0EA0` | `MI_02` |
| Pulsefire Haste 2 Core | `0x04B5` | — | `0x05B5` | `MI_02` |
| Pulsefire Haste 2 Core Wireless | — | `0x0AB5` | `0x0BB5` | `MI_02` |
| Pulsefire Haste 2 S Wireless | `0x0AB8` | `0x0BB8` | `0x0CB8` / `0x0DB8` | `MI_02` |
| Pulsefire Haste 2 Pro | `0x0ABD` | `0x0CBD` | `0x0BBD` / `0x0DBD` | `MI_02` (souris) / `MI_03` (dongle) |
| Pulsefire Fuse Wireless | `0x0FBD` | — | `0x0FBD&MI_01&Col02` | `MI_02` |
| Pulsefire Saga | `0x0EBE` | — | `0x0FBE` | `MI_02` |
| Pulsefire Saga Pro | `0x04BF` | `0x06BF` | `0x05BF` / `0x07BF` | `MI_02` (souris) / `MI_03` (dongle) |

### 1.3 Claviers complémentaires et Universels

| Modèle | VID | PID | DFU (à exclure) | Interface HID | Protocole |
|---|---|---|---|---|---|
| Alloy MKW 100 | `0x0951` / `0x03F0` | `0x1756` / `0x0B8F` | `0x0C8F` | `MI_00` | Hérité MKW100 **(incertain)** |
| Alloy Origins 2 65 | `0x03F0` | `0x0CC2` | — | `MI_03` | Rapport `0x44` (74 touches) |
| Alloy Eve 1800 | `0x03F0` | `0x08C2` | — | `MI_02` | Rapport `0x44` (10 zones) + `0x40` |
| Alloy Rise | `0x03F0` | `0x0FA0` | `0x01A1` | `MI_02` | Universel 64 B (`0x10..0x71`) |
| Alloy Rise 75 | `0x03F0` | `0x02A1` | `0x03A1` | `MI_02` | Universel 64 B (`0x10..0x71`) |
| Alloy Rise 75 Wireless | `0x03F0` | `0x0EB8` (clavier) / `0x01B9` (dongle) | `0x0FB8` / `0x02B9` | `MI_02` | Universel 64 B (batterie + veille) |

### 1.4 Casques filaires et sans fil

| Modèle | VID | PID | Interface / Collection HID | Protocole |
|---|---|---|---|---|
| Cloud Flight Wireless | `0x0951` / `0x03F0` | `0x16C4`, `0x1723` / `0x0E90` | `MI_03&Col03` | `0x21 0xFF 0x05` + table tension |
| Cloud Flight S | `0x0951` | `0x16EA` | `MI_05&Col03` | `0x06 ... 0xBB` (65 B) |
| Cloud Alpha S | `0x0951` / `0x03F0` | `0x16ED` / `0x0C91` | `MI_05&Col03` (`Col01` DFU) | `0x06 0x01 0x9B ...` + notif `0x0A` |
| Cloud Stinger Core Wired + 7.1 | `0x0951` | `0x1709` | `MI_03&Col02` | Audio/EQ Windows (pas de contrôle HID dédié) |
| Cloud Stinger S | `0x0951` | `0x1715` | `MI_03&Col02` | Audio/EQ Windows |
| Cloud Stinger Core Wireless + 7.1 | `0x0951` | `0x170B` | `MI_03&Col03` | Batterie HID (`GetInputReport`) |
| Cloud II Wireless (hérité) | `0x0951` / `0x03F0` | `0x1718` (dongle), `0x1719` (casque) / `0x0C8A` | `MI_03&Col03` / `Col02` | `0x06 ... 0xBB` (batterie, charge, sidetone, veille) |
| Cloud II Wireless DTS (Onsemi / TI) | `0x03F0` | `0x018B` / `0x058B` (Onsemi), `0x0696` / `0x038B` (TI) | `MI_03&Col03` (dongle) / `Col02` | `0x06 0xFF 0xBB` (batterie, sidetone, veille) |
| Cloud Alpha Wireless | `0x0951` / `0x03F0` | `0x1743`, `0x1765` / `0x098D` | `MI_03&Col01` | `0x21 0xBB` (batterie, charge, mute, sidetone, veille) |
| Cloud Stinger 2 Wireless | `0x03F0` | `0x0D93` | `MI_03&Col03` | `0x06 0xFF 0xBB` |
| Cloud II Core Wireless (+ Tread) | `0x03F0` | `0x0995`, `0x069F` | `MI_03&Col02` | Buffer 264 B ID `0x07` (`buf[1]=0x89/0x8A/...`, resp `0x66 0x07`) |
| Cloud III (filaire) | `0x03F0` | `0x089D` | `MI_03&Col02` | Sidetone + EQ |
| Cloud III Wireless (+ Atlas) | `0x03F0` | `0x0C9D` / `0x0E9D`, `0x05B7` / `0x06B7` | `MI_03&Col01` | Buffer 264 B ID `0x07` (`buf[1]=0x89/0x8A/0x01/0x05/0x02/0x03`) |
| Cloud III S Wireless | `0x03F0` | `0x06BE` | `MI_03&Col05` | En-tête `[buf[1..5]=02 03 00 00 sub, ...]` + TIO child `0x05BE` |
| Cloud Flight 2 | `0x03F0` | `0x0AC1` (dongle), `0x0BC1` (casque) | `MI_03&Col04` | Universel 64 B (`0x10..0x71`) + TIO child `0x0BC1` |
| Cloud MIX Buds / Buds 2 / Mix 2 | `0x03F0` | `0x078A`, `0x039E`, `0x0FAE` | `MI_03&Col04`/`Col05`/`Col06` | Universel / audio casque |
| Dongles 3-en-1 (TIO Emblems) | `0x03F0` | `0x01BF` (appairé), `0x01C5` (non appairé) | `MI_05&Col02` / `MI_04&Col02` / `Col05` | Universel 64 B multiplexé (bits 7..6 d'octet 1) |

### 1.5 Tapis et Microphones (rappel VID : PID complets)

- **Tapis** : `FURY Ultra` (`0x0951:0x1705`, `0x03F0:0x0493`), `Pulsefire Mat RGB` (`0x0951:0x1741`, `0x03F0:0x0F8D`).
- **Microphones** : `QuadCast S` (`0x0951:0x171F`, `0x03F0:0x048C`, `0x068C`, `0x0F8B`, `0x028C`), `DuoCast` (`0x03F0:0x098C`), `SoloCast` (`0x0951:0x170F`, `0x03F0:0x078B`, `0x0592`, `0x098B`, `0x0992`, `0x0B8B`), `QuadCast 2` (`0x03F0:0x09AF` / `0x07AF` / `0x07B4`), `QuadCast 2 S` (`0x03F0:0x02B5` / `0x0D84`).

---

## 2. Protocole Universel HyperX (64 octets)

Paquets HID de **64 octets**. Sous `hidapi`, `buf[0]` porte le *Report ID* :
- `buf[0]` : identifiant de requête (`RequestReports`).
- `buf[1]` : `(sub_id & 0x3F) | ((tio_slot & 0x03) << 6)`, où `tio_slot` vaut `0` (périphérique principal), `1` (`Child1`), `2` (`Child2`) ou `3` (`Child3`).

| Requête | Réponse | Sub-ID | Rôle et charge utile (octets `buf[2..]`) |
|---|---|---|---|
| `0x10` | `0x11` | `0x01` | Info appareil : `2..17` = ID 4×u32 ; `35..36` VID LE ; `41..42` PrimaryPID LE ; `43..44` PrimaryFW ; `45..46` SecondaryPID LE ; `47..48` SecondaryFW ; `53` catégorie (& `0x80` = sans fil) ; `54` LED supportées ; `55` MCU ; `56..59` drapeaux A..D ; `60..61` touches/couches |
| `0x30` | — | `0x01` | Mode jeu : `2..3` masque profils LE, `4` bool actif, `5` bitmask (`1`=Win, `2`=Alt+Tab, `4`=Alt+F4, `8`=Shift+Tab, `16`=Ctrl+Esc) |
| `0x32` | `0x33` | `0x01` (set) / `0x02` (get) | DPI & polling souris : `2..3` masque profils LE, `4` PollingRate (`1`=8kHz, `2`=4kHz, `4`=2kHz, `8`=1kHz, `16`=500Hz, `32`=250Hz, `64`=125Hz), `5` bitmask paliers actifs, `6` index palier actif (`0..4`), puis par palier `i` à `7 + i*5` : `val = (dpi / 50) - 1` en `u16` LE + `R, G, B` |
| `0x36` | `0x37` | `0x01` (set) / `0x02` (get) | Capteur souris : `2..3` masque profils LE, `4` `AngleSnapping` (0/1), `5` angle accordé (`AngleTunes` : `-30°=226`, `-10°=246`, `0°=0`, `+15°=15`, `+30°=30`), `6` accord d'angle actif (`0`/`1`), `7` `LiftDistance` (`0` = 1 mm, `2` = 2 mm) |
| `0x40` | `0x41` | `0x01` (set) / `0x02` (get) | Luminosité : `2..3` masque profils LE, `4` = `pct * 255 / 100` |
| `0x42` | — | `0x02` | Éclairage preset (`PresetLight`) : `2..3` profils LE ; Layer 1 (`4`=mode `0..5` Off/Solid/Breath/Wave/Cycle/Confetti, `5..6`=vitesse LE, `7`=direction `0..6`, `8`=type couleur `0..3`/`254`/`255`, `9..17`=RGB×3) ; Layer 2 réactive (`18`=mode `128..130`, `19..20`=vitesse LE, `21`=direction, `22`=type couleur, `23..`=RGB×3) |
| `0x44` | `0x45` | `0x01`..`0x04` | LED par touche / image : `0x03` entête frame (`2` = nb de blocs), `0x04` paquets de LED (`2..3` index, `4..6` R,G,B) |
| `0x46` | — | `0x01` / `0x03` | Rétroéclairage clavier : `sub=0x01` délai d'extinction auto (`2..3` profils LE, `4` actif `0/1`, `5..6` secondes LE ex. `30/60/300/600/1800`) ; `sub=0x03` capteur de luminosité ambiante (`2..3` profils LE, `4` actif `0/1`, `5..8` paliers `25/50/75/100` ou `100/75/50/25`) |
| `0x50` | `0x51` | `0x01` (seuil) / `0x02` (get) | Batterie : `sub=0x01` seuil batterie faible (`buf[2]` `%` dans `5..25`, `buf[3]` save `0/1`) ; `sub=0x02` réponse `buf[2]` niveau `%` (`0..100`), `buf[3]` charge (`0`=non, `1`=en charge, `2`→`3`/`4`=plein), `buf[4..5]` temp, `buf[6..7]` tension LE |
| `0x52` | `0x53` | `0x01` | État de connexion sans fil |
| `0x54` | `0x55` | `0x01` (set) / `0x02` (get) | Veille auto sans fil : `2..3` masque profils LE (ou profil voulu), `4..6` délai en **secondes** sur 24 bits LE (`sec & 0xFF`, `(sec>>8)&0xFF`, `(sec>>16)&0xFF`) |
| `0x60` | `0x61` | `0x01` (set) / `0x02` (get) | Audio générique : `2` mic mute (`0/1`), `3` volume micro (`0..100`), `4` directivité (`1=Cardioid, 2=Stereo, 3=Bidir, 4=Omni`), `5` filtre global (`0/1`), `6` sidetone (`0/1`), `7` niveau sidetone (`0..100`), `8` balance état (`0/1`), `9` niveau balance (`0..100`), `10` chatmix (`0..100`), `11` playback unmute (`0/1`), `12` volume playback (`0..100`), `13` playback EQ idx, `14` mic EQ idx, `15` reverse light (`0/1`) |
| `0x62` | `0x63` | `0x01` / `0x02` | Audio avancé : `2` invite sonore (`0`=Bip, `1`=Voix) |
| `0x64` | `0x65` | `0x01` / `0x02` | Noise Gate (`VoiceMeter`) : `2` actif (`0/1`) |
| `0x66` | `0x67` | `0x01` (set) / `0x02` (get) | Égaliseur embarqué : `2..3` profils LE, `4` type (`0=Mic, 1=Playback`), `5` index preset, puis par bande `k=0..9` (`off = 5*(k+1)+1`) : `[+0]=freq_hi, [+1]=freq_lo, [+2]=gain_frac (0..9), [+3]=(abs(gain_int) \| (neg ? 0x80 : 0)), [+4]=filtre (0..7)` (gain stocké en facteur `gain_db * 8`) |
| `0x70` | `0x71` | `0x01`.. | Dongle TIO 3-en-1 (statut appairage / mode enfants ; PID enfants : `0x05BE` Cloud III S, `0x0ABD` Haste 2 Pro, `0x04BF` Saga Pro, `0x0BC1` Cloud Flight 2) |

Notifications asynchrones `0xFB` : octet 1 = sub-id, octet 2 = état connexion, octets `3..6` paliers batterie (100 / 75 / 50 / 25 %).

---

## 3. Protocoles Souris héritées (non Universelles)

### 3.1 Pulsefire Dart et Pulsefire Haste Wireless (Output / Input 65 B)

Trame 65 octets, `buf[0] = 0x00`.

- **Batterie** (`0x51`) : envoyer `[0x00, 0x51, 0..]`, lire Input report : `resp[5]` = pourcentage (%), `resp[6]` = statut charge (`0` non, `1` en charge, `2` plein), `resp[7]` = seuil batterie faible.
- **Fréquence d'interrogation** (`0xD0`) : `[0x00, 0xD0, 0x00, 0x00, 0x01, code, 0..]` où `code` ∈ `{0=125 Hz, 1=250 Hz, 2=500 Hz, 3=1000 Hz}` (valeur par défaut constructeur `3` = `1000 Hz` **(incertain sur sens 0↔3 pour Haste Wireless)**).
- **Paliers DPI** (`0xD3`) :
  - Activer paliers : `[0x00, 0xD3, 0x01, 0x00, 0x01, mask, 0..]`
  - Par palier `i` : `[0x00, 0xD3, 0x02, i, 0x02, dpi_lo, dpi_hi]` avec valeur `(dpi / 50)` en `u16` LE (Dart) ou code registre DPI (Haste Wireless), puis couleur `[0x00, 0xD3, 0x03, i, 0x03, R, G, B]`
  - Palier actif : `[0x00, 0xD3, 0x00, 0x00, 0x01, active_idx]`
- **Seuil batterie faible** (`0xD1`, Dart & Haste Wireless) : `[0x00, 0xD1, 0x00, 0x00, 0x01, seuil_5_25, 0..]` en Output + Input report.
- **Effets matériels intégrés** (`0xD2 0x20`, Dart & Haste Wireless) : `[0x00, 0xD2, 0x20, type, 0x08, r1, g1, b1, r2, g2, b2, luminosité, vitesse, 0..]`.
- **Sauvegarde flash** :
  - `Pulsefire Dart` : `[0x00, 0xDE, 0xFF, 0..]` — **uniquement** sur demande explicite de l'utilisateur.
  - `Pulsefire Haste Wireless` : `[0x00, 0xDE, 0x03, 0..]` suite à la configuration.

### 3.2 Pulsefire FPS Pro 2 (`0x1729` / `0x0693`)

Même format `0xD3` / `0xD0` que Pulsefire Dart en `SetOutputReport` 65 octets (sans sauvegarde `0xDE`).

### 3.3 Pulsefire Haste filaire (`0x1727` / `0x0F8F`)

`SetFeatureReport` / `GetFeatureReport` sur tampon **264 octets** (`buf[0] = 0x07`) ou `65 octets` pour les trames RGB directes `0x04 0xF2`:
- Profil (`GetProfile` : `buf[1]=0x04, buf[2]=0x56|0x66, buf[9]=0x01` ; `SetProfile` : `buf[1]=0x03, buf[13]=luminosité*255/100, buf[17]=log2(polling_code), buf[18]=enable_mask, buf[21+2*i]=dpi, buf[33+4*i..+3]=R,G,B, buf[63]=0xAA, buf[64]=0x55`).

---

## 4. Protocoles Casques hérités & Égaliseurs

### 4.1 Famille `0x21 0xBB` (`Cloud Alpha Wireless`)

Rapports de 65 octets (`buf[0] = 0x21, buf[1] = 0xBB, buf[2] = cmd, buf[3] = val`) :

| Commande `buf[2]` | Rôle |
|---|---|
| `0x05` / `0x10` | Lire / régler état Sidetone (`buf[3] = 0|1`) |
| `0x06` / `0x11` | Lire / régler volume Sidetone |
| `0x07` / `0x12` | Lire / régler extinction automatique (`minutes`) |
| `0x0A` / `0x15` | Lire / régler coupure micro (`0|1`) |
| `0x0B` | Lire batterie ; réponse : `resp[2]=0x25` ou `0x0B`, `resp[3]` = `%` |
| `0x0C` | Lire statut charge ; réponse : `resp[2]=0x26` ou `0x0C`, `resp[3]` = état (`0..4`) |

### 4.2 Famille `0x06 0xFF 0xBB` (`Cloud II Wireless DTS`, `Cloud Stinger 2 Wireless`)

`[0x06, 0xFF, 0xBB, sub, val, ...]` :
- `sub = 0x02` : lire batterie (`resp[7]` = `%`)
- `sub = 0x03` : statut charge (`resp[4]` : `0..3`)
- `sub = 0x06` / `0x21` : lire / régler sidetone (`buf[4] = 0|1`)
- `sub = 0x23` : régler volume sidetone
- `sub = 0x07` / `0x22` : lire / régler extinction automatique
- `CMD 33 / 34 / 35` sur `Stinger 2 Wireless` : noise gate (`33`), auto-off (`34`), volume sidetone (`35`).

### 4.3 Famille `0x06 .. 0xBB` & `TuneEQ` (`Cloud II Wireless`, `Cloud Flight S`, `Cloud Alpha S`)

Entête `[0x06, 0x00, 0x02, 0x00, 0x9A, 0x00, 0x00, 0x68, 0x4A, 0x8E, 0x0A, 0x00, 0x00, 0x00, 0xBB, sub, val, ...]` :
- `sub = 0x02` batterie (`resp[7]`), `0x03` charge (`resp[4]`), `0x19` (`25`) sidetone on/off, `0x0C` volume sidetone (`Cloud Flight S`), `0x18` (`24`) / `0x1A` (`26`) extinction auto.
- **`TuneEQ` DSP (`Cloud Alpha S`, `Cloud Flight S`, `Cloud II Wireless`)** : 2 passes par bande (`pass = 0..1`, octet 6 = `4 + pass` sur Alpha S, `10 + pass` sur Flight S / Cloud II Wireless) :
  - `[0x06, 0x00, 0x0D, 0x00, 0x41, 0x00, buf6, 0x30, 0x2B, 0xAE]`
  - puis octets `10..29` : `BandID (i32 LE)`, `FilterType (i32 LE)`, `Frequency (i32 LE)`, `QFactor (f32 LE)`, `Gain_dB (f32 LE)`.

### 4.4 `Cloud Flight Wireless` (`0x16C4`, `0x1723`, `0x0E90`)

Requête `[0x21, 0xFF, 0x05, 0..]` → réponse `[0x21, 0xFF, 0x05, high_byte, low_byte, ...]`.
- `high_byte == 20` (en charge) : `low_byte == 1` → `100 %`, sinon charge en cours.
- Sinon (`high_byte == 14..17`), table de correspondance tension `low_byte` vers `10..100 %` (paliers de 5 %, ex. `(14, >=240) -> 65 %`, `(15, <=19) -> 70 %`, `(15, 20..49) -> 75 %`, `(15, 50..69) -> 80 %`, `(15, 70..99) -> 85 %`, `(15, 100..119) -> 90 %`, `(15, 120..129) -> 95 %`, `(15, >=130) | >=16 -> 100 %`).

### 4.5 `Cloud III Wireless` et `Cloud II Core Wireless` (ID `0x07`, 264 octets)

Tampon 264 octets avec `buf[0] = 0x07`, `buf[1] = cmd`, `buf[2] = arg` :
- `cmd = 0x89` (`137`) : lire batterie ; réponse Input report `0x66 0x07`: si `resp[2] != 0 || resp[3] != 0`, `resp[4]` = niveau batterie (`**(incertain : pourcentage 0..100 ou palier selon firmware)**`).
- `cmd = 0x8A` (`138`) : lire charge (`resp[2] == 1` → en charge).
- `cmd = 0x84` (`132`) / `0x01` : lire / régler sidetone (`buf[2] = 0|1`).
- `cmd = 0x88` (`136`) / `0x05` : lire / régler volume sidetone (borné à `[-5, +5]` dB en `i8` sur `buf[2]`).
- `cmd = 0x02` : extinction auto (`0` jamais, `10`, `20`, `30` minutes).
- `cmd = 0x03` : couper le micro (`0|1`).

### 4.6 `Cloud III S Wireless` (`0x06BE`)

Tampon 264 octets (`buf[0] = 0x07`), préfixe `[buf[1]=0x02, buf[2]=0x03, buf[3]=0x00, buf[4]=0x00, buf[5]=sub, buf[6..]]` :
- `sub = 0x06` batterie (`resp[0]=0x0C` / notif `0x0D`), `0x48` (`72`) charge, `0x01` mic mute (`buf[6]=0|1`), `0x0D` (`13`) sidetone (`buf[6]=0|1`), `0x4A` (`74`) extinction auto en **secondes** `u16` BE (`0`, `600`, `1200`, `1800` → `0 / 10 / 20 / 30` min).
- `sub = 0x0F` (`15`) / `0x1C` (`28`) : régler / lire index de preset EQ embarqué (`buf[6] ∈ 0..5`).
- `sub = 0x5F` (`95`) : régler gain d'une des 10 bandes EQ (`[32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]` Hz) : `buf[6] = band_idx (0..9)`, `buf[7..=8] = ((gain_db * 8.0 * 100.0) as i16).to_be_bytes()`.

### 4.7 `Cloud Alpha S` (`0x16ED`, `0x0C91`)

`[0x06, 0x00, 0x01, 0x00, 0x9B, 0x00, 0x00, 0x68, 0x4A, 0x8E, 0x09, 0x07, ...]`, et notification Input `resp[0] = 0x0A` (`resp[2]=2` → sidetone/mic `resp[4]`).

### 4.8 `Cloud Stinger Core Wireless + 7.1` (`0x170B`)

Requête `SetFeatureReport` (32 B utiles, tampon 65 B) `[0xFF, 0x07, 0x00, 0xFD, 0x04, 0x0C, 0xF1, 0x02, 0x01, 0x04, 0xF0, 0x0C, 0...]` (20 ms d'attente) → `GetFeatureReport` :
- `resp[9]` : état de charge (`3` = décharge, `5` = en charge, `6` = chargé, `>7` = inconnu).
- `voltage = u16(resp[11..13]) / 1000.0` V **(incertain LE/BE sur matériel réel ; trame décodée en LE avec garde de plage `2.5..4.6 V`)**.
- Interpolation linéaire par morceaux sur 11 paliers (0 %..100 % par pas de 10 %) :
  - Charge : `[3.4000, 3.8309, 3.8707, 3.9162, 3.9299, 3.9456, 3.9674, 4.0000, 4.0497, 4.0900, 4.1300]` V
  - Décharge : `[3.5174, 3.6508, 3.6998, 3.7381, 3.7633, 3.7847, 3.8381, 3.9052, 3.9542, 4.0423, 4.1205]` V

### 4.9 Périmètre propriétaire Windows APO / DTS:X

Les égaliseurs et traitements spatiaux des modèles `Cloud III` (filaire), `Cloud III Wireless`, `Cloud II Core Wireless`, `Cloud II Wireless DTS`, `Cloud Stinger 2 Wireless` et `Cloud Alpha Wireless`, ainsi que les filtres temps réel `Limiter` / `Noise Gate` / `AINR` du `DuoCast`, s'exécutent uniquement dans les pilotes audio Windows propriétaires (APO COM / Realtek / DTS Headphone:X) et non via des commandes HID du firmware ; OpenGear n'embarque aucune DLL propriétaire.

---

## 5. Microphones & Tapis complémentaires

- **`QuadCast 2` (`0x09AF`) & `QuadCast 2 S` (`0x02B5`)** — Feature/Output report `0x77` (64 B) :
  - `[0x77, 0x05, pat]` : directivité (`0=Bidirectionnel, 1=Cardioïde, 2=Omnidirectionnel, 3=Stéréo`)
  - `[0x77, 0x06, mute]` : coupure micro (`0/1`)
  - `[0x77, 0x07, hp]` : filtre passe-haut (`0/1`)
  - `[0x77, 0x08, val]` (`QuadCast 2 S`) : mix / gain micro (`0..100`)
  - `[0x77, 0x0A, bal]` (`QuadCast 2 S`) : balance micro / retour (`0..100`)
- **`DuoCast` (`0x098C`)** — Feature report `0x04` (65 B) :
  - `[0x00, 0x04, 0x05, 1|2]` (`1=Cardioïde, 2=Omnidirectionnel`)
  - `[0x00, 0x04, 0x07, 0|1]` (passe-haut)
  - `[0x00, 0x04, 0x08, gain]` (`0..100`)
  - `[0x00, 0x04, 0x09, 0|1]` (sidetone on/off)
  - `[0x00, 0x04, 0x57, ..., 13: brightness(0..255), 17: reverse(0|1)]`
- **`SoloCast`** — `SetFeatureReport` `0x66` (`[0x66, 0x01, mute]`, 65 B).
- **Tapis `FURY Ultra`** (`0x1705` / `0x0493`) — réglage luminosité `[0x00, 0x04, 0x57, ..., 13: brightness*255/100, 63: 0xAA, 64: 0x55]` suivi d'une validation `[0x00, 0x04, 0x02]`.
