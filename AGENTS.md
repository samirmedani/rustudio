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
  l'utilisateur. Ne jamais affaiblir ces contrôles.
- La voix reste naturelle par défaut : aucun traitement, aucun retour micro ne s'active sans un choix explicite.

## Développement

- Rust stable, `cargo test` ; fausse GoXLR (`--simulate`) pour tout ce qui touche l'appareil.
- `scripts/check.sh` avant une pull request ; `scripts/leakscan.py` doit passer.
- Aucune donnée personnelle dans le dépôt (voir [CONTRIBUTING.md](CONTRIBUTING.md)).
- Ne jamais lancer GoXLR Utility pendant un test sur une vraie GoXLR.
- Mettre à jour [CHANGELOG.md](CHANGELOG.md) avec tout changement visible. Une release = un tag `vX.Y.Z` identique à
  `version` dans [Cargo.toml](Cargo.toml) : GitHub fabrique, teste l'installation et publie.
