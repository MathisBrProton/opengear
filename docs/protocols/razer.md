# Razer — fiche protocole

Source analysée : OpenRGB f899bc8, dossier `Controllers/RazerController` (pilote générique,
contrôleur ARGB, casques Kraken / Kraken V3 / Kraken V4, AIO Hanbo). Faits rédigés pour
l'interopérabilité ; aucun code repris. Tout ce qui est marqué **(incertain)** n'est pas établi
par le source.

## 1. Vue d'ensemble

OpenRGB distingue six familles de messages chez Razer :

| Famille | Appareils | Transport | Rapport |
|---|---|---|---|
| **Générique « 90 octets »** | claviers, portables Blade, souris, tapis, pavés, Tiamat, accessoires (docks, supports, Base Station, Core, Chroma HDK, Seiren Emote, Leviathan V2, Nommo…) | HID **feature report** (set/get) | 90 octets de charge utile, ID de rapport 0x00 (0x07 pour les Leviathan V2 / V2 X) |
| **ARGB** (Chroma Addressable RGB Controller 1532:0F1F) | 6 canaux ARGB | commandes génériques sur l'interface 0 + trames couleur en feature report de 320 octets sur l'interface 1 | voir §6 |
| **Kraken (1re génération)** | Kraken 7.1, 7.1 Chroma, 7.1 V2, Ultimate, Kitty Black Edition V2 | HID **output report** (écriture) + input report (lecture) | 37 octets, ID 0x04 (réponses ID 0x05) |
| **Kraken V3** | Kraken V3, V3 X, V3 HyperSense, Kitty V2 Pro | HID output report | 15 octets, ID 0x40 |
| **Kraken V4** | Kraken V4 (filaire / sans fil), Kitty V3 Pro | HID output report | 64 octets, ID 0x02 |
| **Hanbo** | AIO Hanbo Chroma (1532:0F35) | HID output report | 64 octets (ID 0x00 non transmis) |

VID : **0x1532** pour tout (0x068E est défini comme VID Bluetooth, mais les entrées Bluetooth sont
désactivées dans le détecteur).

Exclusion mutuelle : sous Windows, OpenRGB ouvre/crée le mutex nommé
`Global\RazerLinkReadWriteGuardMutex` (descripteur de sécurité à DACL nulle, non hérité) et le
détient pendant chaque envoi de feature report (pilote générique et ARGB). Si l'attente renvoie
« abandonné », il est relâché puis réacquis. Il s'agit de cohabiter avec le logiciel Razer ; à
reproduire si OpenGear peut tourner en même temps que Synapse.

## 2. Protocole générique « 90 octets »

### 2.1 Transport

- Écriture : `SetFeature` sur l'interface/usage indiqué dans le tableau des appareils (§9).
  Dans le tampon hidapi, l'octet 0 est l'ID de rapport (0x00 sauf Leviathan V2 / V2 X : 0x07),
  suivi des 90 octets ci-dessous.
- Lecture : `GetFeature` avec le même ID et la même longueur ; la réponse a la même structure.
- Tous les envois passent par le même chemin (aucune écriture en output report).

### 2.2 Format du rapport (offsets dans les 90 octets, hors ID)

| Offset | Taille | Champ | Valeur à l'envoi |
|---|---|---|---|
| 0 | 1 | statut | 0x00 |
| 1 | 1 | identifiant de transaction | propre au modèle : 0x1F, 0x3F, 0x9F, 0xFF… (colonne « TxID » du §9). Bits 0–2 « device », bits 3–7 « id » ; OpenRGB traite l'octet en bloc |
| 2–3 | 2 | paquets restants | 0x0000 |
| 4 | 1 | type de protocole | 0x00 |
| 5 | 1 | taille des données | nombre d'octets d'arguments significatifs (voir chaque commande) |
| 6 | 1 | classe de commande | |
| 7 | 1 | identifiant de commande | bit 7 = 1 pour une lecture (0x80+) |
| 8–87 | 80 | arguments | complétés par des zéros |
| 88 | 1 | somme de contrôle | voir ci-dessous |
| 89 | 1 | réservé | 0x00 |

**Somme de contrôle** : OU exclusif de tous les octets des offsets 2 à 87 inclus (paquets restants,
type, taille, classe, commande, 80 arguments). Le statut et la transaction n'y entrent pas.

**Statut dans les réponses** : 0x00 nouveau, 0x01 occupé, 0x02 succès, 0x03 échec,
0x04 délai dépassé, 0x05 non supporté.

### 2.3 Lecture fiable d'une réponse

Procédure d'OpenRGB pour les requêtes qui attendent une valeur :

1. attendre 2 ms, envoyer la requête ;
2. jusqu'à 25 fois : attendre 2 ms, lire le feature report ; accepter la réponse si transaction,
   classe et commande sont identiques à la requête **et** si le statut n'est ni 0x00 ni 0x01 ;
3. si rien n'est accepté, renvoyer la requête (3 envois au total).

Pour un appareil **relayé par un récepteur** (souris derrière un Mouse Dock Pro ou un dongle
HyperSpeed), chaque envoi est suivi d'une attente active : jusqu'à 50 lectures espacées de 2 ms,
jusqu'à obtenir statut ≠ 0x00/0x01 pour la même transaction/classe/commande. Sans cela, le récepteur
répond « occupé » et **perd** les commandes envoyées entre-temps.

Quelques lectures simples (mode, disposition clavier) se contentent d'un envoi, d'une pause
(1 à 5 ms) et d'une seule lecture.

### 2.4 Commandes d'information (classe 0x00)

| Commande | Taille | Arguments envoyés | Réponse |
|---|---|---|---|
| 0x81 version firmware | 0x02 | — | arg0 = majeur, arg1 = mineur (« vX.Y ») |
| 0x82 numéro de série | 0x16 | — | arg0..arg21 : 22 caractères ASCII (remplacer < 0x1E ou > 0x7E par des espaces) |
| 0x84 mode de l'appareil | 0x02 | — | arg0 = mode (0x00 matériel, 0x03 logiciel) |
| 0x86 infos clavier | 0x00 | — | arg0 = disposition, arg1 = variante (0x00 noir, 0x80 « Quartz », 0x82 « Mercury ») |
| 0xBF connexions sans fil | 0x50 | — | voir §2.8 |
| 0x04 (écriture) mode de l'appareil | 0x02 | arg0 = mode (0x00 matériel, 0x03 logiciel/pilote), arg1 = 0x00 | — |
| 0x44 (écriture) détection au démarrage ARGB | 0x01 | arg0 = 0/1 | défini, jamais utilisé par OpenRGB |

Codes de disposition (0x86, arg0) et normes qu'OpenRGB en déduit : 1 US (ANSI), 2 grec (ISO),
3 allemand (QWERTZ), 4 français (AZERTY), 5 russe (ANSI), 6 Royaume-Uni (ISO), 7 nordique (ISO),
8 chinois traditionnel (ANSI), 9 coréen (ISO), 10 turc (ANSI), 11 thaï (ANSI), 12 japonais (JIS),
13 portugais Brésil (ISO), 14 espagnol Amérique latine (ISO), 15 suisse (ISO), 16 espagnol Europe
(ISO), 17 italien (ISO), 18 portugais Portugal (ISO), 19 hébreu (ISO), 20 arabe (ANSI). Seuls US,
allemand, français, Royaume-Uni et nordique sont marqués confirmés dans le source ; les autres
correspondances norme ↔ code sont **(incertain)**.

### 2.5 Deux « dialectes » : matrice standard et matrice étendue

Chaque modèle a un type de matrice (colonne « Matrice » du §9) qui choisit le jeu de commandes :

| Type | Commandes d'éclairage | Remarque |
|---|---|---|
| `standard` | classe 0x03 | anciens claviers/portables Blade, Core, Naga Hex V2 |
| `extended` | classe 0x0F | la grande majorité depuis ~2018 |
| `linear` | effets classe 0x03, trame via 0x03/0x0C | une seule rangée (DeathStalker Chroma, Firefly, Mamba 2015/TE, Naga Chroma, Diamondback, Mug Holder) |
| `extended_argb` | classe 0x0F + trames sur l'interface ARGB | contrôleur ARGB seulement |
| `custom` | cas particuliers par PID (§2.9) | DeathAdder Chroma, Naga Epic Chroma, Mamba 2012, Tartarus Chroma, Orbweaver Chroma |
| `none` | pas de protocole générique | casques Kraken (protocoles dédiés) |

L'**identifiant de LED** (colonne « LED ») est passé dans les commandes d'effet et de luminosité :

| Valeur | Nom | Valeur | Nom |
|---|---|---|---|
| 0x00 | zéro / « toutes zones » | 0x0C | profil rouge |
| 0x01 | molette | 0x0D | profil vert |
| 0x03 | batterie | 0x0E | profil bleu |
| 0x04 | logo | 0x10 | côté droit |
| 0x05 | rétroéclairage | 0x11 | côté gauche |
| 0x07 | macro | 0x1A–0x1F | canaux ARGB 1 à 6 |
| 0x08 | mode jeu | | |

