# Ajouter un appareil à Rustudio

Rustudio est fait pour piloter tout ton matériel : interface audio, souris, clavier, écran… Chaque appareil est
un **module**. Ce guide explique comment en écrire un, le tester sans l'appareil et le proposer à tous. Il est écrit
pour toi, et pour l'agent de code qui t'aide.

## Les statuts de la bibliothèque

| Statut | Sens |
| --- | --- |
| **Officiel** | Maintenu dans ce dépôt, testé sur le vrai matériel, livré avec Rustudio. |
| **Communautaire** | Proposé par une pull request, relu et testé par son auteur sur son appareil. |
| **Générique** | Couvre toute une famille d'appareils (par exemple les écrans DDC/CI). Pour ton modèle précis, adapte-le (fork) ou propose ta version. |
| **À contribuer** | Demandé, pas encore écrit. C'est par là qu'on commence. |

## 1. La fiche

Chaque appareil a une fiche dans [library/index.json](../library/index.json). L'appli la lit sur GitHub, avec une
copie locale quand tu es hors ligne.

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

## 2. Le code

Aujourd'hui, un module est un crate Rust compilé dans Rustudio, sur le modèle de
[crates/goxlr](../crates/goxlr) :

1. **Le transport** : la façon de parler à l'appareil (pilote du fabricant, HID, DDC/CI…), derrière un trait,
   comme `Tusb` pour la GoXLR.
2. **La fausse version** : un simulateur qui implémente le même trait et reproduit les pièges du vrai matériel.
   C'est lui qui permet de tout tester sans l'appareil, et de relire ta pull request.
3. **Le protocole** : les commandes, construites octet par octet, sans état métier.
4. **Les tests** : chaque comportement sur le simulateur, et chaque constat fait sur le vrai matériel ajouté au
   simulateur pour qu'il ne revienne jamais.

Prochaine étape du projet : des modules téléchargeables depuis la bibliothèque, isolés (WebAssembly) et limités à
leur appareil. Les modules écrits aujourd'hui y passeront sans être réécrits.

## 3. Découvrir le protocole de ton appareil

- Cherche d'abord un projet libre qui le pilote déjà (OpenRGB, libratbag, goxlr-utility, ddcutil…) et respecte
  sa licence.
- Sinon, observe ce qu'envoie le logiciel officiel : [Wireshark](https://www.wireshark.org) avec USBPcap sous
  Windows. Note chaque commande et sa réponse dans le simulateur.
- **Jamais de mise à jour de firmware** dans un module communautaire : une commande mal comprise peut rendre
  l'appareil inutilisable. Les firmwares restent l'affaire du fabricant.

## 4. Proposer ton module

1. Ouvre une issue « Nouvel appareil » (modèle fourni) pour annoncer ton travail.
2. Fork, branche `appareil/<id>`, puis le code, le simulateur, les tests et la fiche.
3. `scripts/check.sh` doit passer : tests, et aucune donnée personnelle (numéro de série de ton appareil, ton nom,
   tes chemins de machine…).
4. Ouvre la pull request avec le modèle fourni : ce que tu as testé sur le vrai appareil, et sur quel firmware.

## Pour un agent de code

Lis [AGENTS.md](../AGENTS.md) avant tout. Ensuite : simulateur d'abord, protocole ensuite, interface en dernier ;
une pull request par appareil ; jamais de donnée de la machine sur laquelle tu travailles.
