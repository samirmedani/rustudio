# La communauté Rustudio

Rustudio est créé et maintenu par **[@samirmedani](https://github.com/samirmedani)**.
Les modules qu’il crée portent la mention **Officiel Rustudio · Créé par samirmedani**.
Un module proposé par un autre créateur reste **Communautaire · Créé par son pseudo GitHub**,
même après son intégration. Sa fiche donne accès à la pull request validée. Les licences et crédits
aux projets dont un protocole est dérivé restent conservés dans les mentions tierces.

## Proposer un module

1. Lis [Ajouter un appareil](MODULES.md), puis ouvre une issue pour présenter ton appareil.
2. Prépare le protocole, son simulateur, son adaptateur, son interface et ses tests dans une branche de ton fork.
3. Ouvre une pull request en brouillon pour obtenir son numéro. Dans le catalogue et le registre compilé,
   renseigne l’origine `community`, ton pseudo public `author`, la crate `source` et ce numéro `review_pr`.
4. Lance `scripts/check.sh`. Indique précisément les essais simulés, les essais physiques, le firmware,
   les limitations et les accès nécessaires. Ne joins aucune donnée de ta machine.
5. Demande la revue. Le mainteneur fait analyser **l’ensemble du changement**, demande les corrections utiles,
   puis valide explicitement la dernière révision. Une modification du code annule une ancienne approbation.
6. Après fusion et publication d’une version de Rustudio, le module apparaît dans l’application.
   L’installation de cette version demande toujours le clic de l’utilisateur.

Configure une adresse **GitHub noreply** avant ton premier commit public. Conserve le créateur d’origine
lors d’un correctif sur un module existant ; actualise `review_pr` si tu modifies sa crate.
Une correction peut citer ses contributeurs supplémentaires dans les notes de version et l’historique GitHub.

## Ce qui protège le catalogue

- Le catalogue est **embarqué dans la version publiée**. Il ne consulte plus le JSON de la branche `main`
  pour afficher de nouvelles fiches. Un changement distant non publié n’entre pas dans l’application.
- Seul le registre Rust compilé déclare un module disponible et son origine. Une fiche JSON ne peut pas
  se déclarer officielle, livrée ou approuvée. Une demande « À contribuer » ne fournit aucun code exécutable.
- `CODEOWNERS` désigne le mainteneur pour tous les fichiers, y compris les scripts de vérification et les workflows.
  La protection de `main` impose aux contributions une revue du propriétaire du code, les contrôles requis,
  la résolution des discussions et une nouvelle approbation après modification.
- Avant publication, `scripts/check-catalog.py --verify-github --public` vérifie pour chaque module communautaire :
  une PR fusionnée dans ce dépôt vers `main`, une approbation du mainteneur sur la dernière révision,
  et l’arbre Git exact de la crate déclarée. Une PR non fusionnée ou une approbation périmée bloque la release.
  Les tests d’une PR ouverte acceptent cette seule PR comme proposition en attente ; cela ne la publie pas.
- GitHub interdit l’auto-approbation. Pour une correction du mainteneur sur un module communautaire, sa propre PR
  doit être fusionnée par son compte : cette fusion constitue sa validation. Ses publications officielles restent
  possibles avec le contournement administrateur ; la vérification des preuves communautaires reste obligatoire
  dans la fabrication d’une release.
- Aucun module communautaire n’est téléchargé comme DLL, script ou plugin depuis une URL. Les mises à jour
  conservent les contrôles GitHub, SHA-256, taille, archive et retour arrière.

Une revue et des tests réduisent les risques ; ils ne garantissent pas l’absence de bugs. Les modules Rust
partagent les droits du moteur : **il n’existe pas de sandbox par module**. La comparaison automatique couvre
la crate déclarée ; les adaptateurs, l’interface, les dépendances et les workflows doivent aussi être analysés
lors de la revue complète. Un agent ne valide ni ne fusionne une contribution communautaire à la place du
mainteneur sans instruction explicite de sa part.

## Revue du mainteneur

Avant d’approuver, examiner le protocole et les limites de valeurs, les accès HID/USB, les reconnexions,
les délais et files de commandes, les dépendances et leurs licences. Vérifier aussi tout accès réseau,
aux fichiers, au registre Windows, tout processus lancé et toute modification des workflows.
Les commandes de firmware sont interdites dans les modules communautaires.

Exiger les tests sur simulateur, les preuves physiques annoncées et leurs limites, une interface utilisable
à petite fenêtre, et aucune donnée personnelle. L’analyse automatique doit rester un outil d’aide à cette
validation humaine ; une CI verte seule n’est pas une approbation.
