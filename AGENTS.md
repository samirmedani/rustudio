# Rustudio — consignes pour contribuer (humains et agents)

Logiciel libre, écrit en Rust, qui pilote le matériel du bureau sous Windows (premier appareil : GoXLR Mini, avec
le pilote officiel). Lire [README.md](README.md) avant de toucher au moteur, [docs/MODULES.md](docs/MODULES.md)
pour ajouter un appareil.

## Règles du moteur (`crates/engine`, `crates/goxlr`)

- `TUSBAUDIO_EnumerateDevices()` invalide **tous** les handles ouverts : seulement handle fermé. Le port HTTP
  est pris avant l'USB, pour qu'un second moteur s'arrête sans toucher au pilote.
- Index de commande sur 16 bits : remise à zéro (commande 0) avant 65 535. La Mini répond avec son propre
  compteur ; un décalage se rattrape par une resynchronisation.
- Un seul fil parle à l'USB. L'API ne touche jamais l'appareil : elle modifie l'état et marque ce qui doit partir.
- Toute erreur pendant un échange USB (panique comprise) ferme le handle et relance une connexion propre qui
  réapplique l'état. Le superviseur du service relance le moteur s'il meurt ou se fige.
- Mises à jour : uniquement depuis les releases GitHub du dépôt, chaque redirection chez GitHub, taille et SHA-256
  vérifiées, archive contrôlée, retour arrière si la nouvelle version ne répond pas, et jamais sans le clic de
  l'utilisateur. Ne jamais affaiblir ces contrôles. Retour à une version précédente : seulement sur demande confirmée,
  vers une release publiée (5.0.0 ou plus récente), mêmes contrôles et manifeste égal à la version demandée.
- La voix reste naturelle par défaut : aucun traitement, aucun retour micro ne s'active sans un choix explicite.

## Développement

- Rust stable, `cargo test` ; fausse GoXLR (`--simulate`) pour tout ce qui touche l'appareil.
- `scripts/check.sh` avant une pull request ; `scripts/leakscan.py` doit passer.
- Aucune donnée personnelle dans le dépôt (voir [CONTRIBUTING.md](CONTRIBUTING.md)).
- Ne jamais lancer GoXLR Utility pendant un test sur une vraie GoXLR.
- Mettre à jour [CHANGELOG.md](CHANGELOG.md) avec tout changement visible. Une release = un tag `vX.Y.Z` identique à
  `version` dans [Cargo.toml](Cargo.toml) : GitHub fabrique, teste l'installation et publie.

## Publier une version (même processus que Wispad)

- Section du CHANGELOG : `## X.Y.Z — 13 novembre 2026`, une phrase d'accroche (« Rustudio X.Y.Z … »), puis les
  rubriques `**Nouveau**`, `**Amélioré**`, `**Corrigé**` (seulement celles qui servent), chaque point sous la forme
  `- **Titre court** : ce qui change pour l'utilisateur`. Tutoiement, sans jargon ni détail interne ; exemples inventés.
- Ces notes sont publiées telles quelles sur GitHub (`scripts/release_notes.py`) et affichées par l'appli : fenêtre
  de mise à jour avant l'installation, puis « Rustudio X.Y.Z est installé. » après le redémarrage. Comportement :
  [docs/MISES-A-JOUR.md](docs/MISES-A-JOUR.md), à tenir à jour dans le même commit qu'un changement de ce comportement.
- Rien ne s'installe sans le clic « Mettre à jour » de l'utilisateur, après lecture des notes.
