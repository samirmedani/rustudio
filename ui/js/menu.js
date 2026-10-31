// Menu, diagnostic, réglages et feuilles d’analyse.
"use strict";

/* ---------- menu bas droite ---------- */
function openMenu() {
  var box = h("div", {style: "width:284px"});
  function item(icon, label, fn) { var b = h("button", {"class": "menu-item"}, svg(icon) + "<span>" + label + "</span>"); b.addEventListener("click", function () { closePop(); fn(); }); box.appendChild(b); }
  item("usb", "Mes appareils", function () { go("devices"); });
  item("grid", "Modules", function () { go("modules"); });
  if (activeModule === "goxlr-mini" && S) {
  box.appendChild(h("div", {"class": "menu-sep"}));
  var micMuted = S && !!muteState("Mic");
  item(micMuted ? "Mic" : "micOff", micMuted ? "Réactiver le micro" : "Couper le micro", function () { setMute("Mic", !micMuted, "all"); });
  box.appendChild(h("div", {"class": "menu-sep"}));
  item("scenes", "Scènes…", function () { go("scenes"); });
  item("BroadcastMix", "Incrustation pour OBS", openOverlaySheet);
  item("settings", "Réglages", openSettingsSheet);
  item("activity", "Diagnostic", openDiag);
  item("shield", "Anti-écho", openEchoSheet);
  item("audioLines", "Son qui crépite ?", openSoundSheet);
  box.appendChild(h("div", {"class": "menu-sep"}));
  item("download", "Exporter ma configuration", function () {
    var a = h("a", {href: URL.createObjectURL(new Blob([JSON.stringify(S.state, null, 2)], {type: "application/json"})), download: "rustudio-config.json"}); a.click();
  });
  item("upload", "Importer une configuration", function () { $("import-file").click(); });
  item("reset", "Revenir aux réglages d\u2019origine", function () {
    sheet({icon: "reset", title: "Revenir aux réglages d\u2019origine ?", text: "Faders, volumes, routage, micro et couleurs reprennent le profil de base. Tes scènes enregistrées sont gardées.",
      actions: [["Annuler", null], ["Réinitialiser", function () {
        api("/api/default-profile").then(function (p) { p.user_scenes = S.state.user_scenes; return withUndo("Réglages d\u2019origine rétablis", function () { return postModule("goxlr-mini", "profile", p).then(refresh); }); })
          .catch(function () { toast("Les réglages d’origine n’ont pas pu être chargés."); });
      }, "danger"]]});
  });
  }
  box.appendChild(h("div", {"class": "menu-sep"}));
  item("refresh", "Rechercher une mise à jour", checkUpdate);
  item("history", "Historique des versions", openHistorySheet);
  item("lifebuoy", "Aide et retours", openHelpSheet);
  item("info", "À propos", function () {
    var repo = (S && S.update && S.update.repo_url) || "https://github.com/samirmedani/rustudio";
    sheet({icon: "info", title: "Rustudio" + (S ? " " + esc(S.version) : ""), text: "Logiciel libre (licence MIT), écrit en Rust, qui réunit tes périphériques sous Windows. Chaque appareil dispose de son module et de son propre espace de réglages." + "<br><br><span class=\"muted-txt\">Projet indépendant, non affilié aux fabricants du matériel.</span>",
      actions: [["Code source sur GitHub", function () { openUrl(repo); }], ["Aide et retours", openHelpSheet], ["Fermer", null, "primary"]]});
  });
  openPop($("fab"), box, "down-right");
}

