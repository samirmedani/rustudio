// Aide et retours (comme Wispad) : signaler un problème avec le diagnostic anonyme, enquêter avec un agent IA,
// proposer une idée, poser une question, demander un appareil. Les pages GitHub s'ouvrent dans le navigateur ; les
// copies vont dans le presse-papiers. Rustudio n'envoie rien lui-même.
"use strict";

/* ---------- aide et retours ---------- */
var REPO_URL = "https://github.com/samirmedani/rustudio";
function copyText(txt, done) {
  var fail = function () { toast("Copie impossible : sélectionne le texte à la main."); };
  if (!navigator.clipboard || !navigator.clipboard.writeText) { fail(); return Promise.resolve(false); }
  return navigator.clipboard.writeText(txt).then(function () { toast(done); return true; }, function () { fail(); return false; });
}
// Diagnostic du moteur (déjà anonyme), complété par les périphériques Windows lus par l'appli.
function supportReport(s, win) {
  var r = Object.assign({}, s.report);
  if (win) r.windows_audio = {sortie: win.render && win.render.console ? win.render.console.name : null,
    micro: win.capture && win.capture.console ? win.capture.console.name : null,
    alertes: (win.issues || []).map(function (i) { return i.kind; })};
  return r;
}
function diagnosticText(s, win) {
  return "Diagnostic de Rustudio (sans numéro de série, nom de PC, chemin personnel ni adresse IP) :\n\n" + String.fromCharCode(96, 96, 96) + "json\n" +
    JSON.stringify(supportReport(s, win), null, 2) + "\n" + String.fromCharCode(96, 96, 96) + "\n";
}
function devicesLine(report) {
  var names = {"goxlr-mini": "GoXLR Mini", "attack-shark-r5-ultra": "Attack Shark R5 Ultra"};
  var d = (report.appareils || []).filter(function (x) { return x.connecte || x.etat === "asleep"; }).map(function (x) { return names[x.module] || x.module; });
  return d.length ? d.join(", ") : "aucun appareil connecté";
}
// « Enquêter avec ton agent IA » : consignes pour Codex, Claude Code… (où regarder, quoi ne jamais lire ni changer,
// comment préparer un ticket anonyme publié seulement avec l'accord de l'utilisateur), diagnostic compris.
function agentPrompt(s, win) {
  var repo = s.repo || "samirmedani/rustudio", fence = String.fromCharCode(96, 96, 96);
  return [
    "Aide-moi à comprendre un problème avec Rustudio, le logiciel libre qui pilote mon matériel sous Windows (https://github.com/" + repo + "), puis à le signaler à ses développeurs.",
    "",
    "Le problème : [décris ici ce qui se passe, ce que tu faisais et depuis quand]",
    "Si je n'ai rien décrit, demande-le-moi d'abord.",
    "",
    "Ce PC : Rustudio " + s.version + ", " + s.windows + ". Appareils : " + devicesLine(s.report || {}) + ".",
    "",
    "Règles, à chaque étape :",
    "- Lis seulement le journal C:\\ProgramData\\Rustudio\\rustudio.log (et rustudio.log.1) et l'API locale de Rustudio en lecture : http://127.0.0.1:14565/api/health, /api/support et /api/update. Le fichier state.json contient mes réglages : ne le publie pas.",
    "- Ne change rien sur ce PC sans mon accord : ni réglage, ni pilote, ni service, ni fichier. Ne lance jamais GoXLR Utility pendant que Rustudio pilote la GoXLR.",
    "- Le ticket sera public : rien de personnel ne doit y entrer (étape 5).",
    "",
    "1. Rassemble les faits",
    "- Le journal de Rustudio : connexions, reconnexions, erreurs USB, mises à jour, coupures du son.",
    "- L'état en direct : http://127.0.0.1:14565/api/health (appareil, reconnexions, coupures USB des dernières 24 heures).",
    "- Le diagnostic à la fin de ce message.",
    "- Windows : Gestionnaire de périphériques (pilote GoXLR de TC-Helicon, port USB) et Observateur d'événements (journal Système) à l'heure du problème.",
    "- Une version plus récente corrige peut-être déjà le problème : https://github.com/" + repo + "/releases",
    "",
    "2. Trouve la cause",
    "Compare ce que je décris avec le journal, l'état en direct et le diagnostic : heure, appareil absent, pilote, reconnexions, coupures USB, mise à jour en échec. Explique-moi simplement ce que tu as trouvé et ce que je peux essayer, et attends mon accord avant d'agir.",
    "",
    "3. Vérifie que le problème n'est pas déjà signalé",
    "gh issue list --repo " + repo + " --state all --search \"<mots-clés>\", ou https://github.com/" + repo + "/issues. S'il l'est déjà, propose-moi d'y ajouter mes informations plutôt que d'ouvrir un nouveau ticket.",
    "",
    "4. Prépare le ticket, en français ou en anglais, sur le modèle du formulaire de bug",
    "- Titre : court et précis.",
    "- Ce qui se passe.",
    "- Pour le reproduire : étapes numérotées.",
    "- Ce que j'attendais.",
    "- Version de Rustudio : " + s.version + ". Windows : " + s.windows + ".",
    "- Appareil concerné.",
    "- Les extraits utiles du journal et du diagnostic, dans un bloc <details>.",
    "- Les captures d'écran utiles : dis-moi lesquelles faire, sans rien de personnel à l'écran.",
    "",
    "5. Anonymise tout",
    "Retire, ou remplace par un exemple neutre : noms de personnes, e-mails, nom d'utilisateur et chemins personnels (C:\\Users\\…), nom du PC, adresses IP, numéros de série d'appareils, jetons et clés, contenu de mes fenêtres. Le nom d'un programme peut rester s'il aide à reproduire le problème.",
    "",
    "6. Publie seulement avec mon accord",
    "Montre-moi le titre et le texte complets du ticket. Si je dis oui et que gh est installé et connecté : gh issue create --repo " + repo + " --title \"…\" --body-file <fichier>. Sinon, ouvre ce formulaire prérempli et dis-moi quoi coller dans chaque champ : " + s.bug_url,
    "",
    "Diagnostic de Rustudio (sans numéro de série, nom de PC, chemin personnel ni adresse IP) :",
    "",
    fence + "json",
    JSON.stringify(supportReport(s, win), null, 2),
    fence
  ].join("\n");
}
function helpItems(s) {
  return [
    {icon: "bug", c: "#FF453A", title: "Signaler un problème", text: "Un ticket public sur GitHub, avec ta version de Rustudio et de Windows déjà remplies. Colles-y le diagnostic : il ne contient ni numéro de série, ni nom de PC, ni chemin personnel.",
      acts: [["Copier le diagnostic", "diag"], ["Ouvrir le formulaire", s.bug_url, "primary"]]},
    {icon: "sparkles", c: "#5E5CE6", title: "Enquêter avec ton agent IA", text: "Pour Codex, Claude Code… : il cherche la cause sur ce PC sans rien modifier sans ton accord, puis prépare avec toi un ticket anonyme.",
      acts: [["Copier le prompt", "prompt"]]},
    {icon: "bulb", c: "#FF9F0A", title: "Proposer une idée", text: "Une fonction, une amélioration : dans les Discussions, où chacun peut voter.", acts: [["Voir les idées", s.ideas_url]]},
    {icon: "messageHelp", c: "#0A84FF", title: "Poser une question", text: "Installation, réglages, compatibilité : la communauté et le mainteneur répondent dans les Discussions.", acts: [["Poser une question", s.questions_url]]},
    {icon: "usb", c: "#30D158", title: "Demander un appareil", text: "Ta souris, ton clavier ou ton écran n’a pas encore de module ? Décris-le, sans numéro de série, ou propose de le coder.", acts: [["Demander un module", s.device_url]]},
    {icon: "history", c: "#8E8E93", title: "Historique des versions", text: "Les nouveautés de chaque version, et le retour à une version précédente si une mise à jour te gêne.", acts: [["Voir l’historique", "history"]]}
  ];
}
function openHelpSheet() {
  var body = h("div", {"class": "help"});
  sheet({wide: true, body: body, actions: [["Fermer", null, "primary"]]});
  body.innerHTML = '<div class="upd-hero">' + chipIco("lifebuoy", "#0A84FF") + '<div><div class="eyebrow">Aide et retours</div><h2>On t’écoute</h2><p class="muted-txt">Tout passe par GitHub, en public : Rustudio n’envoie rien lui-même.</p></div></div><p class="hist-note">Chargement…</p>';
  supportData().then(function (s) {
    if (!sheetState || sheetState.body !== body) return;
    var list = h("div", {"class": "help-list"});
    helpItems(s).forEach(function (it) {
      var row = h("div", {"class": "help-item"}, chipIco(it.icon, it.c) + "<div><b>" + it.title + "</b><p>" + it.text + '</p><div class="help-acts"></div></div>');
      var acts = row.querySelector(".help-acts");
      it.acts.forEach(function (a) {
        var b = h("button", {"class": "pill " + (a[2] || "")}, a[0]);
        b.addEventListener("click", function () { helpAction(a[1], s); });
        acts.appendChild(b);
      });
      list.appendChild(row);
    });
    body.removeChild(body.querySelector(".hist-note"));
    body.appendChild(list);
  }, function () {
    var n = body.querySelector(".hist-note"); if (n) n.textContent = "Le moteur ne répond pas : réessaie dans un instant.";
  });
}
function helpAction(what, s) {
  var win = typeof WIN !== "undefined" ? WIN : null;
  if (what === "diag") return copyText(diagnosticText(s, win), "Diagnostic copié : colle-le dans ton ticket.");
  if (what === "prompt") return copyText(agentPrompt(s, win), "Prompt copié : colle-le dans Codex, Claude Code ou ton agent IA.");
  if (what === "history") { openHistorySheet(); return; }
  openUrl(what);
}
function supportData() { return api("/api/support"); }
