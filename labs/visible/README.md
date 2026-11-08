# Banc Windows : mixeur visible et bulle micro

La référence Tauri affiche quatre faders en lecture seule et demande les
positions toutes les 20 ms. Le mixeur GPUI complet s’abonne aux changements
et affiche aussi les commandes et les autres voies. Le test utilise le
port **14566**, avec un autre dossier de données. Il refuse de démarrer si le
port est déjà utilisé et ne contacte pas le service installé sur **14565**.

1. Dans une **copie de compilation jetable** du dépôt, lance
   `python labs/visible/prepare.py`. Cette copie utilise un identifiant Tauri
   distinct, démarre son mixeur sans prendre le focus et désactive le démarrage
   du service installé en cas d'absence du simulateur. Elle embarque le banc
   HTML uniquement dans le moteur de test.
2. Compile `cargo build --release --locked -p rustudio-app -p rustudio-engine`
   pour Windows. Copie `Rustudio.exe` et `rustudio-engine.exe` à côté de
   `measure.py`, sous les noms `RustudioTauriBench.exe` et
   `rustudio-engine-bench.exe`. Ajoute le
   client GPUI (`crates/app`, compilé avec le SDK Windows) sous
   le nom `rustudio-gpui-mixer.exe` (banc Tauri) ou `Rustudio.exe` (bancs suivants).
3. Pour capturer chaque fenêtre indépendamment du bureau, compile la DLL
   avec l’invite de commandes développeur Visual Studio x64 et un SDK Windows
   récent, depuis le dossier du banc :

   ```bat
   cl /nologo /std:c++20 /O2 /LD /EHsc capture.cpp /link d3d11.lib dxgi.lib dwmapi.lib windowsapp.lib /OUT:capture.dll
   ```

4. Dans une session Windows graphique, lance
   `python measure.py --result report.json`. Les fenêtres s’affichent sans
   prendre le focus. Le banc utilise Windows Graphics Capture si la DLL
   est présente, sinon GDI (dans ce cas, laisse la zone du banc dégagée).
   Pour résumer : `python summarize.py report.json`.

Par défaut : trois passages par application, ordre alterné, 22 mouvements
chronométrés par passage, 8 s de repos puis 8 s à 20 mouvements/s répartis
sur les quatre faders. La mémoire privée résidente et le temps CPU additionnent
le processus de l'application et ses enfants, dont WebView2 ; le moteur
simulé est commun et reste hors des totaux. Le CPU est exprimé en pourcentage
d'un processeur logique.

L'ouverture est mesurée du lancement au premier fader visible. Le délai de
réaction va de l'envoi du mouvement simulé au déplacement de la poignée dans
une image de la fenêtre : il inclut le moteur, l'attente de lecture, le rendu
et le coût de capture. Le banc chronomètre séparément les captures et les réponses HTTP.
Il ne conserve aucune image du bureau. Avec
`--rounds 0 --skip-osd --verify-gpui`, il vérifie les commandes du mixeur,
le glissement et le retour du fader simulé. L’option `--screenshot mixer.png`
capture uniquement la zone client du mixeur GPUI. Un passage séparé avec
`--observe-engine --skip-osd` relève la position dans l’API à quelques
millisecondes d’intervalle pour estimer le temps entre la publication par le
moteur et le rendu. Ce passage ajoute une lecture de contrôle ; il est
distinct de la comparaison principale.

Enfin, l'application Tauri isolée démarre près de l'horloge et le moteur
simulé enchaîne trois coupures et trois réactivations du micro. Le banc vérifie
la couleur composée à l'écran, la disparition automatique et le maintien
du focus. Le code Win32 de la bulle est celui de Rustudio.

## Accueil GPUI et profils

Place `desktop_test.py`, `measure.py`, `capture.dll`, le moteur
`rustudio-engine-bench.exe` et `Rustudio.exe` dans le même
dossier. Dans une session Windows graphique, lance `python desktop_test.py`.
Le test utilise un profil et un dossier neufs, débranche et rebranche
la GoXLR simulée, ouvre le mixeur, vérifie le mouvement, saisit un nom,
enregistre puis réapplique le profil. Le test Rust du client vérifie aussi
l’export et l’import, sans ouvrir les dialogues du système. Les captures
ne contiennent que la fenêtre de démonstration.

## R5 Ultra et bulle micro GPUI

Avec les mêmes exécutables de démo et `capture.dll`, lance `python r5_test.py`.
Le test modifie le DPI et la fréquence dans la fenêtre, vérifie que le
brouillon n’écrit rien, enregistre puis vérifie la relecture, la veille et le
réveil. Il vérifie aussi l’apparition et le masquage de la bulle micro, puis
lance les essais des réglages GoXLR et de l’empreinte vocale de synthèse.

`python tray_test.py` vérifie séparément que l’icône GPUI démarre sans fenêtre
principale, que la bulle apparaît et se masque, et que la session garde le focus.
