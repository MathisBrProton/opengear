# Politique de sécurité

OpenGear écrit dans la mémoire flash de certains périphériques et pilote des ventilateurs et des
pompes. Une faille peut donc avoir des conséquences matérielles réelles. Nous prenons ces
signalements au sérieux.

## Versions prises en charge

| Version | Support de sécurité |
|---|---|
| 0.4.x (dernière) | Oui |
| < 0.4 | Non |

## Signaler une faille

**Ne publie pas la faille en issue publique.**

Utilise le signalement privé de GitHub :
**Security → Advisories → Report a vulnerability** sur la page du dépôt, ou directement :
<https://github.com/MathisBrProton/opengear/security/advisories/new>

Indique, si possible :

- la version d'OpenGear et la version de Windows ;
- les étapes pour reproduire le problème ;
- l'impact attendu (écriture flash, ventilateurs, élévation de privilèges, fuite de données…) ;
- si un appareil physique est impliqué, et si la faille a déjà touché un appareil.

Ne joins pas de données personnelles (noms d'utilisateur, chemins complets, identifiants).

## Ce qui se passe ensuite

- **Accusé de réception** : dans les 7 jours.
- **Évaluation** : le mainteneur confirme la faille ou demande des précisions.
- **Correction** : un correctif est préparé en privé, puis publié avec une note de version.
- **Crédit** : si tu le souhaites, ton nom est cité dans la note de version.

Ce projet est bénévole : il n'y a pas de programme de primes.

## Périmètre

Sont dans le périmètre : écritures flash, planchers de sécurité des ventilateurs, retour au BIOS,
bus SMBus, élévation de privilèges, téléchargement et installation (installeur, PawnIO), exécution
de code via l'interface WebView2.

Ne sont pas dans le périmètre : les failles de Windows, des pilotes tiers ou des logiciels
constructeurs, sauf si OpenGear les rend exploitables.
