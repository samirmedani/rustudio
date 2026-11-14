<p align="center">
  <img src="docs/icon.png" alt="Icône de Rustudio" width="96" height="96">
</p>

<h1 align="center">Rustudio</h1>

<p align="center">
  <strong>Tes périphériques Windows, au même endroit.</strong><br>
  Une application libre écrite en Rust, avec des modules pour détecter et régler le matériel de ton bureau.
</p>

<p align="center">
  <a href="https://github.com/samirmedani/rustudio/releases/latest/download/Rustudio-Setup.exe"><img src="https://img.shields.io/badge/T%C3%A9l%C3%A9charger-Rustudio--Setup.exe-0A84FF?style=for-the-badge&amp;logo=windows" alt="Télécharger Rustudio pour Windows"></a>
</p>

<p align="center">
  <a href="https://github.com/samirmedani/rustudio/releases/latest"><img src="https://img.shields.io/github/v/release/samirmedani/rustudio?label=version" alt="Dernière version"></a>
  <a href="https://github.com/samirmedani/rustudio/actions/workflows/ci.yml"><img src="https://github.com/samirmedani/rustudio/actions/workflows/ci.yml/badge.svg" alt="Vérifications"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/licence-MIT-30D158" alt="Licence MIT"></a>
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011-64D2FF" alt="Windows 10 et 11">
</p>

<p align="center">
  <a href="#compatibilité">Appareils compatibles</a> ·
  <a href="#installer">Installation</a> ·
  <a href="#contribuer">Contribuer</a> ·
  <a href="docs/START-HERE.en.md">English</a>
</p>

## Tes appareils, tes modules

Rustudio réunit les réglages de tes périphériques dans **Mes appareils**. Branche un appareil pris en charge : il apparaît automatiquement, avec son état et ses réglages. Débranche-le : sa carte et tes préférences restent disponibles.

Chaque module apporte les contrôles propres à son matériel. Dans **Modules**, tu peux voir ce qui est livré et les appareils dont la prise en charge reste à créer. Les nouveaux modules peuvent être proposés par le mainteneur ou par la communauté.

<p align="center">
  <img src="docs/captures/devices.jpg" alt="Mes appareils dans Rustudio : deux cartes pour deux périphériques distincts, avec leur état de connexion et leurs réglages" width="900">
  <br><sub>Démonstration : la GoXLR Mini et la R5 Ultra illustrent les deux modules fournis. Les cartes ne sont pas un inventaire du PC du mainteneur.</sub>
</p>

<p align="center">
  <img src="docs/captures/sombre.jpg" alt="Le mixeur de Rustudio en thème sombre : mêmes faders colorés sur un fond anthracite" width="900">
  <br><sub>Clair ou sombre : Rustudio suit le mode de Windows, ou celui que tu choisis dans Réglages.</sub>
</p>

<p align="center">
  <img src="docs/captures/mixer.jpg" alt="Le mixeur de la GoXLR Mini : quatre grands faders colorés, la coupure de chaque voie et les autres voies en dessous" width="440">
  <img src="docs/captures/mouse.jpg" alt="Les réglages de la R5 Ultra : batterie, sensibilité par palier, fréquence et options du capteur" width="440">
</p>

## Compatibilité

| Module livré | Ce que tu peux régler | Compatibilité |
| --- | --- | --- |
| [Attack Shark R5 Ultra](docs/R5-ULTRA.md) | DPI par axe, fréquence, veille et capteur ; état de la batterie | Récepteur 2,4 GHz testé sur la souris ; USB testé en simulation |
| [Cooler Master MK770](docs/MK770.md) | Effets lumineux, luminosité, vitesse, couleur et couleur par touche | USB testé sur le clavier ; 2,4 GHz et Bluetooth non pris en charge |
| [TC-Helicon GoXLR Mini](docs/GOXLR-MINI.md) | Mixeur, micro, routage, éclairage, scènes, égaliseur casque et diagnostic audio | Pilote officiel TC-Helicon ; firmware 1.2.x testé, 1.3 pris en charge |

**Tu ne vois pas ton appareil ?** [Demande un module](https://github.com/samirmedani/rustudio/issues/new?template=appareil.yml). Indique la marque, le modèle, le mode de connexion et le réglage qui te serait le plus utile. Une fiche « À contribuer » dans le [catalogue](library/index.json) indique une demande, pas une compatibilité.

## Installer

1. Télécharge [Rustudio-Setup.exe](https://github.com/samirmedani/rustudio/releases/latest/download/Rustudio-Setup.exe) et lance l’installation.
2. Ouvre **Mes appareils** et branche ton matériel. Les prérequis propres à un module sont indiqués dans le tableau.
3. Ouvre la carte de l’appareil pour le régler. Tu retrouves Rustudio près de l’horloge Windows.

Windows 10 ou 11, 64 bits. Pour désinstaller Rustudio, ouvre **Paramètres → Applications**. Tes réglages restent sur ton PC.

## Mises à jour, historique et retour arrière

Rustudio te propose les nouvelles [versions GitHub](https://github.com/samirmedani/rustudio/releases). L’installation commence uniquement quand tu la confirmes. L’application vérifie la taille, l’empreinte SHA-256 et le contenu de l’archive. Elle rétablit la version précédente si la nouvelle ne répond pas.

Clique sur le numéro de version pour lire l’historique et, si nécessaire, revenir à une version publiée compatible. Les changements sont détaillés dans le [journal des versions](CHANGELOG.md).

## Aide et confidentialité

**Pas de compte ni de télémétrie.** Tes réglages restent sur ton PC. Rustudio contacte GitHub pour les versions et les téléchargements que tu demandes. Dans **⋯ → Aide et retours**, tu peux copier un diagnostic anonymisé et le relire avant de le partager.

[Signaler un bug](https://github.com/samirmedani/rustudio/issues/new?template=bug.yml) · [Poser une question ou proposer une idée](https://github.com/samirmedani/rustudio/discussions) · [Signaler une faille en privé](SECURITY.md)

## Contribuer

Tu peux **demander un appareil, tester un module sur ton matériel, écrire du code ou améliorer la documentation**. Un module communautaire garde le crédit de son créateur. Après revue et validation, il est intégré à une version de Rustudio. Le catalogue affiche les modules livrés et les demandes ; il n’installe pas de code tiers à la volée.

- [Guide de contribution](CONTRIBUTING.md) : démarrer et préparer une pull request.
- [Ajouter un appareil](docs/MODULES.md) : protocole, simulateur, intégration et interface.
- [Modules communautaires](docs/COMMUNITY.md) : attribution, revue et publication.

## Développer

Tout est écrit en Rust. Le service gère le matériel et expose une API locale ; l’appli Windows est native, dessinée avec [GPUI](https://www.gpui.rs), sans navigateur intégré. Le registre des modules décrit les capacités et les écrans disponibles ; chaque module adapte son protocole au moteur.

```sh
git clone https://github.com/samirmedani/rustudio.git
cd rustudio
cargo test
```

Sous Windows, `cargo run -p rustudio-app -- --start-demo` ouvre l’appli sur une démonstration : un moteur simulé démarre sur son propre port, sans toucher à ton installation ni à ton matériel. Lance `scripts/check.sh` avant une pull request.

Rustudio est libre sous [licence MIT](LICENSE). Les dépendances et crédits sont listés dans les [mentions tierces](THIRD-PARTY-NOTICES.md).
