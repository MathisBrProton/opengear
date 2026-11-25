# HyperX — fiche protocole (claviers, souris, tapis, micros)

Source analysée : OpenRGB f899bc8, dossiers `HyperXKeyboardController`, `HyperXMouseController`,
`HyperXMousematController`, `HyperXMicrophoneController`, `HyperXMicrophoneV2Controller` (la RAM
HyperX, protocole SMBus, est hors périmètre). Faits rédigés pour l'interopérabilité ; aucun code
repris. **(incertain)** = non établi par le source.

VID : **0x0951** (Kingston, produits d'origine) et **0x03F0** (HP, produits rachetés et
renumérotés). Plusieurs appareils existent sous les deux VID avec des PID différents.

Conventions : les offsets incluent l'octet 0 du tampon hôte. Quand cet octet vaut 0x00, l'appareil
n'a pas d'ID de rapport et Windows ne le transmet pas ; quand il vaut autre chose (0x07, 0x44,
0x40, 0x42), c'est l'ID de rapport. Couleurs en R, G, B sauf mention contraire.

## 1. Appareils

| Nom | VID:PID | Type | Famille (§) | HID Windows (interface / page / usage) | LED |
|---|---|---|---|---|---|
| Alloy Elite RGB | 0951:16BE | clavier | Alloy Elite (§3.1) | 2 / 0xFF01 / * | 106 touches + 22 (barre, média) = 128 |
| Alloy FPS RGB | 0951:16DC | clavier | Alloy FPS (§3.2) | 2 / 0xFF01 / * | 106 |
| Alloy Elite 2 | 0951:1711 | clavier | trames « 0x81 » (§3.3) | 3 / 0xFF90 / * (autres OS : interface 0) | 128 (dont 18 de barre) |
| Alloy Elite 2 (HP) | 03F0:058F | clavier | idem | idem | idem |
| Alloy Origins | 0951:16E5 | clavier | trames « 0x81 » | 3 / * / * (autres OS : 0) | 107 |
| Alloy Origins (HP) | 03F0:0591 | clavier | idem | 3 / 0xFF90 / * | 107 |
| Alloy Origins 60 | 0951:1734 | clavier | trames « 0x81 » | 3 / * / * (autres OS : 0) | 71 emplacements |
| Alloy Origins 60 (HP) | 03F0:0C8E | clavier | idem | idem | 71 |
| Alloy Origins 65 (HP) | 03F0:038F | clavier | idem | idem | 77 emplacements |
| Alloy Origins Core | 0951:16E6 | clavier | Origins Core (§3.4) | 2 / * / * | 87 (ANSI) |
| Alloy Origins Core (HP) | 03F0:098F | clavier | idem | 2 / * / * | 87 |
| Origins 2 65 (HP) | 03F0:0CC2 | clavier | rapports 0x44 (§3.5) | 3 / * / * | 74 emplacements |
| Eve 1800 (HP) | 03F0:08C2 | clavier | rapports 0x44 (§3.5) | 2 / * / * | 10 zones |
| Pulsefire Surge | 0951:16D3 | souris | rapport 0x07 (§4.1) | 1 / 0xFF01 / * | 32 + logo |
| Pulsefire Surge (HP) | 03F0:0490 | souris | idem | idem | idem |
| Pulsefire FPS Pro | 0951:16D7 | souris | rapport 0x07 (§4.2) | 1 / 0xFF01 / * | 1 |
| Pulsefire Core | 0951:16DE | souris | idem | idem | 1 |
| Pulsefire Core (HP) | 03F0:0D8F | souris | idem | idem | 1 |
| Pulsefire Dart (sans fil) | 0951:16E1 | souris | Dart (§4.3) | 2 / 0xFF00 / * | 2 |
| Pulsefire Dart (sans fil, HP) | 03F0:068E | souris | idem | idem | 2 |
| Pulsefire Dart (filaire) | 0951:16E2 | souris | idem | 1 / 0xFF13 / * | 2 |
| Pulsefire Dart (filaire, HP) | 03F0:088E | souris | idem | idem | 2 |
| Pulsefire Raid | 0951:16E4 | souris | rapport 0x07 (§4.4) | 1 / 0xFF01 / 0x01 | 2 |
| Pulsefire Haste | 0951:1727 | souris | trames « 0x81 » (§4.5) | 3 / 0xFF90 / * | 1 |
| Pulsefire Haste (HP) | 03F0:0F8F | souris | idem | 3 / 0xFF90 / * | 1 |
| Fury Ultra | 0951:1705 | tapis | trames « 0x81 » (§5) | 0 / * / * | 15 + 5 = 20 |
| Pulsefire Mat | 03F0:0F8D | tapis | idem | 1 / 0xFF90 / 0xFF00 | 20 |
| Pulsefire Mat RGB Mouse Pad XL | 0951:1741 | tapis | idem | 1 / 0xFF90 / 0xFF00 | 2 |
| QuadCast S | 0951:171F | micro | QuadCast (§6.1) | 0 / * / * | 2 |
| QuadCast S (HP, six PID) | 03F0:0F8B, 068C, 0294, 028C, 048C, 0D8B | micro | idem | 0 / * / * | 2 |
| DuoCast | 03F0:098C | micro | idem | 0 / * / * | 2 |
| QuadCast 2 S | 03F0:02B5 | micro | QuadCast 2 (§6.2) | 1 / 0xFF13 / 0xFF00 | 108 |

Sur d'autres systèmes que Windows, OpenRGB sélectionne parfois une autre interface (Alloy Elite 2,
Origins, Origins 60/65 : interface 0 ; Haste : page 0x0001 usage 0x01 ou page 1 usage 2 ; tapis XL :
interface 0, page 0x0C, usage 0x01). Les valeurs ci-dessus sont celles de Windows.

## 2. Familles de protocoles

| Famille | Rapport | Remarque |
|---|---|---|
| « Rapport 0x07 » (Alloy Elite, Alloy FPS, Pulsefire Surge, FPS Pro/Core, Raid) | **feature report de 264 octets, ID 0x07** | 1re génération Kingston |
| « Trames 0x81 » (Alloy Elite 2, Origins, Origins 60/65, Haste, tapis, QuadCast S) | **feature reports de 65 octets** (0x00 + 64) : un en-tête « 0x04 0xF2 » puis des paquets de 16 LED codées sur 4 octets `0x81 R G B` | même schéma partout ; l'octet 9 de l'en-tête vaut le nombre de paquets de couleur qui suivent (sauf exceptions) |
| « Rapports 0x44 » (Origins 2 65, Eve 1800, QuadCast 2 S) | **output reports de 65 / 64 octets, ID 0x44** | génération HP |
| Origins Core | output reports 0xA2 (flux), 0xA7 (luminosité) | protocole à part, flux continu |
| Dart | output report 0xD2 | effets matériels par LED, sauvegarde 0xDE |

Aucune somme de contrôle dans ces protocoles. Aucun mutex inter-logiciels.

## 3. Claviers

### 3.1 Alloy Elite RGB (16BE)

Feature report **0x07**, 264 octets. Quatre « canaux » de couleur : 1 = rouge, 2 = vert, 3 = bleu,
4 = étendu (barre lumineuse et touches média, R/G/B mélangés).

| Message | Octet 1 | Octets suivants |
|---|---|---|
| Choisir le profil | 0x01 | 2 = profil, 6 = 0x03, 7 = 0x01 (défini, non utilisé) |
| Effet de profil | 0x02 | voir ci-dessous |
| Couleurs de profil | 0x06 | 2 = profil, 3 = canal ; puis une valeur d'intensité par touche à l'offset donné par la table §8.1 |
| Trame directe | 0x16 | 2 = canal, 3 = 0xA0 ; puis valeurs d'intensité aux offsets de la table §8.1 |

Pour le canal 4 (étendu), le message contient aux offsets 0x08 à 0x93 les octets de la table des
offsets « étendus » (§8.1) : chaque LED de barre/média a un offset distinct pour son rouge, son vert
et son bleu, tous dans le même rapport.

Rapport d'effet (octet 1 = 0x02) :

