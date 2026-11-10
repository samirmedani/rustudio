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

Depuis la 6.0.1, l’appli native est la version distribuée : installation, instance unique, démarrage avec
Windows, mises à jour et retour à une version publiée passent par elle. La réécoute d’un enregistrement court, le
graphique de voix et le test acoustique d’écho sont portés.

Restent à qualifier sur du matériel réel, hors du banc simulé : la capture WASAPI de l’empreinte vocale et le
diagnostic audio. Le tableau ci-dessus décrit l’aperçu de démonstration, dont les données et les écritures sont
simulées.
