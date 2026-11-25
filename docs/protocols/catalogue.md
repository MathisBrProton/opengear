# Catalogue des familles de pilotes (OpenRGB f899bc8, dossier `Controllers/`)

Une ligne par dossier (201). Inventaire établi en survolant les fichiers de détection et de
contrôle : il sert à prioriser, il ne remplace pas une fiche détaillée.

Légende :

- **Transport** : `HID` (hidapi : feature report, output report), `libusb` (transferts USB bruts),
  `SMBus` (I2C sur le bus SMBus de la carte mère ou d'un GPU), `série` (port COM / CDC-ACM),
  `réseau` (UDP/TCP/HTTP sur le LAN), `WMI`, `SuperIO` (ports d'E/S de la puce SuperIO), `autre`.
- **VID** : identifiant USB vendeur quand il est évident (bus USB uniquement ; pour les GPU sur SMBus,
  la détection se fait par identifiants PCI de sous-système, non listés ici).
- **Cx** : complexité approximative d'une réimplémentation (S : un appareil, quelques messages ;
  M : plusieurs modèles ou un protocole à état ; L : grande famille, nombreuses variantes).
- **Risques** : `flash` = le pilote peut écrire une configuration persistante ; `SMBus` = écritures
  sur un bus partagé avec la RAM/les capteurs (risque de brique si mauvaise adresse) ; `firmware` =
  le source signale des incohérences de firmware ; `—` = rien de particulier relevé.
- **OpenGear** : `oui` = famille déjà couverte nativement ; `partiel` = une partie seulement ;
  `non`.

| Dossier | Catégorie | Marque | Transport | VID | Cx | Risques | OpenGear |
|---|---|---|---|---|---|---|---|
| A4TechController | souris, clavier (Bloody) | A4Tech | HID feature | 09DA | S | — | non |
| AMBXController | lumières d'ambiance amBX | Philips | libusb | 0471 | S | — | non |
| AMDWraithPrismController | ventirad CPU | AMD (Cooler Master) | HID output | 2516 | M | — | non |
| AOCKeyboardController | clavier | AOC | HID feature + output | 3938 | S | — | non |
| AOCMouseController | souris | AOC | HID feature | 3938 | S | — | non |
| AOCMousematController | tapis | AOC | HID feature | 3938 | S | — | non |
| ASRockPolychromeUSBController | contrôleur RGB carte mère (USB) | ASRock | HID output | 26CE | M | flash (sauvegarde gérée par l'appareil) | non |
| ASRockSMBusController | contrôleur RGB carte mère | ASRock | SMBus + détection DMI | — | M | SMBus, flash | non |
| AcerNitroHidKeyboardController | clavier de portable | Acer | HID feature | 0CF2 | S | — | non |
| AlienwareController | contrôleur LED Dell G / Alienware | Dell | HID feature | 187C | M | — | non |
| AlienwareKeyboardController | claviers AW410K/AW510K | Dell/Alienware | HID feature + output | 04F2 | M | — | non |
| AlienwareMonitorController | moniteurs | Dell/Alienware | HID output | 187C | S | — | non |
| AnnePro2Controller | clavier 60 % | Obinslab | HID output | 04D9, 3311 | S | — | non |
| ArcticController | contrôleur ARGB | Arctic | série (CH340) | 1A86 | S | — | non |
| AresonController | souris (ZET Gaming) | Areson | HID feature | 25A7 | S | — | non |
| AsusAuraCoreController | claviers/barres de portables ROG | ASUS | HID feature | 0B05 | M | — | non |
| AsusAuraGPUController | GPU Aura | ASUS | SMBus | — | M | SMBus | non |
| AsusAuraUSBController | Aura USB : cartes mères, claviers, souris, tapis, casques, supports… | ASUS | HID feature + output | 0B05 | L | flash (certains modèles) | partiel (contrôleur carte mère 0B05:19AF) |
| AsusLegacyUSBController | anciens périphériques ROG | ASUS | HID feature + output | 0B05, 195D | M | flash | non |
| AsusMonitorController | moniteurs ROG | ASUS | HID output | 0B05 | S | — | non |
| AsusTUFLaptopController | clavier de portable TUF | ASUS | WMI (Windows) / sysfs | — | M | — | non |
| BlinkyTapeController | ruban LED | BlinkinLabs | série | 1D50 | S | — | non |
| CherryKeyboardController | claviers MX Board | Cherry | HID output | 046A | M | flash (géré par l'appareil) | non |
| ClevoKeyboardController | clavier de portable (ITE 829x) | Clevo | HID feature + output | 048D | M | — | non |
| ClevoLightbarController | barre lumineuse de portable | Clevo | HID feature | 048D | S | — | non |
| ColorfulGPUController | GPU | Colorful | SMBus | — | S | SMBus | non |
| ColorfulTuringGPUController | GPU (Turing) | Colorful | SMBus | — | S | SMBus, flash | non |
| CoolerMasterController | claviers, souris, tapis, hubs ARGB, GPU | Cooler Master | HID output | 2516 | L | flash | non |
| CorsairCommanderCoreController | Commander Core / Core XT | Corsair | HID output | 1B1C | M | mutex partagé avec iCUE | non |
| CorsairController | utilitaire (mutex inter-logiciels `Global\CorsairLinkReadWriteGuardMutex`) | Corsair | — | — | S | — | partiel (mutex déjà côté plateforme) |
| CorsairDRAMController | RAM Vengeance/Dominator | Corsair | SMBus | — | M | SMBus | non |
| CorsairHydro2Controller | AIO H100i v2 | Corsair | libusb | 1B1C | S | — | non |
| CorsairHydroController | AIO Hydro (Asetek) | Corsair | libusb | 1B1C | M | — | non |
| CorsairHydroPlatinumController | AIO Platinum / Pro XT | Corsair | HID output | 1B1C | M | mutex iCUE | non |
| CorsairICueLinkController | hub iCUE Link | Corsair | HID output | 1B1C | M | mutex iCUE | non |
| CorsairLightingNodeController | Lighting Node, Commander Pro, LS100 | Corsair | HID output | 1B1C | M | flash (mode matériel), mutex iCUE | non |
| CorsairPeripheralController | claviers/souris/tapis/supports Corsair ancienne génération | Corsair | HID feature + output | 1B1C | L | — | non |
| CorsairPeripheralV2Controller | claviers/souris Corsair génération « Bragi » | Corsair | HID output | 1B1C | L | — | oui (Bragi v2 claviers/souris/casques) |
| CorsairVengeanceController | RAM Vengeance RGB (ancienne) | Corsair | SMBus | — | S | SMBus | non |
| CorsairVoidController | casques Void Pro | Corsair | HID output | 1B1C | S | — | partiel (casques Bragi seulement) |
| CougarController | clavier 700K EVO, souris Revenger ST | Cougar | HID feature + output | 060B, 12CF | M | flash | non |
| CreativeController | cartes son AE-5, G6 | Creative | HID output | 041E | M | — | non |
| CrucialController | RAM Ballistix | Crucial | SMBus | — | M | SMBus | non |
| CryorigH7QuadLumiController | ventirad (puce NZXT) | Cryorig | HID output | 1E71 | S | — | non |
| DDPController | protocole DDP (rubans réseau) | générique | réseau UDP | — | S | — | non |
| DMXController | éclairage DMX | générique | série | — | S | — | non |
| DRGBController | contrôleurs ARGB DeepRGB | DeepRGB | HID output | 1368, 2023, 2486 | M | — | non |
| DarkProject | clavier KD3B V2 | Dark Project | HID output | 195D | S | — | non |
| DasKeyboardController | claviers 4Q/5Q | Das Keyboard | HID feature | 24F0 | M | — | non |
| DebugController | faux appareils de test | — | aucun | — | — | — | sans objet |
| DreamCheekyController | notificateur e-mail USB | Dream Cheeky | HID output | 1D34 | S | — | non |
| DuckyKeyboardController | claviers Shine 7 / One 2 | Ducky | HID output | 04D9 | M | — | non |
| DygmaRaiseController | clavier Raise | Dygma | série | 1209 | S | — | non |
| E131Controller | protocole E1.31 (sACN) | générique | réseau UDP | — | S | — | non |
| EKController | Loop Connect | EK | HID output | 0483 | S | flash | non |
| ENESMBusController | RAM, GPU, SSD et cartes mères à puce ENE | ENE (Aura, XPG…) | SMBus | — | L | SMBus, flash | partiel (RAM ENE) |
| EVGAAmpereGPUController | GPU RTX 30 | EVGA | SMBus | — | M | SMBus, flash | non |
| EVGAGP102GPUController | GPU GP102 | EVGA | SMBus | — | S | SMBus | non |
| EVGAPascalGPUController | GPU Pascal | EVGA | SMBus | — | S | SMBus, flash | non |
| EVGASMBusController | carte mère (ACX 3.0) | EVGA | SMBus | — | S | SMBus, flash | non |
| EVGATuringGPUController | GPU RTX 20 | EVGA | SMBus | — | M | SMBus, flash | non |
| EVGAUSBController | claviers Z15/Z20, souris X20 | EVGA | HID feature | 3842 | M | — | non |
| EVisionKeyboardController | claviers à puce EVision (marques blanches) | EVision | HID output | 0C45, 320F, 3299 | M | flash | non |
| EdifierHaloController | barre de son | Edifier | HID output | 2D99 | S | flash (géré par l'appareil) | non |
| ElgatoKeyLightController | Key Light | Elgato | réseau HTTP | — | S | — | non |
| ElgatoLightStripController | Light Strip | Elgato | réseau HTTP | — | S | — | non |
| EspurnaController | firmware Espurna | générique | réseau | — | S | — | non |
| FanBusController | FanBus | générique | série | — | S | — | non |
| FaustusController | clavier ASUS via module noyau | ASUS | sysfs Linux | — | S | — | sans objet (Linux) |
| FnaticStreakController | claviers Streak | Fnatic | HID output | 2F0E | M | flash | non |
| FrameworkDesktopController | Framework Desktop | Framework | EC (Linux) | — | S | — | sans objet (Linux) |
| GaiZongGaiKeyboardController | claviers/pavés GaiZhongGai | GaiZhongGai | HID output | 3061 | M | — | non |
| GalaxGPUController | GPU | Galax/KFA2 | SMBus | — | M | SMBus | non |
| GameSirController | manette Nova 2 Lite | GameSir | HID output | 3537 | S | — | non |
| GigabyteAorusCPUCoolerController | ventirad ATC800 | Gigabyte | HID feature | 1044 | S | — | non |
| GigabyteAorusLaptopController | claviers de portables Aorus | Gigabyte | HID feature + output | 0414 | M | flash | non |
| GigabyteAorusMouseController | souris M2 | Gigabyte | HID feature | 1044 | S | — | non |
| GigabyteAorusPCCaseController | boîtier C300 | Gigabyte | HID feature | 1044 | S | flash | non |
| GigabyteCastor3Controller | AIO Waterforce X II | Gigabyte | HID output | 0414 | M | — | non |
| GigabyteRGBFusion2BlackwellGPUController | GPU RTX 50 | Gigabyte | SMBus | — | M | SMBus, flash | non |
| GigabyteRGBFusion2DRAMController | RAM Aorus | Gigabyte | SMBus | — | S | SMBus | non |
| GigabyteRGBFusion2GPUController | GPU Fusion 2 | Gigabyte | SMBus | — | M | SMBus, flash | non |
| GigabyteRGBFusion2SMBusController | carte mère Fusion 2 (SMBus) | Gigabyte | SMBus | — | M | SMBus | non |
| GigabyteRGBFusion2USBController | carte mère Fusion 2 (USB ITE) | Gigabyte | HID feature | 048D, 0414 | L | — | non |
| GigabyteRGBFusionController | carte mère Fusion 1 | Gigabyte | SMBus | — | S | SMBus | non |
| GigabyteRGBFusionGPUController | GPU Fusion 1 | Gigabyte | SMBus | — | S | SMBus, flash | non |
| GigabyteSuperIORGBController | carte mère (SuperIO) | Gigabyte | SuperIO | — | S | ports E/S | non |
| GoveeController | lampes Govee | Govee | réseau UDP | — | M | — | non |
| HIDLampArrayController | standard HID LampArray | générique | HID feature | — | M | — | non |
| HPOmen30LController | PC de bureau Omen 30L | HP | HID output | 103C | S | — | non |
| HPOmenLaptopController | clavier 4 zones de portable Omen | HP | WMI | — | S | — | oui |
| HYTEKeyboardController | clavier Keeb TKL | HYTE | HID feature + output | 3402 | M | — | non |
| HYTEMousematController | tapis CNVS | HYTE | série / libusb | 3402 | S | — | non |
| HYTENexusController | boîtiers Y60/Nexus | HYTE | série | 3402 | M | — | non |
| HoltekController | souris et tapis à puce Holtek | Holtek | HID feature | 04D9 | S | — | non |
| HyperXDRAMController | RAM Fury RGB | HyperX | SMBus | — | M | SMBus | non |
| HyperXKeyboardController | claviers Alloy / Origins | HyperX (Kingston, HP) | HID feature + output | 0951, 03F0 | L | flash (profil Alloy Elite) | non |
| HyperXMicrophoneController | micros QuadCast S / DuoCast | HyperX | HID feature | 0951, 03F0 | S | flash (sauvegarde) | non |
| HyperXMicrophoneV2Controller | micro QuadCast 2 S | HyperX | HID output | 03F0 | M | flash (sauvegarde) | non |
| HyperXMouseController | souris Pulsefire | HyperX | HID feature + output | 0951, 03F0 | M | flash (Dart) | non |
| HyperXMousematController | tapis Pulsefire / Fury | HyperX | HID feature | 0951, 03F0 | S | — | non |
| InstantMouseController | souris « Instant » (marques blanches) | Instant | HID feature | 1440, 1540, 30FA | S | — | non |
| IntelArcA770LEController | GPU Arc A770 LE | Intel | HID output | 2516 | S | flash | non |
| IonicoController | clavier/barre de portable | Ionico | HID feature + output | 048D | S | flash | non |
| JGINYUEInternalUSBController | contrôleur ARGB interne de carte mère | JGINYUE | HID feature + output | 0416 | M | — | non |
| JGINYUEInternalUSBV2Controller | idem, V2 | JGINYUE | HID output / série | 1A86 | M | — | non |
| KasaSmartController | prises/ampoules Kasa | TP-Link | réseau | — | S | — | non |
| KeychronKeyboardController | claviers Keychron (firmware propriétaire) | Keychron | HID feature | 05AC | M | — | non |
| KingstonFuryDRAMController | RAM Fury DDR4/DDR5 | Kingston | SMBus | — | M | SMBus | non |
| KlipschLuminaController | enceintes ProMedia Lumina | Klipsch | HID / libusb | 18B5 | S | — | non |
| LEDStripController | rubans DIY (Arduino, ESP) | générique | série / réseau / I2C | — | S | — | non |
| LGMonitorController | moniteurs UltraGear | LG | HID output | 043E | S | flash | non |
| LIFXController | ampoules LIFX | LIFX | réseau UDP | — | S | — | non |
| LaviewTechnologyController | souris Glorious Model I | Laview | HID feature | 22D4 | S | flash | non |
| LegoDimensionsToypadBaseController | socle Lego Dimensions | Lego | HID output | 0E6F | S | — | non |
| LenovoControllers | claviers de portables Legion/Ideapad, souris | Lenovo | HID feature + output | 048D, 17EF | L | flash | non |
| LenovoMotherboardController | carte mère Lenovo | Lenovo | HID feature | 17EF | S | — | non |
| LexipMouseController | souris Np93 | Lexip | HID output | 04D8 | S | — | non |
| LianLiController | Uni Hub, écrans, ventilateurs | Lian Li | HID output / libusb | 0416, 0CF2 | L | — | non |
| LightSaltController | périphériques LightSalt | LightSalt | HID output | 0483 | M | — | non |
| LinuxLEDController | LED sysfs génériques | — | sysfs Linux | — | S | — | sans objet (Linux) |
| LogitechController | souris, claviers, casques, tapis | Logitech | HID feature + output | 046D, 0738 | L | flash | partiel (HID++ 2.0 souris/claviers/casque ; anciens protocoles non) |
| LuxaforController | voyant Flag | Luxafor | HID output | 04D8 | S | — | non |
| MNTKeyboardController | claviers MNT Reform | MNT | HID output | 1209 | S | — | non |
| MSI3ZoneController | clavier de portable 3 zones | MSI (SteelSeries) | HID feature | 1770 | S | — | non |
| MSIClutchController | souris Clutch GM11 | MSI | HID feature + output | 0DB0 | S | flash | non |
| MSIGPUController | GPU | MSI | SMBus | — | M | SMBus, flash | non |
| MSIKeyboardController | claviers/portables Mystic Light | MSI | HID feature | 1462 | M | flash | non |
| MSILaptopController | clavier/barre de portable (puce SteelSeries) | MSI | HID feature + output | 1038 | M | — | non |
| MSIMonitorController | moniteurs | MSI | HID feature + output | 1462 | M | flash | non |
| MSIMotherboardController | cartes mères Mystic Light (USB) | MSI | HID feature + output | 1462, 0DB0 | L | flash | non |
| MSIOptixController | moniteurs Optix | MSI | HID feature | 1462 | S | — | non |
| MSIRGBController | anciennes cartes mères (SuperIO) | MSI | SuperIO | — | S | ports E/S | non |
| MSIVigorController | clavier Vigor GK30 | MSI | HID feature | 0DB0 | S | flash | non |
| MadCatzCyborgController | lampe Cyborg Gaming Light | Mad Catz | HID feature | 06A3 | S | — | non |
| ManliGPUController | GPU | Manli | SMBus | — | S | SMBus, flash | non |
| MintakaKeyboardController | clavier VSG Mintaka | VSG | HID feature | 05AC | S | — | non |
| MountainKeyboardController | claviers Everest | Mountain | HID feature + output | 3282 | M | flash | non |
| N5312AController | souris N5312A | — | HID feature | 4E53 | S | — | non |
| NVIDIAIlluminationController | GPU Founders Edition | NVIDIA | NvAPI | — | M | — | non |
| NZXTHue1Controller | Smart Device V1 | NZXT | HID output | 1E71 | S | — | non |
| NZXTHue2Controller | Hue 2, RGB & Fan Controller | NZXT | HID output | 1E71 | M | flash | non |
| NZXTHuePlusController | Hue+ | NZXT | série | 04D8 | S | flash | non |
| NZXTKrakenController | AIO Kraken X/M | NZXT | HID output | 1E71 | M | — | non |
| NZXTMouseController | souris Lift | NZXT | HID output | 1E71 | S | — | non |
| NanoleafController | panneaux Nanoleaf | Nanoleaf | réseau | — | S | — | non |
| NollieController | contrôleurs ARGB Nollie | Nollie | HID output | 16D2, 16D5, 3061 | M | — | non |
| NvidiaESAController | châssis Dell XPS 730x | NVIDIA ESA | HID output | 0955 | S | — | non |
| OKSController | clavier optique | OKS | HID output | 1C4F | S | — | non |
| PNYARGBEpicXGPUController | GPU ARGB Epic-X | PNY | SMBus | — | S | SMBus | non |
| PNYLovelaceGPUController | GPU RTX 40 | PNY | SMBus | — | S | SMBus | non |
| PalitGPUController | GPU | Palit | SMBus | — | M | SMBus | non |
| PatriotViperController | RAM Viper RGB | Patriot | SMBus | — | M | SMBus | non |
| PatriotViperMouseController | souris V550 | Patriot | HID feature | 0C45 | S | — | non |
| PatriotViperSteelController | RAM Viper Steel | Patriot | SMBus | — | S | SMBus | non |
| PhilipsHueController | ponts Hue (dont Entertainment) | Philips | réseau HTTP/DTLS | — | M | — | non |
| PhilipsWizController | ampoules WiZ | Philips | réseau UDP | — | S | — | non |
| PowerColorGPUController | GPU | PowerColor | SMBus | — | M | SMBus | non |
| QMKController | claviers QMK/VIA/Vial/ZMK | communauté | HID output | 20A0, 2E8A, 3434… | L | flash (EEPROM) | non |
| RazerController | claviers, souris, tapis, casques, accessoires, portables | Razer | HID feature (+ output pour casques) | 1532 (068E BT) | L | flash possible (octet de stockage) | non — voir `razer.md` |
| RealtekARGBController | pont ARGB RTL9209 | Realtek | HID feature | 0BDA | S | — | non |
| RealtekBridgeController | ponts NVMe/USB RTL92xx (boîtiers SSD) | Realtek | HID feature | 0BDA | M | — | non |
| RedSquareKeyroxController | claviers Keyrox TKL | Red Square | HID feature | 1A2C | M | — | non |
| RedragonController | souris | Redragon | HID feature | 04D9 | M | flash | non |
| RetroidAYNController | consoles portables | Retroid/AYN | sysfs Linux | — | S | — | sans objet (Linux) |
| RobobloqLightStripController | ruban de moniteur | Robobloq | HID output | 1A86 | S | flash | non |
| RoccatController | claviers, souris, tapis, casques | Roccat / Turtle Beach | HID feature + output | 1E7D, 10F5 | L | flash | non |
| RoyuanKeyboardController | claviers Akko/Epomaker/Skyloong (puce Royuan) | Royuan | HID feature | 3151 | M | flash | non |
| SRGBmodsController | contrôleurs LED SRGBmods | SRGBmods | HID output | 16D0 | S | — | non |
| SapphireGPUController | GPU Nitro+ | Sapphire | SMBus | — | M | SMBus | non |
| SayoDeviceController | pavés SayoDevice | SayoDevice | HID output | 8089 | S | flash | non |
| SeagateController | disque FireCuda | Seagate | SCSI pass-through | — | S | flash | non |
| SinowealthController | souris/claviers à puce Sinowealth (Glorious…) | Sinowealth | HID feature | 258A | L | flash | non |
| SkydimoHIDController | ruban Skydimo (HID) | Skydimo | HID output | 1A86 | S | — | non |
| SkydimoSerialController | ruban Skydimo (série) | Skydimo | série | 1A86 | S | — | non |
| SkyloongController | clavier GK104 Pro | Skyloong | HID output | 1EA7 | S | — | non |
| SonyGamepadController | DualShock 4, DualSense | Sony | HID output | 054C | M | — | non |
| SteelSeriesController | souris, claviers, casques, tapis | SteelSeries | HID feature + output | 1038 | L | flash (sauvegarde explicite ; Rival 5 automatique) | non — voir `steelseries.md` |
| StreamDeckController | Stream Deck MK.2 (luminosité) | Elgato | HID feature | 0FD9 | S | — | non |
| TForceXtreemController | RAM Xtreem | TeamGroup | SMBus | — | M | SMBus, flash | non |
| TecknetController | souris M008 | Tecknet | HID feature | 04D9 | S | — | non |
| ThermaltakePoseidonZRGBController | clavier Poseidon Z | Thermaltake | HID feature | 264A | S | — | non |
| ThermaltakeRiingController | contrôleurs de ventilateurs Riing | Thermaltake | HID output | 264A | M | — | non |
| ThingMController | voyant blink(1) | ThingM | HID feature | 27B8 | S | — | non |
| ThrustmasterSolController | manette Sol | Thrustmaster | libusb | 044F | M | flash | non |
| TrustController | souris GXT | Trust | HID feature | 145F | S | — | non |
| TurtleBeachKP7Controller | pavé KP7 | Turtle Beach | HID feature + output | 10F5 | S | — | non |
| ValkyrieKeyboardController | claviers VK99 | Valkyrie | HID feature | 05AC | S | — | non |
| ValveSteamMachineController | LED de la Steam Machine | Valve | I2C (PawnIO) / sysfs | — | M | bus I2C | non |
| ViewSonicController | moniteurs XG | ViewSonic | HID feature | 0416, 0543 | S | — | non |
| WinbondGamingKeyboardController | claviers à puce Winbond | Winbond | HID output | 0416 | M | flash | non |
| WitmodKeyboardController | claviers Witmod | Witmod | HID output | 0416 | S | flash | non |
| WootingKeyboardController | claviers Wooting | Wooting | HID feature + output | 03EB, 31E3 | M | — | non |
| WushiController | station d'accueil JSAUX | JSAUX | HID feature | 306F | S | flash | non |
| XPGSummonerKeyboardController | clavier Summoner | XPG | HID output | 125F | S | — | non |
| YeelightController | ampoules Yeelight | Yeelight | réseau TCP | — | S | — | non |
| ZETKeyboardController | clavier Blade Optical | ZET | HID output | 2EA8 | S | — | non |
| ZalmanZSyncController | hub ARGB Z Sync | Zalman | HID output | 1C57 | S | — | non |
| ZotacBlackwellGPUController | GPU RTX 50 | Zotac | SMBus | — | M | SMBus, flash | non |
| ZotacTuringGPUController | GPU RTX 20 | Zotac | SMBus | — | S | SMBus, flash | non |
| ZotacV2GPUController | GPU (V2) | Zotac | SMBus | — | M | SMBus, flash | non |

## Remarques transverses

- **Mutex inter-logiciels** : OpenRGB prend un mutex Windows nommé avant chaque écriture sur
  certaines familles pour ne pas entrer en collision avec le logiciel du fabricant :
  `Global\CorsairLinkReadWriteGuardMutex` (Commander Core, Hydro Platinum, iCUE Link, Lighting Node)
  et `Global\RazerLinkReadWriteGuardMutex` (tous les appareils Razer du pilote générique et l'ARGB).
- **VID partagés** : 0x1038 (SteelSeries) sert aussi aux claviers de portables MSI ; 0x2516 (Cooler
  Master) aussi à l'AMD Wraith Prism et à l'Intel Arc A770 LE ; 0x04D9 (Holtek) est une puce
  générique présente chez plusieurs marques ; 0x0416 (Nuvoton/Winbond) et 0x1A86 (WCH, CH340) idem.
  Le VID seul ne suffit donc jamais : il faut le couple VID:PID et souvent l'interface/usage HID.
- **Détections sans USB** : les familles `SMBus`, `SuperIO`, `WMI`, `NvAPI`, `sysfs` et `réseau`
  demandent des garde-fous spécifiques (cf. AGENTS.md §3) avant toute écriture.