| Offset | Valeur |
|---|---|
| 2 | profil (OpenRGB : 0x01) |
| 9 | 0x01 |
| 10 | mode : 0x00 vague, 0x01 statique, 0x02 respiration |
| 13, 14 | direction (même valeur) : 0x00 droite, 0x01 gauche, 0x02 haut, 0x03 bas, 0x04 vers l'intérieur, 0x05 vers l'extérieur |
| 27–34 | mode réactif ×8 : 0x03 « trigger », 0x04 explosion, 0x05 flamme HyperX, 0xFF aucun (OpenRGB : 0xFF) |
| 41 / 65 / 89 | rouge / vert / bleu de la couleur unique |
| 42 / 66 / 90 | rouge / vert / bleu de la 1re couleur double |
| 43 / 67 / 91 | rouge / vert / bleu de la 2e couleur double |
| 107–112 | constantes 0x09, 0x09, 0x05, 0x05, 0x06, 0x05 |
| 113 | vitesse 0x00–0x09 |
| 114 | 0x09 |
| 115–122 | mode de couleur ×8 : 0x00 une couleur, 0x01 deux couleurs, 0x02 spectre |

OpenRGB transmet sa propre valeur de direction (gauche = 0, droite = 1), qui est l'inverse des
définitions ci-dessus : le sens réel n'est pas établi **(incertain)**. Il attend 100 ms après
l'envoi d'un effet.

Séquences :

- **(a) Contrôle logiciel** : aucune commande préalable ; envoyer des trames directes.
- **(b) Trame directe** : canal 1 (rouges des 106 touches), pause 5 ms, canal 2, pause 5 ms, canal 3,
  pause 5 ms, canal 4 (barre + média). Quatre rapports par trame.
- **Maintien** : en mode direct, OpenRGB renvoie la dernière trame dès que 50 ms se sont écoulées
  sans mise à jour (vérification toutes les 10 ms) : sans flux continu, le clavier revient
  vraisemblablement à son profil **(cause non documentée)**.
- **(c) Effet matériel** : rapport d'effet (profil 1) ; en mode « statique », les couleurs par touche
  sont envoyées avec les rapports « couleurs de profil » (même découpage en 4 canaux, pauses de
  5 ms).
- **(d) Sauvegarde** : les rapports 0x02 et 0x06 ciblent le profil 1 et sont marqués
  « sauvegarde automatique » par OpenRGB : ils modifient vraisemblablement la mémoire du clavier
  **(persistance non confirmée)**.

Modes proposés : direct, statique (couleurs par touche), vague (vitesse 0–9, gauche/droite),
respiration (vitesse 0–9, 1 ou 2 couleurs ou aléatoire = mode spectre).

### 3.2 Alloy FPS RGB (16DC)

Identique à la trame directe de l'Alloy Elite, **sans canal étendu** : trois rapports 0x07 / 0x16 /
canal (1, 2, 3) / 0xA0 avec les 106 valeurs aux offsets de la table §8.2, 10 ms entre chaque.
Maintien identique (50 ms). Mode direct seulement.

### 3.3 Famille « trames 0x81 » (Alloy Elite 2, Origins, Origins 60/65)

En-tête (feature 65 octets) : octet 1 = 0x04, octet 2 = 0xF2, octet 9 = nombre de paquets de
couleur (Origins : 0x09 ; Origins 60/65 : 0x05 ; Alloy Elite 2 : 0x00, alors que 9 paquets suivent).

Paquet de couleur (feature 65 octets) : 16 emplacements de 4 octets à partir de l'octet 1 :
`0x81, R, G, B`. Les emplacements sans touche reçoivent `0x81, 0, 0, 0`.

| Modèle | Paquets par trame | Emplacements | Table |
|---|---|---|---|
| Alloy Elite 2 | 9 (complétés par des paquets vides) | 144 | §8.3 |
| Origins | 9 | 126 utiles (le reste à zéro) | §8.4 |
| Origins 60 | 5 | 71 | §8.5 |
| Origins 65 | 5 | 77 | §8.6 |

Séquence de trame : en-tête puis les paquets, sans pause. Quand il n'y a plus de couleur, OpenRGB
envoie un paquet ne contenant qu'un emplacement noir pour atteindre le nombre de paquets.

Maintien : Alloy Elite 2 → renvoi de la trame après 1 s sans mise à jour (vérification toutes les
50 ms). Origins et Origins 60/65 : le seuil prévu est 50 ms, mais OpenRGB ne met jamais à jour son
horodatage pour ces modèles, si bien que la trame est en pratique renvoyée toutes les ≈10 ms ; la
cadence minimale réellement nécessaire n'est pas connue **(incertain)**. Mode direct seulement ; pas
de sauvegarde.

### 3.4 Alloy Origins Core (16E6, 098F)

Interface 2. Output reports de 65 octets (octet 0 = 0x00).

| Message | Octets 1.. | Réponse |
|---|---|---|
| Lire la variante | 0x10 | lecture de 65 octets ; octet 56 = disposition : 0x09 ANSI, 0x10 ABNT2 |
| Luminosité | 0xA7, 0x00, 0x00, 0x01, valeur 0–255 | — |
| Paquet de flux | 0xA2, séquence (0, 1, 2…), 0x00, longueur, puis `longueur` octets | — |

La version de firmware est le champ `bcdDevice` du descripteur USB (« MM.mm »).

**Tampon d'image** de 380 octets, découpé en blocs de 48 octets par groupe de 12 LED. Pour la
valeur matérielle `v` d'une touche (table §8.7) :

- position de base `p = 48·⌊v/12⌋ + 8·(⌊v/6⌋ mod 2) + (v mod 6)` ;
- vert à `p`, rouge à `p + 16`, bleu à `p + 32`.

Le tampon est envoyé en paquets 0xA2 de 60 octets de charge (le dernier : 20 octets), numérotés à
partir de 0, chaque rapport ayant la longueur 5 + charge (pas de bourrage). 7 paquets par image.

Séquence : remplir le tampon à chaque mise à jour ; un fil l'envoie **en continu** (toutes les
≈10 ms, OpenRGB ne mettant jamais à jour son horodatage) ; luminosité envoyée au changement de mode.
Mode direct seulement ; pas de sauvegarde.

### 3.5 Famille « rapports 0x44 » (Origins 2 65, Eve 1800)

Output reports de 65 octets, **ID 0x44**.

| Message | Octets 1.. |
|---|---|
| Début de trame | 0x01, 0x04, 0x00 |
| Paquet de couleur | 0x02, numéro de paquet, 0x00, puis jusqu'à 20 triplets R,G,B à partir de l'octet 4 |
| Luminosité (Eve 1800) | **ID 0x40** : 0x01, 0x00, 0x00, valeur 0–255 |

