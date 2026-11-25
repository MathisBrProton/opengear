# SteelSeries — fiche protocole

Source analysée : OpenRGB f899bc8, dossier `Controllers/SteelSeriesController`. Faits rédigés pour
l'interopérabilité ; aucun code repris. **(incertain)** = non établi par le source.

VID **0x1038** pour tous les appareils (attention : les claviers de portables MSI utilisent aussi
0x1038 et relèvent d'un autre pilote).

Conventions de ce document :

- « output report » = écriture HID sur le canal de sortie (`WriteFile` / `HidD_SetOutputReport`) ;
  « feature » = `HidD_SetFeature` ; « lecture » = input report lu sur le canal d'entrée.
- Sauf mention contraire, l'octet 0 des tampons est l'**ID de rapport 0x00** (non transmis sur le
  fil par Windows pour un appareil sans ID) ; les offsets donnés **incluent** cet octet 0, ce qui
  correspond à la façon d'écrire le tampon côté hôte.
- Couleurs en **R, G, B** sauf mention contraire.

## 1. Appareils

| Nom | VID:PID | Type | Famille de protocole (§) | HID (interface / page / usage) | LED |
|---|---|---|---|---|---|
| Aerox 3 (filaire) | 1038:1836 | souris | Aerox filaire (§3.2) | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 3 Wireless (2,4 GHz) | 1038:1838 | souris | Aerox sans fil (§3.3) | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 3 Wireless (câble) | 1038:183A | souris | Aerox sans fil, mode câble | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 3 CS2 Dragon Lore (2,4 GHz) | 1038:1878 | souris | Aerox sans fil | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 3 CS2 Dragon Lore (câble) | 1038:187A | souris | Aerox sans fil, mode câble | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 3 Wireless Gen 2 | 1038:1890 | souris | Aerox sans fil | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 5 (filaire) | 1038:1850 | souris | Aerox 5 filaire (§3.2) | 3 / 0xFFC0 / 0x01 | 4 |
| Aerox 5 Wireless (2,4 GHz / câble) | 1038:1852 / 1854 | souris | Aerox sans fil | 3 / 0xFFC0 / 0x01 | 3 (voir §3.3) |
| Aerox 5 Destiny 2 (2,4 GHz / câble) | 1038:185C / 185E | souris | Aerox sans fil | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 5 Diablo IV (2,4 GHz / câble) | 1038:1860 / 1862 | souris | Aerox sans fil | 3 / 0xFFC0 / 0x01 | 3 |
| Aerox 9 Wireless (2,4 GHz) | 1038:1858 | souris | Aerox sans fil | 3 / 0xFFC0 / 0x01 | 4 |
| Aerox 9 Wireless (câble) | 1038:185A | souris | Aerox sans fil, mode câble | 3 / 0xFFC0 / 0x01 | 4 |
| Rival 3 Wireless (2,4 GHz) | 1038:1830 | souris | Aerox sans fil, variante Rival 3 W | 3 / 0xFFC0 / 0x01 | 1 |
| Rival 3 (ancien firmware) | 1038:1824 | souris | Rival 3 (§3.1) | 3 / * / * | 4 |
| Rival 3 | 1038:184C | souris | Rival 3 (§3.1) | 3 / * / * | 4 |
| Rival 5 | 1038:183C | souris | Rival 5 (§3.4) | 0 / 0xFFC0 / 0x01 | 10 |
| Rival 100, 100 DotA 2, 105, 106, 110 | 1038:1702, 170C, 1814, 1816, 1729 | souris | Rival ancien, variante 100 (§3.5) | 0 / * / * | 1 |
| Rival 300, 300 Black Ops (même PID), Acer Predator (Rival 300), 300 CS:GO Fade, 300 CS:GO Fade (stm32), 300 Hyperbeast, 300 Dota 2, 300 HP Omen | 1038:1710, 1714, 1394, 1716, 171A, 1392, 1718 | souris | Rival ancien, variante 300 | 0 / * / * | 2 |
| Rival 600, 600 Dota 2 | 1038:1724, 172E | souris | Rival ancien, variante 600 | 0 / * / * | 8 |
| Rival 650, 650 Wireless | 1038:172B, 1726 | souris | Rival ancien, variante 650 | 0 / * / * | 8 |
| Rival 700, 710 | 1038:1700, 1730 | souris | Rival ancien, variante 700 | 0 / * / * | 2 |
| Rival 310, 310 CS:GO Howl, 310 PUBG | 1038:1720, 171E, 1736 | souris | Sensei / Rival 310 (§3.6) | 0 / * / * | 2 |
| Sensei TEN, Sensei TEN Neon Rider, Sensei 310 | 1038:1832, 1834, 1722 | souris | Sensei (§3.6) | 0 / * / * | 2 |
| Siberia 350 | 1038:1229 | casque | Siberia (§5.1) | 3 / * / * | 1 |
| Arctis 5 (deux révisions) | 1038:1250, 12AA | casque | Arctis 5 (§5.2) | 5 / * / * | 2 |
| Arctis Nova 3 | 1038:12EC | casque | Arctis Nova 3 (§5.3) | 4 / 0xFFC0 / 0x01 | 2 |
| QCK Prism Cloth Medium, XL, XL Destiny, XL Destiny 2 Lightfall, XL CS:GO Neon Rider, XL CS:GO Neo Noir, 3XL, 4XL, 5XL | 1038:150A, 150D, 151E, 1520, 1514, 151C, 1516, 1518, 151A | tapis | QCK (§6) | 0 / * / * | 2 |
| Apex 5, Apex 7, Apex 7 TKL, Apex Pro, Apex Pro TKL, Apex Pro 3 | 1038:161C, 1612, 1618, 1610, 1614, 1640 | clavier | Apex (§4.1), génération 1 (Pro TKL : 1 ou 3 selon firmware) | 1 / * / * | jusqu'à 112 |
| Apex 9 TKL, Apex 9 Mini | 1038:1634, 1620 | clavier | Apex, génération 2 (3 si firmware ≥ 1.19.7), rapport court | 1 / * / * | idem |
| Apex Pro TKL 2023 (filaire) | 1038:1628 | clavier | Apex, génération 2 | 1 / * / * | idem |
| Apex Pro TKL 2023 Wireless | 1038:1630, 1632 | clavier | Apex, génération 3, variante sans fil | 3 / 0xFFC0 / 0x01 | idem |
| Apex Pro TKL Gen 3 (filaire) | 1038:1642 | clavier | Apex, génération 3 | 1 / * / * | idem |
| Apex Pro TKL Gen 3 Wireless | 1038:1644, 1646 | clavier | Apex, génération 3, variante sans fil | 3 / 0xFFC0 / 0x01 | idem |
| Apex M750 | 1038:0616 | clavier | Apex M (§4.2) | 2 / * / * | 112 (≈104 utilisées) |
| Apex (OG) / Apex Fnatic, Apex 350 | 1038:1202, 1206 | clavier | Apex ancien (§4.3) | 0 / * / * | 5 zones |
| Apex 3 | 1038:161A | clavier | Apex 3, 10 zones (§4.4) | 3 / * / * | 10 |
| Apex 3 TKL | 1038:1622 | clavier | Apex 3 TKL, 8 zones (§4.5) | 1 / 0xFFC0 / 0x01 | 8 |

Collisions de PID : 0x1710 désigne à la fois le Rival 300 et le Rival 300 Black Ops ; 0x185A est
déclaré deux fois (Aerox 9 filaire et Aerox 9 Wireless en mode câble) et seul le détecteur « Aerox 9
Wireless (câble) » est enregistré. Le nombre réel de LED d'un Aerox 5 Wireless n'est pas clair : le
pilote sans fil ne déclare que 3 LED (pas de LED « réactive ») alors que l'Aerox 5 filaire en déclare
4 **(incertain)**.

## 2. Principes communs

- Pas de somme de contrôle dans aucun des protocoles SteelSeries vus ici.
- Les souris « modernes » (Aerox, Rival 3, Rival 5) partagent un jeu de commandes à un octet :
  0x21 couleur, 0x22 effet arc-en-ciel, 0x23 luminosité, 0x26 couleur réactive, 0x2D réglage DPI,
  0x90 version, 0x11 sauvegarde. Les anciennes souris (Rival 100–700, Sensei) ont des commandes
  différentes.
- Mutex inter-logiciels : aucun.
- Plusieurs commandes de sauvegarde écrivent en **mémoire non volatile** (§7).

## 3. Souris

### 3.1 Rival 3 (1824, 184C)

Output reports de taille variable, octet 0 = 0x00 :

| Message | Taille | Octets 1.. |
|---|---|---|
| Couleur d'une zone | 8 | 0x05, 0x00, zone, R, G, B, luminosité (0–100) |
| Effet global | 4 | 0x06, 0x00, effet |
| Version firmware | 3 | 0x10, 0x00 ; puis lire 2 octets = entier 16 bits petit-boutiste |
| Sauvegarde | 10 | 0x09 (reste à zéro) |

Zones : 1 avant, 2 milieu, 3 arrière, 4 logo (valeurs envoyées telles quelles).

Effets (octet « effet ») : 0x00 cycle spectral, 0x01 respiration rapide, 0x02 respiration moyenne,
0x03 respiration lente, 0x04 direct/statique, 0x05 respiration arc-en-ciel, 0x06 « disco » (flashs
aléatoires : désactivé dans OpenRGB et dans le logiciel du fabricant, risque photosensible, et noir
s'il suit directement une respiration). Le Rival 3 propose : direct, respiration (vitesse 0–2 →
effet 0x03 − vitesse), cycle spectral, respiration arc-en-ciel.

Séquence d'une mise à jour : pour chaque zone, message « couleur » (la luminosité du mode y est
incluse), puis message « effet » avec la valeur du mode. Sauvegarde : message 0x09 sur demande
explicite.