**Octet de stockage** (premier argument des commandes d'effet/luminosité) : 0x00 = volatil,
0x01 = sauvegarde dans l'appareil. **OpenRGB envoie toujours 0x00** ; il n'existe aucune commande de
sauvegarde explicite dans ce pilote. Utiliser 0x01 n'a pas été validé par le source **(incertain)**.

### 2.6 Commandes d'éclairage — matrice étendue (classe 0x0F)

Arguments notés a0, a1… (offset 8 + n dans le rapport).

| Action | Cmd | Taille | Arguments |
|---|---|---|---|
| Ligne de trame « custom frame » | 0x03 | 5 + 3·n | a0 = 0, a1 = 0, a2 = ligne, a3 = colonne de début, a4 = colonne de fin, puis n triplets R,G,B (n = fin − début + 1) |
| Effet (forme générale) | 0x02 | 6 + 3·k | a0 = stockage, a1 = LED, a2 = effet, a3 = drapeaux/sous-mode, a4 = vitesse/durée, a5 = k (nombre de couleurs), puis k triplets R,G,B |
| Luminosité | 0x04 | 0x03 | a0 = stockage, a1 = LED, a2 = luminosité (0–255 ; 0–100 pour le DeathAdder Essential V2) |
| Taille des zones ARGB | 0x08 | 0x0D | voir §6 |
| Lister les régions | 0x80 | 0x50 | réponse : enregistrements de 5 octets dont le 1er est l'identifiant de LED de la région ; 0x00 termine la liste |
| Lister les effets d'une région | 0x81 | 0x50 | a0 = LED ; réponse : a1..a(taille−1) = identifiants d'effets supportés |

Identifiants d'effet (a2) : 0x00 éteint, 0x01 statique, 0x02 respiration, 0x03 cycle spectral,
0x04 vague, 0x05 réactif, 0x06 ondulation (ripple), 0x07 « starlight », 0x08 trame personnalisée,
0x09 feu.

Messages exacts utilisés par OpenRGB (stockage = 0x00) :

| Mode | Taille | a1 | a2 | a3 | a4 | a5 | couleurs |
|---|---|---|---|---|---|---|---|
| Éteint | 0x06 | LED | 0x00 | 0 | 0 | 0 | — |
| Statique | 0x09 | LED | 0x01 | 0 | 0 | 1 | R,G,B |
| Respiration aléatoire | 0x06 | LED | 0x02 | 0 | 0 | 0 | — |
| Respiration 1 couleur | 0x09 | LED | 0x02 | 0x01 | 0 | 1 | R,G,B |
| Respiration 2 couleurs | 0x0C | LED | 0x02 | 0x02 | 0 | 2 | R1,G1,B1,R2,G2,B2 |
| Cycle spectral | 0x06 | LED | 0x03 | 0 | 0 | 0 | — |
| Vague | 0x06 | LED | 0x04 | direction (1 = vers la droite, 2 = vers la gauche) | 0x28 | 0 | — |
| Réactif | 0x09 | LED | 0x05 | 0 | durée 1 court / 2 moyen / 3 long | 1 | R,G,B |
| Starlight aléatoire | 0x06 | LED | 0x07 | 0x00 | durée 1–3 | 0 | — |
| Starlight 1 couleur | 0x09 | LED | 0x07 | 0x01 | durée 1–3 | 1 | R,G,B |
| Starlight 2 couleurs | 0x0C | LED | 0x07 | 0x02 | durée 1–3 | 2 | 2 triplets |
| Ondulation | 0x09 | LED | 0x06 | 0 | 0x1E | 1 | R,G,B |
| Feu | 0x09 | LED | 0x09 | 0 | 0 | 1 | 0xFF,0x00,0x00 (le firmware refuse le feu sans couleur ; le logiciel Razer envoie du rouge) |
| Activer la trame | 0x0C | **0x00** | 0x08 | 0 | 0 | 0 | — (a0 = 0, a1 = 0 : LED zéro, quelle que soit la LED du modèle) |

« LED » = identifiant de LED du modèle (colonne « LED » du §9).

### 2.7 Commandes d'éclairage — matrice standard / linéaire (classe 0x03)

| Action | Cmd | Taille | Arguments |
|---|---|---|---|
| Ligne de trame (standard) | 0x0B | 4 + 3·n | a0 = 0xFF, a1 = ligne, a2 = début, a3 = fin, puis n triplets R,G,B |
| Rangée de trame (linéaire) | 0x0C | **0x32 fixe** | a0 = début, a1 = fin, puis triplets R,G,B |
| Luminosité | 0x03 | 0x03 | a0 = stockage, a1 = LED, a2 = luminosité 0–255 |
| Couleur d'une LED (ancien) | 0x01 | 0x05 | a0 = stockage, a1 = LED, a2..a4 = R,G,B |
| Effet d'une LED (ancien) | 0x02 | 0x03 | a0 = stockage, a1 = LED, a2 = effet borné à 5 (0 statique, 2 respiration, 4 spectre) |
| Effet de matrice | 0x0A | variable | a0 = effet, voir ci-dessous |

Effets 0x03/0x0A (pas d'octet de stockage ni de LED) :

| Mode | Taille | Arguments |
|---|---|---|
| Éteint | 0x01 | 0x00 |
| Vague | 0x02 | 0x01, direction (1 droite, 2 gauche) |
| Respiration 1 couleur | 0x08 | 0x03, 0x01, R, G, B |
| Respiration 2 couleurs | 0x08 | 0x03, 0x02, R1, G1, B1, R2, G2, B2 |
| Respiration aléatoire | 0x08 | 0x03, 0x03 |
| Cycle spectral | 0x01 | 0x04 |
| Activer la trame | 0x02 | 0x05, stockage (0x00) |
| Statique | 0x04 | 0x06, R, G, B |

Pas de réactif, starlight, ondulation ni feu dans ce dialecte.

Capacité d'un paquet : 80 octets d'arguments, soit au plus 25 LED par ligne (étendu : 75 octets de
couleur après 5 d'en-tête ; standard : 76 après 4). Les grilles les plus larges (Blade Pro 2016/2017 :
25 colonnes) tiennent donc en un paquet par ligne.

### 2.8 Récepteurs sans fil (commande 0x00/0xBF)

Réponse : a0 = nombre d'emplacements ; puis, pour chaque emplacement i, 3 octets à partir de
a(1 + 3i) : état de connexion (1 = connecté), PID sur 2 octets **gros-boutiste**
(0xFFFF = vide). Les appareils qui ne sont pas des récepteurs rejettent la commande (statut ≠ succès).

OpenRGB ouvre le **même chemin HID** que le récepteur pour chaque PID appairé connu (autre que le
récepteur lui-même) et lui parle avec l'**identifiant de transaction du périphérique appairé** (pris
dans sa ligne du §9) : le récepteur relaie alors la commande. Les appareils appairés sont listés
même s'ils dorment. Mode « attente de réponse » obligatoire (§2.3). Exemple : Mouse Dock Pro
(1532:00A4, TxID 0xFF).

### 2.9 Cas particuliers « custom »

| Modèle | Trame (mode direct) | Effets |
|---|---|---|
| Tartarus Chroma (0208) | 0x03/0x01 sur LED 0x05 avec la couleur unique | 0x03/0x01 puis 0x03/0x02 sur LED 0x05 (effet 0 statique, 2 respiration 1 couleur, 4 spectre) ; éteint = couleur noire + effet 0 |
| DeathAdder Chroma (0043) | 0x03/0x01 sur LED 0x01 (couleur 0) puis LED 0x04 (couleur 1) | idem sur les deux LED |
| Naga Epic Chroma (003E) | 0x03/0x01 sur LED 0x01 (couleur 0), pause 1 ms, LED 0x05 (couleur 1) | idem avec pauses de 1 ms entre chaque message |
| Mamba 2012 filaire/sans fil (0024/0025) | 0x03/0x01 sur LED 0x01 | idem sur LED 0x01 |
| Orbweaver Chroma (0207) | matrice standard 0x03/0x0B, mais présentée comme 4×5 : ligne logique r → ligne matérielle r+1, colonne c → c+1 ; pour la ligne logique 3, la 1re couleur va en colonne matérielle 1, la colonne 2 est laissée noire, les 4 suivantes vont en 3–6 (disposition physique décrite plus bas) | effets 0x03/0x0A comme une matrice standard ; luminosité 0x03/0x03 |

Disposition physique de l'Orbweaver (grille matérielle 5×7, `xx` = absent) :

| Ligne | c0 | c1 | c2 | c3 | c4 | c5 | c6 |
|---|---|---|---|---|---|---|---|
| 0 | xx | xx | xx | xx | xx | xx | xx |
| 1 | xx | 01 | 02 | 03 | 04 | 05 | xx |
| 2 | xx | 06 | 07 | 08 | 09 | 10 | xx |
| 3 | xx | 11 | 12 | 13 | 14 | 15 | xx |
| 4 | xx | 16 | xx | 17 | 18 | 19 | 20 |

Pour les modèles `custom`, la commande « activer la trame » est remplacée par l'effet 0 (statique)
sur chaque LED concernée ; pas de vague sauf Orbweaver.

## 3. Séquences — pilote générique

### (a) Passer en contrôle logiciel

- Aucun « mode pilote » n'est envoyé pour la plupart des appareils : il suffit d'envoyer une trame
  puis la commande « activer la trame ».
- Exceptions : Charging Pad Chroma (0F26), Chroma Mug Holder (0F07) et Firefly Hyperflux (0068)
  reçoivent **0x00/0x04 [0x03, 0x00]** (mode logiciel) à l'initialisation.
- À l'ouverture OpenRGB lit aussi : firmware (0x81), série (0x82), disposition/variante (0x86, y
  compris pour les non-claviers), puis, pour les matrices étendues, la liste des régions et des
  effets (0x0F/0x80 et 0x0F/0x81) pour décider quels modes proposer (§4).

### (b) Envoyer une trame complète

1. Pour chaque ligne r de 0 à lignes−1 : attendre ≈1 ms, envoyer la commande « ligne de trame » du
   dialecte avec début = 0, fin = colonnes−1 et les couleurs de la ligne (ordre **R, G, B**).
2. Attendre ≈1 ms, envoyer « activer la trame » (étendu : 0x0F/0x02 [0,0,0x08] ; standard/linéaire :
   0x03/0x0A [0x05, 0x00] ; ARGB : rien).
3. Envoyer la luminosité si besoin (le mode direct d'OpenRGB a un réglage de luminosité séparé,
   envoyé avec chaque changement de mode).

Les couleurs sont fournies dans l'ordre ligne-majeur de la grille `lignes × colonnes` du modèle :
l'index i correspond à (ligne = i / colonnes, colonne = i mod colonnes). Pour un appareil à une seule
ligne, les zones de la colonne « Zones » du §9 se suivent dans l'ordre indiqué (ex. Basilisk V3 :
colonne 0 logo, 1 molette, 2–10 bandeau). Pour les claviers, la correspondance touche ↔ (ligne,
colonne) est donnée au §10.

Envoyer une trame bascule l'appareil sur l'effet « trame personnalisée » : OpenRGB n'écrit donc des
couleurs qu'en mode direct.

### (c) Appliquer un effet matériel

Un seul message 0x0F/0x02 (étendu) ou 0x03/0x0A (standard) selon les tableaux §2.6 / §2.7, suivi
d'un message de luminosité (0x0F/0x04 ou 0x03/0x03 avec la LED du modèle ; 255 si le mode n'a pas de
luminosité). Pour l'ARGB, la luminosité est envoyée six fois, LED 0x1A à 0x1F.

### (d) Sauvegarde

Non implémentée par OpenRGB (octet de stockage toujours 0x00). L'état n'est pas persistant : au
redémarrage l'appareil reprend son profil interne.

### Maintien (keep-alive)

Blade 14 (2021) 0270 et Blade 14 (2022) 028C : l'éclairage s'éteint sinon. OpenRGB lance un fil qui,
toutes les secondes, envoie une lecture 0x00/0x84 (mode de l'appareil) dès que plus de 2,5 s se sont
écoulées depuis la dernière mise à jour. Aucun autre modèle n'en a besoin selon le source.

## 4. Modes proposés et paramètres

| Mode | Paramètres | Disponibilité |
|---|---|---|
| Direct | couleur par LED, luminosité 0–255 (0–100 DeathAdder Essential V2) | tous |
| Éteint | — | tous |
| Statique | 1 couleur, luminosité | tous |
| Respiration | 1 ou 2 couleurs, ou aléatoire ; luminosité | tous sauf Basilisk V3 / V3 35K / V3 Pro (toutes variantes) et Basilisk V4 Pro, sauf si le firmware liste l'effet 0x02 |
| Cycle spectral | luminosité | tous |
| Vague | direction gauche/droite ; vitesse fixe 0x28 | firmware listant 0x04, ou liste fixe de PID (claviers, Blade, nombreuses souris, accessoires) |
| Réactif | 1 couleur, durée 1–3 | firmware listant 0x05 **et** appareil avec entrée (clavier, souris, pavé) |
| Starlight | 1–2 couleurs ou aléatoire, durée 1–3 | firmware listant 0x07 |
| Ondulation | 1 couleur | firmware listant 0x06 et appareil avec entrée |
| Feu | — | firmware listant 0x09 |

Interrogation des effets (matrices étendues seulement) : si la LED du modèle ≠ 0x00, ne demander
que cette région ; sinon lister les régions (0x0F/0x80) puis demander les effets de chacune
(0x0F/0x81) et garder l'**intersection**. En cas d'échec, se rabattre sur les listes par PID. La
liste du firmware n'est pas exhaustive (le Basilisk V3 Pro 35K et le Mouse Dock Pro exécutent la
vague sans la lister) : elle ne sert qu'à **ajouter** des modes.

## 5. Particularités par modèle

- **Leviathan V2 / V2 X** : ID de rapport 0x07 au lieu de 0x00 (requête et réponse).
- **DeathAdder Essential V2** : luminosité maximale 100.
- **Blackwidow V4 / V4 Pro** (0287, 028D) : deux enregistrements de détection selon la version de
  firmware (< 1.5 : interface 3, page 0x01, usage 0x00 ; ≥ 1.5 : interface 3, page 0x0C, usage 0x01).
- **Nommo Chroma / Pro** : sous Windows l'interface 1 (page 0x01, usage 0x03) est obligatoire ;
  l'interface 0 n'est utilisée que sur Linux/macOS.
- **Grilles plus grandes que la somme des zones** : Blackwidow V4 (8×23 pour 6×23 + 9 + 9 LED),
  V4 Pro (8×23 pour 181 LED), Basilisk V4 Pro (1×13 pour 7 LED). OpenRGB envoie quand même la grille
  entière ; les cellules sans LED déclarée doivent être mises à zéro. L'emplacement exact des
  bandeaux sous la rangée 5 se déduit seulement de l'ordre linéaire (V4 : ligne 6 colonnes 0–8 =
  bandeau gauche, 9–17 = bandeau droit ; V4 Pro : ligne 6 colonnes 18–22 = « Void », ligne 7
  colonnes 0–19 = repose-poignet) **(incertain, non vérifié sur matériel)**.
