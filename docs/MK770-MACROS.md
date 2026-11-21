# Macros embarquées du MK770

> **Expérimental.** L’envoi des macros dans le clavier n’a pas encore été validé sur un vrai MK770.
> Garde une copie de tes macros sur le PC ou en JSON avant de les envoyer.

Rustudio implémente son propre éditeur et le protocole de programmation des macros du MK770 en USB.
Le clavier exécute les séquences ; aucun moteur d’injection de touches n’est nécessaire dans Rustudio.
Les commandes de programmation sont réservées au câble ; le récepteur 2,4 GHz et le Bluetooth ne sont
pas pris en charge par ce module. La programmation et la lecture sont testées en simulation ; la
persistance après coupure et l’exécution physique restent à valider sur le matériel.

## Utilisation

Ouvre **MK770 → Macros**. Crée une séquence en enregistrant les touches dans la zone de capture,
ou ajoute manuellement les appuis et relâchements. La capture ajoute à la fin de la séquence ;
Échap ou un clic hors de la zone la termine et ajoute les relâchements manquants. Elle ne capte
pas les frappes dans les autres applications. Les raccourcis réservés à Windows et certaines
touches spéciales peuvent nécessiter l’édition manuelle.

Sous Windows, la capture utilise les codes physiques des messages de sa propre fenêtre, via
un hook `WH_GETMESSAGE` limité au fil graphique et retiré à la fin de la capture. Les touches du
pavé numérique et les côtés des modificateurs conservent ainsi leur identité, indépendamment
de la disposition AZERTY/QWERTY. Aucun hook global, aucune interception d’un autre processus.
Références : [GetMsgProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/getmsgproc),
[SetWindowsHookExW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw).

Sélectionne un événement pour modifier sa touche ou son délai, le déplacer ou le supprimer.
Choisis **Une fois**, **Tant que la touche est tenue**, ou **Un appui démarre, un autre arrête**.
Un appui et un relâchement comptent séparément ; les délais précèdent les événements.
La validation refuse les touches laissées appuyées, les relâchements orphelins et les doublons d’appui.

**Enregistrer sur ce PC** conserve la macro dans une bibliothèque locale de 64 entrées au plus,
indépendante du clavier. Les fichiers JSON d’import/export sont versionnés et validés ; importer ne
programme rien. Les modifications non enregistrées restent dans l’éditeur tant que la fenêtre reste ouverte.

Relis les macros du clavier, puis choisis un emplacement et une touche avant **Envoyer au clavier**.
L’action habituelle de cette touche est remplacée dans le profil actif. Remplacer un emplacement
rétablit ses anciennes touches puis affecte la nouvelle. Une autre affectation déjà présente sur la
touche choisie n’est pas écrasée. **Retirer du clavier** supprime la macro et restaure la touche par défaut.
La bibliothèque du PC reste indépendante. Change de profil avec les raccourcis du clavier, puis relis les macros.

## Origine des faits et limites

