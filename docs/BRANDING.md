# Icône Rustudio

Un carré aux coins arrondis, en dégradé bleu vers violet, porte trois curseurs de mixeur blancs à des hauteurs
différentes. Le dessin est géométrique, sans texte ni effet décoratif, et reste lisible à 16 pixels.

Les sources sont vectorielles :

- [brand/rustudio.svg](brand/rustudio.svg) : version détaillée (48 pixels et plus, installeur, documentation) ;
- [brand/rustudio-small.svg](brand/rustudio-small.svg) : version simplifiée pour 16 à 32 pixels (barre des tâches,
  icône près de l'horloge), avec des curseurs plus épais.

Les fichiers livrés en sont des exports : [icon.png](icon.png) (512 × 512), `crates/app/icons/icon.ico` (16 à
256 pixels, la petite version jusqu'à 32), `crates/app/icons/32x32.png` (icône près de l'horloge) et
`crates/app/assets/logo.png` (barre latérale). Pour les régénérer :

```sh
rsvg-convert -w 512 -h 512 docs/brand/rustudio.svg -o docs/icon.png
```

Couleurs : bleu `#3A86FF`, indigo `#5B5CF6`, violet `#9B4CF2`, curseurs blancs. L'interface reprend le bleu
système `#0071E3` pour les actions.
