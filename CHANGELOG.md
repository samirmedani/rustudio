# Nouveautés

## 5.6.2

- **GoXLR Mini : synchronisation des faders corrigée.** Le moteur ne renvoie plus au matériel les niveaux déjà appliqués par les faders, ce qui pouvait écraser un mouvement récent et laisser un niveau bloqué.
- Les mouvements continus sont suivis sans attendre l’immobilisation, avec filtrage des pics isolés et butées exactes à 0 et 100 %.
- Un curseur manipulé à la souris reprend automatiquement la dernière valeur reçue après le glissement. La courte protection du geste ne laisse plus le curseur figé.
- L’interface explique la priorité : un réglage logiciel change le volume sans déplacer la poignée non motorisée ; le prochain mouvement physique reprend la main.

- **Modules** : le statut « Dans Mes appareils » reste à gauche de la carte et le bouton « Ouvrir » à droite, avec un espacement adapté à la largeur disponible.

## 5.6.1

- **Souris : modifier, puis enregistrer.** Les DPI, le palier, la fréquence et les options se modifient immédiatement à l’écran sans envoyer de commande. Un bouton **Enregistrer** apparaît pour appliquer tous les changements ensemble ; **Annuler** les abandonne.
- Les changements en attente sont conservés pendant les actualisations et en cas d’erreur. Un changement de profil empêche de les appliquer par erreur à un autre profil.
- L’enregistrement vérifie uniquement les réglages modifiés, au lieu de relire toute la souris avant et après chaque changement. Les écritures ne sont pas renvoyées automatiquement.
- Un seul message de réussite après l’enregistrement, sans confirmation à chaque modification d’un contrôle.


## 5.6.0

- **R5 Ultra : nouveau dashboard**. Batterie et charge, DPI actifs, fréquence, connexion et profil visibles d’un coup d’œil. Sensibilité et capteur se règlent côte à côte lorsque la fenêtre le permet.
- Les six paliers X/Y occupent moins de place ; le palier actif est surligné. Le libellé « Palier actif » et son sélecteur disposent d’un vrai espacement. Les saisies non enregistrées sont signalées et restent conservées en cas d’erreur.
- Icônes de batterie, capteur et connexion, interrupteurs accessibles et animations discrètes respectant la réduction des mouvements.
- Les modules indiquent leur origine et leur créateur : **Officiel Rustudio** ou **Communautaire**, avec accès à la PR validée. Le catalogue filtre ces origines et explique comment contribuer.
- Le catalogue est livré avec chaque version : aucune nouvelle fiche distante non validée ne s’affiche. Les modules communautaires exigent une PR GitHub fusionnée, la validation du mainteneur et une crate identique à la révision relue avant publication.
- Espacements et retours à la ligne améliorés dans les cartes, les actions et la recherche du catalogue.
- Les pages audio exploitent aussi la largeur utile quand la navigation est repliée. Changer de menu revient en haut de la nouvelle page.


## 5.5.0

- Nouveau module **Attack Shark R5 Ultra** : batterie, DPI X/Y par palier, fréquence, veille et réglages du capteur dans un espace dédié. DPI, fréquence et délai anti-double clic vérifiés sur la vraie souris en 2,4 GHz, puis restaurés.
- Les lectures HID perdues sont réessayées sans renvoyer les écritures. Les réglages sont relus avant d’annoncer leur application ; une saisie DPI reste disponible en cas d’erreur.
- Les paliers existants, y compris celui à 42 000 DPI, sont préservés. Les interfaces souris/clavier et les modes de mise à jour de firmware ne sont pas ouverts.
- Bluetooth, macros et remappage des boutons ne font pas partie de ce premier module.

- Le délai entre deux recherches manuelles de mise à jour passe de 60 à 5 secondes. Les recherches ne déclenchent aucune installation.


## 5.4.2

- La croix de retrait des appareils est centrée dans son bouton, avec une icône indépendante de la police système.


## 5.4.1