Analyse statique du paquet officiel [MasterPlus+ 1.9.6](https://www.coolermaster.com/en-global/masterplus.html),
distribué par Cooler Master : `CMUOT.dll` et ses symboles `CMUOT.pdb`. Aucun composant propriétaire n’est
distribué, chargé ni exécuté par Rustudio. Aucune dépendance à MasterPlus+ ou à son pilote.
SHA-256 de la DLL étudiée : `64f02ab1950924c816c879a417ce2d280d128fcb0161736528e4867194f5956e`.

Les structures nommées `mk770_macro`, `mk770_macro_record_t`, `mk770_macro_map_t`, les méthodes
`MK770::setMK770Macro`, `getMK770Macro`, `getMK770MacroAll`, `clearMK770Macro`, `GetKeyIndex`,
`ProtocolWrite`, `ProtocolRead` et `CMUOT_MK770::changeProfileId` établissent le contrat suivant :

| Élément | Format |
| --- | --- |
| Emplacements | 12, identifiants 0 à 11 |
| Taille maximale proposée par le SDK | 70 événements (tampon physique de 104) |
| Enregistrement natif | 355 octets : id, répétition, nombre d’événements, nom sur 40 octets, 104 × 3 octets |
| Nom écrit par Rustudio | UTF-8, 39 octets au plus puis zéro ; aucun tronquage |
| Événement | usage HID sur 8 bits, puis LE16 : délai sur 15 bits et appui sur le bit 15 |
| Répétition | 1 pour une seule lecture, 255 pour maintien/bascule |
| Affectations | 104 × LE16, matrice distincte des LED ; zéro = touche par défaut |
| Code d’affectation | bits 15..12 : mode (3 une fois, 4 maintien, 5 bascule), bits 11..0 : emplacement |
| Profil | registre 804 (lecture uniquement), 0 à 3 |

Rapports USB de 64 octets, sans report ID (Windows ajoute 0). Une trame continue commence par
`opcode, 0x80 | nombre_de_rapports`, puis 62 octets. Les suivants commencent par `opcode, index`
(index 1 à n−1), puis 62 octets. Contrairement aux effets et aux LED, les réponses macros n’ont
pas d’en-tête adresse/longueur de quatre octets.

| Commande | Charge utile |
| --- | --- |
| `04 WRITE_MACRO_DAT` | enregistrement de 355 octets, donc 6 rapports |
| `05 READ_MACRO_DAT` | requête : id ; réponse : enregistrement de 355 octets |
| `06 DELET_MACRO_DAT` | id, après retrait vérifié de ses affectations |
| `08 SET_RECORD_KEY` | index de matrice, nombre de touches, codes LE16 |
| `09 REQ_RECORD_KEY` | requête vide ; réponse de 208 octets, donc 4 rapports |

Le fabricant n’appelle pas de commande de sauvegarde supplémentaire : sa méthode `saveConfig`
du MK770 retourne sans écriture. Les fonctions « reset profil », « reset usine », mise à jour
du firmware et désactivation globale des macros ne sont jamais utilisées.

## Vérification et erreurs

Un seul fil HID du moteur traite les macros et l’éclairage, hors du fil graphique. La connexion
n’écrit aucune macro et la lecture périodique des effets ne déclenche pas la lecture des macros.
Une commande incorrecte est validée avant toute écriture. L’écriture du record est relue avant
de modifier les affectations ; celles-ci sont relues également. Les autres macros, réaffectations
et couleurs sont préservées. En cas d’échec, l’ancien record et les affectations sont restaurés
et relus si le clavier est encore joignable dans le même profil. Un changement de profil ou une
déconnexion interrompt la commande et interdit une restauration dans un autre profil.
Le protocole ne propose pas de transaction atomique entre changement de profil et programmation :
ne change pas de profil pendant un envoi.

API locale protégée par les mêmes contrôles que les réglages :

- `POST /api/modules/cooler-master-mk770/macros`, `{}` : lecture des macros affectées du profil actif.
- `POST /api/modules/cooler-master-mk770/macro-assign` : `{ "slot": 0, "key": 69, "data": { "name": "Exemple", "mode": "once", "events": [{ "key": 4, "down": true, "delay_ms": 0 }, { "key": 4, "down": false, "delay_ms": 50 }] } }`.
- `POST /api/modules/cooler-master-mk770/macro-remove`, `{ "slot": 0 }`.

Réponse réussie : `ok`, état des effets `state`, liste `macros` (id, affectations et séquence).
Une absence de réponse n’est jamais assimilée à une réussite. Les noms invalides et modes inconnus
du firmware sont signalés plutôt que convertis silencieusement.

Tests : `cargo test --locked -p rustudio-core -p rustudio-cooler-master -p rustudio-app`, plus
l’auto-test de l’appli sur moteur simulé. Les trames, les limites, les macros persistantes dans le
simulateur après recréation du client, les refus avant écriture et la restauration après corruption
sont couverts. Ces tests ne prouvent pas l’exécution physique ni la persistance après débranchement.
