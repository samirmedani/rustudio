# Diagnostic des coupures du son

Rustudio compte les crépitements sans jouer de son de test, en relisant les statistiques que le pilote Thesycon
tient lui-même sur le transport USB. Cette page dit d'où viennent ces chiffres, comment ils ont été vérifiés, et
comment les étendre à un autre appareil.

## Ce que le pilote expose

`TUSBAUDIO_QueryDriverStatistics(handle, tampon, taille)` (pilote TC-Helicon 5.57) remplit un bloc de
2 719 696 octets (`0x297FD0`) propre à l'appareil :

| Offset | Contenu |
| --- | --- |
| `0x14` | identifiant du vendeur USB (GoXLR : `0x1220`) |
| `0x18` | identifiant du produit (Mini : `0x8FE4`) |
| `0x1C0` | flux isochrone d'entrée (GoXLR → PC) |
| `0x278` | flux isochrone de sortie (PC → GoXLR) |

Dans chacun de ces deux enregistrements :

| Décalage | Contenu |
| --- | --- |
| `+0x28` | 1 quand le flux est arrêté, 0 pendant que le son passe |
| `+0x2C` | millisecondes depuis le démarrage du flux |
| `+0x30` | micro-trames USB servies (8 000 par seconde) |
| `+0x34` | micro-trames perdues |

Le pilote coupe le flux dès qu'aucune application ne joue : au repos, ces compteurs ne bougent pas. À l'arrêt du
flux d'entrée, il ajoute quelques micro-trames au compteur de pertes ; Rustudio ignore donc tout écart calculé
pendant que le flux ne tourne pas.

## Ce qui a été vérifié

Mesures du 29 octobre 2026, GoXLR Mini, pilote 5.57 :

- une coupure entendue fait avancer les deux compteurs de 8 ou 16 (une trame d'1 ms, doublée quand deux trames
  sont perdues), en entrée comme en sortie, dans la même lecture ;
- un test en boucle de 100 s avec une rafale de lancements de programmes : 3 coupures mesurées par l'aller-retour
  d'un son de test, 3 retrouvées par les compteurs du pilote, à la milliseconde ;
- 8 700 lectures en 50 secondes : mémoire et handles stables, rien à libérer.

La sonde `crates/goxlr/examples/usbstats.rs` reproduit ces mesures hors du moteur :

**Arrêter le service Rustudio avant d'utiliser la sonde.** Elle énumère les appareils du pilote, ce qui invalide
ses handles ouverts. Elle réserve le port local du moteur avant de charger le pilote et le garde pendant toute
la mesure : si Rustudio est déjà lancé, elle refuse de démarrer. Pour un moteur sur un port personnalisé, définir
le même `RUSTUDIO_PORT` pour la sonde. Ne jamais la lancer périodiquement en parallèle du service.

```powershell
cargo xwin build -p rustudio-goxlr --example usbstats --target x86_64-pc-windows-msvc --release
usbstats.exe check           # le chemin exact du moteur, trois lectures
usbstats.exe 60 100          # une minute de veille, champs qui changent rarement
```

## Ce que ce compteur ne couvre pas

- GoXLR pleine taille (`PID_8FE0`) et autres pilotes Thesycon : la disposition du bloc peut différer. Rustudio
  vérifie les identifiants USB et le drapeau de marche avant d'utiliser ces chiffres ; sinon la section reste
  muette au lieu d'afficher des valeurs fausses.
- Une coupure peut aussi venir d'un programme qui n'a pas fourni le son à temps : le journal Windows
  `Microsoft-Windows-Audio/GlitchDetection` la compte séparément, et Rustudio le résume à côté.
- Sans lecture ni enregistrement, les compteurs restent figés : aucune coupure enregistrée au repos ne prouve
  pas que le son est stable en utilisation. Les programmes apparus autour d'une coupure sont des corrélations,
  pas une preuve de causalité.

## Garder la surveillance légère

Le fil USB ne fait que mettre à jour les compteurs en mémoire. Le fil de diagnostic prend une copie de
l'historique, relâche le verrou, puis écrit le fichier temporaire et remplace l'ancien fichier. Un numéro de
révision distingue cette copie des coupures reçues pendant l'écriture : elles seront sauvegardées au passage
suivant. Une écriture échouée reste en attente. Ce fil écrit aussi les messages de coupure dans le journal et
retire les événements de plus de 24 heures, même si le son ne coupe plus.

## Ajouter un appareil

1. Lire le bloc brut et repérer l'identité USB, puis les deux enregistrements de flux (compteurs qui avancent de
   8 000 par seconde pendant la lecture d'un son connu).
2. Provoquer une coupure mesurable (par exemple en saturant le processeur avec le son de test) et vérifier quel
   champ avance exactement au même moment ; c'est le compteur de pertes.
3. Ajouter l'identifiant du produit et les offsets dans [`crates/goxlr/src/stream.rs`](../crates/goxlr/src/stream.rs),
   avec un test sur un bloc fabriqué, puis un relevé daté dans ce document.