- Les cartes de Mes appareils s’animent au survol et s’ouvrent au clic, avec accès au clavier.
- Une croix discrète remplace le bouton de retrait ; une confirmation protège contre les clics accidentels.
- La confirmation explique la conservation des réglages et propose leur export ou import pour la GoXLR Mini.


## 5.4.0

**Nouveau**
- Rustudio s’ouvre sur **Mes appareils** : retrouve ton matériel, son état de connexion et ses réglages au même endroit.
  Les appareils détectés sont ajoutés à ton espace ; tu peux aussi les ajouter ou les retirer depuis les modules.
- Chaque module possède son propre espace. La GoXLR Mini regroupe son mixeur, son micro, son casque, son routage,
  son éclairage et ses scènes. La navigation se construit à partir des pages déclarées par le module.
- Le catalogue **Modules** distingue les modules livrés avec Rustudio et ceux encore à contribuer. La GoXLR Mini
  est le premier module livré ; les autres appareils ne sont pas présentés comme déjà compatibles.

**Amélioré**
- L’état connecté, débranché ou en erreur est affiché sur la carte de l’appareil concerné. L’accueil reste utilisable
  sans GoXLR ; le micro et les outils audio se trouvent dans son espace.
- L’accueil et le catalogue lisent le moteur quatre fois par seconde, au lieu de vingt. Les faders conservent leur
  suivi rapide dans l’espace GoXLR.
- Le menu près de l’horloge donne accès aux appareils et aux modules ; les raccourcis audio sont regroupés sous
  GoXLR Mini. Les réglages audio et les anciennes adresses des pages sont conservés.

## 5.3.0

**Nouveau**
- Nouvelle icône Rustudio : les faders et le signal audio se rejoignent dans un « R ». La même identité accompagne
  l’application, son installeur et la barre latérale.

**Amélioré**
- Les curseurs se règlent aussi au clavier : flèches pour ajuster, Début et Fin pour les extrêmes. Leur nom et leur
  valeur sont annoncés aux lecteurs d’écran. La molette ne change plus le son au simple passage de la souris.
- Les fenêtres de dialogue gardent le focus au clavier, puis le rendent au contrôle qui les a ouvertes.
  Le réglage Windows qui réduit les animations est respecté.

**Corrigé**
- Une sauvegarde refusée par le moteur ou interrompue par une coupure de connexion n’annonce plus de faux succès.
  Le nom saisi pour une scène ou un profil reste disponible pour réessayer.
- Si le disque ou le dossier des réglages est temporairement indisponible, Rustudio réessaie de sauvegarder
  automatiquement. Les derniers réglages ne sont plus oubliés faute d’une nouvelle action.

## 5.2.4

**Corrigé**
- Le fader physique pilotait le volume et l'éclairage une fois sur deux : la position du potentiomètre oscille de
  quelques unités, et Rustudio exigeait trois lectures strictement identiques. Une position intermédiaire
  n'était donc presque jamais retenue, la barre restait figée et le volume ne bougeait pas. La comparaison
  tolère maintenant ce bruit, tout en ignorant un pic isolé.
- Quand un fader bouge, Rustudio renvoie la nouvelle position au mixeur de la GoXLR, comme le fait le logiciel de
  référence. Avant, la position était enregistrée dans l'appli mais l'appareil gardait son ancien volume.

## 5.2.3

**Corrigé**
- Un micro ou une sortie qui refuse le test ne reçoit plus de faux résultat « rien de perdu ». Chaque voie
  non mesurée est nommée, avec la raison ; seules les voies effectivement analysées sont validées.
- Si Windows refuse l'accès au journal des coupures ou ne l'active pas, le diagnostic affiche « indisponible ».
  Une absence d'événement enregistré n'est plus présentée comme la preuve que tout le son est resté intact.
- Une erreur pendant la lecture des échantillons arrête la mesure et remonte sa cause. Les formats non reconnus
  sont signalés au lieu d'être interprétés comme du silence ; le son de test fonctionne aussi en PCM 24 bits.
- Les sorties affichent leurs interruptions et leur saturation ensemble quand les deux sont présentes.
- Les positions de paquets marquées invalides par Windows ne créent plus de faux trous dans le son : le test
  signale que leur horodatage est incertain. Les tampons vides ne sont plus comptés comme des paquets reçus.