- Origins 2 65 : 4 paquets numérotés 0–3 de 20 LED (74 emplacements, table §8.8 ; paquets complétés
  par un emplacement noir s'il ne reste rien). Maintien : comme pour l'Origins, flux de fait toutes
  les ≈10 ms chez OpenRGB (seuil prévu 50 ms).
- Eve 1800 : un seul paquet (numéro 0) avec les 10 zones (zones 1–10, ordre physique non documenté).
  Luminosité envoyée au changement de mode. Maintien : renvoi après 50 ms.

## 4. Souris

### 4.1 Pulsefire Surge (16D3, 0490)

Feature **0x07**, 264 octets.

| Message | Octet 1 | Contenu |
|---|---|---|
| Trame directe | 0x14 | 3 = 0xA0 ; offsets 8–39 = rouges des 32 LED de l'anneau, 40–71 = verts, 72–103 = bleus ; 108–110 = R, G, B du logo |
| Luminosité de profil | 0x03 | 2 = profil, 3 = 0x01, 4 = 0x01, 5 = 0x01, 6 = luminosité (défini, non utilisé) |
| Choisir le profil | 0x07 | 2 = profil (défini, non utilisé) |
| Configuration de profil | 0x01 | non utilisé ; modes définis : 0 uni, 1 cycle, 2 respiration, 3 vague, 4 « trigger » |

Mode direct uniquement ; maintien : renvoi après 50 ms.

### 4.2 Pulsefire FPS Pro et Core (16D7, 16DE, 0D8F)

Feature **0x07**, 264 octets : octet 1 = 0x0A, 2–4 = R, G, B, 8 = 0xA0. Une LED (logo). Maintien :
renvoi après 50 ms.

### 4.3 Pulsefire Dart (16E1, 16E2, 068E, 088E)

Output report de 65 octets (octet 0 = 0x00), pause de 2 ms après chaque envoi.

| Offset | Valeur |
|---|---|
| 1 | 0xD2 |
| 2 | LED : 0x00 logo, 0x10 molette, 0x20 toutes |
| 3 | mode : 0x00 statique, 0x12 cycle spectral, 0x20 respiration, 0x30 réactif |
| 4 | 0x08 (nombre d'octets qui suivent) |
| 5–7 | R, G, B |
| 8–10 | R, G, B (répétés) |
| 11 | luminosité 0–100 |
| 12 | vitesse : 0x64 lent … 0x00 rapide (moyen 0x32 ; 0 pour le statique) |

Sauvegarde : octet 1 = 0xDE, octet 2 = 0xFF (écrit l'état courant dans la souris).

Séquences : direct/respiration/réactif = un message pour la molette (couleur 0) puis un pour le logo
(couleur 1) ; cycle spectral = un message « toutes LED ». Mode et couleur sont envoyés à chaque
mise à jour. Pas de maintien.

### 4.4 Pulsefire Raid (16E4)

Feature **0x07**, 264 octets : octet 1 = 0x0A, 2–4 = R,G,B LED 0 (molette), 5–7 = R,G,B LED 1
(logo), 8 = 0xA0. Pause de 10 ms après l'envoi. Un réglage de luminosité 0–100 est exposé mais n'est
envoyé nulle part **(incertain)**. Maintien : renvoi après 1 s.

### 4.5 Pulsefire Haste (1727, 0F8F)

Deux feature reports de 65 octets à chaque mise à jour : en-tête 0x04, 0xF2, octet 8 = 0x02 ; puis
couleur 0x81, R, G, B, octet 8 = 0x02. Une LED (logo). Maintien : renvoi après 50 ms.

## 5. Tapis (Fury Ultra, Pulsefire Mat, Pulsefire Mat XL)

Trois feature reports de 65 octets par trame : en-tête 0x04, 0xF2, octet 9 = 0x02 ; puis deux
paquets de 16 emplacements `0x81 R G B`.

Ordre des LED : zone « Underglow » d'abord puis « LED Strip ». Fury Ultra et Pulsefire Mat :
15 LED de dessous + 5 LED de bandeau (20) ; Pulsefire Mat XL (1741) : 2 LED de dessous seulement
(les autres emplacements doivent être noirs ; OpenRGB lit au-delà de son tableau de couleurs, ne pas
reproduire). Ordre physique dans chaque zone non documenté. Maintien : renvoi après 50 ms.

## 6. Micros

### 6.1 QuadCast S / DuoCast

Feature reports de 65 octets (0x00 + 64), pause de 15 ms après chaque envoi. Deux LED : 0 = haut,
1 = bas.

| Message | Octets 1.. |
|---|---|
| Couleur directe | 0x81, R0, G0, B0, 0x81, R1, G1, B1 |
| Registre | 0x04, registre, 0, 0, 0, 0, 0, param1 (octet 8), param2 (octet 9) |
| Fin de transfert | 0x08 ; octets 0x3C = 0x28, 0x3D = nombre d'images, 0x3E = 0x00, 0x3F = 0xAA, 0x40 = 0x55 |
| Paquet d'images | jusqu'à 8 images de 8 octets : `0x81 R G B 0x81 R G B` (haut, bas) |

Registres : 0xF2 = appliquer, 0x53 = début de sauvegarde (param1 = nombre de paquets d'images),
0x02 = ?, 0x23 = ? (param1 = 1) **(rôle de 0x02 et 0x23 incertain)**.

- **Trame directe** : couleur directe, pause 15 ms, registre 0xF2 (param2 = 1). Maintien : renvoi
  après 50 ms.
- **Sauvegarde** (écriture en mémoire de l'appareil) : registre 0x53 (param1 = ⌈images / 8⌉), les
  paquets d'images (une seule image chez OpenRGB), registre 0x02, registre 0x23 (param1 = 1), fin de
  transfert, registre 0x02, puis une trame directe.

### 6.2 QuadCast 2 S (02B5)

Output reports de **64 octets, ID 0x44** ; après chaque envoi, lire un input report (délai 2 s) et
vérifier que ses octets 14 et 15 reprennent les octets 0 et 1 envoyés.

| Message | Octets |
|---|---|
| Début de trame directe | 0x44, 0x01, 0x06 |
| Paquet de couleur direct | 0x44, 0x02, numéro 0–5, 0x00, puis 20 triplets R,G,B (le dernier paquet : 8 LED) |
| Début de sauvegarde | 0x44, 0x03, 0x01, 0x06 |
| Paquet de couleur à sauvegarder | 0x44, 0x04, numéro 0–5, 0x00, triplets |
| Fin de sauvegarde 1 | **ID 0x42** : 0x02, 0, 0, 0, 0xE8, 0x03 (1000 en petit-boutiste) |
| Fin de sauvegarde 2 | **ID 0x40** : 0x01, 0, 0, 0xFF |

108 LED envoyées dans l'ordre de leur index matériel. Disposition physique (9 rangées × 12
colonnes ; colonnes 0–5 = face avant, 6–11 = face arrière ; valeur = index matériel) :

| Rangée | Avant 0–5 | Arrière 6–11 |
|---|---|---|
| 0 | 26 27 44 45 62 63 | 80 81 98 99 8 9 |
| 1 | 25 28 43 46 61 64 | 79 82 97 100 7 10 |
| 2 | 24 29 42 47 60 65 | 78 83 96 101 6 11 |
| 3 | 23 30 41 48 59 66 | 77 84 95 102 5 12 |
| 4 | 22 31 40 49 58 67 | 76 85 94 103 4 13 |
| 5 | 21 32 39 50 57 68 | 75 86 93 104 3 14 |
| 6 | 20 33 38 51 56 69 | 74 87 92 105 2 15 |
| 7 | 19 34 37 52 55 70 | 73 88 91 106 1 16 |
| 8 | 18 35 36 53 54 71 | 72 89 90 107 0 17 |

Gestion d'erreurs d'OpenRGB : à partir de 5 échecs de réponse en moins de 10 s, vider la file
d'entrée ; à 10 échecs, suspendre les envois 5 s. Maintien : renvoi après 1 s (vérification toutes
les 250 ms).

## 7. Sécurité et persistance

| Appareil | Écriture persistante |
|---|---|
| Alloy Elite RGB | rapports d'effet et de couleurs de profil (profil 1) : modifient vraisemblablement la mémoire ; ne pas les envoyer en boucle |
| Pulsefire Dart | 0xDE 0xFF explicite |
| QuadCast S / DuoCast | séquence 0x53 … fin de transfert |
| QuadCast 2 S | séquence 0x03 / 0x04 / 0x42 / 0x40 |
| Autres | aucune (trames volatiles) |

- La plupart des appareils HyperX reviennent à leur éclairage interne si le flux s'interrompt : un
  renvoi périodique (50 ms à 1 s selon le modèle) est nécessaire en mode direct.
- L'Origins Core reçoit un flux continu (~100 Hz) chez OpenRGB : prévoir une cadence raisonnable et
  vérifier sur matériel si un renvoi plus lent suffit **(incertain)**.
- Appliquer les garde-fous flash d'AGENTS.md avant toute sauvegarde.

## 8. Tables de LED des claviers

Codes de touche physiques W3C ; « — » = pas de touche (LED de barre, emplacement vide). Disposition ANSI/US telle que décrite par OpenRGB (les touches ISO présentes dans les tables, comme `IntlBackslash`, n’existent que sur les claviers ISO ; envoyer du noir sinon).

### 8.1 Alloy Elite RGB : offsets par touche

Les 106 premières LED ont un offset unique, utilisé dans les trois rapports de canal R, G et B (octet 3 = 0xA0 en direct). Les 22 suivantes (barre lumineuse 18 LED + 4 touches média) sont dans le rapport de canal 4, avec un offset distinct par composante.

| Index | Touche / LED | Offset (rapports canal R, G, B) | Offset R (canal 4) | Offset G (canal 4) | Offset B (canal 4) | Code W3C |
|---|---|---|---|---|---|---|
| 0 | Escape | 0x08 | — | — | — | Escape |
| 1 | ` | 0x09 | — | — | — | Backquote |
| 2 | Tab | 0x0A | — | — | — | Tab |
| 3 | Caps Lock | 0x0B | — | — | — | CapsLock |
| 4 | Left Shift | 0x0C | — | — | — | ShiftLeft |
| 5 | Left Control | 0x0D | — | — | — | ControlLeft |
| 6 | F12 | 0x0E | — | — | — | F12 |
| 7 | = | 0x0F | — | — | — | Equal |
| 8 | F9 | 0x10 | — | — | — | F9 |
| 9 | 9 | 0x11 | — | — | — | Digit9 |
| 10 | O | 0x12 | — | — | — | KeyO |
| 11 | L | 0x13 | — | — | — | KeyL |
| 12 | , | 0x14 | — | — | — | Comma |
| 13 | Menu | 0x15 | — | — | — | ContextMenu |
| 14 | Enter (ISO) | 0x16 | — | — | — | Enter |
| 15 | Left Arrow | 0x17 | — | — | — | ArrowLeft |
| 16 | F1 | 0x18 | — | — | — | F1 |
| 17 | 1 | 0x19 | — | — | — | Digit1 |
| 18 | Q | 0x1A | — | — | — | KeyQ |
| 19 | A | 0x1B | — | — | — | KeyA |
| 20 | \ (ISO) | 0x1C | — | — | — | IntlBackslash |
| 21 | Left Windows | 0x1D | — | — | — | MetaLeft |
| 22 | Print Screen | 0x1E | — | — | — | PrintScreen |
| 23 | F10 | 0x20 | — | — | — | F10 |
| 24 | 0 | 0x21 | — | — | — | Digit0 |
| 25 | P | 0x22 | — | — | — | KeyP |
| 26 | ; | 0x23 | — | — | — | Semicolon |
| 27 | . | 0x24 | — | — | — | Period |
| 28 | Enter | 0x26 | — | — | — | Enter |
| 29 | Down Arrow | 0x27 | — | — | — | ArrowDown |
| 30 | F2 | 0x28 | — | — | — | F2 |
| 31 | 2 | 0x29 | — | — | — | Digit2 |
| 32 | W | 0x2A | — | — | — | KeyW |
| 33 | S | 0x2B | — | — | — | KeyS |
| 34 | Z | 0x2C | — | — | — | KeyZ |
| 35 | Left Alt | 0x2D | — | — | — | AltLeft |
| 36 | Scroll Lock | 0x2E | — | — | — | ScrollLock |
| 37 | Backspace | 0x2F | — | — | — | Backspace |
| 38 | F11 | 0x30 | — | — | — | F11 |
| 39 | - | 0x31 | — | — | — | Minus |
| 40 | [ | 0x32 | — | — | — | BracketLeft |
| 41 | ' | 0x33 | — | — | — | Quote |
| 42 | / | 0x34 | — | — | — | Slash |
| 43 | Right Arrow | 0x37 | — | — | — | ArrowRight |
| 44 | F3 | 0x38 | — | — | — | F3 |
| 45 | 3 | 0x39 | — | — | — | Digit3 |
| 46 | E | 0x3A | — | — | — | KeyE |
| 47 | D | 0x3B | — | — | — | KeyD |
| 48 | X | 0x3C | — | — | — | KeyX |
| 49 | Pause/Break | 0x3E | — | — | — | Pause |
| 50 | Delete | 0x3F | — | — | — | Delete |
| 51 | Number Pad 7 | 0x41 | — | — | — | Numpad7 |
| 52 | Num Lock | 0x44 | — | — | — | NumLock |
| 53 | Number Pad 6 | 0x45 | — | — | — | Numpad6 |
| 54 | F4 | 0x48 | — | — | — | F4 |
| 55 | 4 | 0x49 | — | — | — | Digit4 |
| 56 | R | 0x4A | — | — | — | KeyR |
| 57 | F | 0x4B | — | — | — | KeyF |
| 58 | C | 0x4C | — | — | — | KeyC |
| 59 | Space | 0x4D | — | — | — | Space |
| 60 | Insert | 0x4E | — | — | — | Insert |
| 61 | End | 0x4F | — | — | — | End |
| 62 | Number Pad 8 | 0x51 | — | — | — | Numpad8 |
| 63 | Number Pad / | 0x54 | — | — | — | NumpadDivide |
| 64 | Number Pad 1 | 0x55 | — | — | — | Numpad1 |
| 65 | F5 | 0x58 | — | — | — | F5 |
| 66 | 5 | 0x59 | — | — | — | Digit5 |
| 67 | T | 0x5A | — | — | — | KeyT |
| 68 | G | 0x5B | — | — | — | KeyG |
| 69 | V | 0x5C | — | — | — | KeyV |
| 70 | Home | 0x5E | — | — | — | Home |
| 71 | Page Down | 0x5F | — | — | — | PageDown |
| 72 | Number Pad 9 | 0x61 | — | — | — | Numpad9 |
| 73 | Number Pad * | 0x64 | — | — | — | NumpadMultiply |
| 74 | Number Pad 2 | 0x65 | — | — | — | Numpad2 |
| 75 | F6 | 0x68 | — | — | — | F6 |
| 76 | 6 | 0x69 | — | — | — | Digit6 |
| 77 | Y | 0x6A | — | — | — | KeyY |
| 78 | H | 0x6B | — | — | — | KeyH |
| 79 | B | 0x6C | — | — | — | KeyB |
| 80 | Page Up | 0x6E | — | — | — | PageUp |
| 81 | Right Shift | 0x6F | — | — | — | ShiftRight |
| 82 | Number Pad - | 0x74 | — | — | — | NumpadSubtract |
| 83 | Number Pad 3 | 0x75 | — | — | — | Numpad3 |
| 84 | F7 | 0x78 | — | — | — | F7 |
| 85 | 7 | 0x79 | — | — | — | Digit7 |
| 86 | U | 0x7A | — | — | — | KeyU |
| 87 | J | 0x7B | — | — | — | KeyJ |
| 88 | N | 0x7C | — | — | — | KeyN |
| 89 | Right Alt | 0x7D | — | — | — | AltRight |
| 90 | ] | 0x7E | — | — | — | BracketRight |
| 91 | Right Control | 0x7F | — | — | — | ControlRight |
| 92 | Number Pad 4 | 0x81 | — | — | — | Numpad4 |
| 93 | Number Pad + | 0x84 | — | — | — | NumpadAdd |
| 94 | Number Pad 0 | 0x85 | — | — | — | Numpad0 |
| 95 | F8 | 0x88 | — | — | — | F8 |
| 96 | 8 | 0x89 | — | — | — | Digit8 |
| 97 | I | 0x8A | — | — | — | KeyI |
| 98 | K | 0x8B | — | — | — | KeyK |
| 99 | M | 0x8C | — | — | — | KeyM |
| 100 | Right Windows | 0x8D | — | — | — | MetaRight |
| 101 | \ (ANSI) | 0x8E | — | — | — | Backslash |
| 102 | Up Arrow | 0x8F | — | — | — | ArrowUp |
| 103 | Number Pad 5 | 0x91 | — | — | — | Numpad5 |
| 104 | Number Pad Enter | 0x94 | — | — | — | NumpadEnter |
| 105 | Number Pad . | 0x95 | — | — | — | NumpadDecimal |
| 106 | RGB Strip 1 | — | 0x08 | 0x29 | 0x39 | — |
| 107 | RGB Strip 2 | — | 0x48 | 0x28 | 0x38 | — |
| 108 | RGB Strip 3 | — | 0x88 | 0x78 | 0x68 | — |
| 109 | RGB Strip 4 | — | 0x09 | 0x19 | 0x3A | — |
| 110 | RGB Strip 5 | — | 0x89 | 0x79 | 0x69 | — |
| 111 | RGB Strip 6 | — | 0x0A | 0x1A | 0x2A | — |
| 112 | RGB Strip 7 | — | 0x8A | 0x7A | 0x6A | — |
| 113 | RGB Strip 8 | — | 0x0B | 0x1B | 0x2B | — |
| 114 | RGB Strip 9 | — | 0x8B | 0x7B | 0x6B | — |
| 115 | RGB Strip 10 | — | 0x0C | 0x1C | 0x2C | — |
| 116 | RGB Strip 11 | — | 0x8C | 0x7C | 0x6C | — |
| 117 | RGB Strip 12 | — | 0x0D | 0x1D | 0x2D | — |
| 118 | RGB Strip 13 | — | 0x8D | 0x7D | 0x6D | — |
| 119 | RGB Strip 14 | — | 0x0E | 0x1E | 0x2E | — |
| 120 | RGB Strip 15 | — | 0x8F | 0x6E | 0x5E | — |
| 121 | RGB Strip 16 | — | 0x8E | 0x7E | 0x5D | — |
| 122 | RGB Strip 17 | — | 0x0F | 0x1F | 0x2F | — |
| 123 | RGB Strip 18 | — | 0x4F | 0x6F | 0x5F | — |
| 124 | Media Previous | — | 0x92 | 0x82 | 0x72 | MediaTrackPrevious |
| 125 | Media Play/Pause | — | 0x13 | 0x23 | 0x33 | MediaPlayPause |
| 126 | Media Next | — | 0x93 | 0x83 | 0x73 | MediaTrackNext |
| 127 | Media Mute | — | 0x12 | 0x22 | 0x32 | AudioVolumeMute |

### 8.2 Alloy FPS RGB : offsets par touche

| Index | Touche | Offset dans le rapport | Code W3C |
|---|---|---|---|
| 0 | Escape | 0x08 | Escape |
| 1 | ` | 0x09 | Backquote |
| 2 | Tab | 0x0A | Tab |
| 3 | Caps Lock | 0x0B | CapsLock |
| 4 | Left Shift | 0x0C | ShiftLeft |
| 5 | Left Control | 0x0D | ControlLeft |
| 6 | F12 | 0x0E | F12 |
| 7 | = | 0x0F | Equal |
| 8 | F9 | 0x10 | F9 |
| 9 | 9 | 0x11 | Digit9 |
| 10 | O | 0x12 | KeyO |
| 11 | L | 0x13 | KeyL |
| 12 | , | 0x14 | Comma |
| 13 | Menu | 0x15 | ContextMenu |
| 14 | Enter (ISO) | 0x16 | Enter |
| 15 | Left Arrow | 0x17 | ArrowLeft |
| 16 | F1 | 0x18 | F1 |
| 17 | 1 | 0x19 | Digit1 |
| 18 | Q | 0x1A | KeyQ |
| 19 | A | 0x1B | KeyA |
| 20 | \ (ISO) | 0x1C | IntlBackslash |
| 21 | Left Windows | 0x1D | MetaLeft |
| 22 | Print Screen | 0x1E | PrintScreen |
| 23 | F10 | 0x20 | F10 |
| 24 | 0 | 0x21 | Digit0 |
| 25 | P | 0x22 | KeyP |
| 26 | ; | 0x23 | Semicolon |
| 27 | . | 0x24 | Period |
| 28 | Enter | 0x26 | Enter |
| 29 | Down Arrow | 0x27 | ArrowDown |
| 30 | F2 | 0x28 | F2 |
| 31 | 2 | 0x29 | Digit2 |
| 32 | W | 0x2A | KeyW |
| 33 | S | 0x2B | KeyS |
| 34 | Z | 0x2C | KeyZ |
| 35 | Left Alt | 0x2D | AltLeft |
| 36 | Scroll Lock | 0x2E | ScrollLock |
| 37 | Backspace | 0x2F | Backspace |
| 38 | F11 | 0x30 | F11 |
| 39 | - | 0x31 | Minus |
| 40 | [ | 0x32 | BracketLeft |
| 41 | ' | 0x33 | Quote |
| 42 | / | 0x34 | Slash |
| 43 | Right Arrow | 0x37 | ArrowRight |
| 44 | F3 | 0x38 | F3 |
| 45 | 3 | 0x39 | Digit3 |
| 46 | E | 0x3A | KeyE |
| 47 | D | 0x3B | KeyD |
| 48 | X | 0x3C | KeyX |
| 49 | Pause/Break | 0x3E | Pause |
| 50 | Delete | 0x3F | Delete |
| 51 | Number Pad 7 | 0x41 | Numpad7 |
| 52 | Num Lock | 0x44 | NumLock |
| 53 | Number Pad 6 | 0x45 | Numpad6 |
| 54 | F4 | 0x48 | F4 |
| 55 | 4 | 0x49 | Digit4 |
| 56 | R | 0x4A | KeyR |
| 57 | F | 0x4B | KeyF |
| 58 | C | 0x4C | KeyC |
| 59 | Space | 0x4D | Space |
| 60 | Insert | 0x4E | Insert |
| 61 | End | 0x4F | End |
| 62 | Number Pad 8 | 0x51 | Numpad8 |
| 63 | Number Pad / | 0x54 | NumpadDivide |
| 64 | Number Pad 1 | 0x55 | Numpad1 |
| 65 | F5 | 0x58 | F5 |
| 66 | 5 | 0x59 | Digit5 |
| 67 | T | 0x5A | KeyT |
| 68 | G | 0x5B | KeyG |
| 69 | V | 0x5C | KeyV |
| 70 | Home | 0x5E | Home |
| 71 | Page Down | 0x5F | PageDown |
| 72 | Number Pad 9 | 0x61 | Numpad9 |
| 73 | Number Pad * | 0x64 | NumpadMultiply |
| 74 | Number Pad 2 | 0x65 | Numpad2 |
| 75 | F6 | 0x68 | F6 |
| 76 | 6 | 0x69 | Digit6 |
| 77 | Y | 0x6A | KeyY |
| 78 | H | 0x6B | KeyH |
| 79 | B | 0x6C | KeyB |
| 80 | Page Up | 0x6E | PageUp |
| 81 | Right Shift | 0x6F | ShiftRight |
| 82 | Number Pad - | 0x74 | NumpadSubtract |
| 83 | Number Pad 3 | 0x75 | Numpad3 |
| 84 | F7 | 0x78 | F7 |
| 85 | 7 | 0x79 | Digit7 |
| 86 | U | 0x7A | KeyU |
| 87 | J | 0x7B | KeyJ |
| 88 | N | 0x7C | KeyN |
| 89 | Right Alt | 0x7D | AltRight |
| 90 | ] | 0x7E | BracketRight |
| 91 | Right Control | 0x7F | ControlRight |
| 92 | Number Pad 4 | 0x81 | Numpad4 |
| 93 | Number Pad + | 0x84 | NumpadAdd |
| 94 | Number Pad 0 | 0x85 | Numpad0 |
| 95 | F8 | 0x88 | F8 |
| 96 | 8 | 0x89 | Digit8 |
| 97 | I | 0x8A | KeyI |
| 98 | K | 0x8B | KeyK |
| 99 | M | 0x8C | KeyM |
| 100 | Right Windows | 0x8D | MetaRight |
| 101 | \ (ANSI) | 0x8E | Backslash |
| 102 | Up Arrow | 0x8F | ArrowUp |
| 103 | Number Pad 5 | 0x91 | Numpad5 |
| 104 | Number Pad Enter | 0x94 | NumpadEnter |
| 105 | Number Pad . | 0x95 | NumpadDecimal |

### 8.3 Alloy Elite 2 : emplacements

Emplacement = 16 × numéro de paquet + position dans le paquet. Les emplacements « (vide) » reçoivent du noir.

| Emplacement matériel | Touche / LED | Code W3C |
|---|---|---|
| 0 | Escape | Escape |
| 1 | ` | Backquote |
| 2 | Tab | Tab |
| 3 | Caps Lock | CapsLock |
| 4 | Left Shift | ShiftLeft |
| 5 | Left Control | ControlLeft |
| 6 | \ (ISO) | IntlBackslash |
| 7 | 1 | Digit1 |
| 8 | Q | KeyQ |
| 9 | A | KeyA |
| 10 | Z | KeyZ |
| 11 | Left Windows | MetaLeft |
| 12 | F1 | F1 |
| 13 | 2 | Digit2 |
| 14 | W | KeyW |
| 15 | S | KeyS |
| 16 | X | KeyX |
| 17 | Left Alt | AltLeft |
| 18 | F2 | F2 |
| 19 | 3 | Digit3 |
| 20 | E | KeyE |
| 21 | D | KeyD |
| 22 | C | KeyC |
| 23 | (vide) | — |
| 24 | F3 | F3 |
| 25 | 4 | Digit4 |
| 26 | R | KeyR |
| 27 | F | KeyF |
| 28 | V | KeyV |
| 29 | (vide) | — |
| 30 | F4 | F4 |
| 31 | 5 | Digit5 |
| 32 | T | KeyT |
| 33 | G | KeyG |
| 34 | B | KeyB |
| 35 | Space | Space |
| 36 | F5 | F5 |
| 37 | 6 | Digit6 |
| 38 | Y | KeyY |
| 39 | H | KeyH |
| 40 | N | KeyN |
| 41 | (vide) | — |
| 42 | F6 | F6 |
| 43 | 7 | Digit7 |
| 44 | U | KeyU |
| 45 | J | KeyJ |
| 46 | M | KeyM |
| 47 | (vide) | — |
| 48 | F7 | F7 |
| 49 | 8 | Digit8 |
| 50 | I | KeyI |
| 51 | K | KeyK |
| 52 | , | Comma |
| 53 | Right Alt | AltRight |
| 54 | F8 | F8 |
| 55 | 9 | Digit9 |
| 56 | O | KeyO |
| 57 | L | KeyL |
| 58 | . | Period |
| 59 | Menu | ContextMenu |
| 60 | F9 | F9 |
| 61 | 0 | Digit0 |
| 62 | P | KeyP |
| 63 | ; | Semicolon |
| 64 | / | Slash |
| 65 | Right Windows | MetaRight |
| 66 | F10 | F10 |
| 67 | - | Minus |
| 68 | [ | BracketLeft |
| 69 | ' | Quote |
| 70 | (vide) | — |
| 71 | (vide) | — |
| 72 | F11 | F11 |
| 73 | = | Equal |
| 74 | ] | BracketRight |
| 75 | # | Backslash |
| 76 | (vide) | — |
| 77 | (vide) | — |
| 78 | F12 | F12 |
| 79 | Backspace | Backspace |
| 80 | \ (ANSI) | Backslash |
| 81 | Enter | Enter |
| 82 | Right Shift | ShiftRight |
| 83 | Right Control | ControlRight |
| 84 | Print Screen | PrintScreen |
| 85 | Insert | Insert |
| 86 | Delete | Delete |
| 87 | (vide) | — |
| 88 | (vide) | — |
| 89 | Left Arrow | ArrowLeft |
| 90 | Scroll Lock | ScrollLock |
| 91 | Home | Home |
| 92 | End | End |
| 93 | (vide) | — |
| 94 | Up Arrow | ArrowUp |
| 95 | Down Arrow | ArrowDown |
| 96 | Pause/Break | Pause |
| 97 | Page Up | PageUp |
| 98 | Page Down | PageDown |
| 99 | (vide) | — |
| 100 | (vide) | — |
| 101 | Right Arrow | ArrowRight |
| 102 | (vide) | — |
| 103 | Num Lock | NumLock |
| 104 | Number Pad 7 | Numpad7 |
| 105 | Number Pad 4 | Numpad4 |
| 106 | Number Pad 1 | Numpad1 |
| 107 | Number Pad 0 | Numpad0 |
| 108 | (vide) | — |
| 109 | Number Pad / | NumpadDivide |
| 110 | Number Pad 8 | Numpad8 |
| 111 | Number Pad 5 | Numpad5 |
| 112 | Number Pad 2 | Numpad2 |
| 113 | (vide) | — |
| 114 | Media Mute | AudioVolumeMute |
| 115 | Number Pad * | NumpadMultiply |
| 116 | Number Pad 9 | Numpad9 |
| 117 | Number Pad 6 | Numpad6 |
| 118 | Number Pad 3 | Numpad3 |
| 119 | Number Pad . | NumpadDecimal |
| 120 | Media Previous | MediaTrackPrevious |
| 121 | Number Pad - | NumpadSubtract |
| 122 | Number Pad + | NumpadAdd |
| 123 | Media Play/Pause | MediaPlayPause |
| 124 | Media Next | MediaTrackNext |
| 125 | Number Pad Enter | NumpadEnter |
| 126 | RGB Strip 1 | — |
| 127 | RGB Strip 2 | — |
| 128 | RGB Strip 3 | — |
| 129 | RGB Strip 4 | — |
| 130 | RGB Strip 5 | — |
| 131 | RGB Strip 6 | — |
| 132 | RGB Strip 7 | — |
| 133 | RGB Strip 8 | — |
| 134 | RGB Strip 9 | — |
| 135 | RGB Strip 10 | — |
| 136 | RGB Strip 11 | — |
| 137 | RGB Strip 12 | — |
| 138 | RGB Strip 13 | — |
| 139 | RGB Strip 14 | — |
| 140 | RGB Strip 15 | — |
| 141 | RGB Strip 16 | — |
| 142 | RGB Strip 17 | — |
| 143 | RGB Strip 18 | — |

### 8.4 Alloy Origins : emplacements

| Emplacement matériel | Touche / LED | Code W3C |
|---|---|---|
| 0 | Escape | Escape |
| 1 | ` | Backquote |
| 2 | Tab | Tab |
| 3 | Caps Lock | CapsLock |
| 4 | Left Shift | ShiftLeft |
| 5 | Left Control | ControlLeft |
| 6 | \ (ISO) | IntlBackslash |
| 7 | 1 | Digit1 |
| 8 | Q | KeyQ |
| 9 | A | KeyA |
| 10 | Z | KeyZ |
| 11 | Left Windows | MetaLeft |
| 12 | F1 | F1 |
| 13 | 2 | Digit2 |
| 14 | W | KeyW |
| 15 | S | KeyS |
| 16 | X | KeyX |
| 17 | Left Alt | AltLeft |
| 18 | F2 | F2 |
| 19 | 3 | Digit3 |
| 20 | E | KeyE |
| 21 | D | KeyD |
| 22 | C | KeyC |
| 23 | (vide) | — |
| 24 | F3 | F3 |
| 25 | 4 | Digit4 |
| 26 | R | KeyR |
| 27 | F | KeyF |
| 28 | V | KeyV |
| 29 | (vide) | — |
| 30 | F4 | F4 |
| 31 | 5 | Digit5 |
| 32 | T | KeyT |
| 33 | G | KeyG |
| 34 | B | KeyB |
| 35 | Space | Space |
| 36 | F5 | F5 |
| 37 | 6 | Digit6 |
| 38 | Y | KeyY |
| 39 | H | KeyH |
| 40 | N | KeyN |
| 41 | (vide) | — |
| 42 | F6 | F6 |
| 43 | 7 | Digit7 |
| 44 | U | KeyU |
| 45 | J | KeyJ |
| 46 | M | KeyM |
| 47 | (vide) | — |
| 48 | F7 | F7 |
| 49 | 8 | Digit8 |
| 50 | I | KeyI |
| 51 | K | KeyK |
| 52 | , | Comma |
| 53 | Right Alt | AltRight |
| 54 | F8 | F8 |
| 55 | 9 | Digit9 |
| 56 | O | KeyO |
| 57 | L | KeyL |
| 58 | . | Period |
| 59 | (vide) | — |
| 60 | F9 | F9 |
| 61 | 0 | Digit0 |
| 62 | P | KeyP |
| 63 | ; | Semicolon |
| 64 | / | Slash |
| 65 | Right Fn | Fn |
| 66 | F10 | F10 |
| 67 | - | Minus |
| 68 | [ | BracketLeft |
| 69 | ' | Quote |
| 70 | (vide) | — |
| 71 | (vide) | — |
| 72 | F11 | F11 |
| 73 | = | Equal |
| 74 | ] | BracketRight |
| 75 | # | Backslash |
| 76 | / (ABNT) | IntlRo |
| 77 | Menu | ContextMenu |
| 78 | F12 | F12 |
| 79 | Backspace | Backspace |
| 80 | \ (ANSI) | Backslash |
| 81 | Enter | Enter |
| 82 | Right Shift | ShiftRight |
| 83 | Right Control | ControlRight |
| 84 | Print Screen | PrintScreen |
| 85 | Insert | Insert |
| 86 | Delete | Delete |
| 87 | (vide) | — |
| 88 | (vide) | — |
| 89 | Left Arrow | ArrowLeft |
| 90 | Scroll Lock | ScrollLock |
| 91 | Home | Home |
| 92 | End | End |
| 93 | (vide) | — |
| 94 | Up Arrow | ArrowUp |
| 95 | Down Arrow | ArrowDown |
| 96 | Pause/Break | Pause |
| 97 | Page Up | PageUp |
| 98 | Page Down | PageDown |
| 99 | (vide) | — |
| 100 | (vide) | — |
| 101 | Right Arrow | ArrowRight |
| 102 | (vide) | — |
| 103 | Num Lock | NumLock |
| 104 | Number Pad 7 | Numpad7 |
| 105 | Number Pad 4 | Numpad4 |
| 106 | Number Pad 1 | Numpad1 |
| 107 | Number Pad 0 | Numpad0 |
| 108 | (vide) | — |
| 109 | Number Pad / | NumpadDivide |
| 110 | Number Pad 8 | Numpad8 |
| 111 | Number Pad 5 | Numpad5 |
| 112 | Number Pad 2 | Numpad2 |
| 113 | (vide) | — |
| 114 | (vide) | — |
| 115 | Number Pad * | NumpadMultiply |
| 116 | Number Pad 9 | Numpad9 |
| 117 | Number Pad 6 | Numpad6 |
| 118 | Number Pad 3 | Numpad3 |
| 119 | Number Pad . | NumpadDecimal |
| 120 | (vide) | — |
| 121 | Number Pad - | NumpadSubtract |
| 122 | Number Pad + | NumpadAdd |
| 123 | (vide) | — |
| 124 | (vide) | — |
| 125 | Number Pad Enter | NumpadEnter |

### 8.5 Alloy Origins 60 : emplacements

| Emplacement matériel | Touche / LED | Code W3C |
|---|---|---|
| 0 | (vide) | — |
| 1 | Escape | Escape |
| 2 | 1 | Digit1 |
| 3 | 2 | Digit2 |
| 4 | 3 | Digit3 |
| 5 | 4 | Digit4 |
| 6 | 5 | Digit5 |
| 7 | 6 | Digit6 |
| 8 | 7 | Digit7 |
| 9 | 8 | Digit8 |
| 10 | 9 | Digit9 |
| 11 | 0 | Digit0 |
| 12 | - | Minus |
| 13 | = | Equal |
| 14 | (vide) | — |
| 15 | Backspace | Backspace |
| 16 | Tab | Tab |
| 17 | Q | KeyQ |
| 18 | W | KeyW |
| 19 | E | KeyE |
| 20 | R | KeyR |
| 21 | T | KeyT |
| 22 | Y | KeyY |
| 23 | U | KeyU |
| 24 | I | KeyI |
| 25 | O | KeyO |
| 26 | P | KeyP |
| 27 | [ | BracketLeft |
| 28 | ] | BracketRight |
| 29 | \ (ANSI) | Backslash |
| 30 | Caps Lock | CapsLock |
| 31 | A | KeyA |
| 32 | S | KeyS |
| 33 | D | KeyD |
| 34 | F | KeyF |
| 35 | G | KeyG |
| 36 | H | KeyH |
| 37 | J | KeyJ |
| 38 | K | KeyK |
| 39 | L | KeyL |
| 40 | ; | Semicolon |
| 41 | ' | Quote |
| 42 | (vide) | — |
| 43 | Enter | Enter |
| 44 | Left Shift | ShiftLeft |
| 45 | (vide) | — |
| 46 | Z | KeyZ |
| 47 | X | KeyX |
| 48 | C | KeyC |
| 49 | V | KeyV |
| 50 | B | KeyB |
| 51 | N | KeyN |
| 52 | M | KeyM |
| 53 | , | Comma |
| 54 | . | Period |
| 55 | / | Slash |
| 56 | (vide) | — |
| 57 | Right Shift | ShiftRight |
| 58 | Left Control | ControlLeft |
| 59 | Left Windows | MetaLeft |
| 60 | Left Alt | AltLeft |
| 61 | Left Space | Space |
| 62 | Space | Space |
| 63 | Right Space | Space |
| 64 | Right Alt | AltRight |
| 65 | Menu | ContextMenu |
| 66 | Right Control | ControlRight |
| 67 | (vide) | — |
| 68 | (vide) | — |
| 69 | (vide) | — |
| 70 | Right Fn | Fn |

### 8.6 Alloy Origins 65 (HP) : emplacements

| Emplacement matériel | Touche / LED | Code W3C |
|---|---|---|
| 0 | (vide) | — |
| 1 | Escape | Escape |
| 2 | 1 | Digit1 |
| 3 | 2 | Digit2 |
| 4 | 3 | Digit3 |
| 5 | 4 | Digit4 |
| 6 | 5 | Digit5 |
| 7 | 6 | Digit6 |
| 8 | 7 | Digit7 |
| 9 | 8 | Digit8 |
| 10 | 9 | Digit9 |
| 11 | 0 | Digit0 |
| 12 | - | Minus |
| 13 | = | Equal |
| 14 | (vide) | — |
| 15 | Backspace | Backspace |
| 16 | Tab | Tab |
| 17 | Q | KeyQ |
| 18 | W | KeyW |
| 19 | E | KeyE |
| 20 | R | KeyR |
| 21 | T | KeyT |
| 22 | Y | KeyY |
| 23 | U | KeyU |
| 24 | I | KeyI |
| 25 | O | KeyO |
| 26 | P | KeyP |
| 27 | [ | BracketLeft |
| 28 | ] | BracketRight |
| 29 | \ (ANSI) | Backslash |
| 30 | Caps Lock | CapsLock |
| 31 | A | KeyA |
| 32 | S | KeyS |
| 33 | D | KeyD |
| 34 | F | KeyF |
| 35 | G | KeyG |
| 36 | H | KeyH |
| 37 | J | KeyJ |
| 38 | K | KeyK |
| 39 | L | KeyL |
| 40 | ; | Semicolon |
| 41 | ' | Quote |
| 42 | (vide) | — |
| 43 | Enter | Enter |
| 44 | Left Shift | ShiftLeft |
| 45 | (vide) | — |
| 46 | Z | KeyZ |
| 47 | X | KeyX |
| 48 | C | KeyC |
| 49 | V | KeyV |
| 50 | B | KeyB |
| 51 | N | KeyN |
| 52 | M | KeyM |
| 53 | , | Comma |
| 54 | . | Period |
| 55 | / | Slash |
| 56 | (vide) | — |
| 57 | Right Shift | ShiftRight |
| 58 | Left Control | ControlLeft |
| 59 | Left Windows | MetaLeft |
| 60 | Left Alt | AltLeft |
| 61 | Left Space | Space |
| 62 | Space | Space |
| 63 | Right Space | Space |
| 64 | Right Alt | AltRight |
| 65 | (vide) | — |
| 66 | (vide) | — |
| 67 | (vide) | — |
| 68 | Right Fn | Fn |
| 69 | Home | Home |
| 70 | Delete | Delete |
| 71 | Page Up | PageUp |
| 72 | Page Down | PageDown |
| 73 | Up Arrow | ArrowUp |
| 74 | Left Arrow | ArrowLeft |
| 75 | Down Arrow | ArrowDown |
| 76 | Right Arrow | ArrowRight |

### 8.7 Alloy Origins Core : valeurs matérielles (ANSI)

Offsets dans le tampon de 380 octets (formule du §3.4). Variante ABNT2 (octet 56 = 0x10), différences avec la table ANSI : la touche `Backslash` ANSI (valeur 69, ligne 2 col. 13) disparaît ; `Backslash` en position ISO « # » prend la valeur 77 (ligne 3 col. 12) ; `IntlBackslash` prend la valeur 34 (ligne 4 col. 1) ; la touche à droite de `Period` (`Slash` en W3C, « ; » sur ABNT2) garde la valeur 82 ; une touche supplémentaire `IntlRo` (« / » ABNT2) prend la valeur 83 (ligne 4 col. 12). 89 LED au total. **(Reconstitution à partir des données de disposition d'OpenRGB, à vérifier sur matériel.)**

| Index LED | Touche (ANSI) | Valeur matérielle v | Ligne | Col. | Offset G | Offset R | Offset B | Code W3C |
|---|---|---|---|---|---|---|---|---|
| 0 | Escape | 0 | 0 | 0 | 0 | 16 | 32 | Escape |
| 1 | F1 | 1 | 0 | 2 | 1 | 17 | 33 | F1 |
| 2 | F2 | 2 | 0 | 3 | 2 | 18 | 34 | F2 |
| 3 | F3 | 3 | 0 | 4 | 3 | 19 | 35 | F3 |
| 4 | F4 | 4 | 0 | 5 | 4 | 20 | 36 | F4 |
| 5 | F5 | 5 | 0 | 6 | 5 | 21 | 37 | F5 |
| 6 | F6 | 6 | 0 | 7 | 8 | 24 | 40 | F6 |
| 7 | F7 | 7 | 0 | 8 | 9 | 25 | 41 | F7 |
| 8 | F8 | 48 | 0 | 9 | 192 | 208 | 224 | F8 |
| 9 | F9 | 49 | 0 | 10 | 193 | 209 | 225 | F9 |
| 10 | F10 | 50 | 0 | 11 | 194 | 210 | 226 | F10 |
| 11 | F11 | 51 | 0 | 12 | 195 | 211 | 227 | F11 |
| 12 | F12 | 52 | 0 | 13 | 196 | 212 | 228 | F12 |
| 13 | Print Screen | 53 | 0 | 14 | 197 | 213 | 229 | PrintScreen |
| 14 | Scroll Lock | 54 | 0 | 15 | 200 | 216 | 232 | ScrollLock |
| 15 | Pause/Break | 55 | 0 | 16 | 201 | 217 | 233 | Pause |
| 16 | ` | 8 | 1 | 0 | 10 | 26 | 42 | Backquote |
| 17 | 1 | 9 | 1 | 1 | 11 | 27 | 43 | Digit1 |
| 18 | 2 | 10 | 1 | 2 | 12 | 28 | 44 | Digit2 |
| 19 | 3 | 11 | 1 | 3 | 13 | 29 | 45 | Digit3 |
| 20 | 4 | 12 | 1 | 4 | 48 | 64 | 80 | Digit4 |
| 21 | 5 | 13 | 1 | 5 | 49 | 65 | 81 | Digit5 |
| 22 | 6 | 14 | 1 | 6 | 50 | 66 | 82 | Digit6 |
| 23 | 7 | 15 | 1 | 7 | 51 | 67 | 83 | Digit7 |
| 24 | 8 | 16 | 1 | 8 | 52 | 68 | 84 | Digit8 |
| 25 | 9 | 56 | 1 | 9 | 202 | 218 | 234 | Digit9 |
| 26 | 0 | 57 | 1 | 10 | 203 | 219 | 235 | Digit0 |
| 27 | - | 58 | 1 | 11 | 204 | 220 | 236 | Minus |
| 28 | = | 59 | 1 | 12 | 205 | 221 | 237 | Equal |
| 29 | Backspace | 60 | 1 | 13 | 240 | 256 | 272 | Backspace |
| 30 | Insert | 61 | 1 | 14 | 241 | 257 | 273 | Insert |
| 31 | Home | 62 | 1 | 15 | 242 | 258 | 274 | Home |
| 32 | Page Up | 63 | 1 | 16 | 243 | 259 | 275 | PageUp |
| 33 | Tab | 17 | 2 | 0 | 53 | 69 | 85 | Tab |
| 34 | Q | 18 | 2 | 1 | 56 | 72 | 88 | KeyQ |
| 35 | W | 19 | 2 | 2 | 57 | 73 | 89 | KeyW |
| 36 | E | 20 | 2 | 3 | 58 | 74 | 90 | KeyE |
| 37 | R | 21 | 2 | 4 | 59 | 75 | 91 | KeyR |
| 38 | T | 22 | 2 | 5 | 60 | 76 | 92 | KeyT |
| 39 | Y | 23 | 2 | 6 | 61 | 77 | 93 | KeyY |
| 40 | U | 24 | 2 | 7 | 96 | 112 | 128 | KeyU |
| 41 | I | 64 | 2 | 8 | 244 | 260 | 276 | KeyI |
| 42 | O | 65 | 2 | 9 | 245 | 261 | 277 | KeyO |
| 43 | P | 66 | 2 | 10 | 248 | 264 | 280 | KeyP |
| 44 | [ | 67 | 2 | 11 | 249 | 265 | 281 | BracketLeft |
| 45 | ] | 68 | 2 | 12 | 250 | 266 | 282 | BracketRight |
| 46 | \ (ANSI) | 69 | 2 | 13 | 251 | 267 | 283 | Backslash |
| 47 | Delete | 70 | 2 | 14 | 252 | 268 | 284 | Delete |
| 48 | End | 71 | 2 | 15 | 253 | 269 | 285 | End |
| 49 | Page Down | 72 | 2 | 16 | 288 | 304 | 320 | PageDown |
| 50 | Caps Lock | 25 | 3 | 0 | 97 | 113 | 129 | CapsLock |
| 51 | A | 26 | 3 | 1 | 98 | 114 | 130 | KeyA |
| 52 | S | 27 | 3 | 2 | 99 | 115 | 131 | KeyS |
| 53 | D | 28 | 3 | 3 | 100 | 116 | 132 | KeyD |
| 54 | F | 29 | 3 | 4 | 101 | 117 | 133 | KeyF |
| 55 | G | 30 | 3 | 5 | 104 | 120 | 136 | KeyG |
| 56 | H | 31 | 3 | 6 | 105 | 121 | 137 | KeyH |
| 57 | J | 32 | 3 | 7 | 106 | 122 | 138 | KeyJ |
| 58 | K | 73 | 3 | 8 | 289 | 305 | 321 | KeyK |
| 59 | L | 74 | 3 | 9 | 290 | 306 | 322 | KeyL |
| 60 | ; | 75 | 3 | 10 | 291 | 307 | 323 | Semicolon |
| 61 | ' | 76 | 3 | 11 | 292 | 308 | 324 | Quote |
| 62 | Enter | 78 | 3 | 13 | 296 | 312 | 328 | Enter |
| 63 | Left Shift | 33 | 4 | 0 | 107 | 123 | 139 | ShiftLeft |
| 64 | Z | 35 | 4 | 2 | 109 | 125 | 141 | KeyZ |
| 65 | X | 36 | 4 | 3 | 144 | 160 | 176 | KeyX |
| 66 | C | 37 | 4 | 4 | 145 | 161 | 177 | KeyC |
| 67 | V | 38 | 4 | 5 | 146 | 162 | 178 | KeyV |
| 68 | B | 39 | 4 | 6 | 147 | 163 | 179 | KeyB |
| 69 | N | 40 | 4 | 7 | 148 | 164 | 180 | KeyN |
| 70 | M | 79 | 4 | 8 | 297 | 313 | 329 | KeyM |
| 71 | , | 80 | 4 | 9 | 298 | 314 | 330 | Comma |
| 72 | . | 81 | 4 | 10 | 299 | 315 | 331 | Period |
| 73 | / | 82 | 4 | 11 | 300 | 316 | 332 | Slash |
| 74 | Right Shift | 84 | 4 | 13 | 336 | 352 | 368 | ShiftRight |
| 75 | Up Arrow | 85 | 4 | 15 | 337 | 353 | 369 | ArrowUp |
| 76 | Left Control | 41 | 5 | 0 | 149 | 165 | 181 | ControlLeft |
| 77 | Left Windows | 42 | 5 | 1 | 152 | 168 | 184 | MetaLeft |
| 78 | Left Alt | 43 | 5 | 2 | 153 | 169 | 185 | AltLeft |
| 79 | Space | 45 | 5 | 6 | 155 | 171 | 187 | Space |
| 80 | Right Alt | 86 | 5 | 10 | 338 | 354 | 370 | AltRight |
| 81 | Right Fn | 87 | 5 | 11 | 339 | 355 | 371 | Fn |
| 82 | Menu | 88 | 5 | 12 | 340 | 356 | 372 | ContextMenu |
| 83 | Right Control | 89 | 5 | 13 | 341 | 357 | 373 | ControlRight |
| 84 | Left Arrow | 90 | 5 | 14 | 344 | 360 | 376 | ArrowLeft |
| 85 | Down Arrow | 91 | 5 | 15 | 345 | 361 | 377 | ArrowDown |
| 86 | Right Arrow | 92 | 5 | 16 | 346 | 362 | 378 | ArrowRight |

### 8.8 Origins 2 65 (HP) : emplacements

| Emplacement matériel | Touche / LED | Code W3C |
|---|---|---|
| 0 | Escape | Escape |
| 1 | 1 | Digit1 |
| 2 | 2 | Digit2 |
| 3 | 3 | Digit3 |
| 4 | 4 | Digit4 |
| 5 | 5 | Digit5 |
| 6 | 6 | Digit6 |
| 7 | 7 | Digit7 |
| 8 | 8 | Digit8 |
| 9 | 9 | Digit9 |
| 10 | 0 | Digit0 |
| 11 | - | Minus |
| 12 | = | Equal |
| 13 | Backspace | Backspace |
| 14 | Delete | Delete |
| 15 | Tab | Tab |
| 16 | Q | KeyQ |
| 17 | W | KeyW |
| 18 | E | KeyE |
| 19 | R | KeyR |
| 20 | T | KeyT |
| 21 | Y | KeyY |
| 22 | U | KeyU |
| 23 | I | KeyI |
| 24 | O | KeyO |
| 25 | P | KeyP |
| 26 | [ | BracketLeft |
| 27 | ] | BracketRight |
| 28 | (vide) | — |
| 29 | Home | Home |
| 30 | Caps Lock | CapsLock |
| 31 | A | KeyA |
| 32 | S | KeyS |
| 33 | D | KeyD |
| 34 | F | KeyF |
| 35 | G | KeyG |
| 36 | H | KeyH |
| 37 | J | KeyJ |
| 38 | K | KeyK |
| 39 | L | KeyL |
| 40 | ; | Semicolon |
| 41 | ' | Quote |
| 42 | # | Backslash |
| 43 | Enter (ISO) | Enter |
| 44 | Page Up | PageUp |
| 45 | Left Shift | ShiftLeft |
| 46 | Z | KeyZ |
| 47 | X | KeyX |
| 48 | C | KeyC |
| 49 | V | KeyV |
| 50 | B | KeyB |
| 51 | N | KeyN |
| 52 | M | KeyM |
| 53 | , | Comma |
| 54 | . | Period |
| 55 | / | Slash |
| 56 | Right Shift | ShiftRight |
| 57 | Up Arrow | ArrowUp |
| 58 | (vide) | — |
| 59 | Page Down | PageDown |
| 60 | Left Control | ControlLeft |
| 61 | \ (ISO) | IntlBackslash |
| 62 | Left Windows | MetaLeft |
| 63 | Left Alt | AltLeft |
| 64 | (vide) | — |
| 65 | Left Space | Space |
| 66 | Space | Space |
| 67 | Right Space | Space |
| 68 | (vide) | — |
| 69 | Right Alt | AltRight |
| 70 | Right Fn | Fn |
| 71 | Left Arrow | ArrowLeft |
| 72 | Down Arrow | ArrowDown |
| 73 | Right Arrow | ArrowRight |
