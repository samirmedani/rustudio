# État de la migration GPUI

L’application native et le moteur se compilent séparément. L’aperçu ne
remplace pas la version distribuée et ne déclenche aucune installation.

| Fonction | État dans l’aperçu |
| --- | --- |
| Mes appareils, catalogue et profils du mixeur | Interface native ; la détection lit l’état des modules |
| R5 Ultra | DPI X/Y, palier, fréquence, batterie, connexion, décrochage, anti-double clic, veille et capteur ; brouillon et relecture par le moteur |
| GoXLR | Mixeur, matrice, VOD automatique, scènes et règles, thèmes, éclairage, réglages micro et casque |
| Voix | Mesure guidée et empreinte ; la démo utilise la synthèse ; la capture WASAPI réelle n’est pas exercée par le banc |
| Son Windows, diagnostic, OBS | Inventaire, correction manuelle des défauts, contrôle de routage, mesure des coupures et lien OBS |
| Session Windows | Icône et bulle facultatives ; l’aperçu ne s’enregistre pas au démarrage |
| Versions et aide | Historique consultable, diagnostic à copier et liens d’aide |

Restent à porter ou à qualifier pour remplacer la version distribuée :
la lecture d’un enregistrement court et les graphiques de voix, le test
acoustique d’écho avec lecture de bips, le démarrage et l’instance unique
en production, l’installation, les mises à jour et les retours de version.
Le diagnostic et la capture audio réelle nécessitent aussi une recette
matérielle distincte ; les données et les écritures du banc restent simulées.
