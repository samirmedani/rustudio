# Contribuer à Rustudio

Merci ! Quelques règles pour que le projet reste fiable et sans données personnelles.

1. Lis [README.md](README.md) et [AGENTS.md](AGENTS.md) (règles du moteur). Pour un nouvel appareil :
   [docs/MODULES.md](docs/MODULES.md).
2. Active les garde-fous Git une fois : `scripts/install-hooks.sh`.
3. Code et teste **sans matériel** grâce au simulateur :
   ```bash
   cargo test
   cargo run -p rustudio-engine -- run --child --simulate --port 14566   # puis http://127.0.0.1:14566/
   ```
4. Avant une pull request : `scripts/check.sh` (tests + recherche de données personnelles).
5. Un comportement d'appareil nouveau = un test sur simulateur ; un constat fait sur un vrai appareil = le
   simulateur mis à jour pour le reproduire.
6. Une branche par sujet, une pull request relue. Commits signés `git commit -s`, première ligne courte.

**Jamais dans le dépôt** : nom, e-mail, chemin de machine (`C:\Users\…`, `/Users/…`), nom de PC, adresse IP du
réseau local, numéro de série d'appareil, `state.json`, journaux, enregistrements audio, binaires
(`.exe`, `.dll`, `.zip`). La vérification GitHub refuse ces fichiers et motifs.

Toute contribution communautaire suit [le circuit de validation](docs/COMMUNITY.md). Les créateurs sont
crédités par leur pseudo GitHub public ; utilise une adresse noreply pour tes commits publics.

Les textes de l'interface sont en français, avec le tutoiement.
