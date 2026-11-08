# Comparaisons des fenêtres sous Windows

## Mixeur GPUI complet — 7 novembre 2026

Trois lancements et 66 mouvements par interface sur le même PC Windows,
avec le même moteur simulé, des zones client de 900 × 890 et Windows
Graphics Capture. La référence Tauri affiche quatre faders en lecture seule.
GPUI affiche le mixeur complet. Le moteur commun est exclu des mesures de RAM
et de CPU, les processus WebView2 sont inclus pour Tauri.

| Mesure | Tauri | GPUI | Écart GPUI |
| --- | ---: | ---: | ---: |
| Fader, délai médian | 128,28 ms | 126,60 ms | −1,3 % |
| Fader, délai au 95e centile | 158,83 ms | 157,26 ms | −1,0 % |
| De la publication par le moteur au rendu, médiane | 16,90 ms | 16,73 ms | −1,0 % |
| Ouverture jusqu’au premier fader visible, médiane | 1 134 ms | 565 ms | −50,2 % |
| Mémoire privée résidente, médiane | 63,72 Mio | 18,90 Mio | −70,3 % |
| CPU visible au repos, médiane | 9,93 % | 0,39 % | −96,1 % |
| CPU avec faders en mouvement, médiane | 10,15 % | 2,54 % | −75,0 % |

Le CPU est exprimé en fraction d’un processeur logique. Le moteur publie la
position au bout d’environ 110 ms pour les deux interfaces. Les écarts de
latence d’environ 2 ms indiquent une réactivité comparable dans ce test.
Les timings de la version précédente, ci-dessous, ont été collectés avec
une autre méthode de capture. Le temps GPU et le délai avec du matériel réel
n’ont pas été mesurés.

Le test d’interaction GPUI a vérifié un changement de volume avant le
relâchement, l’envoi de la position finale, puis la reprise par le fader
simulé dans une image de la fenêtre. Les commandes de coupure, réactivation,
permutation et cible ont aussi été confirmées par le simulateur. Aucun
passage n’a pris le focus ; aucune installation ni release n’a été faite.

## Premier prototype à quatre faders — 7 novembre 2026

Windows, écran 2 560 × 1 440. Trois lancements par interface, 66 mouvements
chronométrés par interface, même moteur simulé. Valeurs médianes sauf le
95e centile (95 % des mouvements apparaissent en moins de ce délai).

| Mesure | Tauri | GPUI | Écart GPUI |
| --- | ---: | ---: | ---: |
| Mémoire privée résidente | 63,9 Mio | 17,6 Mio | −72,5 % |
| CPU visible au repos | 8,96 % | 0,78 % | −91,3 % |
| CPU avec faders en mouvement | 12,48 % | 1,75 % | −86,0 % |
| Ouverture jusqu’au premier fader visible | 1 128 ms | 507 ms | −55,1 % |
| Fader, délai médian | 130 ms | 143 ms | +10,1 % |
| Fader, délai au 95e centile | 160 ms | 161 ms | +0,7 % |

Le CPU est exprimé en fraction d’un processeur logique. Les processus
WebView2 sont inclus ; le moteur commun est exclu. Les écarts sont calculés
sur les valeurs non arrondies. Le banc compare les vues de quatre faders,
pas deux versions complètes de Rustudio. Il ne mesure pas le temps GPU.

Un passage séparé (32 mouvements par interface) a relevé la publication
par le moteur après environ 109 à 110 ms. De la publication au rendu observé,
la médiane est de 18,6 ms pour Tauri et 33,1 ms pour GPUI. L’observation
ajoute des requêtes au simulateur et n’entre pas dans le tableau principal.

La bulle native a réussi les six transitions : rouge à la coupure, verte
à la réactivation, aucune prise de focus, masquage automatique. Apparition
médiane : 346 ms à la coupure, 344 ms à la réactivation ; durée visible :
environ 1,66 s. Le service Windows installé n’a pas participé au test.

[Protocole et code du banc](README.md).
