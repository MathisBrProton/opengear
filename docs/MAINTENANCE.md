# Maintenance du dépôt

Ce document décrit ce que le mainteneur doit faire pour garder le dépôt sûr et publier une release.
Il est destiné au mainteneur, pas aux contributeurs (voir [`CONTRIBUTING.md`](../CONTRIBUTING.md)).

## 1. Réglages GitHub à conserver

| Réglage | Où | Valeur attendue |
|---|---|---|
| Double authentification | github.com → Settings → Password and authentication | Activée, de préférence avec une clé matérielle |
| Secret scanning + push protection | Settings → Code security | Activés |
| Dependabot alerts et security updates | Settings → Code security | Activés |
| Ruleset `protect-main` | Settings → Rules → Rulesets | Actif : PR obligatoire, 1 approbation, CI `Build, test & lint (Windows)`, revue Code Owners |
| Ruleset `protect-release-tags` | Settings → Rules → Rulesets | Actif sur `v*` : création et suppression réservées aux admins |
| Approbation des workflows de fork | Settings → Actions → General | « Require approval for all outside collaborators » |
| Permissions par défaut du `GITHUB_TOKEN` | Settings → Actions → General | Lecture seule |
| Collaborateurs | Settings → Collaborators | Permission de base « Read ». Aucun admin en plus du mainteneur |
| E-mails | github.com → Settings → Emails | « Keep my email addresses private » et « Block command line pushes that expose my email » activés |

Vérification rapide avec `gh` :

```powershell
gh api repos/MathisBrProton/opengear --jq '.security_and_analysis'
gh api repos/MathisBrProton/opengear/rulesets --jq '.[] | "\(.name) \(.enforcement)"'
gh api repos/MathisBrProton/opengear/actions/permissions/fork-pr-contributor-approval
```

## 2. Traiter les PR et les issues

**Chaque semaine**

- Trier les nouvelles issues avec les labels `bug`, `device-support`, `hardware-needed`, `enhancement`.
- Vérifier les alertes Dependabot et les PR de mise à jour (`cargo` et `github-actions`).
- Répondre aux contributeurs sous 48 h, même pour dire « je regarde ».

**Pour chaque PR**

- Vérifier que la CI est verte.
- Relire les zones sensibles (matériel, CI, chaîne de build) : elles passent par `CODEOWNERS`.
- Vérifier la règle clean-room : aucun code copié d'un projet tiers.
- Fusionner en **squash** une fois la conversation résolue. Le mainteneur peut fusionner ses propres PR via le rôle admin, mais seulement après relecture.

**Ne jamais**

- Passer `pull_request_target` dans un workflow sans une revue très stricte : ce déclencheur donne accès aux secrets à du code non fiable.
- Ajouter un secret dans le dépôt pour un test, puis le retirer : il reste dans l'historique.

## 3. Publier une release

La release est créée en **brouillon** : rien n'est publié tant que tu ne l'as pas validée.

1. Sur `main`, vérifie que la CI est verte et que `Cargo.toml` contient la bonne version (`version = "X.Y.Z"`).
2. En local, lance les contrôles de la CI :
   ```powershell
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --locked -- -D warnings
   cargo test --workspace --locked
   ```
3. Crée et pousse le tag (le ruleset `protect-release-tags` autorise seulement les admins) :
   ```powershell
   git tag -a vX.Y.Z -m "OpenGear X.Y.Z"
   git push origin vX.Y.Z
   ```
4. Le workflow `Release` (`.github/workflows/release.yml`) :
   - vérifie que le tag correspond à `Cargo.toml` ;
   - lance les tests, compile l'application et l'installeur ;
   - crée une release **brouillon** avec `OpenGear-Setup.exe`, `OpenGear-portable.zip` et `SHA256SUMS.txt`.
5. Vérifie le brouillon : téléchargement, somme SHA-256, notes de version. Puis publie-le :
   ```powershell
   gh release edit vX.Y.Z --draft=false
   ```
   ou depuis l'interface GitHub (Releases → Edit → Publish).

**Tester le workflow sans publier** : `Actions → Release → Run workflow` (déclenchement manuel).
Il construit les artefacts et les garde 90 jours, sans créer de release.

## 4. Plan d'incident

**Secret publié par erreur**

1. Révoque ou régénère la clé **immédiatement**, chez le fournisseur.
2. Retire le secret du code et ouvre une PR.
3. Seulement ensuite, décide si tu nettoies l'historique. Un secret retiré du code reste lisible
   dans l'historique : la révocation est la seule vraie protection.

**PR malveillante ou spam**

1. Ferme la PR et, si besoin, bloque le compte (Settings → Moderation → Block user).
2. Vérifie qu'aucun workflow ne s'est exécuté avec des droits élevés (Actions → logs).

**Faille de sécurité signalée**

1. Traite-la en privé : Security → Advisories → la faille signalée.
2. Crée une correction dans un fork privé, puis publie l'advisory avec la version corrigée.
3. Publie une release de correction et mentionne-la dans les notes.

**Faille dans la flash, les ventilateurs ou le SMBus** : traite-la avec la même procédure, et
préviens les utilisateurs concernés dans la note de version avec les consignes de restauration
(`Système → Sauvegardes`).

## 5. Ce qui reste à faire à la main

- Vérifier la confidentialité des e-mails sur les commits existants : les commits publics affichent
  encore leurs adresses. Réécrire l'historique est destructif : à n'envisager que si tu le décides
  explicitement, et en prévenant les forks.
- Tester sur matériel réel chaque pilote marqué `experimental()`, et mettre à jour la matrice de
  compatibilité du README.
