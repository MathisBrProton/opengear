# Contribuer à OpenGear

Merci de ton intérêt ! OpenGear pilote du matériel réel : chaque contribution, même petite, compte.
Ce document explique comment participer. Les règles techniques détaillées sont dans
[`AGENTS.md`](AGENTS.md), qui fait foi.

## Par où commencer

| Tu es… | Tu peux… |
|---|---|
| **Utilisateur avec un appareil** | Tester un pilote en **lecture seule** et ouvrir une issue « Support d'appareil » (c'est la contribution la plus utile) |
| **Développeur Rust** | Corriger un bug, améliorer l'interface, ajouter des tests (voir les issues `good first issue`) |
| **Rédacteur technique** | Compléter les fiches de `docs/protocols/` à partir de sources publiques, sans code |
| **Documentaliste** | Améliorer le README, traduire, clarifier les messages d'erreur |

## Avant de commencer

1. Ouvre une issue (ou commente une issue existante) pour dire ce que tu veux faire, surtout
   pour une fonctionnalité ou un nouveau pilote. Ça évite de travailler pour rien.
2. Fork le dépôt, puis crée une branche depuis `main` : `fix/…`, `feat/…`, `docs/…`.

## Mise en place

Prérequis : Windows 10/11 x64 et [rustup](https://rustup.rs). La toolchain (`1.99.1`) est
installée automatiquement via `rust-toolchain.toml`. Aucun Visual Studio ni MinGW n'est nécessaire.

```powershell
cargo build --release
cargo test --workspace --locked
```

Pour compiler et lancer la version du dépôt en un clic : `tools\build-and-run.ps1`.

## Avant d'ouvrir une PR

Ces trois commandes sont exactement celles de la CI. Si l'une échoue, la PR ne sera pas fusionnée :

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Règles d'architecture (obligatoires)

Le résumé ci-dessous ne remplace pas [`AGENTS.md`](AGENTS.md) :

- **Une marque = un crate.** Un crate marque ne dépend jamais d'un autre crate marque.
- **Rien de propre à une marque** dans `opengear-core`, `opengear-platform` ou `opengear-mainboard`.
- **`opengear-app` ne parle jamais au matériel** : pas de HID, WMI ni `DeviceIoControl` dedans.
- Les tests `crates/opengear-app/tests/architecture.rs` vérifient ces règles. Un test rouge
  signifie que le code est mal placé : ne pas assouplir le test.

## Matériel : sécurité d'abord

OpenGear écrit dans la mémoire flash de certaines souris et de certains claviers, et pilote des
ventilateurs. Une écriture ratée peut abîmer un appareil.

- **Lecture seule d'abord.** Toute nouvelle écriture matérielle doit avoir un chemin `probe`
  en lecture seule, utilisé pour le diagnostic.
- Garde-fous équivalents à ceux d'`AGENTS.md` §3 : sauvegarde, vérification, retour arrière,
  confirmation explicite.
- Un test unitaire couvre la logique de sécurité.
- Ce qui n'a pas été testé sur du vrai matériel doit être **dit** dans la PR (test `#[ignore]`
  et mention dans la description). Un pilote non testé est marqué `experimental()`.

## Clean-room : ne jamais copier de code tiers

OpenGear est écrit « en salle blanche » pour les protocoles :

- **Ne copie jamais** de code, de noms de fonctions, de tables ou de commentaires d'un autre
  projet, en particulier OpenRGB (GPL-2.0) ou des projets Razer, SteelSeries et HyperX.
- Les protocoles sont décrits dans `docs/protocols/` sous forme de **faits** (identifiants USB,
  formats de rapports, séquences). Si une fiche est incomplète, complète la fiche à partir de
  sources publiques et marque ce qui est incertain **(incertain)**.
- Dans la PR, indique d'où viennent les informations de protocole.

## Conventions

- **Commits** : style [Conventional Commits](https://www.conventionalcommits.org/fr/) :
  `feat:`, `fix:`, `docs:`, `refactor:`, `chore:`, `test:`.
- **Langue** : interface et documentation utilisateur en français ; code, identifiants et
  commentaires techniques en anglais.
- **Une PR = un sujet**, de préférence petite. Décris le pourquoi, pas seulement le quoi.
- La PR est fusionnée en **squash**. Les conversations doivent être résolues avant fusion.

## Revue et fusion

- Chaque PR doit passer la CI (`Build, test & lint (Windows)`) et être approuvée par le
  mainteneur. Les fichiers sensibles (matériel, CI, chaîne de build) sont réservés au mainteneur
  via [`.github/CODEOWNERS`](.github/CODEOWNERS).
- Les réponses peuvent prendre quelques jours : c'est un projet bénévole.

## Licence des contributions

OpenGear est distribué sous **MIT OR Apache-2.0**. En soumettant une contribution, tu acceptes
qu'elle soit distribuée sous ces deux licences (voir [`LICENSE-MIT`](LICENSE-MIT) et
[`LICENSE-APACHE`](LICENSE-APACHE)).

## Code de conduite et sécurité

- Participation respectueuse : voir [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
- Faille de sécurité : **ne la signale pas en issue publique**. Suis [`SECURITY.md`](SECURITY.md).
