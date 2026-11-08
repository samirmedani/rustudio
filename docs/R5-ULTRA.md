# Module Attack Shark R5 Ultra

Livré avec Rustudio 5.5.0. Les lectures et les changements de DPI, fréquence et délai anti-double clic ont
été vérifiés sur une vraie R5 Ultra en **2,4 GHz**, puis les réglages d’origine ont été restaurés et comparés.
Le mode USB est implémenté et testé sur le simulateur ; il n'a pas été vérifié physiquement pendant ces essais.
Aucun firmware n'est modifié et aucun pilote supplémentaire n'est installé.


Le dashboard regroupe la batterie et la charge, les DPI actifs, la fréquence et la connexion en tête.
Les six paliers X/Y et les options du capteur se répartissent en colonnes selon la largeur disponible ;
les contrôles préparent un brouillon local. Le bouton **Enregistrer** applique les changements ensemble ;
**Annuler** les abandonne. Une actualisation conserve les changements en attente. Les indicateurs en tête
représentent les dernières valeurs enregistrées, pas les choix encore en attente.

## Fonctions implémentées

- Batterie et charge, version du firmware, profil matériel courant, état USB / 2,4 GHz.
- Lecture des paliers DPI X/Y, modification d'un palier, sélection du palier actif.
- Fréquence 125–8 000 Hz (maximum 1 000 Hz en filaire).
- LOD 0,7 / 1 / 2 mm, délai anti-double clic 0–30 ms, veille, correction des lignes, Motion Sync, Ripple Control.
- Limite du modèle à 42 000 DPI, par pas de 50. Une sonde a lu une limite native de 30 000 alors qu’un palier overclocké de 42 000 est enregistré. La commande de limite native étant intermittente sur ce firmware, elle ne fait plus partie des lectures du module. Les paliers existants sont préservés.
- Enregistrement groupé : validation complète avant écriture, puis relecture des seuls champs modifiés.
  Aucun message de succès avant confirmation ; un échec peut avoir appliqué une partie du groupe, et conserve le brouillon.

Bluetooth, remappage des boutons, macros, éclairage et flash du firmware ne sont pas implémentés.
Les préférences restent dans la souris : Rustudio lit l'état au démarrage et à la reconnexion, sans appliquer
un profil par défaut et sans recopier l'état de la GoXLR.

## Transport et état

VID `373E`, PID `0046` (câble) ou `0047` (récepteur), usage page `FFFF` uniquement. Les interfaces souris et
clavier ainsi que les PID de bootloader sont exclus. Aucun pilote supplémentaire n'est installé.

`crates/attack-shark` contient le protocole, les bornes de validation et la fausse souris. Le backend Windows
utilise `hidapi` avec `windows-native`. Le report HID compte 64 octets, précédés du report ID 0 dans les appels
Windows. Les réponses sont contrôlées : marqueur, adresse, cible, commande et longueur. Un accusé `A0` n’est jamais pris pour une configuration valide. Les lectures perdues sur la liaison radio sont renvoyées au maximum trois fois ; les écritures ne sont pas renvoyées automatiquement. Une écriture dont l’accusé est perdu n’est déclarée réussie que si sa valeur est relue correctement.

`crates/engine/src/mouse.rs` possède son propre fil HID, sa file bornée et son cache. L'API lit le cache ; les
commandes passent par ce fil. Le fil USB GoXLR n'est pas utilisé. Réessai après déconnexion ; lecture périodique
à faible fréquence quand la souris répond. Aucune écriture automatique à la connexion.

- `GET /api/modules/attack-shark-r5-ultra/state`
- `POST /api/modules/attack-shark-r5-ultra/refresh`
- `POST /api/modules/attack-shark-r5-ultra/settings` avec `{"profile":1,"changes":{"polling_hz":1000,"debounce_ms":4}}`
- Ancienne route compatible : `POST /api/modules/attack-shark-r5-ultra/setting` avec `{"name":"polling_hz","value":1000}`

Les POST gardent les protections locales de Rustudio. Aucune commande brute HID n'est exposée.

## Sources et attribution

L'implémentation Rust utilise les faits de protocole décrits par le projet [miyoi](https://github.com/ellerotta/miyoi),
notamment [PROTOCOL.md](https://github.com/ellerotta/miyoi/blob/main/docs/PROTOCOL.md), sous licence MIT.
La notice complète est conservée dans `crates/attack-shark/LICENSE-miyoi` et dans les notices distribuées.
La limite de 42 000 DPI est également documentée par le [fabricant](https://attackshark.com/products/attack-shark-r5ultra-carbon-fibre-wireless-8k-paw3950max-gaming-mouse).

La documentation miyoi indique des lectures et écritures vérifiées sur une R5 Ultra ; cela ne remplace pas
la validation propre à Rustudio sur Windows.

## Vérification sans installation

`cargo test -p rustudio-attack-shark` teste trames, limites, commandes, valeurs ignorées et déconnexion.
Les tests d'API vérifient la séparation avec l'audio et le refus d'actions étrangères au module.

L'exemple Windows `probe` lit la souris sans installation. Le mode `--wait --verify-write` attend son réveil,
allonge temporairement la veille, vérifie DPI, fréquence et délai anti-double clic, puis restaure l’état initial.
En cas d’erreur de lecture, les restaurations sont aussi tentées directement avec les valeurs sauvegardées.
Ne jamais lancer plusieurs sondes en parallèle.
