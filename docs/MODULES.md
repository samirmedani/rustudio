# Ajouter un appareil à Rustudio

Rustudio organise le matériel par **modules**. Un module apporte le protocole, l'adaptateur du moteur et les pages
propres à une famille d'appareils. L'accueil **Mes appareils** présente l'état des modules livrés ; **Modules**
présente le catalogue de contributions.

Les modules **GoXLR Mini** et **R5 Ultra** sont livrés. Ajouter une fiche de souris ou
d'écran au catalogue ne suffit pas à le piloter. Ce guide décrit le code, les déclarations et les tests nécessaires.

Le module [R5 Ultra](R5-ULTRA.md) fournit un exemple de transport HID indépendant du fil audio. Sa validation physique couvre le mode 2,4 GHz ; la portée des essais doit toujours être précisée.

## Les statuts du catalogue

| Statut | Sens |
| --- | --- |
| **Officiel** | Maintenu dans ce dépôt, testé sur le vrai matériel, livré avec Rustudio. |
| **Communautaire** | Proposé par une pull request, relu et testé par son auteur sur son appareil. |
| **Générique** | Couvre toute une famille d'appareils (par exemple les écrans DDC/CI). Pour ton modèle précis, adapte-le (fork) ou propose ta version. |
| **À contribuer** | Demandé, pas encore écrit. C'est par là qu'on commence. |

## 1. La fiche

Chaque appareil proposé a une fiche dans [library/index.json](../library/index.json). L'appli consulte ce catalogue
sur GitHub, avec une copie locale hors ligne. Cette fiche décrit le projet et son statut ; elle ne charge pas de
code et ne fait pas apparaître un nouvel appareil dans **Mes appareils**.

```json
{
  "id": "attack-shark-r5-ultra",
  "name": "R5 Ultra",
  "vendor": "Attack Shark",
  "kind": "mouse",
  "status": "community",
  "usb": [{"vid": "XXXX", "pid": "XXXX"}],
  "source": "crates/attack-shark",
  "summary": "Ce que le module sait régler, en une phrase.",
  "features": ["DPI", "Polling rate", "Boutons", "Éclairage"]
}
```

