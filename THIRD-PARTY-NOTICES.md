# Composants tiers

Rustudio est distribué sous licence MIT (voir [LICENSE](LICENSE)). Il s'appuie sur :

| Composant | Utilisation | Licence |
| --- | --- | --- |
| [goxlr-utility](https://github.com/GoXLR-on-Linux/goxlr-utility) | Identifiants et formats des commandes USB de la GoXLR, repris comme référence (aucun code copié tel quel) | MIT |
| [hidapi-rs](https://github.com/ruabmbua/hidapi-rs) | Interface HID vendor de la R5 Ultra (backend natif Windows) | MIT |
| [Tauri](https://tauri.app), [wry](https://github.com/tauri-apps/wry), [tao](https://github.com/tauri-apps/tao) | Appli Windows et fenêtre de l'installeur | MIT ou Apache-2.0 |
| [windows-rs](https://github.com/microsoft/windows-rs), [webview2-com](https://github.com/wravery/webview2-rs) | Accès aux API Windows (audio, fenêtres, WebView2) | MIT ou Apache-2.0 |
| [serde](https://serde.rs), [serde_json](https://github.com/serde-rs/json), [indexmap](https://github.com/indexmap-rs/indexmap) | Réglages et API JSON | MIT ou Apache-2.0 |
| [tiny_http](https://github.com/tiny-http/tiny-http), [ureq](https://github.com/algesten/ureq), [native-tls](https://github.com/sfackler/rust-native-tls) | API locale, releases GitHub (TLS de Windows) | MIT ou Apache-2.0 |
| [zip](https://github.com/zip-rs/zip2), [sha2](https://github.com/RustCrypto/hashes), [chrono](https://github.com/chronotope/chrono) | Archives, empreintes, journal | MIT ou Apache-2.0 |
| [libloading](https://github.com/nagisa/rust_libloading), [winreg](https://github.com/gentoo90/winreg-rs), [windows-service](https://github.com/mullvad/windows-service-rs) | Pilote GoXLR, registre, service Windows | ISC, MIT, MIT ou Apache-2.0 |
| [Microsoft Edge WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) | Affiche l'interface (runtime de Windows) | Licence du SDK WebView2 (BSD) |
| [Python 3.11 embarqué](https://www.python.org/downloads/windows/) | Seulement dans `GoXLR-Mini.zip` (passage depuis GoXLR Mini 4.x) | PSF License |
| [Lucide](https://lucide.dev) | Icônes de l'interface | ISC |
| [Inter](https://rsms.me/inter) | Police de l'interface, intégrée à `ui/index.html` | SIL Open Font License 1.1 |
| [AutoEq](https://github.com/jaakkopasanen/AutoEq) | Corrections de casques (cible Harman, mesures d'oratory1990) reprises dans le moteur | MIT |
| [Equalizer APO](https://sourceforge.net/projects/equalizerapo/) | Égaliseur du casque : téléchargé depuis sa page officielle à la demande, empreinte vérifiée, jamais livré avec Rustudio | GPL-2.0 |

La liste complète des dépendances Rust et de leurs licences est dans `Cargo.lock` (`cargo tree`).

Le **pilote officiel GoXLR** (TC-Helicon / Music Tribe) n'est pas distribué avec Rustudio : il s'installe à part.
GoXLR est une marque de Music Tribe. Ce projet indépendant n'est ni affilié, ni soutenu par TC-Helicon ou Music Tribe.

## Lucide (icônes)

Icônes de l'interface (`ui/index.html`), version 1.61.0, https://lucide.dev — licence ISC.

Copyright (c) 2026 Lucide Icons and Contributors. Permission to use, copy, modify, and/or distribute this software
for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission
notice appear in all copies. THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD
TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS.

## Inter (police)

Police de l'interface, intégrée à `ui/index.html` (sous-ensemble latin, @fontsource-variable/inter 5.3.0),
https://rsms.me/inter — Copyright (c) 2016 The Inter Project Authors, SIL Open Font License 1.1
(https://openfontlicense.org).


## Documentation du protocole R5 Ultra — miyoi

Source : https://github.com/ellerotta/miyoi

MIT License

Copyright (c) 2026 miyoi contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.


## hidapi-rs (backend HID natif Windows)

Copyright 2017 The hidapi-rs Developers

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial
portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