- **Seiren Emote** : la zone est une matrice 8×8 mais la grille matérielle déclarée est 4×16 (même
  nombre de LED) ; l'ordre linéaire est conservé.
- **PID partagé 0x0F21** : utilisé à la fois par le Kraken Kitty Black Edition (détecteur générique,
  interface 1, page 0x01, usage 0x03) et par le Thunderbolt 4 Dock Chroma (page 0x0C, usage 0x01).
  La table d'OpenRGB n'a qu'une entrée (dock, 1×12) : le casque est donc décrit avec la géométrie du
  dock. Il faut distinguer les deux par l'interface/usage **(ambigu)**.
- **Kraken 7.1 « Classic »** (0501, 0506) : détectés par le pilote Kraken mais absents de la table
  d'appareils ; OpenRGB retombe alors sur la première entrée de la table (incohérent). À traiter
  comme un casque à une LED avec les adresses « classiques » du §7 **(incertain)**.
- **Entrées non détectées** : Blackwidow V3 Pro (Bluetooth) 025B, Basilisk V3 Pro (Bluetooth) 00AC,
  Basilisk V3 Pro 35K Phantom Green (Bluetooth) 00D8 : détecteurs en commentaire.

## 6. Contrôleur ARGB (1532:0F1F)

Deux interfaces HID du même appareil, appariées par chemin :

- **Interface 0** : rapports génériques de 90 octets (§2), matrice `extended_argb`, TxID 0x3F, LED
  0x05 pour les effets.
- **Interface 1** : trames couleur, feature report de **320 octets** (+ octet d'ID 0x00 en tête
  dans le tampon hidapi).

Format de la trame ARGB (offsets dans les 320 octets) :

| Offset | Champ |
|---|---|
| 0 | 0x04 pour les canaux d'index 0 à 4, **0x84** pour l'index 5 |
| 1 | index du canal (0–5) |
| 2 | index du canal (répété) |
| 3 | 0x00 |
| 4 | index de la dernière LED (nombre de LED − 1 ; OpenRGB envoie toujours 79) |
| 5–319 | triplets R,G,B (315 octets, 105 LED max ; OpenRGB en utilise 80) |

Séquence de trame : un rapport par canal (6), sans commande « activer la trame ». Les couleurs d'un
canal sont prises dans un tampon de 6 × 80 LED (index = 80·canal + LED).

Taille des zones (commande générique 0x0F/0x08, taille 0x0D, interface 0) :
a0 = 0x06, puis pour chaque canal k = 1..6 deux octets : (0x19 si taille > 0, sinon k) et la taille
(0–80). Envoyé quand l'utilisateur redimensionne un canal. Effets et luminosité : comme une matrice
étendue, la luminosité étant envoyée pour les LED 0x1A à 0x1F. Modes proposés : direct, éteint,
statique, respiration (1–2 couleurs/aléatoire), spectre, et vague/réactif selon le firmware.

## 7. Kraken 1re génération

Transport : **output report** de 37 octets (écriture) ; lecture par input report (ID 0x05).

