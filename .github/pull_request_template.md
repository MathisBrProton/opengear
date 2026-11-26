## Résumé

<!-- Quoi et pourquoi, en 2-3 lignes. -->

## Type de changement

- [ ] Correction de bug
- [ ] Nouvelle fonctionnalité
- [ ] Nouveau pilote ou nouvel appareil (**expérimental** tant qu'il n'est pas testé sur du vrai matériel)
- [ ] Documentation
- [ ] Refactor, CI ou chaîne de build

## Vérifications (AGENTS.md §4)

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`

## Matériel (si concerné)

- [ ] Pas d'écriture matérielle, ou : lecture préalable, garde-fous d'`AGENTS.md` §3, chemin `probe` en lecture seule, test unitaire de la sécurité
- [ ] Testé sur matériel réel : appareil, firmware, système :
- [ ] Non testé sur matériel réel : indiqué dans la description, pilote marqué `experimental()`

## Clean-room (si protocole)

- [ ] Aucun code, nom de fonction ou table copié d'un projet tiers
- [ ] Source des informations de protocole indiquée : 

## Notes pour le relecteur

<!-- Points à regarder en priorité, choix discutables, ce qui reste à faire. -->