- `id` : lettres, chiffres et tirets ; `kind` : `audio-interface`, `mouse`, `keyboard`, `monitor`, `headset` ou `other`.
- `usb` : identifiants USB visibles dans le Gestionnaire de périphériques (Détails › Numéros d'identification).
- Les textes sont en français, avec le tutoiement.

## 2. Le module et son adaptateur

Un module est du code Rust compilé dans Rustudio. Pour la GoXLR Mini, le protocole et son simulateur sont dans
[crates/goxlr](../crates/goxlr) ; le moteur et l'interface assurent son intégration :

1. **Le transport** : la façon de parler à l'appareil (pilote du fabricant, HID, DDC/CI…), derrière un trait,
   comme `Tusb` pour la GoXLR.
2. **La fausse version** : un simulateur qui implémente le même trait et reproduit les pièges du vrai matériel.
   C'est lui qui permet de tout tester sans l'appareil, et de relire ta pull request.
3. **Le protocole** : les commandes, construites octet par octet, sans état métier.
4. **Les tests** : chaque comportement sur le simulateur, et chaque constat fait sur le vrai matériel ajouté au
   simulateur pour qu'il ne revienne jamais.
5. **L'adaptateur du moteur** : connexion, reconnexion, commandes et état d'exécution. L'API lit cet état sans
   ouvrir le matériel. Respecte les contraintes de session : par exemple, un écran peut demander un adaptateur
   dans l'appli Windows plutôt que dans un service système.
6. **Les pages** : implémente les contrôles et les actions propres à ton appareil. Les pages implémentées, puis déclarées dans le
   registre, alimentent la navigation ; la déclaration ne fabrique ni leurs contrôles ni leur protocole.

Le téléchargement de code tiers et l'isolation WebAssembly ne sont pas implémentés. Une éventuelle architecture
de plugins nécessitera un contrat de transport, de permissions et d'état ; ce guide ne garantit pas une
migration sans adaptation.

## 3. Déclarer le module livré

Le registre compilé [crates/core/src/modules.rs](../crates/core/src/modules.rs) fait autorité sur les modules
réellement disponibles. Ajoute un `ModuleDescriptor` et référence-le dans `INSTALLED` seulement quand son
adaptateur et ses pages fonctionnent.

| Champ | Rôle |
| --- | --- |
| `id` | Identifiant stable du module, identique à la fiche du catalogue. |
| `name`, `vendor`, `kind`, `summary` | Présentation locale, utilisable sans réseau. |
| `capabilities` | Fonctions effectivement implémentées, avec des identifiants stables. |
| `pages` | Entrées de navigation : `id`, `label` et nom d'icône connu de l'interface. |
| `actions` | Noms des commandes prises en charge par l'adaptateur de ce module. Les commandes globales de Rustudio restent en dehors de cette liste. |

Le registre GoXLR Mini déclare par exemple les capacités `mixer`, `microphone`, `headphone-eq`, `routing`,
`lighting` et `scenes`, puis les pages `mixer`, `mic`, `hp`, `routing`, `light` et `scenes`.

L'adaptateur [crates/engine/src/modules.rs](../crates/engine/src/modules.rs) expose l'état matériel sans nouvelle
détection USB. Pour un autre appareil, ajoute son propre état d'exécution et son raccordement à ces vues :

- `GET /api/modules` : `{installed: [...], devices: [...]}` ;
- `GET /api/modules/{id}/state` : `{module, device, state, meta, live, v}`, limité au module demandé ;
- `POST /api/modules/{id}/{action}` : commande envoyée à son adaptateur local, parmi les `actions` déclarées ;
- `GET /api/state` : la vue d'ensemble `{installed: [...], devices: [...]}` dans `modules` ;
- `GET /api/live` : `modules.devices`, pour actualiser la connexion sans relire tous les réglages.

Dans la vue propre au module, `module` est son descripteur et `device` son état d'exécution. `state`, `meta` et
`live` contiennent ses réglages, ses métadonnées et ses valeurs en direct ; `v` permet de repérer un changement.

Chaque entrée de `devices` contient `id`, `status`, `connected`, `simulated`, `firmware` et `error`. Les statuts sont
`ready`, `disconnected`, `starting`, `error` et `blocked`. Les réglages et commandes restent propres au module ;
ce registre ne remplace pas leur implémentation.

### Commandes du module et commandes globales

Les contrôles d'un appareil utilisent son espace d'API. Par exemple, pour le volume Musique de la GoXLR Mini :

```http
POST /api/modules/goxlr-mini/volume
Content-Type: application/json
X-Rustudio: 1

{"channel":"Music","value":128}
```

Dans l'interface, le même appel s'écrit `postModule("goxlr-mini", "volume", {channel: "Music", value: 128})`.
L'adaptateur du module valide et applique la commande à son état ; l'API ne parle pas directement au matériel.
Déclare l'action `volume` dans `actions` seulement si cet adaptateur la prend en charge.

Les mises à jour de Rustudio (`update-*`), le catalogue (`library-refresh`) et le redémarrage du moteur
(`restart-engine`) sont des commandes globales. Elles gardent leurs routes `/api/{action}` et ne figurent pas
dans les `actions` d'un module. Une reconnexion propre à un appareil appartient, elle, à son adaptateur.

Les anciennes routes de la GoXLR Mini, par exemple `/api/volume`, restent compatibles. Pour de nouveaux contrôles,
utilise l'espace `/api/modules/{id}` et `postModule` afin que la commande désigne explicitement son module.

Les routes générales sont `#devices` et `#modules`. Les pages GoXLR Mini vivent sous `#goxlr-mini/<page>`, par
exemple `#goxlr-mini/mixer`. L'interface actuelle implémente ces pages audio ; pour ton nouveau module, ajoute ses
composants, ses actions et la gestion de sa route. Les anciens liens audio restent des alias pour la GoXLR Mini.

## 4. Découvrir le protocole de ton appareil

- Cherche d'abord un projet libre qui le pilote déjà (OpenRGB, libratbag, goxlr-utility, ddcutil…) et respecte
  sa licence.
- Sinon, observe ce qu'envoie le logiciel officiel : [Wireshark](https://www.wireshark.org) avec USBPcap sous
  Windows. Note chaque commande et sa réponse dans le simulateur.
- **Jamais de mise à jour de firmware** dans un module communautaire : une commande mal comprise peut rendre
  l'appareil inutilisable. Les firmwares restent l'affaire du fabricant.

## 5. Proposer ton module

1. Ouvre une issue « Nouvel appareil » (modèle fourni) pour annoncer ton travail.
2. Fork, branche `appareil/<id>`, puis protocole, simulateur, adaptateur, pages, déclaration locale, tests et fiche.
3. `scripts/check.sh` doit passer : tests, et aucune donnée personnelle (numéro de série de ton appareil, ton nom,
   tes chemins de machine…).
4. Ouvre la pull request avec le modèle fourni : ce que tu as testé sur le vrai appareil, et sur quel firmware.

## Pour un agent de code

Lis [AGENTS.md](../AGENTS.md) avant tout. Ensuite : simulateur d'abord, protocole ensuite, interface en dernier ;
une pull request par appareil ; jamais de donnée de la machine sur laquelle tu travailles.
