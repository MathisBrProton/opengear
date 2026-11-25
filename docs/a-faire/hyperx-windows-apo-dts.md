# HyperX — Effets audio Windows (APO / DTS:X / AINR)

## Constat technique

Contrairement aux casques et souris équipés d'un DSP HID configurable (`Cloud Alpha S`, `Cloud Flight S`, `Cloud II Wireless`, `Cloud III S Wireless`, périphériques `Universal`), plusieurs périphériques HyperX pilotent une partie de leurs effets audio **uniquement côté Windows** via des objets COM **Audio Processing Objects (APO)** attachés au pilote USB Audio dans la pile WASAPI, contrôlés par `NGenuity2Helper.exe` (`HandleAPOCommand` / `APOSetMode`) :

- **Casques concernés (EQ 10 bandes et/ou DTS:X Spatial)** :
  - `HyperX Cloud III` (`0x089D`)
  - `HyperX Cloud III Wireless` (`0x0C9D` / `0x0E9D` / `0x05B7` / `0x06B7`)
  - `HyperX Cloud II Core Wireless` (`0x0995` / `0x069F`)
  - `HyperX Cloud II Wireless DTS` (`0x018B` / `0x058B` / `0x0696` / `0x038B`)
  - `HyperX Cloud Stinger 2 Wireless` (`0x0D93`)
  - `HyperX Cloud Alpha Wireless` (`0x1743` / `0x1765` / `0x098D`)
- **Microphones concernés** :
  - `HyperX DuoCast` (`0x098C`) : `Limiter`, `Noise Gate` et `AINR` (réduction de bruit IA).

Il n'existe pas de commande USB HID directe pour ces traitements sur le firmware de ces modèles ; `opengear-platform` ne contient actuellement aucune couche audio WASAPI/APO.

## Deux approches possibles

### Piste A — Piloter l'APO HyperX/DTS déjà installé sur Windows

- **Principe** : rétro-ingénier l'interface IPC / COM / registre utilisée par `NGenuity2Helper.exe` (`HandleAPOCommand`, `APOSetMode`) pour envoyer les commandes EQ / DTS / Limiter / Noise Gate / AINR à la DLL APO propriétaire déjà installée sur la machine de l'utilisateur.
- **Avantages** : rendu strictement identique à NGENUITY (y compris DTS Headphone:X).
- **Limites** : dépend entièrement de la présence préalable du pilote audio/APO HyperX sur le PC (aucun binaire propriétaire HyperX ou DTS ne doit être redistribué dans OpenGear).

### Piste B — Réimplémenter un APO Windows clean-room 100 % OpenGear

- **Principe** : créer une DLL COM `MFX`/`EFX` Windows open-source (`crates/opengear-platform` + enregistrement admin lors de l'installation dans `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio` et redémarrage d'`Audiosrv`) capable d'injecter des traitements temps réel sur n'importe quel endpoint audio.
- **Ce qui est réimplémentable à l'identique** :
  - Égaliseur paramétrique / graphique 10 bandes (`32 Hz` à `16 kHz`, filtres biquad IIR),
  - Noise Gate et Limiter temps réel sur le flux microphone.
- **Ce qui nécessite une alternative libre** :
  - **DTS:X Spatial** (algorithme et profils propriétaires sous licence) → remplacer par un moteur binaural open-source (ex. convolution HRTF 7.1→stéréo via tables SOFA libres) ou bascule Windows Sonic.
  - **AINR** (modèle neuronal propriétaire) → remplacer par un débruiteur open-source léger tel que **RNNoise**.

## Contraintes d'architecture à respecter

- Conserver la frontière actuelle vérifiée par `crates/opengear-app/tests/architecture.rs` : toute logique Win32/COM/WASAPI reste dans `opengear-platform`, et les capacités spécifiques HyperX dans `opengear-hyperx`.
- Ne jamais embarquer ni redistribuer de DLL ou blob de poids propriétaires extraits de NGENUITY / DTS.
