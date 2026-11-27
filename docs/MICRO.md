# Module Micro

Module officiel pour **n'importe quel micro de Windows** : USB, jack, micro-casque, webcam ou interface audio. Il
apparaît dans **Mes appareils** dès que Windows voit un micro, sans pilote ni logiciel à garder ouvert.

## Ce que fait la page « Mon micro »

- **Ton micro** : liste des entrées actives de Windows. La page s'ouvre sur le micro de Windows (sinon le premier qui
  n'est pas celui de la GoXLR, qui garde sa propre page Micro).
- **Windows** : niveau d'entrée en direct, volume du micro (en % et en dB quand le pilote les donne), coupure, et
  « Utiliser ce micro dans Windows » pour les trois rôles (applis, multimédia, appels). Ces réglages appartiennent à
  la session de l'utilisateur : l'appli les fait elle-même, le moteur (compte système) n'y touche pas.
- **Ta voix** : gain (−12 à +12 dB), coupe-bas (40 à 200 Hz, 12 dB par octave) et égaliseur 5 bandes pensées pour la
  voix parlée : graves (étagère 120 Hz), bas-médiums (320 Hz), médiums (1 kHz), présence (3,5 kHz), air (étagère
  10 kHz), de −9 à +9 dB. Préréglages : Naturel, Voix chaude, Radio, Clair, Adoucir. Un préréglage règle le coupe-bas
  et l'égaliseur ; le gain reste celui choisi, qui dépend du micro.
- **Suppression du bruit (IA)** : RNNoise, le filtre d'OBS, sur le micro affiché. C'est le même réglage que sur la
  page Micro de la GoXLR : un seul micro à la fois en profite, et la page dit sur quel micro il tourne avant de le
  déplacer.
- **Te réécouter** : 6 secondes enregistrées en mode partagé, donc telles que Discord ou OBS les reçoivent, traitements
  compris, puis rejouées sur la sortie de Windows.

## Fonctionnement

Le traitement passe par Equalizer APO, branché sur la seule entrée choisie (même procédure que pour la suppression du
bruit : clé `FxProperties` de l'entrée, valeurs d'origine gardées dans `Child APOs`). Le moteur écrit
`rustudio-micro-voix.txt`, inclus par `config.txt` après `rustudio-micro.txt` : le bruit est retiré avant
l'égaliseur, comme dans OBS. Chaque micro actif a son bloc, visé par son identifiant Windows, jamais par son nom.

```
Device: {CC214E79-D5E6-45C0-B617-DBB4992E5A7E}
Preamp: 3.0 dB
Filter 1: ON HPQ Fc 80 Hz Q 0.71
Filter 2: ON LSC Fc 120 Hz Gain 3.0 dB Q 0.71
Device: all
```

Les réglages de chaque micro sont gardés dans `micro.json`, à côté de `state.json` (16 micros au plus).

## Ce qui ne s'active jamais seul

- La voix reste naturelle par défaut : aucun traitement tant que tu ne l'as pas activé pour ce micro.
- Brancher Equalizer APO (et l'installer s'il manque, 1.4.2, empreinte vérifiée) ou RNNoise demande une confirmation :
  le son de Windows redémarre, environ 2 secondes de silence.
- **Retirer** remet les effets d'origine de l'entrée et coupe la voix et la suppression du bruit sur ce micro ; les
  réglages restent gardés pour plus tard.

## Limites

- Pas de compresseur, de noise gate ni de limiteur : Equalizer APO n'en a pas. Avec une GoXLR, sa propre dynamique
  reste sur la page Micro de la GoXLR.
- Une appli qui ouvre le micro en mode exclusif contourne les effets de Windows, donc ce traitement.
- Un gain élevé peut saturer une voix forte : monter d'abord le volume du micro dans Windows.

## Vérifications

- Tests sur simulateur (`crates/windows-mic`, `crates/engine/tests/modules.rs`) : voix coupée par défaut, réglages
  bornés, fichier limité aux micros actifs et branchés, branchement, suppression du bruit puis retrait.
- Sur un PC Windows 11 avec Equalizer APO 1.4.2 : volume et dB lus sur deux micros, et trace d'Equalizer APO montrant
  le préampli, le coupe-bas `HPQ`, les étagères et les cloches appliqués à l'entrée visée.