## 5.2.2

**Corrigé**
- La surveillance des crépitements écrit son historique et son journal depuis un fil séparé, sans garder le
  verrou des compteurs USB. Un disque lent ne retarde plus les commandes de la GoXLR à cause de ces écritures.
  Une coupure arrivée pendant une sauvegarde reste bien enregistrée à la suivante ; une écriture ratée est réessayée.
- « Son qui crépite ? » distingue l'absence de flux audio et une mesure devenue indisponible d'une absence de
  coupure relevée. Les pertes vers le casque et vers le PC (micro, stream) sont affichées séparément.
- Les programmes lancés au moment d'une coupure sont présentés comme des pistes à vérifier, sans leur attribuer
  automatiquement la panne.
- Les coupures de plus de 24 heures sont retirées du fichier d'historique même si aucune nouvelle coupure ne survient.

## 5.2.1

**Amélioré**
- L'historique des coupures du son survit aux redémarrages du PC et aux mises à jour : « Ces dernières 24 heures »
  reste complet. C'est précieux quand un crépitement n'arrive qu'une fois par jour.
- La fenêtre « Son qui crépite ? » nomme le programme revenu le plus souvent au moment des coupures. Une tâche
  qui tourne toutes les 30 minutes se repère ainsi d'un coup d'œil.
- « Mise à jour proposée » : les nouveautés des versions que tu as sautées sont séparées par un trait et un petit
  titre, au lieu d'être à la suite du reste.

## 5.2.0

**Nouveau**
- Rustudio compte les crépitements tout seul. Le pilote de la GoXLR note chaque milliseconde de son qu'il n'a pas
  pu faire passer par l'USB ; Rustudio relit ce compteur chaque seconde, sans son de test et sans rien envoyer à la
  GoXLR. « Son qui crépite ? » affiche les coupures des dernières 24 heures, à la seconde près, avec le son perdu.
- À chaque coupure, Rustudio note aussi les programmes que Windows venait de lancer. Une rafale de lancements
  (synchronisation, compilation, mise à jour) suffit à faire sauter le son USB : tu vois laquelle décaler.
- Le test « Son qui crépite ? » est confirmé par ce compteur : le pilote dit lui-même s'il a perdu du son pendant
  le test.

**Amélioré**
- Mise à jour proposée : si tu as sauté des versions, tu lis aussi leurs nouveautés, à la suite.
- Chaque version est testée en se mettant à jour depuis la précédente, avec l'updater de celle-ci, avant et après
  sa publication. Si la mise à jour réelle échoue, la version n'est plus proposée.

**À savoir**
- Tu viens de la 5.0 ? La 5.1 a ajouté « Son qui crépite ? » (test de 30 secondes ou 5 minutes, port USB
  conseillé, journal Windows des coupures), l'alerte de l'égaliseur au-delà de +12 dB de basses, et une interface
  qui s'endort quand sa fenêtre est cachée.

## 5.1.0

**Nouveau**
- « Son qui crépite ? » (menu ⋯ et Diagnostic) : un test de 30 secondes ou de 5 minutes, à lancer pendant que
  tu joues ou écoutes de la musique. Rustudio fait l'aller-retour d'un son aigu discret entre le PC et la GoXLR
  par le Stream Mix : tu ne l'entends pas, Discord non plus. Il écoute aussi chacune de tes voies. Le verdict est
  donné en clair : coupures entre le PC et la GoXLR, données perdues à l'entrée, coupures notées par Windows,
  niveaux et saturation de chaque voie, micro qui sature.
- Port USB conseillé : quand le test trouve des coupures, Rustudio regarde sur quel contrôleur USB est branchée
  la GoXLR et avec combien d'appareils elle le partage. Il désigne un meilleur port en nommant l'appareil déjà
  branché dessus (« à la place de ta webcam ») : sur les PC AMD, il sait reconnaître les ports reliés
  directement au processeur.
- Le journal Windows des coupures du son, éteint d'usine, est allumé par Rustudio. La fenêtre du test résume
  les dernières 24 heures : un crépitement rare se voit même après coup.