/* ---------- diagnostic, incrustation, reglages ---------- */
var diagTimer = null;
function fmtDur(s) { if (s == null) return "—"; if (s < 60) return s + " s"; if (s < 3600) return Math.floor(s / 60) + " min"; return Math.floor(s / 3600) + " h " + Math.floor(s % 3600 / 60) + " min"; }
var STATUS_FR = {ok: ["Tout va bien", "#30D158", "ok"], absent: ["GoXLR débranchée", "#FF9F0A", "warn"], error: ["Connexion impossible", "#FF9F0A", "warn"], blocked: ["Pilote USB bloqué", "#FF453A", "warn"], starting: ["Démarrage", "#64D2FF", "info"]};
function openDiag() {
  var body = h("div");
  sheet({wide: true, body: body, actions: [["Copier le diagnostic", function () { copyReport(); }, "", true], ["Son qui crépite ?", openSoundSheet, "", true], ["Reconnecter la GoXLR", function () { postModule("goxlr-mini", "reconnect").then(function (r) { if (r) toast("Reconnexion demandée"); }); }, "", true], ["Fermer", null, "primary"]]});
  function paint() {
    api("/api/health").then(function (hl) {
      if (!sheetState || sheetState.body !== body) return;
      var st = STATUS_FR[hl.status] || [hl.status, "#8E8E93", "info"], s = hl.stats || {};
      var cells = [["Appareil", hl.connected ? "Connectée" : hl.present === false ? "Absente" : "Hors ligne"], ["Firmware", hl.firmware || "—"], ["Connectée depuis", fmtDur(hl.device_uptime_s)], ["Moteur lancé depuis", fmtDur(hl.engine_uptime_s)],
        ["Commandes", (s.commands || 0).toLocaleString("fr-FR")], ["Index actuel", hl.command_index + " / 65535"], ["Remises à zéro", s.index_resets || 0], ["Resynchros", s.resyncs || 0],
        ["Reconnexions", hl.reconnects], ["Relances moteur", hl.engine_restarts], ["Boucle USB", hl.loop_age_ms + " ms"], ["En attente", hl.pending]];
      if (WIN) cells.push(["Sortie Windows", WIN.render.console.name || "—"], ["Micro Windows", WIN.capture.console.name || "—"]);
      var why = !hl.connected && USB_TXT[hl.usb] && hl.usb !== "mini" ? USB_TXT[hl.usb] : hl.last_error && !hl.connected ? hl.last_error : "";
      body.innerHTML = '<div class="diag-head">' + chipIco(st[2], st[1]) + "<div><h2>" + st[0] + "</h2><p class=\"muted-txt\">Moteur v" + hl.version + (hl.simulated ? " · appareil simulé" : "") + (hl.supervised ? " · surveillé par son superviseur" : "") + (why ? " · " + esc(why) : "") + "</p></div></div>" +
        '<div class="diag-grid">' + cells.map(function (c) { return "<div><small>" + c[0] + "</small><b>" + esc(c[1]) + "</b></div>"; }).join("") + "</div>" +
        '<div class="diag-log">' + esc((hl.log || []).slice(-14).join("\n")) + "</div>";
      if (hl.simulated) body.appendChild(simTools());
      sheetState.report = WIN ? Object.assign({}, hl, {windows_audio: WIN}) : hl;
    }).catch(function () { body.innerHTML = "<h2>Le moteur ne répond pas</h2><p class=\"muted-txt\">Il est relancé automatiquement par son superviseur.</p>"; });
  }
  paint();
  clearInterval(diagTimer);
  diagTimer = setInterval(function () { if (!sheetState || sheetState.body !== body) { clearInterval(diagTimer); return; } paint(); }, 1000);
}
// Le rapport copié est celui des tickets : anonyme (ni numéro de série, ni nom de PC, ni chemin personnel).
function copyReport() {
  supportData().then(function (s) { return copyText(diagnosticText(s, WIN), "Diagnostic copié, sans donnée personnelle"); }, function () { toast("Le moteur ne répond pas"); });
}
function simTools() {
  var box = h("div", {"class": "diag-sim"});
  function b(label, path, payload) { box.appendChild(h("button", {"class": "pill", onclick: function () { post("sim/" + path, payload || {}); }}, label)); }
  b("Appui mute 1", "press", {button: "Fader1Mute", ms: 150}); b("Appui long mute 2", "press", {button: "Fader2Mute", ms: 900});
  b("Bouton micro", "press", {button: "MicrophoneMute", ms: 150}); b("Bleep", "press", {button: "Bleep", ms: 600});
  b("Parler", "talk", {on: true}); b("Silence", "talk", {on: false});
  b("Micro statique", "mic-input", {input: "condenser"}); b("Micro dynamique", "mic-input", {input: "dynamic"});
  b("Coupure USB", "usb-drop", {ms: 1}); b("Débrancher", "unplug"); b("Rebrancher", "plug"); b("Bloquer l’USB 20 s", "hang", {seconds: 20});
  return box;
}
function openOverlaySheet() {
  var base = location.origin + "/overlay";
  var body = h("div");
  body.innerHTML = '<div class="diag-head">' + chipIco("BroadcastMix", "#BF5AF2") + "<div><h2>Incrustation pour OBS</h2><p class=\"muted-txt\">Ajoute une source « Navigateur » dans OBS avec cette adresse (400 × 120). Elle affiche « Micro coupé » quand le stream ne t\u2019entend plus.</p></div></div>" +
    '<div class="url-box" id="ov-url">' + base + "</div>" +
    '<div class="chips" style="margin-top:12px" id="ov-opts"><button data-q="" class="on">Seulement si coupé</button><button data-q="mode=always">Toujours visible</button><button data-q="mode=always&amp;meter=1">Avec vumètre</button></div>';
  sheet({wide: true, body: body, actions: [["Copier l’adresse", function () { var u = $("ov-url").textContent; (navigator.clipboard ? navigator.clipboard.writeText(u) : Promise.reject()).then(function () { toast("Adresse copiée"); }, function () { toast("Sélectionne l’adresse et copie-la"); }); }, "", true], ["Fermer", null, "primary"]]});
  body.querySelectorAll("#ov-opts button").forEach(function (bt) {
    bt.addEventListener("click", function () { body.querySelectorAll("#ov-opts button").forEach(function (x) { x.classList.toggle("on", x === bt); }); $("ov-url").textContent = base + (bt.dataset.q ? "?" + bt.dataset.q : ""); });
  });
}
function openSettingsSheet() {
  var body = h("div");
  body.innerHTML = '<div class="diag-head">' + chipIco("settings", "#0071E3") + "<div><h2>Réglages</h2><p class=\"muted-txt\">Comportement des boutons et notifications de l\u2019appli.</p></div></div>";
  var f = h("div", {"class": "field"}, '<div class="field-top">Durée d\u2019un appui long<span id="hold-val"></span></div>');
  var sl = Slider({label: "Durée d’un appui long", format: function (v) { return (v / 1000).toFixed(2).replace(".", ",") + " s"; }, min: 250, max: 1500, step: 50, value: S.state.prefs.hold_ms, colour: "var(--accent)",
    onInput: function (v) { $("hold-val").textContent = (v / 1000).toFixed(2).replace(".", ",") + " s"; },
    onCommit: function (v) { S.state.prefs.hold_ms = v; postModule("goxlr-mini", "settings", {prefs: {hold_ms: v}}); }});
  f.appendChild(sl.el); body.appendChild(f);
  var r = h("div", {"class": "row-switch", style: "margin-top:10px"}, "<div><b>Bulle à l\u2019écran quand le micro change</b><small>Petit rappel en bas de l\u2019écran, même en jeu (appli Windows).</small></div>");
  var sw = h("button", {"class": "switch" + (S.state.prefs.osd ? " on" : "")});
  sw.addEventListener("click", function () { var v = !S.state.prefs.osd; S.state.prefs.osd = v; sw.classList.toggle("on", v); postModule("goxlr-mini", "settings", {prefs: {osd: v}}); });
  r.appendChild(sw); body.appendChild(r);
  sheet({wide: true, body: body, actions: [["Fermer", null, "primary"]]});
  $("hold-val").textContent = (S.state.prefs.hold_ms / 1000).toFixed(2).replace(".", ",") + " s";
}