### 3.2 Aerox 3 et Aerox 5 filaires (1836, 1850)

Rapports de **65 octets**, octet 0 = 0x00.

| Message | Transport | Octets 1.. |
|---|---|---|
| Initialisation (envoyée à l'ouverture) | feature | 0x2D (le reste à zéro) |
| Version | feature | 0x90, 0x00 ; puis lectures (délai 250 ms) jusqu'à recevoir un rapport commençant par 0x90 ; version = 16 caractères ASCII à partir de l'octet 1 |
| Couleur Aerox 3 | output | 0x21, masque = 1 << zone, puis R,G,B à l'offset 3 + 3·zone |
| Couleur Aerox 5, zones 0–2 | output | 0x21, masque (0x01, 0x02, 0x04), R,G,B aux offsets 3 / 6 / 9 selon la zone |
| Couleur Aerox 5, zone 3 « réactive » | output | 0x26, 0x01, 0x00, R, G, B |
| Luminosité | output | 0x23, valeur 0–100 (envoyée seulement si elle change) |
| Sauvegarde | output | 0x11, 0x00 |

Zones : 0 avant, 1 milieu, 2 arrière (Aerox 5 : 3 réactive). L'Aerox 5 attend 2 ms après chaque
message. Seul le mode direct est proposé (pas d'effet matériel). Le paquet de sauvegarde vient d'un
projet tiers et n'a pas été confirmé comme fonctionnel par les auteurs d'OpenRGB.

**Attention, initialisation 0x2D** : d'après le source, cette commande règle la **sensibilité
(paliers DPI)** : octet 2 = nombre de paliers (0–5), octets 4–8 = DPI / 50 (0x04–0xC7). OpenRGB
l'envoie avec des zéros pour « autoriser le mode logiciel ». Cela peut donc écraser les paliers DPI
de l'utilisateur **(effet exact incertain)**. À ne pas reproduire sans lecture préalable des
paliers.

### 3.3 Aerox sans fil et Rival 3 Wireless

Rapports de **64 octets** en output (octet 0 = 0x00, compris dans les 64). En mode **2,4 GHz**
(PID 1838, 1878, 1890, 1852, 185C, 1860, 1858) :

- l'octet 1 (commande) reçoit **en plus le bit 0x40** (0x21 → 0x61, 0x23 → 0x63, 0x22 → 0x62,
  0x11 → 0x51, 0x26 → 0x66) ;
- chaque écriture est suivie d'une lecture (délai 250 ms) de la réponse.

En mode câble (183A, 187A, 1854, 185E, 1862, 185A) et pour le Rival 3 Wireless : pas de bit 0x40, pas
de lecture.

| Message | Octets 1.. (avant ajout éventuel du bit 0x40) |
|---|---|
| Initialisation (feature, à l'ouverture) | 0x2D — mêmes réserves DPI qu'au §3.2 |
| Version (feature + lectures) | 0x90, 0x00 |
| Couleur (Aerox) | 0x21, 0x01, zone, R, G, B |
| Couleur réactive (Aerox 9, zone 3) | 0x26, 0x01, 0x00, R, G, B |
| Couleur (Rival 3 Wireless) | 0x13, zone, R, G, B |
| Arc-en-ciel (effet 0x05 demandé) | 0x22, 0xFF |
| Luminosité | 0x23, valeur ; OpenRGB divise la luminosité 0–100 par 10 (0–10) |
| Sauvegarde | 0x11, 0x00 (jamais pour le Rival 3 Wireless : « opérations EEPROM désactivées ») |

Zones : 0 avant, 1 milieu, 2 arrière, 3 réactive (Aerox 9) ; Rival 3 Wireless : une seule zone de
valeur 1 (molette). Modes proposés : direct et respiration arc-en-ciel (Aerox 3/5/9) ; direct
seulement pour le Rival 3 Wireless.

### 3.4 Rival 5 (183C)

Feature reports de 64 octets (octet 0 = 0x00).

| Message | Octets 1.. |
|---|---|
| Couleurs | 0x21, 0xFF, 0x03, puis 10 triplets R,G,B à partir de l'octet 4 |
| Luminosité | 0x23, valeur (OpenRGB arrondit en 5 paliers : ≥ 88 → 0x64, ≥ 63 → 0x32, ≥ 38 → 0x19, ≥ 13 → 0x0C, sinon 0x00) |
| Réactif activé | 0x26, 0x01, 0x00, R, G, B |
| Réactif désactivé | 0x26 (le reste à zéro) |
| Arc-en-ciel | 0x22, 0xFF, 0x03 |

Ordre des 10 LED : 0 molette, 1 corps gauche 1, 2 corps droit 1, 3 gauche 2, 4 droit 2, 5 gauche 3,
6 droit 3, 7 gauche 4, 8 droit 4, 9 logo.

Séquences : direct = « réactif désactivé » (le réactif est **persistant** dans l'appareil et doit
être coupé explicitement), puis luminosité si elle a changé, puis couleurs. Réactif = message
réactif activé. Arc-en-ciel = message 0x22 (effet fixe, non paramétrable). Pas de lecture de version.

**Pas de commande de sauvegarde : le Rival 5 enregistre automatiquement son état** d'après le source.
Chaque trame écrit donc potentiellement en mémoire persistante : limiter fortement la cadence
(risque d'usure, voir §7) **(mécanisme exact incertain)**.

### 3.5 Rival anciens (100, 300, 600, 650, 700/710)

Messages « courts » : output report de (taille + 1) octets, octet 0 = 0x00 puis les données.

| Variante | Couleur | Effet |
|---|---|---|
| 100 | 9 octets de données : 0x05, 0x00, R, G, B | 0x07, 0x00, effet |
| 300 | 0x08, zone + 1, R, G, B (zone 0 logo, 1 molette) | 0x07, zone + 1, effet (envoyé pour les deux zones) |
| 600 | **output report de 7 octets dont le premier vaut 0x1C** (traité comme ID de rapport) : 0x1C, 0x27, 0x00, masque = 1 << zone, R, G, B | aucun message d'effet |
| 650 | séquence de 4 messages de 60 octets (ci-dessous) | 0x00, 0x00, effet (9 octets de données), envoyé 8 fois (une par LED 0x10–0x17) sans que l'identifiant de LED figure dans le message — vraisemblablement inopérant **(incertain)** |
| 700/710 | **feature report** de 579 octets (0x00 + 578) : 0x05, 0x00, zone, R, G, B, 0,0,0,0,0, zone, 0x01, reste à zéro | aucun |

Effets : 0x01 direct/statique, 0x02 pulsation lente, 0x03 moyenne, 0x04 rapide.

Séquence Rival 650 pour une LED d'identifiant `z` (données, octet 0 = 0x00 ajouté devant) :

1. 0x03 suivi de : offset 4 = 0x30, 6 = 0x10, 7 = 0x27, 0x16 = 0x01, 0x1E = 0x04, 0x1F..0x21 = R,G,B,
   0x22 = 0xFF, 0x27 = 0xFF, 0x29 = 0x54, 0x2C = 0xFF, 0x2D = 0x54, 0x2E..0x30 = R,G,B, 0x31 = 0x56
   (offsets dans les 60 octets de données) ;
2. 0x03, offset 2 = 0x30, offset 4 = 0x2C ;
3. 0x05, offset 2 = z, offset 3 = 0xFF, offset 8 = 0x5C ;
4. 0x1C, offset 2 = 0x55, offset 4 = 0x46.

La signification de ces octets n'est pas expliquée par le source **(incertain)** ; ils sont
envoyés tels quels pour chaque LED.

Identifiants de LED :

| Variante | LED |
|---|---|
| 100 | logo (0) |
| 300, 700/710 | logo 0, molette 1 |
| 600 | logo 0x01, molette 0x00, gauche haut 0x02, gauche milieu 0x04, gauche bas 0x06, droite haut 0x03, droite milieu 0x05, droite bas 0x07 (le message couleur utilise le masque 1 << valeur) |
| 650 | logo 0x11, molette 0x10, gauche 1/2/3 = 0x12/0x14/0x16, droite 1/2/3 = 0x13/0x15/0x17 |

Version firmware (300 et 700 seulement) : données 0x10, 0x00 puis lecture de 2 octets (entier
16 bits petit-boutiste). Sauvegarde : données 0x09 + 8 zéros (message court) — écrit la
configuration interne.

Séquences : mode direct = effet 0x01 sur toutes les zones, puis couleur de chaque LED ; pulsation =
effet 0x02–0x04 puis couleurs ; sauvegarde = renvoyer le mode puis 0x09. (Le Rival 600/700 n'a pas
de message d'effet : seul le direct est réellement appliqué.)

### 3.6 Sensei et Rival 310

Messages de 65 octets (output report : 0x00 puis 65 octets de données ; OpenRGB envoie donc
66 octets **(à vérifier : longueur réellement acceptée)**). Offsets ci-dessous dans les données.

| Message | Contenu |
|---|---|
| Couleur statique | 0x00 = 0x5B, 0x02 = zone, 0x13 = 0x01 (drapeau statique), 0x1B = 0x01 (nombre de couleurs), 0x1C..0x1E = R,G,B, 0x1F..0x21 = R,G,B (copie) |
| Respiration | 0x00 = 0x5B, 0x02 = zone, 0x03 = durée octet bas, 0x04 = durée octet haut, 0x1B = 0x03, 0x1C..0x21 = couleur ×2, 0x26 = 0x7F, 0x27..0x29 = couleur, 0x2A = 0x7F |
| Arc-en-ciel | 0x00 = 0x5B, 0x02 = zone, durée en 0x03/0x04, 0x1B = 0x07, puis points de couleur fixes : 0x1C = 0xFF, 0x1D..0x1E = G,B de la couleur, 0x1F = 0xFF, 0x22 = 0x14, 0x23 = 0xFF, 0x24 = 0xFF, 0x26 = 0x2B, 0x28 = 0xFF, 0x2A = 0x2B, 0x2C = 0xFF, 0x2D = 0xFF, 0x2E = 0x28, 0x31 = 0xFF, 0x32 = 0x2B, 0x33 = 0xFF, 0x35 = 0xFF, 0x36 = 0x2B, 0x37 = 0xFF, 0x3A = 0x14 |
| Sauvegarde | 0x00 = 0x59 (message de 9 octets) — défini, non appelé par OpenRGB |

Durées (millisecondes, 16 bits, octet bas en 0x03) : respiration lente 10 000 (0x2710), moyenne
5 000 (0x1388), rapide 2 500 (0x09C4) ; arc-en-ciel lent 20 000 (0x4E20), moyen 10 000, rapide
5 000. Les octets 0x26/0x2A/0x2E… sont des positions de points de couleur sur 0xFF (proportion de la
durée).

Zones : 0 logo, 1 molette. Mise à jour : un message par zone (statique en mode direct, sinon
l'effet).

## 4. Claviers

### 4.1 Apex 5 / 7 / 9 / Pro (protocole « Apex »)

Interface 1 (sans fil : 3 / 0xFFC0 / 0x01). Les commandes « courtes » sont des output reports de
65 octets (0x00 + 64), les lectures des input reports de 64 octets.

**Générations** : G1 par défaut ; G2 pour Apex 9 TKL, Apex 9 Mini, Apex Pro TKL 2023 filaire ; G3 pour
Apex Pro TKL 2023 sans fil et Apex Pro TKL Gen 3 (filaire et sans fil). Apex Pro TKL (1614), Apex 9
TKL et Apex 9 Mini passent en **G3 si la version clavier est ≥ 1.19.7** (lue avec 0x90).

| Message | Transport | Octets 1.. | Remarque |
|---|---|---|---|
| Version | output + lecture | 0x90 | réponse : chaîne « X.Y.Z » ; en G2+ le 1er octet de la réponse vaut 0x90 et doit être retiré. En G1, une 2e requête 0x90, 0x01 renvoie la version du contrôleur LED |
| Numéro de série (G1) | output + lecture | 0xFF | 19 premiers octets utiles ; les 5 premiers caractères = référence produit (SKU) qui donne la disposition |
| Région (Gen 3 filaire 1642) | output + lecture | 0xF5 | réponse octet 0 = 0xF5, octet 2 = région : 0x1 → 64740, 0x3 → 64741, 0x4 → 64743, 0x6 → 64744, 0xA → 64742, 0xD → 64745 ; à défaut : Apex 9 TKL → 64847, Apex 9 Mini → 64837, autres → 64865 |
| Initialisation G3 | feature 65 | 0x4B | envoyée à l'ouverture et en repassant en mode direct ; « sans danger » sur les modèles qui n'en ont pas besoin selon le source |
| Retour au mode embarqué | output 65 | 0x3B (G1/G2) ou 0x41 (G3) | à la fermeture, et quand on choisit le mode embarqué ; **nécessaire** pour que les profils matériels se réappliquent |
| Choix de profil | feature 65 | 0x89, profil | défini, non utilisé |
| Lire luminosité | output + lecture | 0xA3 | réponse : octet 0 = 0xA3, octet 1 = 0x00 (0xFF = non supporté), octet 2 = 0–10 |
| Régler luminosité | output 65 | 0x23, 0–10 | n'agit que sur l'éclairage embarqué ; ne s'applique pas au direct |
| Trame directe | feature (643 octets ; **513** pour Apex 9 TKL/Mini) | id, n, puis n enregistrements de 4 octets : code de touche, R, G, B | id = 0x3A (G1), 0x40 (G2/G3 filaire), 0x61 (G3 sans fil : 1630, 1632, 1644, 1646) |

Envoyer une trame directe avec une mauvaise longueur bloque le transfert de contrôle et l'appareil
ignore silencieusement la mise à jour. Le nombre d'enregistrements est limité à
(longueur − 3) / 4.

Séquences :

- (a) Contrôle logiciel : (G3) feature 0x4B ; puis trames directes. Aucune autre commande.
- (b) Trame : un seul feature report contenant toutes les touches (table §4.1.1, 112 entrées).
- (c) Effet matériel : retour au mode embarqué (0x3B / 0x41) puis, si supporté, luminosité 0x23.
  Les effets eux-mêmes sont ceux du profil embarqué (non pilotés par OpenRGB).
- (d) Sauvegarde : non supportée.
- Fermeture : toujours renvoyer 0x3B / 0x41.

#### 4.1.1 Codes de touche de la trame directe

L'index LED est l'ordre dans lequel OpenRGB range ses couleurs ; seul le **code envoyé** compte pour
l'appareil (codes d'usage HID clavier, plus quelques codes propres : 0xF0 = Fn, 0xFB = touche
multimédia de la rangée F des TKL). La disposition physique dépend de la référence produit (US, ISO
UK/nordique, JP, TKL, Mini) ; seules les positions changent, pas les codes.

| Index LED | Code envoyé | Touche (US) | Code W3C |
|---|---|---|---|
| 0 | 0x04 | A | KeyA |
| 1 | 0x05 | B | KeyB |
| 2 | 0x06 | C | KeyC |
| 3 | 0x07 | D | KeyD |
| 4 | 0x08 | E | KeyE |
| 5 | 0x09 | F | KeyF |
| 6 | 0x0A | G | KeyG |
| 7 | 0x0B | H | KeyH |
| 8 | 0x0C | I | KeyI |
| 9 | 0x0D | J | KeyJ |
| 10 | 0x0E | K | KeyK |
| 11 | 0x0F | L | KeyL |
| 12 | 0x10 | M | KeyM |
| 13 | 0x11 | N | KeyN |
| 14 | 0x12 | O | KeyO |
| 15 | 0x13 | P | KeyP |
| 16 | 0x14 | Q | KeyQ |
| 17 | 0x15 | R | KeyR |
| 18 | 0x16 | S | KeyS |
| 19 | 0x17 | T | KeyT |
| 20 | 0x18 | U | KeyU |
| 21 | 0x19 | V | KeyV |
| 22 | 0x1A | W | KeyW |
| 23 | 0x1B | X | KeyX |
| 24 | 0x1C | Y | KeyY |
| 25 | 0x1D | Z | KeyZ |
| 26 | 0x1E | 1 | Digit1 |
| 27 | 0x1F | 2 | Digit2 |
| 28 | 0x20 | 3 | Digit3 |
| 29 | 0x21 | 4 | Digit4 |
| 30 | 0x22 | 5 | Digit5 |
| 31 | 0x23 | 6 | Digit6 |
| 32 | 0x24 | 7 | Digit7 |
| 33 | 0x25 | 8 | Digit8 |
| 34 | 0x26 | 9 | Digit9 |
| 35 | 0x27 | 0 | Digit0 |
| 36 | 0x28 | Enter | Enter |
| 37 | 0x29 | Escape | Escape |
| 38 | 0x2A | Backspace | Backspace |
| 39 | 0x2B | Tab | Tab |
| 40 | 0x2C | Space | Space |
| 41 | 0x2D | - | Minus |
| 42 | 0x2E | = | Equal |
| 43 | 0x2F | [ | BracketLeft |
| 44 | 0x30 | ] | BracketRight |
| 45 | 0x32 | # | Backslash |
| 46 | 0x33 | ; | Semicolon |
| 47 | 0x34 | ' | Quote |
| 48 | 0x35 | ` | Backquote |
| 49 | 0x36 | , | Comma |
| 50 | 0x37 | . | Period |
| 51 | 0x38 | / | Slash |
| 52 | 0x39 | Caps Lock | CapsLock |
| 53 | 0x3A | F1 | F1 |
| 54 | 0x3B | F2 | F2 |
| 55 | 0x3C | F3 | F3 |
| 56 | 0x3D | F4 | F4 |
| 57 | 0x3E | F5 | F5 |
| 58 | 0x3F | F6 | F6 |
| 59 | 0x40 | F7 | F7 |
| 60 | 0x41 | F8 | F8 |
| 61 | 0x42 | F9 | F9 |
| 62 | 0x43 | F10 | F10 |
| 63 | 0x44 | F11 | F11 |
| 64 | 0x45 | F12 | F12 |
| 65 | 0x46 | Print Screen | PrintScreen |
| 66 | 0x47 | Scroll Lock | ScrollLock |
| 67 | 0x48 | Pause/Break | Pause |
| 68 | 0x49 | Insert | Insert |
| 69 | 0x4A | Home | Home |
| 70 | 0x4B | Page Up | PageUp |
| 71 | 0x4C | Delete | Delete |
| 72 | 0x4D | End | End |
| 73 | 0x4E | Page Down | PageDown |
| 74 | 0x4F | Right Arrow | ArrowRight |
| 75 | 0x50 | Left Arrow | ArrowLeft |
| 76 | 0x51 | Down Arrow | ArrowDown |
| 77 | 0x52 | Up Arrow | ArrowUp |
| 78 | 0x64 | \ (ISO) | IntlBackslash |
| 79 | 0xE0 | Left Control | ControlLeft |
| 80 | 0xE1 | Left Shift | ShiftLeft |
| 81 | 0xE2 | Left Alt | AltLeft |
| 82 | 0xE3 | Left Windows | MetaLeft |
| 83 | 0xE4 | Right Control | ControlRight |
| 84 | 0xE5 | Right Shift | ShiftRight |
| 85 | 0xE6 | Right Alt | AltRight |
| 86 | 0xE7 | Right Windows | MetaRight |
| 87 | 0xF0 | Right Fn | Fn |
| 88 | 0x31 | \ (ANSI) | Backslash |
| 89 | 0x87 | _ | IntlRo |
| 90 | 0x88 | かな | KanaMode |
| 91 | 0x89 | ¥ | IntlYen |
| 92 | 0x8A | 変換 | Convert |
| 93 | 0x8B | 無変換 | NonConvert |
| 94 | 0x53 | Num Lock | NumLock |
| 95 | 0x54 | Number Pad / | NumpadDivide |
| 96 | 0x55 | Number Pad * | NumpadMultiply |
| 97 | 0x56 | Number Pad - | NumpadSubtract |
| 98 | 0x57 | Number Pad + | NumpadAdd |
| 99 | 0x58 | Number Pad Enter | NumpadEnter |
| 100 | 0x59 | Number Pad 1 | Numpad1 |
| 101 | 0x5A | Number Pad 2 | Numpad2 |
| 102 | 0x5B | Number Pad 3 | Numpad3 |
| 103 | 0x5C | Number Pad 4 | Numpad4 |
| 104 | 0x5D | Number Pad 5 | Numpad5 |
| 105 | 0x5E | Number Pad 6 | Numpad6 |
| 106 | 0x5F | Number Pad 7 | Numpad7 |
| 107 | 0x60 | Number Pad 8 | Numpad8 |
| 108 | 0x61 | Number Pad 9 | Numpad9 |
| 109 | 0x62 | Number Pad 0 | Numpad0 |
| 110 | 0x63 | Number Pad . | NumpadDecimal |
| 111 | 0xFB | Media Play/Pause | MediaPlayPause |

### 4.2 Apex M750 (0616)

Feature reports de **513 octets** (0x00 + 512), interface 2.

Activation du contrôle des LED (à l'ouverture), trois feature reports dont seuls les octets listés
sont non nuls (offsets dans le tampon de 513, octet 0 = 0x00) :

1. offset 4 = 0x01, offset 6 = 0x85 ;
2. offset 4 = 0x03, offset 5 = 0x01, offset 7 = 0xFF ;
3. offset 4 = 0x01, offset 6 = 0x85, et offset 7 = 0xFF (OpenRGB ne remet pas le tampon à zéro
   entre les messages 2 et 3 ; on ne sait pas si cet octet résiduel compte **(incertain)**).

Trame : offsets 3 = 0x01, 4 = 0x8E, 5 = 0x01, 6 = 0x03, 7 = 0x06, 8 = 0x16 ; puis à partir de
l'offset 9, **132 emplacements** de 3 octets R,G,B (6 rangées × 22). Les emplacements sans touche
reçoivent la valeur fixe 0xFF, 0x32, 0x00. Ordre des emplacements : rangée du bas d'abord (0 = rangée
Ctrl/Espace), puis vers le haut ; 22 emplacements par rangée, de gauche à droite.

| Rangée | emplacements (index:touche ; « — » = vide) |
|---|---|
| 0 (bas) | 0:ControlLeft 1:MetaLeft 2:AltLeft 3:— 4:Space 5–8:— 9:AltRight 10:MetaRight 11:Fn 12:ControlRight 13–14:— 15:ArrowLeft 16:ArrowDown 17:ArrowRight 18:— 19:Numpad0 20:— 21:NumpadDecimal |
| 1 | 0:ShiftLeft 1:KeyZ 2:KeyX 3:KeyC 4:KeyV 5:KeyB 6:KeyN 7:KeyM 8:Comma 9:Period 10:Slash 11:— 12:ShiftRight 13–15:— 16:ArrowUp 17:— 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 2 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 12:— 13:Enter 14–17:— 18:Numpad4 19:Numpad5 20:Numpad6 21:— |
| 3 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:— 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 4 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 13:— 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 5 (haut) | 0:Escape 1:F1 2:F2 3:F3 4:F4 5:— 6:F5 7:F6 8:F7 9:F8 10:— 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 18–21:— |

Incohérence relevée : les emplacements du pavé numérique sont libellés comme ci-dessus dans le
source, mais OpenRGB y lit les couleurs d'index qui, dans sa liste de touches actuelle (§4.1.1),
désignent d'autres touches (touches japonaises, pavé décalé). La disposition physique ci-dessus est
celle des libellés ; à confirmer sur le matériel **(incertain)**. Pas d'effet matériel, pas de
sauvegarde, pas de retour au mode embarqué connu.

### 4.3 Apex (OG) / Apex Fnatic / Apex 350 (1202, 1206)

Un seul output report : 0x00 suivi de 32 octets de données.

| Offset (données) | Contenu |
|---|---|
| 0 | 0x07 |
| 1 | 0x00 (toutes zones) |
| 2–5 | zone QWERTY : R, G, B, A |
| 6–9 | pavé numérique : R, G, B, A |
| 10–13 | touches de fonction : R, G, B, A |
| 14–17 | touches MX (macro) : R, G, B, A |
| 18–21 | logo : R, G, B, A |

L'octet A est un niveau/effet par zone ; OpenRGB y met toujours 0x08 (« direct »). Le source indique
qu'il règle la luminosité de la zone, sans donner l'échelle **(incertain)**. Les 5 zones sont
toujours envoyées ensemble. Pas de sauvegarde.

### 4.4 Apex 3 (161A) — 10 zones

Output reports de **33 octets** (0x00 + 32), interface 3.

| Message | Octets 1.. |
|---|---|
| Luminosité | 0x0A, 0x00, luminosité (0–100) |
| Couleurs | 0x0B, 0x00, puis 10 triplets R,G,B (octets 3–32) |
| Sauvegarde | message 1 : 0x06, 0x00, 0x08 ; message 2 : 0x09, 0x00, 0x00 |

Séquence de mise à jour : luminosité puis couleurs, à chaque trame. Zones 0 à 9 de gauche à droite
**(ordre physique non documenté dans le source, présumé)**. Mode unique : direct (avec sauvegarde
manuelle).

### 4.5 Apex 3 TKL (1622) — 8 zones

Output reports de **65 octets** (0x00 + 64), interface 1 / page 0xFFC0 / usage 0x01.

| Message | Octets 1.. |
|---|---|
| Couleurs | 0x21, masque (0xFF = toutes les zones ; bit N à 0 = zone N inchangée), puis 8 triplets R,G,B |
| Vague arc-en-ciel | 0x22, 0xFF (OpenRGB le construit comme le message couleur avec 0x21 + 1) |
| Luminosité | 0x23, valeur 0x00–0x10 (agit comme facteur multiplicatif ; réglable aussi au clavier) |
| Lire luminosité | 0xA3 ; réponse dont l'octet 0 = 0xA3 |
| Inconnu | 0x6C, 0x00, 0x01, 0x01 : déclenche une réponse sur le point de terminaison interrupt (0x6C puis zéros) **(sens inconnu)** |
| Version présumée | 0x90 ; réponse commençant par 0x90 **(incertain)** |

Séquence : couleurs (ou vague), puis luminosité si elle a changé. Pas de sauvegarde connue.

## 5. Casques

### 5.1 Siberia 350 (1229)

Output reports de 16 octets construits ainsi : octet 0 = 0x01 (ID), octet 1 = 0x00, octet 2 =
commande, octet 3 = nombre d'octets de paramètres, octets 4.. = paramètres.

Séquence pour une couleur (4 messages) :

| # | Commande | Paramètres |
|---|---|---|
| 1 | 0x95 | 0x80, 0xBF |
| 2 | 0x80 | 0x52, 0x20 |
| 3 | 0x83 | R, G, B |
| 4 | 0x93 | 0x03, 0x80 |

Une seule LED, mode direct uniquement ; la signification des messages 1, 2 et 4 n'est pas
documentée **(incertain)**.

### 5.2 Arctis 5 (1250, 12AA)

Output reports de **37 octets**, ID 0x06. Zones : 0 gauche, 1 droite. Pour chaque zone, 6 messages
(offsets incluant l'ID) :

| # | Octets 1.. |
|---|---|
| 1 | 0x81, 0x43, 0x01, 0x22 |
| 2 | 0x81, 0x43, 0x01, 0x23 |
| 3 (couleur) | 0x8A, 0x42, 0x00, 0x20, 0x41, 0x00, R, G, B, 0xFF, 0x32, 0xC8, 0xC8 |
| 4 (affectation à la zone) | 0x8A, 0x42, 0x00, 0x20, 0x41, 0x08, zone, 0x01 |
| 5 | 0x8A, 0x42, 0x00, 0x20, 0x60, zone |
| 6 (application) | 0x8A, 0x42, 0x00, 0x20, 0x05 |

Mode direct uniquement, pas de sauvegarde.

### 5.3 Arctis Nova 3 (12EC)

Output reports, ID **0x06**. Deux tailles : « effet » de **521 octets**, « commande » de 64 octets.
Zone 0 = LED droite, zone 1 = LED gauche (c'est l'ordre des LED dans OpenRGB).

Commandes courtes (64 octets) :

| Nom | Octets 1.. |
|---|---|
| Sélection | 0xA5, (0x02 pour la zone 0, 0x01 pour la zone 1), état (0x00 = éteint, 0x0A = actif) |
| Application | 0x09 pour la zone 0, 0xA3 pour la zone 1 |

Rapport d'effet (521 octets, offsets incluant l'ID) : octet 1 = 0xAA, octet 2 = 0x01 pour la zone 0 /
0x00 pour la zone 1, octets 0x0D–0x0E = vitesse (voir table), 0x15 = 0x55, 0x17 = 0x96, octet 0x25 =
type, puis points de couleur à partir de 0x26 :

| Effet | 0x0D, 0x0E | 0x1D | 0x25 | Octets suivants |
|---|---|---|---|---|
| Direct (statique) | 0x98, 0x3A | 0x01 | 0x02 | 0x2D–0x2F = R,G,B ; 0x30 = 0xFF |
| Respiration | vitesse | 0 | 0x03 | 0x29–0x2B = R,G,B ; 0x30 = 0x7F ; 0x31–0x33 = R,G,B ; 0x34 = 0x7F |
| Arc-en-ciel | vitesse | 0 | 0x07 | à partir de 0x26 : FF 00 00 FF 00 00 00 FF FF 00 2B 00 FF 00 2B 00 FF FF 2B 00 00 FF 2B FF 00 FF 2B FF 00 00 26 |
| Dégradé orange | vitesse | 0 | 0x03 | à partir de 0x26 : FF EA 00 FF EA 00 00 FF 4D 00 7F FF EA 00 7F |
| Dégradé bleu | vitesse | 0 | 0x03 | à partir de 0x26 : 8C 00 FF 8C 00 FF 00 3B D1 FF 7F 8C 00 FF 7F |

Vitesse 1–10 → (octet 0x0D, octet 0x0E) : 1 → 30 75, 2 → 2E 68, 3 → 2C 5B, 4 → 20 4E, 5 → 1E 41,
6 → 1C 34, 7 → 1A 27, 8 → 0E 1A, 9 → 0C 0D, 10 → 94 02 (défaut OpenRGB : 9 pour la respiration,
6 pour les dégradés). Ces paires ressemblent à des durées 16 bits petit-boutistes non monotones ;
reproduire la table telle quelle **(incertain)**.

Séquences (par zone) : effet = rapport d'effet, puis sélection (état 0x0A), puis application.
Éteint = sélection (état 0x00) puis application. Sauvegarde = application seule, pour les deux
zones. Le source ne dit pas si l'application écrit en mémoire persistante **(incertain)**.

## 6. Tapis QCK Prism (150A…1520)

Deux LED : index 0 = bas, index 1 = haut.

1. Feature report de **525 octets** (offsets incluant l'octet 0 = 0x00) : 1 = 0x0E, 3 = 0x02,
   5–7 = R,G,B bas, 8 = 0xFF, 9 = 0x32, 10 = 0xC8, 14 = 0x01, 17–19 = R,G,B haut, 20 = 0xFF,
   21 = 0x32, 22 = 0xC8, 25 = 0x01, 26 = 0x01, 28 = 0x01.
2. Output report de 65 octets : octet 1 = 0x0D (application).

Mode direct uniquement, pas de sauvegarde.

## 7. Sauvegarde, persistance et sécurité

| Appareil | Commande | Persistance |
|---|---|---|
| Rival 3 | output 0x09 | sauvegarde explicite |
| Aerox 3/5 filaires et sans fil | 0x11, 0x00 (0x51 en 2,4 GHz) | sauvegarde explicite, non confirmée par OpenRGB |
| Rival 3 Wireless | aucune | OpenRGB désactive volontairement les écritures EEPROM |
| Rival 5 | aucune | **enregistrement automatique** de l'état : chaque trame est potentiellement écrite |
| Rival 100/300/600/650/700 | 0x09 + 8 zéros | sauvegarde explicite |
| Sensei / Rival 310 | 0x59 | définie, non utilisée |
| Apex 3 | 0x06…0x08 puis 0x09 | sauvegarde explicite |
| Arctis Nova 3 | application (0x09 / 0xA3) | inconnue |
| Apex (G1–G3), Apex M, Apex OG, Apex 3 TKL, QCK, Siberia, Arctis 5 | — | volatil |

Points de vigilance :

- **Initialisation 0x2D des Aerox** : écrit des paliers DPI (à zéro chez OpenRGB). Risque de
  modifier la sensibilité de la souris ; lire et réécrire les paliers existants, ou s'abstenir.
- **Apex** : longueur exacte du feature report obligatoire (643 / 513) ; toujours renvoyer 0x3B /
  0x41 en quittant, sinon le clavier reste figé sur les dernières couleurs logicielles.
- **Rival 5** : limiter la fréquence des trames (usure flash possible).
- **Mode « disco »** des Rival 3/Aerox (effet 0x06) : flashs rapides, potentiellement dangereux
  pour les personnes photosensibles ; OpenRGB et le logiciel du fabricant ne l'exposent pas.
- Aucun mécanisme de maintien n'est nécessaire pour les appareils SteelSeries d'après le source.