- Égaliseur du casque : au-delà de +12 dB de basses, Rustudio prévient. Le reste du son est alors baissé
  d'autant, le volume finit au maximum, et le casque grésille sur les sons forts. Le bouton « Adoucir »
  ramène tes réglages à +10 dB au plus.

**Amélioré**
- Fenêtre cachée ou réduite, l'interface s'endort : plus de rendu ni d'animation, une lecture du moteur toutes
  les 2 secondes au lieu de 20 par seconde, et plus de lecture du vumètre. Avant, elle occupait
  jusqu'à la moitié d'un cœur du processeur en arrière-plan.
- Le fond de l'interface ne s'anime plus : même affichée sur un second écran pendant un jeu, elle ne charge
  plus la carte graphique.
- Le fichier temporaire laissé par GoXLR Mini dans le dossier d'Equalizer APO est retiré.

## 5.0.0

GoXLR Mini devient **Rustudio**, réécrit en Rust.

**Nouveau**
- Moteur et appli réécrits en Rust. Le moteur tourne en service Windows, relancé tout seul s'il s'arrête ou se fige
  dans le pilote ; l'appli passe à Tauri 2. L'installeur pèse moins de 10 Mo au lieu de 59, et le moteur démarre en
  une fraction de seconde.
- Mises à jour proposées : « Mise à jour proposée » apparaît en bas à gauche. Tu lis les nouveautés, bien mises en
  forme, puis tu confirmes. Rien ne s'installe sans ton clic.
- Anti-écho : une fenêtre réunit tout ce qui peut faire entendre une voix en double (routage de la GoXLR, micro et
  sortie que Windows donne à Discord et aux jeux, réglages Discord), avec une correction en un clic, et un vrai
  test : trois bips joués dans Discord, ton micro écouté, écho mesuré.
- Bibliothèque d'appareils : les appareils pris en charge et ceux à contribuer. Chacun peut proposer le module de
  son matériel (souris, clavier, écran…) sur GitHub ; le guide est dans docs/MODULES.md.
- Nouvel installeur Rustudio-Setup.exe : un seul fichier, fenêtre claire, désinstallation dans Paramètres ›
  Applications (tes réglages sont gardés).

**À savoir**
- Depuis GoXLR Mini 4.x, clique « Mettre à jour » : Rustudio s'installe à sa place et reprend tes réglages (voix,
  scènes, couleurs, égaliseur du casque). GoXLR Mini n'est retiré qu'une fois ta GoXLR reprise par Rustudio ;
  sinon, il revient tout seul.
- Les mises à jour viennent maintenant du dépôt samirmedani/rustudio.

## 4.8.0

**Nouveau**
- Page Micro, « Traitements de ta voix » : égaliseur, noise gate, compresseur et de-esser ont chacun leur
  interrupteur. Tous sont coupés par défaut : tu pars de ta voix naturelle et tu allumes ce que tu veux.
  Un traitement coupé garde ses réglages, mais rien n'en part vers la GoXLR.
- « M'entendre dans le casque » : le retour micro devient un interrupteur, coupé par défaut, avec son volume.
- Nouveau préréglage « Naturel » (aucun traitement), proposé en premier ; l'ancien « Naturel » s'appelle « Léger ».

**Corrigé**
- Le noise gate des préréglages pouvait couper la voix dès qu'elle baissait un peu, au point qu'on ne
  t'entendait plus. Il se place maintenant bien sous tes passages calmes et ne coupe plus net (24 dB au
  plus, sauf Anti-bruit).
- « Régler ma voix » cale le gain et prépare des réglages doux, sans plus allumer de traitement tout seul.
- Les scènes intégrées ne changent plus ni les traitements de ta voix, ni le retour micro.
- Après la mise à jour, les traitements et le retour micro sont coupés une fois : tu retrouves ta voix
  naturelle et tu rallumes ce que tu veux.

## 4.7.0

