# Son Windows : comprendre les sources et voir le son

Dans **Mes appareils → Son Windows**, chaque ligne affiche son niveau en direct. Clique une ligne ou
choisis une source dans le sélecteur pour voir son spectre : les graves sont à gauche, les aigus à droite.
Le même spectre apparaît en petit sous la ligne choisie. **Pause** ferme l’analyse et les indicateurs ;
**Reprendre** les rouvre. Quitter cette page ou fermer sa fenêtre libère les clients audio.

Le guide peut se réduire après lecture. Les noms techniques restent affichés sous les noms expliqués.
Les badges distinguent le son principal de Windows, son entrée principale et ses choix pour les appels.
Choisir une source à observer ne la définit pas comme périphérique par défaut.
Une entrée que personne n’utilise peut rester à zéro dans l’indicateur Windows : sélectionne-la
pour ouvrir son analyse et la tester.

## Les choix courants avec une GoXLR

| Usage | Choix |
| --- | --- |
| Son général de Windows | Sortie **System** |
| Écouter les voix dans Discord | Sortie **Chat** |
| Parler dans Windows ou Discord | Entrée **Chat Mic** |
| Envoyer le mix du stream à OBS | Entrée **Broadcast Stream Mix**, aussi appelée **Stream Mix 1** |
| Séparer les jeux ou la musique | Sorties **Game** ou **Music** dans les applis concernées |
| Faire un second mix d’enregistrement | Entrée **VOD / Stream Mix 2**, selon le pilote |

Le contenu réel de Chat Mic, Stream Mix et VOD dépend du routage Rustudio. Le nom VOD ne garantit
pas que la musique est absente. Dans OBS, évite de reprendre séparément les mêmes sources si elles
sont déjà dans le mix capturé : cela peut doubler le son. Le mix du stream n’est généralement pas
le bon micro pour Discord, car il peut inclure les voix des correspondants.

Le bouton **Configurer Windows pour la GoXLR** conserve sa confirmation et la politique existante :
il corrige les entrées qui ne sont pas Chat Mic et les sorties hors GoXLR, en gardant les sorties
GoXLR déjà choisies. Il ne configure pas les choix propres à Discord, aux jeux ou à OBS.

Ces choix suivent le [guide de démarrage du fabricant](https://mediadl.musictribe.com/media/PLM/data/docs/P0DI7/QSG_TH_0803-AAB_GoXLR-MINI_WW.pdf)
et le [routage du module Rustudio](GOXLR-MINI.md).

## Analyse et portée

Le moniteur fonctionne dans la session de l’utilisateur, indépendamment du service USB. Il lit
les identifiants stables IMMDevice de l’inventaire. Un fil COM mesure toutes les lignes avec
IAudioMeterInformation ; une seule capture WASAPI partagée analyse la source choisie. Les sorties
sont lues en loopback, les entrées en capture. Aucune lecture n’est renvoyée au casque, aucun
réglage ou routage n’est modifié, aucun échantillon audio n’est enregistré sur disque ou transmis.

Le spectre utilise une FFT de 4 096 points avec fenêtre de Hann et 48 bandes logarithmiques entre
20 Hz et 20 kHz (limitées à la fréquence de Nyquist de la source). La puissance des canaux est
moyennée après FFT pour conserver un signal stéréo en opposition de phase. Mise à jour environ
20 fois par seconde, décroissance visuelle des barres et extinction quand aucun paquet n’arrive.
Les niveaux sont numériques en dBFS, sans mesure du volume acoustique du casque.

Une source débranchée ou occupée produit un état d’erreur et une reconnexion périodique. Les
indicateurs Windows peuvent être indisponibles avec certains flux exclusifs ; les contenus protégés
peuvent être absents du loopback. Un silence n’est jamais remplacé par une animation fictive.
Le mode de démonstration utilise un signal synthétique annoncé comme tel.

Références Microsoft : [loopback WASAPI](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording),
[indicateurs de niveau](https://learn.microsoft.com/en-us/windows/win32/coreaudio/peak-meters).

## Vérifications

`cargo test --locked -p rustudio-windows-desktop -p rustudio-app` vérifie la fréquence et le niveau
d’une sinusoïde à 44,1 / 48 / 96 kHz, la stéréo en opposition de phase, le silence, les valeurs non
finies, le changement de source, la pause et les explications des voies et alias de pilotes.

`cargo run --locked -p rustudio-windows-desktop --example spectrum` teste la sortie réelle par défaut ;
ajoute `-- --input` pour l’entrée. La sonde lit pendant trois secondes puis vérifie l’arrêt ; elle ne
joue rien et n’écrit aucun fichier audio. Sur le poste Windows, entrée et sortie ont été ouvertes à
48 kHz et l’entrée a livré un spectre actif. La vue native a aussi été vérifiée sur ces périphériques.