/* ---------- feuille et analyses ---------- */
var sheetState = null, sheetReturnFocus = null;
function sheetControls() {
  return Array.from($("sheet-box").querySelectorAll('button:not(:disabled),input:not(:disabled):not([type="hidden"]),select:not(:disabled),textarea:not(:disabled),a[href],[tabindex="0"]'))
    .filter(function (e) { return e.getClientRects().length && !e.closest("[inert]"); });
}
function focusSheet() {
  var b = $("sheet-box"), title = b.querySelector("h2");
  b.setAttribute("aria-label", title ? title.textContent : "Rustudio");
  if (document.activeElement === b || !b.contains(document.activeElement)) (sheetControls()[0] || b).focus({preventScroll: true});
}
document.addEventListener("keydown", function (e) {
  if (!sheetState || e.key !== "Tab") return;
  var controls = sheetControls(), first = controls[0], last = controls[controls.length - 1];
  if (!first) { e.preventDefault(); $("sheet-box").focus(); }
  else if (e.shiftKey && (document.activeElement === first || !controls.includes(document.activeElement))) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && (document.activeElement === last || !controls.includes(document.activeElement))) { e.preventDefault(); first.focus(); }
});
function sheet(o) {
  var b = $("sheet-box");
  if (!sheetState) sheetReturnFocus = document.activeElement;
  if (sheetState && sheetState !== o && sheetState.onClose) sheetState.onClose();
  sheetState = o;
  b.classList.toggle("wide", !!o.wide);
  if (o.body) { b.innerHTML = ""; b.appendChild(o.body); }
  else {
    var html = '<div class="ring">' + (o.progress != null ? '<svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="44" fill="none" stroke="rgba(0,0,0,.08)" stroke-width="7"/><circle id="ring-p" cx="50" cy="50" r="44" fill="none" stroke="#0071E3" stroke-width="7" stroke-linecap="round" stroke-dasharray="276.5" stroke-dashoffset="' + (276.5 * (1 - o.progress)) + '" style="transition:stroke-dashoffset .2s"/></svg>' : "") + '<div class="ico">' + svg(o.icon) + "</div></div>";
    html += "<h2>" + o.title + "</h2><p>" + (o.text || "") + "</p>";
    if (o.meter) { html += '<div class="sheet-meter"><div class="meter" id="sheet-meter">' + new Array(35).join("<i></i>") + "</div></div>"; }
    b.innerHTML = html;
  }
  var acts = h("div", {"class": "sheet-actions"});
  (o.actions || []).forEach(function (a) {
    var btn = h("button", {"class": "pill " + (a[2] || "")}, a[0]);
    btn.addEventListener("click", function () { if (!a[3]) closeSheet(); if (a[1]) a[1](); });
    acts.appendChild(btn);
  });
  b.appendChild(acts);
  $("app").inert = true;
  $("sheet").inert = false;
  $("sheet").setAttribute("aria-hidden", "false");
  $("sheet").classList.add("on");
  b.focus({preventScroll: true});
  requestAnimationFrame(function () { if (sheetState === o) focusSheet(); });
}
function closeSheet() {
  var o = sheetState, back = sheetReturnFocus;
  $("sheet").classList.remove("on"); sheetState = null; sheetReturnFocus = null;
  $("sheet").inert = true;
  $("sheet").setAttribute("aria-hidden", "true");
  $("app").inert = false;
  if (o && o.onClose) o.onClose();
  if (back && !back.isConnected) back = $("fab");
  if (!sheetState && back && !back.closest("[inert]")) back.focus({preventScroll: true});
}
var jobSeen = null;
function startJob(kind, params) {
  postModule("goxlr-mini", kind, params).then(function (r) { if (r && r.ok) { jobSeen = {kind: kind, id: r.job}; showJob({kind: kind, running: true, progress: 0, step: "Préparation…"}); } });
}
function showJob(j) {
  if (j.kind === "voice-setup") return showVoiceJob(j);
  if (j.running) {
    var title = j.kind === "detect-mic" ? "Détection du micro" : "Réglage automatique du gain";
    var text = j.kind === "auto-gain" ? "<b>Parle normalement</b>, comme en live, pendant quelques secondes." : (j.step || "");
    if (sheetState && sheetState.job) {
      var rp = $("ring-p"); if (rp) rp.setAttribute("stroke-dashoffset", 276.5 * (1 - j.progress));
      var p = document.querySelector(".sheet-box p"); if (p && j.kind === "detect-mic") p.textContent = j.step || "";
      return;
    }
    sheet({job: true, icon: j.kind === "detect-mic" ? "radar" : "wand", title: title, text: text, progress: j.progress, meter: true,
      actions: [["Annuler", function () { postModule("goxlr-mini", "job-cancel"); }]]});
    return;
  }
  if (j.error) { sheet({icon: "warn", title: "Analyse interrompue", text: j.error, actions: [["OK", null]]}); return; }
  var r = j.result || {};
  if (j.kind === "detect-mic") {
    if (!r.detected) {
      sheet({icon: "Mic", title: "Aucun micro détecté", text: r.message, actions: [["Fermer", null], ["Essayer avec 48 V", function () { startJob("detect-mic", {phantom: true}); }]]});
    } else {
      sheet({icon: "ok", title: r.message, text: "Je te conseille : <b>" + MIC_TYPE_LABELS[r.detected] + "</b>, gain <b>" + r.recommended_gain + " dB</b> et le préréglage <b>" + frLabel(S.meta.mic_presets[r.recommended_preset].label) + "</b>.",
        actions: [["Plus tard", null], ["Appliquer", function () {
          postModule("goxlr-mini", "mic", {type: r.detected, gain_db: r.recommended_gain}).then(function (x) { return x ? postModule("goxlr-mini", "mic-preset", {name: r.recommended_preset}) : null; }).then(function (x) { if (x) { toast("Micro configuré"); refresh(); } });
        }, "primary"]]});
    }
  } else {
    sheet({icon: "ok", title: "Gain réglé", text: "De <b>" + r.old_gain_db + " dB</b> à <b>" + r.new_gain_db + " dB</b>. Pic de ta voix mesuré à " + Math.round(r.voice_peak_db) + " dB.", actions: [["Parfait", null]]});
  }
}
function liveJob() {
  if (!L || !L.job || !jobSeen) return;
  if (jobSeen.id != null && L.job.started !== jobSeen.id) return;     // resultat d'une analyse precedente
  showJob(L.job);
  if (!L.job.running) { jobSeen = null; refresh(); }
}