**Nouveau**
- Anti-écho Windows : l'appli surveille les périphériques son par défaut de Windows. Si Discord et tes jeux
  enregistrent « Broadcast Stream Mix » (le mix du stream, qui contient déjà le son de tout le monde) au lieu de
  ton micro, une alerte s'affiche avec « Corriger » : Windows reprend « Chat Mic » en un clic.
- Même vérification pour la sortie : si le son du PC part ailleurs que sur la GoXLR, faders, mute et égaliseur
  du casque ne s'y appliquent pas ; « Passer par la GoXLR » le remet sur « System » (et « Chat » pour les
  communications). Un choix déjà fait sur une voie GoXLR (Jeu, Musique…) est gardé.
- Le diagnostic affiche la sortie et le micro utilisés par Windows, et les ajoute au rapport copié.

## 4.6.0

**Nouveau**
- Page **Casque** : un égaliseur pour tout le son du PC qui passe par la GoXLR. Choisis ton casque pour le
  ramener à la courbe de référence Harman (corrections AutoEq de 21 casques courants), ajoute un style (Basses +,
  Jeu, Voix claire, Film, Musique, Doux), règle finement sur 10 bandes et enregistre tes profils.
- Un clic installe Equalizer APO (logiciel libre, téléchargé depuis sa page officielle, empreinte vérifiée) et le
  branche sur les voies GoXLR choisies ; GoXLR Mini le rebranche tout seul si la GoXLR change de port USB.
- Filtres sans latence, volume baissé juste ce qu'il faut pour ne jamais saturer. Le stream recevant aussi ce son,
  l'égaliseur se coupe tout seul quand OBS est ouvert (réglable).

## 4.5.0

**Nouveau**
- « M'écouter » : enregistre-toi 8 secondes puis réécoute ta voix telle que l'entendent Discord et ton stream,
  ou écoute-toi en direct dans le casque avec un réglage de volume.

**Corrigé**
- Le moteur envoie maintenant le réglage des sous-mixages (casque et sorties sur le mix A) sur les firmwares
  Mini 1.2.0.46 et plus récents, comme goxlr-utility. Il manquait pour le retour micro dans le casque.
- Une position de fader lue une seule fois ne change plus le volume d'une voie, et l'appli n'affiche plus
  « Fader physique à 0 % » quand la GoXLR ne remonte pas la position de ses faders.
- Les scènes intégrées ne remplacent plus le préréglage de ta voix une fois qu'elle a été mesurée.
- Le diagnostic USB ne reste plus sur « appareil en échec » quand la GoXLR revient juste à ce moment-là.

## 4.4.0

**Nouveau**
- « Régler ma voix », sur la page Micro : en vingt secondes, l'appli trouve ton micro (elle essaie le 48 V pour
  un micro statique), écoute ta voix puis le silence de ta pièce, et cale le gain, le noise gate et le
  compresseur sur ces mesures. Elle signale aussi ce qui fait de l'écho (Discord renvoyé vers Discord, enceintes,
  retour micro trop fort) avec une correction en un clic.
- Préréglage « Studio » pour les micros statiques : voix nette et posée, graves allégés, présence, de-esser. Une
  fois ta voix mesurée, chaque préréglage place ses seuils par rapport à elle, et ils suivent le gain.
- « M'écouter » : ta voix traitée dans le casque pendant 15 secondes, puis tout revient comme avant.
- Quand la GoXLR n'apparaît pas, l'appli dit ce que Windows voit sur l'USB : rien (câble qui ne fait que
  charger, port non relié), un appareil qui n'arrive pas à se présenter, ou une GoXLR complète.

**Corrigé**
- Le moteur répond tout de suite même quand le pilote USB tarde à se charger (au réveil du PC par exemple), et
  le diagnostic indique l'étape où il bloque.

## 4.3.0

