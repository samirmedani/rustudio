# Sécurité

Pour signaler une faille, **n'ouvre pas d'issue publique**. Utilise « Report a vulnerability » dans l'onglet
Security du dépôt (signalement privé). Décris le problème, comment le reproduire et son impact.

Points sensibles de Rustudio :

- **Service sous le compte système** : le moteur écoute seulement sur `127.0.0.1:14565`. Les actions (`POST`) exigent
  l'en-tête `X-Rustudio: 1` et un hôte `127.0.0.1` ou `localhost`, ce qui bloque les requêtes venues d'une page web.
  L'appli n'appelle ses commandes (micro et sortie par défaut de Windows) que depuis cette page locale.
- **Mises à jour** : seule source acceptée, les releases GitHub de ce dépôt
  (`https://github.com/samirmedani/rustudio/releases/download/`), chaque redirection vérifiée vers les serveurs de
  fichiers de GitHub. Taille et empreinte SHA-256 comparées à celles annoncées par GitHub ; archive vérifiée
  (aucun chemin hors du dossier, aucun lien) ; retour arrière automatique si la nouvelle version ne répond pas ;
  rien ne s'installe sans le clic de l'utilisateur.
- **Retour à une version précédente** : seulement à la demande de l'utilisateur, confirmée dans l'historique des
  versions, vers une release publiée de ce dépôt (ni brouillon ni pré-version, 5.0.0 ou plus récente). La release est
  relue sur GitHub au moment du clic, et l'archive passe les mêmes contrôles qu'une mise à jour ; son manifeste doit
  annoncer exactement la version demandée. Une mise à jour ordinaire refuse toujours une archive plus ancienne.
  L'état des réglages est copié avant le retour (`C:\ProgramData\Rustudio\backups`).
- **Diagnostic** : le rapport « Copier le diagnostic » (`/api/support`) retire numéro de série, nom de PC,
  nom d'utilisateur, chemins personnels, numéros d'instance USB et adresses IP. Rustudio ne l'envoie jamais : c'est
  l'utilisateur qui le relit et le colle dans un ticket.
- **Aucune télémétrie** : les seuls contacts extérieurs sont la lecture de
  `api.github.com/repos/samirmedani/rustudio/releases/latest` (5 s après le démarrage, puis toutes les 6 heures), de la
  liste des releases quand tu ouvres l'historique des versions, de la liste de la bibliothèque
  (`raw.githubusercontent.com/samirmedani/rustudio/main/library/index.json`), et le téléchargement d'une version
  que tu as acceptée.

*To report a vulnerability, use "Report a vulnerability" in the Security tab. Please do not open a public issue.*