| Offset | Champ |
|---|---|
| 0 | ID 0x04 |
| 1 | destination : 0x20 lecture, 0x40 écriture |
| 2 | longueur (nombre d'octets de données) |
| 3 | adresse, octet de poids fort |
| 4 | adresse, octet de poids faible |
| 5–36 | données (32 octets) |

Adresses selon le modèle :

| Modèle | Mode LED | Couleur « custom » | Couleurs de respiration 1 / 2 / 3 |
|---|---|---|---|
| Kraken 7.1 V2 (0510), Kitty Black V2 (0560), Ultimate (0527) | 0x172D | 0x1189 | 0x1741 / 0x1745 / 0x174D |
| Kraken 7.1 (0501, 0506), Kraken 7.1 Chroma (0504) | 0x1008 | 0x1189 | 0x15DE (une seule) |

Octet d'effet écrit à l'adresse « mode LED » (longueur 1) : bit 0 allumé/statique, bit 1
respiration 1 couleur, bit 2 cycle spectral, bit 3 synchro, bit 4 respiration 2 couleurs, bit 5
respiration 3 couleurs.

Séquences :

- **Éteint** : écrire 0x00 en « mode LED ».
- **Statique** : écrire R,G,B (longueur 3) à l'adresse de respiration 1 (sauf pour 0501/0506 où
  seule l'écriture du mode est faite), puis 0x01 en « mode LED ».
- **Direct** (une LED) : écrire R,G,B à l'adresse « custom » 0x1189 (sauf 0501/0506), puis 0x01 en
  « mode LED ».
- **Respiration 1 couleur** : R,G,B à l'adresse 1, puis 0x0B (bits 0, 1, 3).
- **Respiration 2 couleurs** : couleur 1 à l'adresse « 2 », couleur 2 à adresse 2 + 4, puis 0x19
  (bits 0, 3, 4).
- **Respiration 3 couleurs** : couleurs à adresse 2, +4, +8, puis 0x29 (bits 0, 3, 5).
- **Spectre** : 0x05 (bits 0, 2).
- Firmware : lecture (destination 0x20, longueur 2, adresse 0x0030) ; réponse ID 0x05,
  version = données[1].données[2] (offsets 2 et 3 de la réponse). Série : longueur 0x16, adresse
  0x7F00, 22 caractères à partir de l'offset 1 de la réponse.
- Le Kraken 7.1 Chroma ne fait que la respiration 1 couleur ; les autres jusqu'à 3.
- Pas de sauvegarde, pas de maintien.

## 8. Kraken V3 et V4, Hanbo

### Kraken V3 (0533, 0537, 0549, 0554)

Output report de 15 octets : [0] = 0x40, [1] = commande, [2..14] = arguments (b0..b12).

| Action | Cmd | Arguments |
|---|---|---|
| Mode direct | 0x01 | b1 = 0x0F, b2 = 0x08 |
| Mode vague | 0x01 | b1 = 0x0F, b2 = 0x03 |
| Respiration | 0x01 | b0 = 0x01, b2 = k, b3 = k (k = 1 ou 2 couleurs), puis k triplets R,G,B à partir de b4 |
| Luminosité | 0x02 | b2 = 0–255 |
| Couleurs directes | 0x03 | à partir de b1 : triplets R,G,B, une LED par triplet (1 LED ; 4 pour le Kitty V2 Pro : chat gauche, chat droit, logo gauche, logo droit) |

Séquence direct : mode direct, luminosité, puis couleurs à chaque mise à jour. Les captures citées
en commentaire dans le source montrent tantôt b0 = 0x00, tantôt b0 = 0x01 pour le changement de
mode : la signification de b0 est **(incertain)**. Respiration proposée uniquement pour le V3
HyperSense. Firmware/série : mêmes requêtes que le Kraken 1re génération (ID 0x04).

### Kraken V4 / Kitty V3 Pro (056B, 056C, 0587, 0588)

Interface 5, page 0xFF14, usage 0x01. Output report de 64 octets, structure proche du générique :

| Offset | Champ |
|---|---|
| 0 | ID 0x02 |
| 1 | statut (0) |
| 2 | transaction 0x60 |
| 3–4 | paquets restants (0) |
| 5 | type de protocole (0) |
| 6 | taille des données |
| 7 | classe |
| 8 | commande |
| 9 | drapeau sans fil : 0x80 pour les PID sans fil (056C, 0588), 0x00 sinon |
| 10–61 | arguments (52) |
| 62 | somme de contrôle |
| 63 | réservé |

OpenRGB **n'écrit pas** la somme de contrôle à l'envoi (octet laissé à 0) ; elle serait le XOR des
octets 0 à 60 d'après la fonction présente dans le source. À vérifier sur matériel **(incertain)**.

| Action | Classe/Cmd | Taille | Arguments |
|---|---|---|---|
| Couleurs directes | 0x0F/0x03 | 5 + 3·n | a2 = 0, a3 = 0, a4 = n − 1, triplets R,G,B à partir de a5 (n = 9 ou 10) |
| Mode vague | 0x00/0x00 | 0x05 | a0 = 0xC0, a2 = 0x01, a3 = 0x04 |
| Luminosité | 0x00/0x00 | 0x05 | a0 = 0xC1, a2 = 0x01, a3 = 0–255 |
| Lire firmware | 0x00/0x00 | 0x04 | a0 = 0x02 ; réponse : version a3.a4.a5.a6 |
| Lire série | 0x00/0x00 | 0x04 | a0 = 0x00 ; réponse : 15 caractères à partir de a3 |

Lectures : pause 2 ms, envoi, pause 5 ms, input report. Modes : direct (couleurs à chaque mise à
jour) et vague (+ luminosité).

### Hanbo Chroma (0F35)

Interface 0, page 0xFF00, usage 0x01. Output report de 65 octets dont l'octet 0 (ID 0x00) n'est pas
transmis ; ci-dessous `h` = octet 1, puis arguments à partir de l'octet 2. Lecture avec délai court
(2 ms).

| Message | h | Arguments |
|---|---|---|
| Lire firmware/série | 0x01 | 0x01 ; deux lectures, la 2e commence par 0x02 : série = 15 octets à partir de l'offset 2, version = octets 29–30 (majeur ; mineur = quartet haut ; correctif = quartet bas) |
| Préparation | 0x82 | 0x01 |
| Réglage | 0x80 | variable (voir séquences) |
| Couleurs pompe | 0x32 | 0x01, 0x07, 0, 0, 0, 0, 0x00 puis 16 triplets **G, R, B** |
| Couleurs ventilateur k (1–3) | 0x40 | 0x01, 0x07, 0, 0, 0, 0, k puis 18 triplets **G, R, B** |
| Luminosité | 0x70 | 0x01, 0x00 (pompe) ou 0x01 (ventilateurs), valeur 0–100 (défini, non utilisé par OpenRGB) |

Séquence « mode direct » (chaque message suivi d'une lecture) : [0x82 0x01] ; [0x80 0x01 0x00 0x01] ;
[0x80 0x01 0x01 0x01] ; [0x82 0x01]. Envoyée à l'ouverture et en entrant en mode direct, puis les
quatre messages de couleur (pompe, ventilateurs 1, 2, 3) à chaque trame, 2 ms après chacun.
Séquence « spectre » : [0x82 0x01] ; [0x80 0x01] ; [0x80 0x01 0x01]. Le mode « éteint » d'OpenRGB
renvoie simplement la séquence directe : l'extinction effective n'est pas garantie **(incertain)**.

## 9. Appareils (pilote, géométrie, identifiants)

Colonnes : **Pilote** std = générique 90 octets, argb, kraken, krakenv3, krakenv4, hanbo ;
**Matrice** = dialecte (§2.5) ; **TxID** = octet de transaction ; **LED** = identifiant de LED des
effets ; **Grille** = lignes × colonnes envoyées dans la trame ; **Zones** = découpage logique
(nom, type, dimensions) ; **HID** = critères de sélection de l'interface (`*` = indifférent).
Les modèles « portables » sont les claviers des Razer Blade/Book.

| Nom | VID:PID | Type | Pilote | Matrice | TxID | LED | Grille | Zones | HID |
|---|---|---|---|---|---|---|---|---|---|
| Razer BlackWidow 2019 | 1532:0241 | keyboard | std | extended | 0x1F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow Chroma | 1532:0203 | keyboard | std | standard | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Blackwidow Chroma Overwatch | 1532:0211 | keyboard | std | standard | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow Chroma Tournament Edition | 1532:0209 | keyboard | std | standard | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow Chroma V2 | 1532:0221 | keyboard | std | standard | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow Elite | 1532:0228 | keyboard | std | extended | 0x1F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Blackwidow V3 | 1532:024E | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x03 page=0x0C usage=0x01 |
| Razer BlackWidow V3 Pro (Wired) | 1532:025A | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow V3 Pro (Wireless) | 1532:025C | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x03 page=0x0C usage=0x01 |
| Razer BlackWidow V3 Pro (Bluetooth) | 1532:025B | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | detecteur desactive (ligne en commentaire) if=* page=0x01 usage=0x00 |
| Razer BlackWidow V3 TKL | 1532:0A24 | keyboard | std | extended | 0x3F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow V3 Mini (Wired) | 1532:0258 | keyboard | std | extended | 0x1F | 0x05 | 5x16 | Keyboard (matrix 5x16) | if=0x03 page=0x0C usage=0x01 |
| Razer BlackWidow V3 Mini (Wireless) | 1532:0271 | keyboard | std | extended | 0x9F | 0x05 | 5x16 | Keyboard (matrix 5x16) | if=0x03 page=0x0C usage=0x01 |
| Razer Blackwidow V4 | 1532:0287 | keyboard | std | extended | 0x1F | 0x05 | 8x23 | Keyboard (matrix 6x23); Underglow Left (linear 1x9); Underglow Right (linear 1x9) | if=0x03 page=0x01 usage=0x00 (firmware < 1.5)<br>if=0x03 page=0x0C usage=0x01 (firmware >= 1.5) |
| Razer Blackwidow V4 Pro | 1532:028D | keyboard | std | extended | 0x1F | 0x05 | 8x23 | Keyboard (matrix 6x23); Underglow Left (linear 1x9); Underglow Right (linear 1x9); Void (linear 1x5); Underglow Wrist Rest (linear 1x20) | if=0x03 page=0x01 usage=0x00 (firmware < 1.5)<br>if=0x03 page=0x0C usage=0x01 (firmware >= 1.5) |
| Razer Blackwidow V4 Pro 75% (Wired) | 1532:02B3 | keyboard | std | extended | 0x1F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x03 page=0x0C usage=0x01 |
| Razer Blackwidow V4 Pro 75% (Wireless) | 1532:02B4 | keyboard | std | extended | 0x1F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x02 page=0x01 usage=0x02 |
| Razer Blackwidow V4 75% (Wired) | 1532:02A5 | keyboard | std | extended | 0x1F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x03 page=0x01 usage=0x00 |
| Razer Blackwidow V4 X | 1532:0293 | keyboard | std | extended | 0x1F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Blackwidow V4 TKL (Wired) | 1532:02D7 | keyboard | std | extended | 0x1F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x03 page=0x0C usage=0x01 |
| Razer Blackwidow V4 TKL (Wireless) | 1532:02D5 | keyboard | std | extended | 0x9F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x02 page=0x01 usage=0x02 |
| Razer Blackwidow V4 Low Profile TKL (Wired) | 1532:02D4 | keyboard | std | extended | 0x1F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x03 page=0x0C usage=0x01 |
| Razer Blackwidow V4 Low Profile TKL (Wireless) | 1532:02D2 | keyboard | std | extended | 0x9F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow X Chroma | 1532:0216 | keyboard | std | standard | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer BlackWidow X Chroma Tournament Edition | 1532:021A | keyboard | std | standard | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Cynosa Chroma | 1532:022A | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Cynosa v2 | 1532:025E | keyboard | std | extended | 0x1F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Cynosa Lite | 1532:023F | keyboard | std | extended | 0x3F | 0x05 | 1x1 | Keyboard (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer DeathStalker Chroma | 1532:0204 | keyboard | std | linear | 0x3F | 0x05 | 1x12 | Keyboard (linear 1x12) | if=0x02 page=0x01 usage=0x02 |
| Razer Deathstalker V2 | 1532:0295 | keyboard | std | extended | 0x1F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x03 page=0x0C usage=0x01 |
| Razer DeathStalker V2 Pro TKL (Wired) | 1532:0298 | keyboard | std | extended | 0x1F | 0x00 | 6x17 | Keyboard (matrix 6x17) | if=0x03 page=0x0C usage=0x01 |
| Razer DeathStalker V2 Pro TKL (Wireless) | 1532:0296 | keyboard | std | extended | 0x9F | 0x00 | 6x17 | Keyboard (matrix 6x17) | if=0x02 page=0x01 usage=0x02 |
| Razer DeathStalker V2 Pro (Wired) | 1532:0292 | keyboard | std | extended | 0x9F | 0x00 | 6x22 | Keyboard (matrix 6x22) | if=0x03 page=0x0C usage=0x01 |
| Razer DeathStalker V2 Pro (Wireless) | 1532:0290 | keyboard | std | extended | 0x9F | 0x00 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Huntsman | 1532:0227 | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Huntsman Elite | 1532:0226 | keyboard | std | extended | 0x3F | 0x05 | 9x22 | Keyboard (matrix 6x22); Underglow (matrix 3x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Huntsman Mini | 1532:0257 | keyboard | std | extended | 0x3F | 0x05 | 5x15 | Keyboard (matrix 5x15) | if=0x02 page=0x01 usage=0x02 |
| Razer Huntsman Mini Analog | 1532:0282 | keyboard | std | extended | 0x1F | 0x05 | 5x15 | Keyboard (matrix 5x15) | if=0x03 page=0x0C usage=0x01 |
| Razer Huntsman Tournament Edition | 1532:0243 | keyboard | std | extended | 0x3F | 0x05 | 6x18 | Keyboard (matrix 6x18) | if=0x02 page=0x01 usage=0x02 |
| Razer Huntsman V2 | 1532:026C | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x03 page=0x0C usage=0x01 |
| Razer Huntsman V2 Analog | 1532:0266 | keyboard | std | extended | 0x3F | 0x05 | 9x22 | Keyboard (matrix 6x22); Underglow (matrix 3x22) | if=0x03 page=0x0C usage=0x01 |
| Razer Huntsman V2 TKL | 1532:026B | keyboard | std | extended | 0x3F | 0x05 | 6x17 | Keyboard (matrix 6x17) | if=0x03 page=0x0C usage=0x01 |
| Razer Huntsman V3 Pro | 1532:02A6 | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x03 page=0x0C usage=0x01 |
| Razer Huntsman V3 Pro TKL White | 1532:02A7 | keyboard | std | extended | 0x3F | 0x05 | 6x19 | Keyboard (matrix 6x19) | if=0x03 page=0x0C usage=0x01 |
| Razer Huntsman V3 HE Magnetic Mini | 1532:02E4 | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x03 page=0x0C usage=0x01 |
| Razer Ornata Chroma | 1532:021E | keyboard | std | extended | 0x3F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Ornata Chroma V2 | 1532:025D | keyboard | std | extended | 0x1F | 0x05 | 6x22 | Keyboard (matrix 6x22) | if=0x02 page=0x01 usage=0x02 |
| Razer Ornata V3 | 1532:028F | keyboard | std | extended | 0x1F | 0x05 | 1x10 | Keyboard (linear 1x10) | if=0x02 page=0x01 usage=0x02 |
| Razer Ornata V3 rev2 | 1532:02A1 | keyboard | std | extended | 0x1F | 0x05 | 1x10 | Keyboard (linear 1x10) | if=0x02 page=0x01 usage=0x02 |
| Razer Ornata V3 TKL | 1532:02A3 | keyboard | std | extended | 0x1F | 0x05 | 1x8 | Keyboard (single 1x8) | if=0x02 page=0x01 usage=0x02 |
| Razer Ornata V3 X | 1532:0294 | keyboard | std | extended | 0x1F | 0x05 | 1x1 | Keyboard (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer Ornata V3 X Rev2 | 1532:02A2 | keyboard | std | extended | 0x1F | 0x05 | 1x1 | Keyboard (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade (2016) | 1532:020F | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade (Late 2016) | 1532:0224 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2018 Advanced) | 1532:0233 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2018 Base) | 1532:023B | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2018 Mercury) | 1532:0240 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2019 Advanced) | 1532:023A | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2019 Base) | 1532:0246 | laptop | std | standard | 0x3F | 0x05 | 1x1 | Keyboard (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2019 Mercury) | 1532:0245 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2019 Studio) | 1532:024D | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2020 Advanced) | 1532:0253 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2020 Base) | 1532:0255 | laptop | std | standard | 0x3F | 0x05 | 1x16 | Keyboard (linear 1x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade (Late 2020) | 1532:0268 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2021 Advanced) | 1532:026D | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2021 Base) | 1532:026F | laptop | std | standard | 0x3F | 0x05 | 1x16 | Keyboard (linear 1x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2021 Base) | 1532:027A | laptop | std | standard | 0x1F | 0x05 | 1x16 | Keyboard (linear 1x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (Late 2021 Advanced) | 1532:0276 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 14 (2021) | 1532:0270 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 14 (2022) | 1532:028C | laptop | std | standard | 0x1F | 0x00 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 14 (2023) | 1532:029D | laptop | std | standard | 0x1F | 0x00 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade 15 (2022) | 1532:028A | laptop | std | standard | 0x1F | 0x00 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Book 13 (2020) | 1532:026A | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Pro (2016) | 1532:0210 | laptop | std | standard | 0x3F | 0x05 | 6x25 | Keyboard (matrix 6x25) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Pro (2017) | 1532:0225 | laptop | std | standard | 0x3F | 0x05 | 6x25 | Keyboard (matrix 6x25) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Pro (2017 FullHD) | 1532:022F | laptop | std | standard | 0x3F | 0x05 | 6x25 | Keyboard (matrix 6x25) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Pro (2019) | 1532:0234 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Pro (Late 2019) | 1532:024C | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Pro 17 (2020) | 1532:0256 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Pro 17 (2021) | 1532:0279 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (2016) | 1532:0205 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (Late 2016) | 1532:0220 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (2017) | 1532:022D | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (Late 2017) | 1532:0232 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (2019) | 1532:0239 | laptop | std | standard | 0x3F | 0x05 | 6x16 | Keyboard (matrix 6x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (Late 2019) | 1532:024A | laptop | std | standard | 0x3F | 0x05 | 1x1 | Keyboard (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (2020) | 1532:0252 | laptop | std | standard | 0x3F | 0x05 | 1x1 | Keyboard (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer Blade Stealth (Late 2020) | 1532:0259 | laptop | std | standard | 0x3F | 0x05 | 1x16 | Keyboard (linear 1x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Abyssus Elite DVa Edition | 1532:006A | mouse | std | extended | 0x3F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Abyssus Essential | 1532:006B | mouse | std | extended | 0x3F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk | 1532:0064 | mouse | std | extended | 0x3F | 0x05 | 1x2 | Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk Essential | 1532:0065 | mouse | std | extended | 0x3F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk Ultimate | 1532:0086 | mouse | std | extended | 0x1F | 0x00 | 1x14 | Scroll Wheel (single 1x1); Logo (single 1x1); Left LED Strip (linear 1x8); Right LED Strip (linear 1x4) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk Ultimate (Wireless) | 1532:0088 | mouse | std | extended | 0x1F | 0x00 | 1x14 | Scroll Wheel (single 1x1); Logo (single 1x1); Left LED Strip (linear 1x8); Right LED Strip (linear 1x4) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V2 | 1532:0085 | mouse | std | extended | 0x3F | 0x05 | 1x2 | Scroll Wheel (single 1x1); Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V3 | 1532:0099 | mouse | std | extended | 0x1F | 0x00 | 1x11 | Logo (single 1x1); Scroll Wheel (single 1x1); LED Strip (linear 1x9) | if=0x03 page=0x0C usage=0x01 |
| Razer Basilisk V3 35K | 1532:00CB | mouse | std | extended | 0x1F | 0x00 | 1x11 | Logo (single 1x1); Scroll Wheel (single 1x1); LED Strip (linear 1x9) | if=0x03 page=0x0C usage=0x01 |
| Razer Basilisk V3 Pro (Wired) | 1532:00AA | mouse | std | extended | 0x1F | 0x00 | 1x13 | Scroll Wheel (single 1x1); Logo (single 1x1); LED Strip (linear 1x11) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V3 Pro (Wireless) | 1532:00AB | mouse | std | extended | 0x1F | 0x00 | 1x13 | Scroll Wheel (single 1x1); Logo (single 1x1); LED Strip (linear 1x11) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V3 Pro 35K (Wired) | 1532:00CC | mouse | std | extended | 0x1F | 0x00 | 1x13 | Logo (single 1x1); Scroll Wheel (single 1x1); LED Strip (linear 1x11) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V3 Pro 35K (Wireless) | 1532:00CD | mouse | std | extended | 0x1F | 0x00 | 1x13 | Logo (single 1x1); Scroll Wheel (single 1x1); LED Strip (linear 1x11) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V3 Pro 35K Phantom Green Edition (Wired) | 1532:00D6 | mouse | std | extended | 0x1F | 0x00 | 1x13 | Scroll Wheel (single 1x1); LED Strip (linear 1x11) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V3 Pro 35K Phantom Green Edition (Wireless) | 1532:00D7 | mouse | std | extended | 0x1F | 0x00 | 1x13 | Scroll Wheel (single 1x1); LED Strip (linear 1x11) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V3 Pro 35K Phantom Green Edition (Bluetooth) | 1532:00D8 | mouse | std | extended | 0x1F | 0x00 | 1x13 | Scroll Wheel (single 1x1); LED Strip (linear 1x11) | detecteur desactive (ligne en commentaire) if=* page=0x01 usage=0x02 |
| Razer Basilisk V3 Pro (Bluetooth) | 1532:00AC | mouse | std | extended | 0x1F | 0x00 | 1x13 | Scroll Wheel (single 1x1); Logo (single 1x1); LED Strip (linear 1x11) | detecteur desactive (ligne en commentaire) if=* page=0x01 usage=0x02 |
| Razer Basilisk V3 X HyperSpeed | 1532:00B9 | mouse | std | extended | 0x1F | 0x00 | 1x1 | Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Basilisk V4 Pro (Wired) | 1532:00E2 | mouse | std | extended | 0x1F | 0x00 | 1x13 | Logo (single 1x1); Scroll Wheel (single 1x1); LED Strip (linear 1x5) | if=0x03 page=0x0C usage=0x01 |
| Razer Basilisk V4 Pro (Wireless) | 1532:00E3 | mouse | std | extended | 0x1F | 0x00 | 1x13 | Logo (single 1x1); Scroll Wheel (single 1x1); LED Strip (linear 1x5) | if=0x03 page=0x0C usage=0x01 |
| Razer Cobra | 1532:00A3 | mouse | std | extended | 0x1F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Cobra Pro (Wired) | 1532:00AF | mouse | std | extended | 0x1F | 0x00 | 1x11 | Logo (single 1x1); Scroll Wheel (single 1x1); Underglow (linear 1x9) | if=0x00 page=0x01 usage=0x02 |
| Razer Cobra Pro (Wireless) | 1532:00B0 | mouse | std | extended | 0x1F | 0x00 | 1x11 | Logo (single 1x1); Scroll Wheel (single 1x1); Underglow (linear 1x9) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder Chroma | 1532:0043 | mouse | std | custom | 0x1F | 0x05 | 1x2 | Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder Elite | 1532:005C | mouse | std | extended | 0x3F | 0x00 | 1x2 | Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder Essential | 1532:006E | mouse | std | extended | 0x3F | 0x05 | 1x2 | Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder Essential V2 | 1532:0098 | mouse | std | extended | 0x1F | 0x00 | 1x2 | Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder Essential (White Edition) | 1532:0071 | mouse | std | extended | 0x3F | 0x05 | 1x2 | Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder V2 | 1532:0084 | mouse | std | extended | 0x1F | 0x00 | 1x2 | Scroll Wheel (single 1x1); Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder V2 Mini | 1532:008C | mouse | std | extended | 0x1F | 0x00 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder V2 (Wired) | 1532:007C | mouse | std | extended | 0x3F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer DeathAdder V2 (Wireless) | 1532:007D | mouse | std | extended | 0x3F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Diamondback Chroma | 1532:004C | mouse | std | linear | 0x3F | 0x05 | 1x21 | LED Strip (linear 1x19); Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Lancehead 2017 (Wired) | 1532:0059 | mouse | std | extended | 0x3F | 0x05 | 1x16 | Right LED Strip (linear 1x7); Left LED Strip (linear 1x7); Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Lancehead 2017 (Wireless) | 1532:005A | mouse | std | extended | 0x3F | 0x05 | 1x16 | Right LED Strip (linear 1x7); Left LED Strip (linear 1x7); Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Lancehead 2019 (Wired) | 1532:0070 | mouse | std | extended | 0x3F | 0x05 | 1x16 | Right LED Strip (linear 1x7); Left LED Strip (linear 1x7); Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Lancehead 2019 (Wireless) | 1532:006F | mouse | std | extended | 0x3F | 0x05 | 1x16 | Right LED Strip (linear 1x7); Left LED Strip (linear 1x7); Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Lancehead Tournament Edition | 1532:0060 | mouse | std | extended | 0x3F | 0x05 | 1x16 | Scroll Wheel (single 1x1); Logo (single 1x1); Right LED Strip (linear 1x7); Left LED Strip (linear 1x7) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba 2012 (Wired) | 1532:0024 | mouse | std | custom | 0x3F | 0x05 | 1x15 | Scroll Wheel (linear 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba 2012 (Wireless) | 1532:0025 | mouse | std | custom | 0x3F | 0x05 | 1x15 | Scroll Wheel (linear 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba 2015 (Wired) | 1532:0044 | mouse | std | linear | 0x3F | 0x05 | 1x15 | Chroma Zone (linear 1x15) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba (Wireless) | 1532:0045 | mouse | std | linear | 0x3F | 0x05 | 1x15 | Chroma Zone (linear 1x15) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba 2018 (Wired) | 1532:0073 | mouse | std | extended | 0x3F | 0x05 | 1x2 | Scroll Wheel Zone (single 1x1); Logo Zone (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba 2018 (Wireless) | 1532:0072 | mouse | std | extended | 0x3F | 0x05 | 1x2 | Scroll Wheel Zone (single 1x1); Logo Zone (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba Tournament Edition | 1532:0046 | mouse | std | linear | 0x3F | 0x05 | 1x16 | Left LED Strip (linear 1x7); Right LED Strip (linear 1x7); Logo (single 1x1); Scroll Wheel (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba Elite | 1532:006C | mouse | std | extended | 0x1F | 0x00 | 1x20 | Scroll Wheel (single 1x1); Logo (single 1x1); Left LED Strip (linear 1x9); Right LED Strip (linear 1x9) | if=0x00 page=0x01 usage=0x02 |
| Razer Mamba Hyperflux (Wired) | 1532:0069 | mouse | std | extended | 0x3F | 0x00 | 1x2 | Scroll Wheel (single 1x1); Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Chroma | 1532:0053 | mouse | std | linear | 0x3F | 0x05 | 1x3 | Scroll Wheel (single 1x1); Logo (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Classic | 1532:0093 | mouse | std | extended | 0x1F | 0x00 | 1x3 | Logo (single 1x1); Scroll Wheel (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Epic Chroma | 1532:003E | mouse | std | custom | 0x1F | 0x05 | 1x2 | Scroll Wheel (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Hex V2 | 1532:0050 | mouse | std | standard | 0x3F | 0x05 | 1x3 | Logo (single 1x1); Scroll Wheel (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Left Handed | 1532:008D | mouse | std | extended | 0x1F | 0x00 | 1x3 | Logo (single 1x1); Scroll Wheel (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Trinity | 1532:0067 | mouse | std | extended | 0x3F | 0x05 | 1x3 | Scroll Wheel (single 1x1); Logo (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Pro (Wired) | 1532:008F | mouse | std | extended | 0x1F | 0x05 | 1x3 | Scroll Wheel (single 1x1); Logo (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Pro (Wireless) | 1532:0090 | mouse | std | extended | 0x1F | 0x05 | 1x3 | Scroll Wheel (single 1x1); Logo (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Pro V2 (Wired) | 1532:00A7 | mouse | std | extended | 0x1F | 0x00 | 1x2 | Logo (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Pro V2 (Wireless) | 1532:00A8 | mouse | std | extended | 0x1F | 0x00 | 1x2 | Logo (single 1x1); Numpad (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Naga Pro V3 (Wired) | 1532:00E7 | mouse | std | extended | 0x1F | 0x00 | 1x3 | Logo (single 1x1); Scroll Wheel (single 1x1); Numpad (single 1x1) | if=* page=0x01 usage=0x03 |
| Razer Naga Pro V3 (Wireless) | 1532:00E8 | mouse | std | extended | 0x1F | 0x00 | 1x3 | Logo (single 1x1); Scroll Wheel (single 1x1); Numpad (single 1x1) | if=* page=0x01 usage=0x03 |
| Razer Viper 8kHz | 1532:0091 | mouse | std | extended | 0x1F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Viper Mini | 1532:008A | mouse | std | extended | 0x3F | 0x04 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Viper Ultimate (Wired) | 1532:007A | mouse | std | extended | 0x3F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Viper Ultimate (Wireless) | 1532:007B | mouse | std | extended | 0x3F | 0x05 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Viper | 1532:0078 | mouse | std | extended | 0x3F | 0x04 | 1x1 | Logo (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Orbweaver Chroma | 1532:0207 | keypad | std | custom | 0x3F | 0x05 | 4x5 | Keypad (matrix 4x5) | if=0x02 page=0x01 usage=0x02 |
| Razer Tartarus Chroma | 1532:0208 | keypad | std | custom | 0x1F | 0x05 | 1x1 | Keypad (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer Tartarus Pro | 1532:0244 | keypad | std | extended | 0x1F | 0x00 | 1x21 | Keypad (matrix 4x5); Keypad LED 20 (single 1x1) | if=0x02 page=0x01 usage=0x02 |
| Razer Tartarus V2 | 1532:022B | keypad | std | extended | 0x1F | 0x00 | 4x6 | Keypad (matrix 4x6) | if=0x02 page=0x01 usage=0x02 |
| Razer Firefly | 1532:0C00 | mousemat | std | linear | 0x3F | 0x05 | 1x15 | LED Strip (linear 1x15) | if=0x00 page=0x01 usage=0x02 |
| Razer Firefly Hyperflux | 1532:0068 | mousemat | std | extended | 0x3F | 0x00 | 1x14 | Scroll Wheel (single 1x1); Logo (single 1x1); Mousemat (linear 1x12) | if=0x00 page=0x01 usage=0x02 |
| Razer Firefly V2 | 1532:0C04 | mousemat | std | extended | 0x3F | 0x00 | 1x19 | LED Strip (linear 1x19) | if=0x00 page=0x01 usage=0x02 |
| Razer Firefly V2 Pro | 1532:0C08 | mousemat | std | extended | 0x1F | 0x00 | 1x17 | LED Strip (linear 1x17) | if=0x00 page=0x01 usage=0x02 |
| Razer Goliathus Chroma 3XL | 1532:0C06 | mousemat | std | extended | 0x1F | 0x00 | 1x1 | LED Strip (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Goliathus | 1532:0C01 | mousemat | std | extended | 0x3F | 0x00 | 1x1 | LED Strip (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Goliathus Extended | 1532:0C02 | mousemat | std | extended | 0x3F | 0x00 | 1x1 | LED Strip (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Strider Chroma | 1532:0C05 | mousemat | std | extended | 0x3F | 0x00 | 1x19 | LED Strip (linear 1x19) | if=0x00 page=0x01 usage=0x02 |
| Razer Kraken 7.1 Chroma | 1532:0504 | headset | kraken | none | 0 | 0x00 | 1x1 | Headset (single 1x1) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken 7.1 V2 | 1532:0510 | headset | kraken | none | 0 | 0x00 | 1x1 | Headset (single 1x1) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken Ultimate | 1532:0527 | headset | kraken | none | 0 | 0x00 | 1x1 | Headset (single 1x1) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken Kitty Edition | 1532:0F19 | headset | std | extended | 0x1F | 0x05 | 1x4 | Headset (linear 1x4) | if=0x01 page=0x01 usage=0x03 |
| Razer Kraken Kitty Black Edition V2 | 1532:0560 | headset | kraken | extended | 0x3F | 0x00 | 1x4 | Headset Left (single 1x1); Headset Right (single 1x1); Cat ears (linear 1x2) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken V3 HyperSense | 1532:0533 | headset | krakenv3 | none | 0 | 0x00 | 1x1 | Headset (single 1x1) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken V3 X | 1532:0537 | headset | krakenv3 | none | 0 | 0x00 | 1x1 | Headset (single 1x1) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken V3 | 1532:0549 | headset | krakenv3 | none | 0 | 0x00 | 1x1 | Headset (single 1x1) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken Kitty V2 Pro | 1532:0554 | headset | krakenv3 | none | 0 | 0x00 | 1x4 | Left Cat Ear (single 1x1); Right Cat Ear (single 1x1); Left Logo (single 1x1); Right Logo (single 1x1) | if=0x03 page=0x0C usage=0x01 |
| Razer Kraken V4 (Wired) | 1532:056B | headset | krakenv4 | extended | 0x60 | 0x00 | 1x9 | Headset (linear 1x9) | if=0x05 page=0xFF14 usage=0x01 |
| Razer Kraken V4 (Wireless) | 1532:056C | headset | krakenv4 | extended | 0x60 | 0x00 | 1x9 | Headset (linear 1x9) | if=0x05 page=0xFF14 usage=0x01 |
| Razer Kraken Kitty V3 Pro (Wired) | 1532:0587 | headset | krakenv4 | extended | 0x60 | 0x00 | 1x10 | Headset (linear 1x9); Ears (single 1x1) | if=0x05 page=0xFF14 usage=0x01 |
| Razer Kraken Kitty V3 Pro (Wireless) | 1532:0588 | headset | krakenv4 | extended | 0x60 | 0x00 | 1x10 | Headset (linear 1x9); Ears (single 1x1) | if=0x05 page=0xFF14 usage=0x01 |
| Razer Tiamat 7.1 V2 | 1532:0F03 | headset | std | extended | 0x3F | 0x05 | 1x17 | Controller (linear 1x15); Headset Left (single 1x1); Headset Right (single 1x1) | if=0x00 page=* usage=* |
| Razer Base Station Chroma | 1532:0F08 | headset_stand | std | extended | 0x3F | 0x00 | 1x15 | LED Strip (linear 1x15) | if=0x00 page=0x01 usage=0x02 |
| Razer Base Station V2 Chroma | 1532:0F20 | headset_stand | std | extended | 0x1F | 0x00 | 1x8 | LED Strip (linear 1x8) | if=0x00 page=0x01 usage=0x02 |
| Razer Mouse Bungee V3 Chroma | 1532:0F1D | mouse | std | extended | 0x1F | 0x00 | 1x8 | LED Strip (linear 1x8) | if=0x00 page=0x01 usage=0x02 |
| Razer Charging Pad Chroma | 1532:0F26 | accessory | std | extended | 0x1F | 0x00 | 1x10 | LED Strip (linear 1x10) | if=0x00 page=0x0C usage=0x01 |
| Razer Chroma Addressable RGB Controller | 1532:0F1F | ledstrip | argb | extended_argb | 0x3F | 0x05 | 6x80 | Channel 1 (linear 1x80); Channel 2 (linear 1x80); Channel 3 (linear 1x80); Channel 4 (linear 1x80); Channel 5 (linear 1x80); Channel 6 (linear 1x80) | if=0x00 page=* usage=* |
| Razer Chroma HDK | 1532:0F09 | ledstrip | std | extended | 0x3F | 0x00 | 4x16 | Channel 1 (linear 1x16); Channel 2 (linear 1x16); Channel 3 (linear 1x16); Channel 4 (linear 1x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Chroma PC Case Lighting Kit | 1532:0F0E | ledstrip | std | extended | 0x3F | 0x05 | 2x16 | Channel 1 (linear 1x16); Channel 2 (linear 1x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Core | 1532:0215 | gpu | std | standard | 0x3F | 0x05 | 1x9 | Side Window Lights (single 1x1); LED Strip (linear 1x8) | if=0x00 page=0xFF00 usage=0x01 |
| Razer Core X | 1532:0F1A | gpu | std | extended | 0x3F | 0x00 | 1x16 | Side Window Lights (single 1x1); LED Strip (linear 1x15) | if=0x02 page=0x01 usage=0x02 |
| Razer Laptop Stand Chroma | 1532:0F0D | ledstrip | std | extended | 0x1F | 0x00 | 1x15 | LED Strip (linear 1x15) | if=0x00 page=0x01 usage=0x02 |
| Razer Laptop Stand Chroma V2 | 1532:0F2B | ledstrip | std | extended | 0x1F | 0x00 | 1x15 | LED Strip (linear 1x15) | if=0x00 page=0x01 usage=0x02 |
| Razer Leviathan V2 | 1532:0532 | speaker | std | extended | 0x1F | 0x00 | 2x9 | Speaker Underglow (linear 2x9) | if=0x02 page=0x0C usage=0x01 |
| Razer Leviathan V2 X | 1532:054A | speaker | std | extended | 0x1F | 0x00 | 1x14 | Speaker Underglow (linear 1x14) | if=0x00 page=0x0C usage=0x01 |
| Razer Chroma Mug Holder | 1532:0F07 | accessory | std | linear | 0x3F | 0x05 | 1x15 | LED Strip (linear 1x15) | if=0x00 page=0x01 usage=0x02 |
| Razer Mouse Dock Chroma | 1532:007E | light | std | extended | 0x3F | 0x05 | 1x1 | Base (single 1x1) | if=0x00 page=0x01 usage=0x02 |
| Razer Mouse Dock Pro | 1532:00A4 | light | std | extended | 0xFF | 0x00 | 1x9 | Base (linear 1x9) | if=0x00 page=0x01 usage=0x02 |
| Razer Nommo Chroma | 1532:0517 | speaker | std | extended | 0x3F | 0x05 | 2x24 | Right Speaker (linear 1x24); Left Speaker (linear 1x24) | if=0x01 page=0x01 usage=0x03<br>if=0x00 page=0x01 usage=0x00 |
| Razer Nommo Pro | 1532:0518 | speaker | std | extended | 0x3F | 0x05 | 2x8 | Left Speaker (linear 1x8); Right Speaker (linear 1x8) | if=0x01 page=0x01 usage=0x03<br>if=0x00 page=0x01 usage=0x00 |
| Lian Li O11 Dynamic - Razer Edition | 1532:0F13 | ledstrip | std | extended | 0x1F | 0x00 | 4x16 | Case LEDs (linear 4x16) | if=0x02 page=0x01 usage=0x02 |
| Razer Seiren Emote | 1532:0F1B | microphone | std | extended | 0x3F | 0x05 | 4x16 | 8-Bit LED Matrix (matrix 8x8) | if=* page=0x0C usage=0x01 |
| Razer Thunderbolt 4 Dock Chroma | 1532:0F21 | accessory | std | extended | 0x3F | 0x05 | 1x12 | LED Strip (linear 1x12) | if=0x01 page=0x01 usage=0x03<br>if=* page=0x0C usage=0x01 |
| Razer Thunderbolt 5 Dock Chroma | 1532:0F52 | accessory | std | extended | 0x3F | 0x05 | 1x12 | LED Strip (linear 1x12) | if=* page=0x0C usage=0x01 |
| Razer Hanbo Chroma | 1532:0F35 | cooler | hanbo | extended | 0x3F | 0x00 | 1x70 | Pump (linear 1x16); Fan 1 (linear 1x18); Fan 2 (linear 1x18); Fan 3 (linear 1x18) | if=0x00 page=0xFF00 usage=0x01 |
| (absent de la table) | 1532:0501 | ? | kraken | - | - | - | - | - | if=0x03 page=0x0C usage=0x01 |
| (absent de la table) | 1532:0506 | ? | kraken | - | - | - | - | - | if=0x03 page=0x0C usage=0x01 |

## 10. Claviers : position des touches dans la grille

Chaque table donne, ligne par ligne, `colonne:touche` pour la disposition **ANSI (US)**. Les
touches sont désignées par leur code physique W3C (`KeyQ`, `Digit1`, `IntlBackslash`…), ou entre
guillemets quand il n'existe pas d'équivalent (« Logo », touches macro « M1 »…). Les cellules
absentes de la liste ne correspondent à aucune touche : envoyer du noir.

Index LED dans la trame = ligne × colonnes + colonne.

**Variante ISO des claviers de bureau** (claviers dont la commande 0x00/0x86 renvoie une disposition ISO, QWERTZ ou AZERTY) : la touche `Backslash` ANSI de la rangée de `Tab` disparaît ; une touche `Backslash` (« # » ISO) apparaît dans la rangée de `CapsLock`, dans la colonne juste à gauche de `Enter` ; une touche `IntlBackslash` apparaît dans la rangée de `ShiftLeft`, dans la colonne juste à gauche de `KeyZ`. Rien d'autre ne change (règle vérifiée en régénérant les 32 dispositions de bureau en ISO). Les six dispositions de **portables** suivent d'autres règles : leurs tables ISO sont données en fin de section.

Les tables sont reconstituées à partir des données de disposition d'OpenRGB (base 104 touches + éditions par modèle) ; quelques modèles y ont des
positions surprenantes (ex. « Logo » en ligne 3 sur la Blackwidow V3 Pro) qu'il faudra confirmer
sur le matériel.

Claviers **sans table** de disposition dans OpenRGB (la grille est envoyée telle quelle, sans
correspondance touche ↔ cellule connue) : Blackwidow Chroma Overwatch (0211, 6×22), Huntsman V3 HE
Magnetic Mini (02E4, 6×22), Blade 2016/Late 2016/2018/2019/2020 et Blade Pro 2016/2017 FullHD/2019/
Late 2019/17 2020, Blade Stealth (Late 2016, 2017, Late 2017, 2019) en 6×16 ou 6×25. Les modèles à
zone unique ou linéaire (Cynosa Lite, Ornata V3 x, DeathStalker Chroma, Blade Base…) n'ont pas de
touches individuelles.

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow 2019 (0x0241)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 6:Space 10:AltRight 11:"Logo" 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow Chroma (0x0203)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 20:"Logo" |
| 1 | 0:"M1" 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 0:"M2" 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 0:"M3" 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 0:"M4" 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 0:"M5" 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow Chroma Tournament Edition (0x0209)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 20:"Logo" |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow Chroma V2 (0x0221)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 20:"Logo" |
| 1 | 0:"M1" 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 0:"M2" 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 0:"M3" 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 0:"M4" 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 0:"M5" 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow Elite (0x0228)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 18:MediaTrackPrevious 19:MediaPlayPause 20:MediaTrackNext 21:AudioVolumeMute |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 6:Space 10:AltRight 11:"Logo" 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Blackwidow V3 (0x024E)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 6:Space 10:AltRight 11:"Logo" 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow V3 Pro (Wired) (0x025A), Razer BlackWidow V3 Pro (Wireless) (0x025C), Razer BlackWidow V3 Pro (Bluetooth) (0x025B)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:PrintScreen 15:ScrollLock 16:Pause 18:MediaTrackPrevious 19:MediaPlayPause 20:MediaTrackNext 21:AudioVolumeMute |
| 1 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 13:Backspace 14:Insert 15:Home 16:PageUp 17:NumLock 18:NumpadDivide 19:NumpadMultiply 20:NumpadSubtract |
| 2 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 14:Delete 15:End 16:PageDown 17:Numpad7 18:Numpad8 19:Numpad9 20:NumpadAdd |
| 3 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter 14:"Logo" 17:Numpad4 18:Numpad5 19:Numpad6 |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 15:ArrowUp 17:Numpad1 18:Numpad2 19:Numpad3 20:NumpadEnter |
| 5 | 0:ControlLeft 1:MetaLeft 2:AltLeft 6:Space 10:AltRight 11:Fn 12:ContextMenu 13:ControlRight 14:ArrowLeft 15:ArrowDown 16:ArrowRight 18:Numpad0 19:NumpadDecimal |

#### Grille 6 lignes x 18 colonnes

Appareils : Razer BlackWidow V3 TKL (0x0A24)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight |

#### Grille 5 lignes x 16 colonnes

Appareils : Razer BlackWidow V3 Mini (Wired) (0x0258), Razer BlackWidow V3 Mini (Wireless) (0x0271)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 14:Backspace 15:Delete |
| 1 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 15:PageUp |
| 2 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter 15:PageDown |
| 3 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 14:Insert 15:ArrowUp |
| 4 | 0:ControlLeft 1:MetaLeft 2:AltLeft 6:Space 7:"Logo" 10:AltRight 11:Fn 12:ControlRight 13:ArrowLeft |

#### Grille 6 lignes x 23 colonnes

Appareils : Razer Blackwidow V4 (0x0287)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:"M6" 2:Escape 4:F1 5:F2 6:F3 7:F4 8:F5 9:F6 10:F7 11:F8 12:F9 13:F10 14:F11 15:F12 16:PrintScreen 17:ScrollLock 18:Pause 19:MediaTrackPrevious 20:MediaPlayPause 21:MediaTrackNext 22:AudioVolumeMute |
| 1 | 1:"M5" 2:Backquote 3:Digit1 4:Digit2 5:Digit3 6:Digit4 7:Digit5 8:Digit6 9:Digit7 10:Digit8 11:Digit9 12:Digit0 13:Minus 14:Equal 15:Backspace 16:Insert 17:Home 18:PageUp 19:NumLock 20:NumpadDivide 21:NumpadMultiply 22:NumpadSubtract |
| 2 | 1:"M4" 2:Tab 3:KeyQ 4:KeyW 5:KeyE 6:KeyR 7:KeyT 8:KeyY 9:KeyU 10:KeyI 11:KeyO 12:KeyP 13:BracketLeft 14:BracketRight 15:Backslash 16:Delete 17:End 18:PageDown 19:Numpad7 20:Numpad8 21:Numpad9 22:NumpadAdd |
| 3 | 1:"M3" 2:CapsLock 3:KeyA 4:KeyS 5:KeyD 6:KeyF 7:KeyG 8:KeyH 9:KeyJ 10:KeyK 11:KeyL 12:Semicolon 13:Quote 15:Enter 19:Numpad4 20:Numpad5 21:Numpad6 |
| 4 | 1:"M2" 2:ShiftLeft 4:KeyZ 5:KeyX 6:KeyC 7:KeyV 8:KeyB 9:KeyN 10:KeyM 11:Comma 12:Period 13:Slash 15:ShiftRight 17:ArrowUp 19:Numpad1 20:Numpad2 21:Numpad3 22:NumpadEnter |
| 5 | 1:"M1" 2:ControlLeft 3:MetaLeft 4:AltLeft 8:Space 12:AltRight 13:Fn 14:ContextMenu 15:ControlRight 16:ArrowLeft 17:ArrowDown 18:ArrowRight 20:Numpad0 21:NumpadDecimal |

#### Grille 6 lignes x 23 colonnes

Appareils : Razer Blackwidow V4 Pro (0x028D)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:"Dial" 2:Escape 4:F1 5:F2 6:F3 7:F4 8:F5 9:F6 10:F7 11:F8 12:F9 13:F10 14:F11 15:F12 16:PrintScreen 17:ScrollLock 18:Pause 19:MediaTrackPrevious 20:MediaPlayPause 21:MediaTrackNext 22:AudioVolumeMute |
| 1 | 1:"M5" 2:Backquote 3:Digit1 4:Digit2 5:Digit3 6:Digit4 7:Digit5 8:Digit6 9:Digit7 10:Digit8 11:Digit9 12:Digit0 13:Minus 14:Equal 15:Backspace 16:Insert 17:Home 18:PageUp 19:NumLock 20:NumpadDivide 21:NumpadMultiply 22:NumpadSubtract |
| 2 | 1:"M4" 2:Tab 3:KeyQ 4:KeyW 5:KeyE 6:KeyR 7:KeyT 8:KeyY 9:KeyU 10:KeyI 11:KeyO 12:KeyP 13:BracketLeft 14:BracketRight 15:Backslash 16:Delete 17:End 18:PageDown 19:Numpad7 20:Numpad8 21:Numpad9 22:NumpadAdd |
| 3 | 1:"M3" 2:CapsLock 3:KeyA 4:KeyS 5:KeyD 6:KeyF 7:KeyG 8:KeyH 9:KeyJ 10:KeyK 11:KeyL 12:Semicolon 13:Quote 15:Enter 19:Numpad4 20:Numpad5 21:Numpad6 |
| 4 | 1:"M2" 2:ShiftLeft 4:KeyZ 5:KeyX 6:KeyC 7:KeyV 8:KeyB 9:KeyN 10:KeyM 11:Comma 12:Period 13:Slash 15:ShiftRight 17:ArrowUp 19:Numpad1 20:Numpad2 21:Numpad3 22:NumpadEnter |
| 5 | 1:"M1" 2:ControlLeft 3:MetaLeft 4:AltLeft 8:Space 12:AltRight 13:Fn 14:ContextMenu 15:ControlRight 16:ArrowLeft 17:ArrowDown 18:ArrowRight 20:Numpad0 21:NumpadDecimal |

#### Grille 6 lignes x 18 colonnes

Appareils : Razer Blackwidow V4 Pro 75% (Wired) (0x02B3), Razer Blackwidow V4 Pro 75% (Wireless) (0x02B4), Razer Blackwidow V4 75% (Wired) (0x02A5)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:"Left Underglow 0" 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:"Left Underglow 1" 15:"Left Underglow 5" 16:"Left Underglow 8" 17:"Right Underglow 0" |
| 1 | 0:"Left Underglow 2" 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace 16:Delete 17:"Right Underglow 2" |
| 2 | 0:"Left Underglow 3" 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 16:PageUp 17:"Right Underglow 3" |
| 3 | 0:"Left Underglow 4" 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 16:PageDown 17:"Right Underglow 4" |
| 4 | 0:"Left Underglow 6" 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 15:ArrowUp 16:Insert 17:"Right Underglow 6" |
| 5 | 0:"Left Underglow 7" 1:ControlLeft 2:MetaLeft 3:AltLeft 5:"Right Underglow 1" 6:"Right Underglow 5" 7:Space 8:"Right Underglow 8" 11:AltRight 12:Fn 13:ControlRight 14:ArrowLeft 15:ArrowDown 16:ArrowRight 17:"Right Underglow 7" |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Blackwidow V4 X (0x0293)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:"M6" 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause |
| 1 | 0:"M5" 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 0:"M4" 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 0:"M3" 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 0:"M2" 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 0:"M1" 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 18 colonnes

Appareils : Razer Blackwidow V4 TKL (Wired) (0x02D7), Razer Blackwidow V4 TKL (Wireless) (0x02D5), Razer Blackwidow V4 Low Profile TKL (Wired) (0x02D4), Razer Blackwidow V4 Low Profile TKL (Wireless) (0x02D2)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause |
| 1 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 13:Backspace 15:Insert 16:Home 17:PageUp |
| 2 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 15:Delete 16:End 17:PageDown |
| 3 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 16:ArrowUp |
| 5 | 0:ControlLeft 1:MetaLeft 2:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow X Chroma (0x0216)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 20:"Logo" |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer BlackWidow X Chroma Tournament Edition (0x021A)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 20:"Logo" |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Cynosa Chroma (0x022A)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 20:"Logo" |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Cynosa v2 (0x025E)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 18:MediaTrackPrevious 19:MediaPlayPause 20:MediaTrackNext 21:AudioVolumeMute |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 15:AudioVolumeUp 16:ArrowUp 17:AudioVolumeDown 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Deathstalker V2 (0x0295)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:PrintScreen 15:ScrollLock 16:Pause |
| 1 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 14:Delete 15:End 16:PageDown 17:Numpad7 18:Numpad8 19:Numpad9 20:NumpadAdd |
| 3 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter 17:Numpad4 18:Numpad5 19:Numpad6 |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 15:ArrowUp 17:Numpad1 18:Numpad2 19:Numpad3 20:NumpadEnter |
| 5 | 0:ControlLeft 1:MetaLeft 2:AltLeft 6:Space 10:AltRight 11:Fn 12:ContextMenu 13:ControlRight 14:ArrowLeft 15:ArrowDown 16:ArrowRight 18:Numpad0 19:NumpadDecimal |

#### Grille 6 lignes x 17 colonnes

Appareils : Razer DeathStalker V2 Pro TKL (Wired) (0x0298), Razer DeathStalker V2 Pro TKL (Wireless) (0x0296)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 |
| 1 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 13:Backspace 14:Insert 15:Home 16:PageUp |
| 2 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 14:Delete 15:End 16:PageDown |
| 3 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 15:ArrowUp |
| 5 | 0:ControlLeft 1:MetaLeft 2:AltLeft 6:Space 10:AltRight 11:Fn 12:ContextMenu 13:ControlRight 14:ArrowLeft 15:ArrowDown 16:ArrowRight |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer DeathStalker V2 Pro (Wired) (0x0292), Razer DeathStalker V2 Pro (Wireless) (0x0290)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:PrintScreen 15:ScrollLock 16:Pause |
| 1 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 13:Backspace 14:Insert 15:Home 16:PageUp 17:NumLock 18:NumpadDivide 19:NumpadMultiply 20:NumpadSubtract |
| 2 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 14:Delete 15:End 16:PageDown 17:Numpad7 18:Numpad8 19:Numpad9 20:NumpadAdd |
| 3 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter 17:Numpad4 18:Numpad5 19:Numpad6 |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 15:ArrowUp 17:Numpad1 18:Numpad2 19:Numpad3 20:NumpadEnter |
| 5 | 0:ControlLeft 1:MetaLeft 2:AltLeft 6:Space 10:AltRight 11:Fn 12:ContextMenu 13:ControlRight 14:ArrowLeft 15:ArrowDown 16:ArrowRight 18:Numpad0 19:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Huntsman (0x0227), Razer Ornata Chroma (0x021E)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Huntsman Elite (0x0226), Razer Huntsman V2 Analog (0x0266)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 18:MediaTrackPrevious 19:MediaPlayPause 20:MediaTrackNext 21:AudioVolumeMute |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 5 lignes x 15 colonnes

Appareils : Razer Huntsman Mini (0x0257), Razer Huntsman Mini Analog (0x0282)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace |
| 1 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash |
| 2 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter |
| 3 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight |
| 4 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight |

#### Grille 6 lignes x 18 colonnes

Appareils : Razer Huntsman Tournament Edition (0x0243)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Huntsman V2 (0x026C)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:PrintScreen 15:ScrollLock 16:Pause 17:MediaTrackPrevious 18:MediaPlayPause 19:MediaTrackNext 20:AudioVolumeMute |
| 1 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 13:Backspace 14:Insert 15:Home 16:PageUp 17:NumLock 18:NumpadDivide 19:NumpadMultiply 20:NumpadSubtract |
| 2 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 14:Delete 15:End 16:PageDown 17:Numpad7 18:Numpad8 19:Numpad9 20:NumpadAdd |
| 3 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter 17:Numpad4 18:Numpad5 19:Numpad6 |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 15:ArrowUp 17:Numpad1 18:Numpad2 19:Numpad3 20:NumpadEnter |
| 5 | 0:ControlLeft 1:MetaLeft 2:AltLeft 6:Space 10:AltRight 11:Fn 12:ContextMenu 13:ControlRight 14:ArrowLeft 15:ArrowDown 16:ArrowRight 18:Numpad0 19:NumpadDecimal |

#### Grille 6 lignes x 17 colonnes

Appareils : Razer Huntsman V2 TKL (0x026B)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:PrintScreen 15:ScrollLock 16:Pause |
| 1 | 0:Backquote 1:Digit1 2:Digit2 3:Digit3 4:Digit4 5:Digit5 6:Digit6 7:Digit7 8:Digit8 9:Digit9 10:Digit0 11:Minus 12:Equal 13:Backspace 14:Insert 15:Home 16:PageUp |
| 2 | 0:Tab 1:KeyQ 2:KeyW 3:KeyE 4:KeyR 5:KeyT 6:KeyY 7:KeyU 8:KeyI 9:KeyO 10:KeyP 11:BracketLeft 12:BracketRight 13:Backslash 14:Delete 15:End 16:PageDown |
| 3 | 0:CapsLock 1:KeyA 2:KeyS 3:KeyD 4:KeyF 5:KeyG 6:KeyH 7:KeyJ 8:KeyK 9:KeyL 10:Semicolon 11:Quote 13:Enter |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 13:ShiftRight 15:ArrowUp |
| 5 | 0:ControlLeft 1:MetaLeft 2:AltLeft 6:Space 10:AltRight 11:Fn 12:ContextMenu 13:ControlRight 14:ArrowLeft 15:ArrowDown 16:ArrowRight |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Huntsman V3 Pro (0x02A6)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 18:"Media group" 20:"Media: Volume Dial" |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 18:Numpad0 19:NumpadDecimal |

#### Grille 6 lignes x 19 colonnes

Appareils : Razer Huntsman V3 Pro TKL White (0x02A7)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:"Xbox Game Bar" 16:MediaPlayPause 18:"Media: Volume Dial" |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight |

#### Grille 6 lignes x 22 colonnes

Appareils : Razer Ornata Chroma V2 (0x025D)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:PrintScreen 16:ScrollLock 17:Pause 18:MediaTrackPrevious 19:MediaPlayPause 20:MediaTrackNext 21:AudioVolumeMute |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Insert 16:Home 17:PageUp 18:NumLock 19:NumpadDivide 20:NumpadMultiply 21:NumpadSubtract |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Delete 16:End 17:PageDown 18:Numpad7 19:Numpad8 20:Numpad9 21:NumpadAdd |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 18:Numpad4 19:Numpad5 20:Numpad6 |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 16:ArrowUp 18:Numpad1 19:Numpad2 20:Numpad3 21:NumpadEnter |
| 5 | 1:ControlLeft 2:MetaLeft 3:AltLeft 7:Space 11:AltRight 12:Fn 13:ContextMenu 14:ControlRight 15:ArrowLeft 16:ArrowDown 17:ArrowRight 19:Numpad0 20:NumpadDecimal |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 15 (Late 2021 Advanced) (0x0276)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:Insert 15:Delete |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 8:Space 9:AltRight 10:Fn 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 14 (2021) (0x0270), Razer Book 13 (2020) (0x026A)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:Delete 15:Power |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 15:Backslash |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 15:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 9:AltRight 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 14 (2022) (0x028C), Razer Blade 14 (2023) (0x029D)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:Delete 15:Power |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 15:Backslash |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 15:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 7:Space 9:AltRight 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 15 (2022) (0x028A)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:Delete 15:Power |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 15:Backslash |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 9:AltRight 10:Fn 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 25 colonnes

Appareils : Razer Blade Pro (2017) (0x0225)

| Ligne | colonne:touche |
|---|---|
| 0 | 2:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:Insert 17:Delete 19:MediaTrackPrevious 20:MediaTrackNext 21:"Media Volume" 23:MediaPlayPause 24:AudioVolumeMute |
| 1 | 2:Backquote 3:Digit1 4:Digit2 5:Digit3 6:Digit4 7:Digit5 8:Digit6 9:Digit7 10:Digit8 11:Digit9 12:Digit0 13:Minus 14:Equal 15:Backspace 19:"Trackpad" 20:"Trackpad" 21:"Trackpad" 22:"Trackpad" 23:"Trackpad" 24:"Trackpad" |
| 2 | 2:Tab 4:KeyQ 5:KeyW 6:KeyE 7:KeyR 8:KeyT 9:KeyY 10:KeyU 11:KeyI 12:KeyO 13:KeyP 14:BracketLeft 15:BracketRight 17:Backslash 19:"Trackpad" 24:"Trackpad" |
| 3 | 1:CapsLock 4:KeyA 5:KeyS 6:KeyD 7:KeyF 8:KeyG 9:KeyH 10:KeyJ 11:KeyK 12:KeyL 13:Semicolon 14:Quote 18:Enter 19:"Trackpad" 24:"Trackpad" |
| 4 | 0:ShiftLeft 4:KeyZ 5:KeyX 6:KeyC 7:KeyV 8:KeyB 9:KeyN 10:KeyM 11:Comma 12:Period 13:Slash 14:ArrowUp 18:ShiftRight 19:"Trackpad" 24:"Trackpad" |
| 5 | 0:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 7:Space 10:AltRight 12:ControlRight 13:ArrowLeft 14:ArrowDown 15:ArrowRight 16:Fn 19:"Trackpad" 20:"Trackpad" 21:"Trackpad" 22:"Trackpad" 23:"Trackpad" 24:"Trackpad" |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade Pro 17 (2021) (0x0279)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:Insert 15:Delete |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 15:Backslash |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 15:Enter |
| 4 | 1:ShiftLeft 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 9:AltRight 10:Fn 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade Stealth (2016) (0x0205)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:Insert 15:Delete |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 14:Backspace 15:Backspace |
| 2 | 0:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Backslash 15:Backslash |
| 3 | 0:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 14:Enter 15:Enter |
| 4 | 0:ShiftLeft 2:KeyZ 3:KeyX 4:KeyC 5:KeyV 6:KeyB 7:KeyN 8:KeyM 9:Comma 10:Period 11:Slash 12:ShiftRight 13:ShiftRight 14:ShiftRight |
| 5 | 0:ControlLeft 1:Fn 2:MetaLeft 3:AltLeft 5:Space 6:Space 8:Space 9:AltRight 10:Fn 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 4 lignes x 6 colonnes

Appareils : Razer Tartarus V2 (0x022B)

| Ligne | colonne:touche |
|---|---|
| 0 | 0:"01" 1:"02" 2:"03" 3:"04" 4:"05" |
| 1 | 0:"06" 1:"07" 2:"08" 3:"09" 4:"10" |
| 2 | 0:"11" 1:"12" 2:"13" 3:"14" 4:"15" |
| 3 | 0:"16" 1:"17" 2:"18" 3:"19" 4:"Scroll Wheel" 5:"20" |


### Variante ISO des portables Blade

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 15 (Late 2021 Advanced) (0x0276)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:Insert 15:Delete |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 14:Enter |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 13:Backslash |
| 4 | 1:ShiftLeft 2:IntlBackslash 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 8:Space 9:AltRight 10:Fn 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 14 (2021) (0x0270), Razer Book 13 (2020) (0x026A)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:Delete 15:Power |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 13:Backslash 15:Enter |
| 4 | 1:ShiftLeft 2:IntlBackslash 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 9:AltRight 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 14 (2022) (0x028C), Razer Blade 14 (2023) (0x029D)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:Delete 15:Power |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 13:Backslash 15:Enter |
| 4 | 1:ShiftLeft 2:IntlBackslash 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 7:Space 9:AltRight 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade 15 (2022) (0x028A)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:Delete 15:Power |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight 15:Enter |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 15:Backslash |
| 4 | 1:ShiftLeft 2:IntlBackslash 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 14:ShiftRight 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 9:AltRight 10:Fn 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

#### Grille 6 lignes x 25 colonnes

Appareils : Razer Blade Pro (2017) (0x0225)

| Ligne | colonne:touche |
|---|---|
| 0 | 2:Escape 3:F1 4:F2 5:F3 6:F4 7:F5 8:F6 9:F7 10:F8 11:F9 12:F10 13:F11 14:F12 15:Insert 17:Delete 19:MediaTrackPrevious 20:MediaTrackNext 21:"Media Volume" 23:MediaPlayPause 24:AudioVolumeMute |
| 1 | 2:Backquote 3:Digit1 4:Digit2 5:Digit3 6:Digit4 7:Digit5 8:Digit6 9:Digit7 10:Digit8 11:Digit9 12:Digit0 13:Minus 14:Equal 15:Backspace 19:"Trackpad" 20:"Trackpad" 21:"Trackpad" 22:"Trackpad" 23:"Trackpad" 24:"Trackpad" |
| 2 | 2:Tab 4:KeyQ 5:KeyW 6:KeyE 7:KeyR 8:KeyT 9:KeyY 10:KeyU 11:KeyI 12:KeyO 13:KeyP 14:BracketLeft 15:BracketRight 19:"Trackpad" 24:"Trackpad" |
| 3 | 1:CapsLock 4:KeyA 5:KeyS 6:KeyD 7:KeyF 8:KeyG 9:KeyH 10:KeyJ 11:KeyK 12:KeyL 13:Semicolon 14:Quote 15:Backslash 18:Enter 19:"Trackpad" 24:"Trackpad" |
| 4 | 0:ShiftLeft 1:IntlBackslash 4:KeyZ 5:KeyX 6:KeyC 7:KeyV 8:KeyB 9:KeyN 10:KeyM 11:Comma 12:Period 13:Slash 14:ArrowUp 18:ShiftRight 19:"Trackpad" 24:"Trackpad" |
| 5 | 0:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 7:Space 10:AltRight 12:ControlRight 13:ArrowLeft 14:ArrowDown 15:ArrowRight 16:Fn 19:"Trackpad" 20:"Trackpad" 21:"Trackpad" 22:"Trackpad" 23:"Trackpad" 24:"Trackpad" |

#### Grille 6 lignes x 16 colonnes

Appareils : Razer Blade Pro 17 (2021) (0x0279)

| Ligne | colonne:touche |
|---|---|
| 0 | 1:Escape 2:F1 3:F2 4:F3 5:F4 6:F5 7:F6 8:F7 9:F8 10:F9 11:F10 12:F11 13:F12 14:Insert 15:Delete |
| 1 | 1:Backquote 2:Digit1 3:Digit2 4:Digit3 5:Digit4 6:Digit5 7:Digit6 8:Digit7 9:Digit8 10:Digit9 11:Digit0 12:Minus 13:Equal 15:Backspace |
| 2 | 1:Tab 2:KeyQ 3:KeyW 4:KeyE 5:KeyR 6:KeyT 7:KeyY 8:KeyU 9:KeyI 10:KeyO 11:KeyP 12:BracketLeft 13:BracketRight |
| 3 | 1:CapsLock 2:KeyA 3:KeyS 4:KeyD 5:KeyF 6:KeyG 7:KeyH 8:KeyJ 9:KeyK 10:KeyL 11:Semicolon 12:Quote 13:Backslash 15:Enter |
| 4 | 1:ShiftLeft 2:IntlBackslash 3:KeyZ 4:KeyX 5:KeyC 6:KeyV 7:KeyB 8:KeyN 9:KeyM 10:Comma 11:Period 12:Slash 15:ShiftRight |
| 5 | 1:ControlLeft 2:Fn 3:MetaLeft 5:AltLeft 9:AltRight 10:Fn 11:ControlRight 12:ArrowLeft 13:ArrowUp 14:ArrowRight 15:ArrowDown |

## 11. Sécurité et points d'attention

- **Persistance** : rien n'est écrit de façon permanente par OpenRGB (stockage 0x00). L'octet 0x01
  existe dans le format mais son effet (écriture en mémoire flash) n'est pas documenté par le source ;
  s'il est utilisé un jour, appliquer les garde-fous flash d'AGENTS.md et limiter la fréquence.
- **Mode logiciel 0x03** : envoyé seulement à trois accessoires. Le laisser actif sur d'autres
  appareils pourrait désactiver leurs effets matériels jusqu'au retour en mode 0x00 **(incertain)**.
- **Récepteurs sans fil** : sans l'attente de réponse (§2.3), des commandes sont perdues
  silencieusement ; ne jamais enchaîner des envois rapides vers un appareil relayé.
- **Cohabitation** : respecter le mutex `Global\RazerLinkReadWriteGuardMutex` pendant chaque envoi.
- **Débit** : une trame de clavier plein format = 6 rapports + 1 activation, ≈1 ms d'écart ; ne pas
  descendre en dessous.
- **Somme de contrôle** : un mauvais XOR fait rejeter le rapport (statut 0x03) — c'est le premier
  point à vérifier en cas de silence de l'appareil.
- **Kraken V4** : somme de contrôle non remplie par OpenRGB alors qu'un algorithme existe ; tester
  les deux variantes.
- **PID ambigu 0x0F21** et **Kraken 7.1 Classic** : voir §5.