**Nouveau**
- Nouvelle interface claire, dans l'esprit d'Apple : barre latérale en verre, grands titres, police Inter,
  icônes Lucide, fond lumineux. Le mixeur montre quatre faders en capsule, un bouton rond pour couper chaque
  voie (orange quand elle est coupée d'une seule sortie) et les autres voies dans une liste groupée.
- La fenêtre se redimensionne et l'interface suit : sur une petite fenêtre, la barre latérale devient une
  colonne d'icônes et les panneaux passent les uns sous les autres.
- Le menu ⋯ est dans l'en-tête, à côté de l'état du micro. Le menu de l'icône de notification passe aussi en clair.

**Corrigé**
- Fenêtre de mise à jour : un point des notes écrit sur plusieurs lignes s'affiche d'un seul tenant, comme sur
  GitHub, au lieu d'être coupé en morceaux.

## 4.2.0

**Nouveau**
- Installeur en un seul fichier, `GoXLR-Mini-Setup.exe` : un clic sur « Installer », Windows demande
  l'autorisation, puis « Ouvrir GoXLR Mini ». Il suit le thème clair ou sombre de Windows, prévient si le pilote
  GoXLR manque ou si GoXLR Utility est ouvert, et sait mettre à jour une version déjà installée. Le ZIP reste
  proposé à côté.
- Installation sans fenêtre : `GoXLR-Mini-Setup.exe /quiet` (options `/nodesktop` et `/nostartup`).

**Corrigé**
- `install.ps1` rend un code de sortie fiable et garde l'option `-Quiet` quand il redemande les droits
  administrateur.

## 4.1.5

**Corrigé**
- Installeur : le raccourci arrive sur le vrai Bureau du compte, même s'il est redirigé (OneDrive par exemple).
- Désinstallation depuis « Programmes et fonctionnalités » : retire aussi le démarrage avec la session et le
  raccourci du compte choisi à l'installation, même lancée depuis un autre compte administrateur.

## 4.1.4

**Corrigé**
- Les fichiers téléchargés pour une mise à jour sont supprimés dans la minute qui suit, même quand le message
  « installé » a déjà été lu.

## 4.1.3

**Corrigé**
- Après une mise à jour, l'appli se relance d'elle-même sur la nouvelle version. Elle lisait la version de son
  exe dans la mémoire du programme déjà ouvert et croyait n'avoir rien de nouveau. Les applis 4.1.0 à 4.1.2 encore
  ouvertes sont fermées puis rouvertes par l'installation.
- Le message « GoXLR Mini … est installé » s'affiche bien après la mise à jour.
- Les fichiers de la mise à jour (environ 200 Mo) sont supprimés dans la minute, et non plus 6 heures plus tard.

## 4.1.2

**Corrigé**
- La mise à jour intégrée restait sur « Installation en cours » sans rien installer : la tâche d'installation
  cherchait ses instructions au mauvais endroit. Elle installe maintenant la nouvelle version.
- Si l'installation ne démarre pas, GoXLR Mini le dit au bout de 90 secondes (rien n'est modifié) et propose
  de réessayer, au lieu d'attendre sans fin.
- Resté bloqué sur « Installation en cours » en 4.1.0 ou 4.1.1 ? Redémarre Windows une fois, puis lance la
  mise à jour : la 4.1.2 s'installe normalement.

## 4.1.1

**Corrigé**
- Les notes de version gardent leurs accents dans la fenêtre de mise à jour et sur GitHub.

## 4.1.0

Première version publique.

**Nouveau**
- Mise à jour intégrée : GoXLR Mini cherche les nouvelles versions sur GitHub, les annonce une fois et les
  installe en un clic (empreinte vérifiée, retour arrière automatique si besoin).
- Installeur : `Installer.cmd`, moteur lancé au démarrage de Windows, désinstallation propre.
- Couper une voie d'une seule sortie (stream, Discord, casque, enceintes) ; appui long = couper partout.
- Bouton micro : bascule, maintenir pour couper, appuyer pour parler.
- VOD sans musique, scènes automatiques selon l'appli ouverte, incrustation OBS, diagnostic en direct.
- Appli Windows : icône d'état, bulle à l'écran quand le micro change, relance du moteur s'il ne répond plus.

**Corrigé**
- Le logiciel se figeait au bout d'une heure environ (index de commande USB sur 16 bits jamais remis à zéro) :
  plus rien ne répondait et les voyants des boutons ne bougeaient plus.
