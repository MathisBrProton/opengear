# Notes de protocoles (interopérabilité)

Ce dossier rassemble des **faits** sur les protocoles de pilotage de l'éclairage de périphériques et
composants PC : identifiants USB, interfaces HID, formats de rapports, valeurs d'octets, séquences de
messages, délais, géométrie des LED et risques connus. Ils servent à écrire des pilotes OpenGear pour
des appareils que le projet ne prend pas encore en charge.

Les notes ont été établies par l'analyse d'OpenRGB (commit `f899bc8`, novembre 2026, dossier
`Controllers/`, licence GPL-2.0-or-later). **Aucun code n'en est repris** : ni source, ni
pseudo-code transcrit, ni commentaire copié, ni nom de fonction, de classe, de variable ou
d'énumération d'OpenRGB, ni découpage de fichiers calqué sur le sien. Seuls des faits fonctionnels
nécessaires à l'interopérabilité sont décrits, avec nos propres mots et sous forme de tableaux de
données.

## Règles « salle blanche »

Le travail est séparé en deux rôles :

1. **Rédacteur de spécification** : lit la source tierce et écrit ces fiches. Il ne transcrit que des
   faits : VID/PID, interface et usage HID, ID et longueur des rapports, type de transfert (feature,
   output, interrupt), offsets et signification des octets, valeurs de commandes, sommes de contrôle
   décrites mathématiquement, nombre de LED et correspondance index ↔ position (tableaux de données),
   ordre des couleurs, plages de luminosité/vitesse, séquences (initialisation, mode, couleurs,
   validation/sauvegarde, maintien), délais, catalogue d'effets, ce qui est écrit en flash ou non, et
   avertissements de sécurité. Quand le comportement n'est pas clair, il l'écrit (**(incertain)**)
   au lieu de deviner.
2. **Implémenteur** : écrit le pilote OpenGear **uniquement** à partir de ces fiches, sans jamais
   consulter la source tierce. Toute question se règle en complétant la fiche, pas en allant lire
   le code d'origine.

Les tableaux de touches indiquent les positions par code physique W3C (`KeyQ`, `IntlBackslash`…),
conformément à la règle d'OpenGear « une touche = une position physique » (AGENTS.md §2). Ils ont
été reconstitués à partir des données de disposition de la source, pas recopiés.

Rien de ce qui est décrit ici n'a été vérifié sur du matériel réel : chaque pilote doit commencer
par un chemin en lecture seule et respecter les garde-fous d'écriture d'AGENTS.md §3.

## Index

| Fichier | Contenu |
|---|---|
| [catalogue.md](catalogue.md) | Inventaire des 201 familles de pilotes : catégorie, marque, transport, VID, complexité, risques, couverture OpenGear |
| [razer.md](razer.md) | Razer : protocole générique 90 octets (claviers, souris, tapis, accessoires, portables Blade), contrôleur ARGB, casques Kraken / Kraken V3 / Kraken V4, AIO Hanbo ; 203 entrées d'appareils ; grilles de touches |
| [steelseries.md](steelseries.md) | SteelSeries : souris Aerox/Rival/Sensei, claviers Apex (G1–G3, M750, OG, Apex 3), casques Siberia/Arctis, tapis QCK ; codes de touche Apex |
| [hyperx.md](hyperx.md) | HyperX : claviers Alloy/Origins, souris Pulsefire, tapis, micros QuadCast ; tables d'emplacements des claviers |

Fiches à venir (autres lots) : voir la colonne « OpenGear » du catalogue.
