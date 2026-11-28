# Module Micro

Dans **Mes appareils > Mon micro**, choisis ton micro USB, ton micro-casque, ta webcam ou ton interface audio. Tu peux régler le volume Windows, couper le micro ou le choisir comme entrée par défaut pour les applis et les appels. Une nouvelle entrée démarre avec sa voix naturelle.

La même page accueille le micro de la **GoXLR Mini**. **Entrée** regroupe le gain et les options de branchement, dont le choix du modèle et l’alimentation 48 V. **Filtres** rassemble les effets. Clique sur le nom d’un filtre pour ouvrir ses réglages sous son interrupteur. Dans **Préréglages**, **Analyser ma voix** mesure ton niveau et ta pièce sur demande.

**Options Windows** donne accès au volume de l’entrée et au choix du micro par défaut. Tu peux aussi retirer un effet Rustudio déjà branché sur cette entrée.

Sur la GoXLR, **Écoute → M’écouter en direct** envoie ton micro dans la sortie casque de la GoXLR, à 5 % au départ. Le curseur permet d’aller jusqu’à 100 %. **Couper le retour** ou quitter la page Micro arrête l’écoute. Si le micro capte un sifflement fort et persistant, Rustudio ramène le retour à 5 %, puis le coupe si le sifflement persiste.

Dans **Écoute**, **M’enregistrer (8 s)** capture la voix, puis **Réécouter doucement** joue la prise à niveau limité sur ta sortie Windows. La prise et la réécoute ont chacune leur bouton d’arrêt.

## Régler la voix

1. Sélectionne le micro, puis **Activer le traitement de ma voix**. Le branchement redémarre brièvement le service audio de Windows. Rustudio vérifie que Windows appelle l’effet en ouvrant uniquement cette entrée.
2. Choisis **Streamer**, **Radio**, **Clair**, **Adoucir**, **Naturel** ou **Voix chaude**. Le gain du micro se règle séparément.
3. Règle le seuil du **noise gate** pour atténuer le clavier dans les pauses. Le **compresseur** réduit les écarts de volume. Le **limiteur** retient les crêtes au plafond choisi. Les réglages avancés donnent accès aux temps, au ratio et à l’atténuation des sifflantes. Chaque indicateur montre la réduction en dB pendant la capture.
4. Si nécessaire, active **Suppression du bruit (IA)** sur cette entrée. Son activation ou son arrêt rouvre le graphe audio. L’intensité et le maintien s’appliquent pendant la capture.

Les réglages sont enregistrés par entrée dans `micro.json`. **Retirer** restaure les propriétés Windows sauvegardées. Si le pilote a remplacé l’une de ces propriétés, Rustudio conserve la nouvelle valeur du pilote. Les réglages personnels restent disponibles pour un branchement ultérieur.

## Chaîne et délai

Sur une entrée compatible, l’effet traite la voix dans cet ordre.

```text
RNNoise (facultatif) → gain et coupe-bas → gate → égaliseur 5 bandes
                     → compresseur → de-esser → limiteur à anticipation
```

La dynamique retarde le signal de 72 échantillons à 48 kHz, soit **1,5 ms**. RNNoise ajoute deux trames de 10 ms à cette fréquence. Le pilote, Windows et l’application qui enregistre ont leurs propres délais. Un micro déjà saturé à l’entrée reste saturé après le traitement.

Le calcul accepte les formats flottants mono et stéréo aux fréquences entières du moteur audio Windows, de 10 Hz à 384 kHz. À très basse fréquence, la durée d’un seul échantillon augmente le délai minimal. Les applications qui contournent les effets système, notamment certains chemins exclusifs, peuvent recevoir l’entrée brute. Un pilote dont tous les emplacements compatibles sont occupés peut refuser le branchement.

## Restauration et anciens réglages

Si Windows retire le branchement après une mise à jour, la page le signale. Tu peux le rétablir en cliquant. En cas de faute détectée dans l’effet, le moteur tente de restaurer la configuration sauvegardée et indique le résultat.

La migration d’une configuration Rustudio utilisant Equalizer APO se fait sur le micro choisi. Un branchement tiers non identifié est conservé. L’égaliseur du casque continue d’utiliser sa propre configuration.

## Vérifications silencieuses

Le banc DSP traite des signaux dans des buffers. Sous Windows, `buffer_host` charge la DLL seule puis dans un agrégat COM et vérifie les réglages, les niveaux et la libération des objets. `registry_check` redirige le registre de son propre processus vers une ruche de test pour contrôler la migration et la restauration.

Dans une VM isolée, `virtual_capture` vérifie que le moteur audio de Windows appelle effectivement la DLL sur l’entrée choisie. Le client indépendant `capture_level` mesure la crête et le niveau efficace reçus en mode partagé. Il conserve les échantillons en mémoire.
