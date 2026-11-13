# Mises à jour : notes, installation, confirmation

Rustudio présente une mise à jour comme Wispad : on lit d'abord ce qui change, l'installation se suit étape par
étape, et la nouvelle version confirme qu'elle est en place.

## Notes de version

Chaque version du [CHANGELOG](../CHANGELOG.md) commence par `## X.Y.Z — date`, une phrase d'accroche, puis les
rubriques **Nouveau**, **Amélioré** et **Corrigé**, avec des points « **Titre** : détail ». La release GitHub reprend
ces notes telles quelles. L'appli les lit avec `rustudio_core::notes` : accroche, rubriques en couleur (Nouveau en
bleu, Amélioré en vert, Corrigé en orange), titre en gras et détail dessous. Les anciennes notes, sans rubrique,
restent lisibles en une seule liste. Les citations et l'empreinte SHA-256 ne s'affichent que sur GitHub.

## Fenêtre de mise à jour

- **Ouverture** : le bouton en bas à gauche, « Voir les nouveautés » ou « Mettre à jour vers la X » (page Versions).
  Sans version connue, le bouton cherche tout de suite ; la fenêtre s'ouvre si une version est trouvée, sinon un
  message dit « Rustudio est à jour. », « Recherche impossible : vérifie ta connexion, puis réessaie. » ou « GitHub
  limite les recherches : réessaie plus tard. ».
- **En-tête** : « Rustudio X.Y.Z », l'étiquette « Nouvelle version », « Tu as la A.B.C » et, s'il y en a plusieurs,
  le nombre de versions à rattraper. L'accroche de la dernière version sert de titre.
- **Notes** : une carte par version à rattraper, de la plus récente à la plus ancienne (30 au plus, lues sur GitHub
  à l'ouverture ; sans réponse, les notes déjà composées par le moteur). Les trois plus récentes sont visibles,
  les autres derrière « Afficher les N versions précédentes ».
- **Pied** : ce que fait le clic (« Rustudio télécharge cette version (6,6 Mo) depuis GitHub, vérifie sa taille et son
  empreinte, puis redémarre tout seul. Tes réglages et tes profils restent. »), « Voir sur GitHub », « Plus tard »,
  « Mettre à jour ». Si l'installation intégrée est impossible (dossier non modifiable, version de développement),
  la raison et « Ouvrir la page de la release ».

## Installation

Quatre étapes numérotées : **Téléchargement** (avec sa progression et la taille), **Vérification** (taille et
empreinte SHA-256, puis contenu de l'archive), **Installation**, **Redémarrage**. Les étapes faites sont cochées en
vert, celle en cours est en bleu. Chaque étape reste affichée 0,6 s au moins, sinon Vérification et Installation
passeraient avant la fermeture. « Annuler » reste possible tant que rien n'est remplacé ; « Masquer » ferme la
fenêtre sans arrêter l'installation, que la barre latérale résume (un clic la rouvre).

En cas d'échec : la raison, « Rien n'a été remplacé. », « Ouvrir la page de la release » et « Réessayer ». Les
contrôles eux-mêmes (source GitHub, redirections, taille, SHA-256, archive, retour arrière si la nouvelle version ne
répond pas) sont ceux du moteur, décrits dans [AGENTS.md](../AGENTS.md).

## Après le redémarrage

L'installation écrit son résultat ; la nouvelle version le lit et rouvre la fenêtre une fois : « Redémarrage » se
coche, puis « Rustudio X.Y.Z est installé. », « Mise à jour depuis la A.B.C » et les nouveautés de la version,
tirées du CHANGELOG embarqué (lisibles hors ligne). Fermer la fenêtre l'accuse auprès du moteur (`update-ack`) :
elle ne se rouvre plus. Un retour à une version précédente affiche « Retour à la X.Y.Z terminé. ».

## Nouveautés de cette version

Page Versions, « Nouveautés de cette version » : la même fenêtre pour la version installée, avec l'étiquette
« Installée » et sa date, sans installation (« Voir sur GitHub », « Fermer »).

## Captures

`RUSTUDIO_PREVIEW_SHEET=update|downloading|failed|done|current` ouvre la fenêtre dans cet état au démarrage
(développement). Le banc `labs/visible/shots.py --sheet=update versions` capture la fenêtre de démonstration.
