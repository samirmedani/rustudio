// GoXLR Mini : périphériques Windows, anti-écho et diagnostic du son.
"use strict";

/* ---------- périphériques Windows par défaut (lus par l'appli, qui tourne dans ta session) ---------- */
var WIN = null, winLoopTold = false;
function winByKind() {
  var k = {};
  if (WIN && WIN.goxlr) (WIN.issues || []).forEach(function (i) { (k[i.kind] = k[i.kind] || []).push(i); });
  return k;
}
function winName(list) { var n = list && list[0] && list[0].name; return n ? "« " + esc(n) + " »" : ""; }
function winFix() {
  if (!inApp()) return Promise.resolve();
  return invoke("audio_fix").then(onWinAudio, function () { toast("L\u2019appli n\u2019a pas répondu"); });
}
function winAdvice(a) {
  var k = winByKind(), fix = function () { return winFix(); };
  if (k["capture-loop"]) a.push({lvl: "warn", t: "Écho : Windows n\u2019écoute pas ton micro", d: "Discord et tes jeux enregistrent " + winName(k["capture-loop"]) + ", qui contient déjà le son de tout le monde : tes amis s\u2019entendent. Ton micro, c\u2019est « Chat Mic ».", label: "Corriger", fn: fix});
  else if (k["capture-other"]) a.push({lvl: "warn", t: "Windows utilise un autre micro", d: "Discord et tes jeux prennent " + winName(k["capture-other"]) + " au lieu de ton micro XLR (« Chat Mic ») : ni le traitement de ta voix, ni le bouton de toux ne s\u2019y appliquent.", label: "Utiliser Chat Mic", fn: fix});
  else if (k["capture-none"]) a.push({lvl: "warn", t: "Aucun micro par défaut dans Windows", d: "Les applis qui suivent le micro par défaut ne t\u2019entendent pas.", label: "Utiliser Chat Mic", fn: fix});
  var out = k["render-other"] || k["render-none"];
  if (out) a.push({t: "Le son du PC contourne la GoXLR", d: (winName(out) ? "Windows joue le son sur " + winName(out) : "Windows n\u2019a pas de sortie par défaut") + " : faders, mute et égaliseur du casque ne s\u2019y appliquent pas.", label: "Passer par la GoXLR", fn: fix});
}
function onWinAudio(d) {
  if (!d || d.type !== "audio-defaults") return;
  WIN = d;
  if (activeModule !== "goxlr-mini") return;
  if (d.fixed != null) toast(d.fixed > 0 ? "Windows utilise maintenant ta GoXLR" : d.fixed === 0 ? "Windows utilisait déjà ta GoXLR" : "Windows ne voit pas encore les voies de la GoXLR");
  var k = winByKind();
  if (k["capture-loop"] && !winLoopTold && d.fixed == null) {
    winLoopTold = true;
    toast("Écho : Windows enregistre « " + (k["capture-loop"][0].name || "le mix") + " » au lieu de ton micro", "Corriger", winFix);
  }
  if (!k["capture-loop"]) winLoopTold = false;
  renderAdvice();
  if (sheetState && sheetState.echo) paintEcho();
}
function winInit() {
  if (!inApp()) return;
  if (TAURI.event && TAURI.event.listen) TAURI.event.listen("audio-defaults", function (e) { onWinAudio(e.payload); });
  invoke("audio_defaults").then(onWinAudio, function () {});
}

/* ---------- anti-écho : routage, Windows, Discord et test acoustique ---------- */
function echoItem(state, t, d, label, fn) {
  var it = h("div", {"class": "advice"});
  it.innerHTML = chipIco(state === "ok" ? "ok" : state === "warn" ? "warn" : state === "info" ? "ear" : "bulb", state === "ok" ? "#30D158" : state === "warn" ? "#FF9F0A" : "#64D2FF");
  var body = h("div", {}, "<b>" + t + "</b><p>" + d + "</p>");
  if (label && fn) body.appendChild(h("button", {"class": "pill", onclick: function () { var r = fn(); if (r && r.then) r.then(function () {}); }}, label));
  it.appendChild(body);
  return it;
}
/* ---------- son qui crepite ---------- */
function openSoundSheet() {
  closePop();
  var body = h("div");
  sheet({wide: true, sound: true, body: body, res: null, day: null, actions: [["Fermer", null, "primary"]],
    onClose: function () { if (st.res && st.res.running) postModule("goxlr-mini", "sound-test", {seconds: 0}); clearInterval(st.timer); }});
  var st = sheetState;
  api("/api/glitches").then(function (g) { if (sheetState === st) { st.day = g; paintSound(); } }, function () {});
  api("/api/usb").then(function (u) { if (sheetState === st) { st.usb = u; paintSound(); } }, function () {});
  paintSound();
}
/* conseil de port USB, d'apres l'arbre des peripheriques lu par le moteur */
function usbAdvice(u) {
  if (!u || !u.found) return null;
  if (u.via_hub) return "Ta GoXLR passe par un hub USB : branche-la directement sur le PC" + (u.suggest && u.suggest.anchor ? ", à la place de « " + esc(u.suggest.anchor) + " »" + (u.suggest.cpu ? " (port relié au processeur)" : "") : "") + ", puis refais le test.";
  if (u.suggest && u.suggest.anchor) return "Ta GoXLR partage son contrôleur USB" + (u.controller_cpu === false ? " (celui du chipset)" : "") + " avec " + u.shared.length + " appareil" + (u.shared.length > 1 ? "s" : "") +
    ". Branche-la à la place de « " + esc(u.suggest.anchor) + " » (" + (u.suggest.cpu ? "port relié directement au processeur" : "autre contrôleur") + "), puis refais le test. Garde ensuite ce port : à chaque nouveau port, Windows recrée les voies GoXLR, et Anti-écho les remet par défaut en un clic.";
  return null;
}
function soundLevel(c) {
  var d = c.peak_db != null ? c.peak_db : c.meter_db;
  return d == null ? "silence" : "max " + (d > -0.05 ? "0" : String(Math.round(d * 10) / 10).replace(".", ",")) + " dB";
}
/* coupures comptees par le pilote : heure, son perdu, programmes lances au meme moment */
function fmtClock(t) {
  var d = new Date(t * 1000), hm = d.toLocaleTimeString("fr-FR", {hour: "2-digit", minute: "2-digit"});
  return d.toDateString() === new Date().toDateString() ? hm : d.toLocaleDateString("fr-FR", {day: "numeric", month: "short"}) + " " + hm;
}
function fmtLost(ms) { return ms < 1000 ? String(Math.round(ms * 10) / 10).replace(".", ",") + " ms" : String(Math.round(ms / 100) / 10).replace(".", ",") + " s"; }
function dropLoss(e) { return fmtLost(e.out_ms) + " vers le casque · " + fmtLost(e.in_ms) + " vers le PC (micro, stream)"; }
// Repères envoyés par l'appli (écrans éteints ou rallumés), mêlés aux programmes lancés : [libellé court, tournure].
var DROP_MARKS = {"@ecrans-eteints": ["écrans éteints", "l’extinction des écrans"], "@ecrans-rallumes": ["écrans rallumés", "le rallumage des écrans"]};
function dropHint(e) {
  var progs = e && e.hint ? e.hint.filter(function (x) { return !DROP_MARKS[x.name]; }) : [];
  return progs.map(function (x) { return esc(x.name) + (x.n > 1 ? " \u00d7" + x.n : ""); }).join(", ");
}
function dropMark(e, long) {
  var marks = e && e.hint ? e.hint.filter(function (x) { return DROP_MARKS[x.name]; }) : [];
  return marks.map(function (x) { return DROP_MARKS[x.name][long ? 1 : 0]; }).join(long ? " et " : ", ");
}
function usbDropsItem(u, st) {
  if (!u || !u.available) return null;
  var since = u.since ? " depuis " + fmtClock(u.since) : "";
  if (!u.count && u.reading === false) return echoItem("info", "Mesure USB en attente", "Les compteurs du pilote ne sont pas lisibles pour le moment. Aucune conclusion sur le son tant que la mesure ne reprend pas.");
  if (!u.count && !u.streaming) return echoItem("info", "Aucun flux audio en cours", "Les compteurs USB avancent seulement pendant une lecture ou un enregistrement. Aucune coupure enregistrée" + since + ".");
  if (!u.count) return echoItem("ok", "Aucune coupure USB enregistrée", "Aucune perte relevée dans les compteurs du pilote" + since + ".");
  var last = u.events[0], hint = dropHint(last), mark = dropMark(last, true), burst = last && last.hint && last.hint.length && last.hint[0].n >= 5;
  var tops = u.top || [];
  var screens = tops.reduce(function (n, x) { return n + (DROP_MARKS[x.name] ? x.count : 0); }, 0);
  var top = tops.filter(function (x) { return !DROP_MARKS[x.name]; })[0];
  if (top && top.count < 2) top = null;
  var screenLead = screens >= 2 && (!top || screens >= top.count);
  var lines = u.events.slice(0, 6).map(function (e) { var hh = dropHint(e), mk = dropMark(e); return new Date(e.t * 1000).toLocaleTimeString("fr-FR") + " \u00b7 " + dropLoss(e) + (mk ? " \u00b7 " + mk : "") + (hh ? " \u00b7 " + hh : ""); }).join("<br>");
  var advice = [screenLead ? "Ce moment dépend de Windows et du pilote graphique : mettre ce pilote à jour ou allonger le délai de mise en veille des écrans peut réduire ces coupures." : "", usbAdvice(st.usb)].filter(Boolean).join(" ");
  return echoItem("warn", u.count + " coupure" + (u.count > 1 ? "s" : "") + " entre le PC et la GoXLR",
    "Comptée" + (u.count > 1 ? "s" : "") + " par le pilote" + since + " : " + dropLoss(u) + "." +
    (mark ? " La dernière a eu lieu au moment de " + mark + "." : "") +
    (hint ? " Au moment de la dernière, Windows lançait : " + hint + "." : "") +
    (burst ? " Une rafale de programmes lancés d\u2019un coup (synchronisation, compilation, mise à jour) suffit à faire sauter le son USB : espace ou décale cette tâche." : "") +
    (screenLead ? " Souvent quand les écrans s\u2019éteignent ou se rallument : " + screens + " coupures sur " + u.count + "." :
      top ? " Le plus souvent lancé à ces moments-là : « " + esc(top.name) + " », " + top.count + " coupure" + (top.count > 1 ? "s" : "") + " sur " + u.count + "." : "") +
    (hint || mark || top || screenLead ? " Ces coïncidences sont des pistes à vérifier, pas une cause prouvée." : "") +
    (advice ? "</p><p>" + advice : "") +
    (u.count > 1 ? "</p><p class=\"drop-list\">" + lines : ""));
}
function windowsGlitchItem(w, during) {
  if (!w || w.unsupported) return null;
  if (w.enabled !== true || w.readable === false || w.total == null) return echoItem("info", "Journal Windows indisponible", esc(w.error || "La lecture du journal n\u2019a pas abouti : aucune conclusion sur les coupures."));
  var kinds = w.kinds || {};
  if (w.total) return echoItem("warn", w.total + " événement(s) de coupure enregistré(s) par Windows" + (during ? " pendant le test" : ""),
    "Ce journal concerne l\u2019ensemble des périphériques audio Windows. " + (kinds.app ? kinds.app + " côté logiciel. " : "") + (kinds.driver ? kinds.driver + " côté pilote ou périphérique. " : "") +
    (w.last ? "Dernière : " + new Date(w.last).toLocaleString("fr-FR") + "." : ""));
  if (w.was_enabled === false) return echoItem("info", "Journal Windows activé", "Il vient d\u2019être allumé. Les périodes où il était désactivé n\u2019ont pas été mesurées.");
  return echoItem("ok", "Aucune coupure enregistrée par Windows" + (during ? " pendant le test" : ""), "Aucun événement retrouvé dans ce journal sur la période demandée.");
}
function measurementFailureItem(c) {
  return echoItem("info", esc(c.name) + " : mesure indisponible", c.status === "silent" || c.frames === 0 && !c.error
    ? "Aucun échantillon reçu : cette voie n\u2019a pas pu être analysée."
    : esc(c.error || "Aucun résultat exploitable pour cette voie."));
}
function timestampItem(c) {
  return echoItem("info", esc(c.name) + " : horodatage incertain", "Windows a livré les échantillons avec des positions invalides. Les trous dans les données ne peuvent pas être vérifiés sur ces paquets.");
}
function inputLossItems(ins) {
  if (!ins.length) return [echoItem("info", "Aucune entrée audio mesurée", "Le test n\u2019a pas trouvé d\u2019entrée GoXLR exploitable.")];
  var items = [], good = [];
  ins.forEach(function (c) {
    if (c.timestamp_errors > 0) items.push(timestampItem(c));
    if (!(c.frames > 0) || ["ok", "clipping", "dropouts"].indexOf(c.status) < 0) items.push(measurementFailureItem(c));
    else if (c.status === "dropouts") items.push(echoItem("warn", esc(c.name) + " : données perdues", "Des pertes ont été relevées sur cette entrée" + (c.gap_ms > 0 ? " (" + fmtLost(c.gap_ms) + " manquantes)" : "") + "."));
    else if (!c.timestamp_errors) good.push(esc(c.name));
  });
  if (good.length) items.push(echoItem("ok", "Entrées mesurées : aucune perte relevée", good.join(", ") + " : aucune perte de données signalée pendant la mesure."));
  return items;
}
function outputLevelItems(outs) {
  if (!outs.length) return [echoItem("info", "Aucune sortie audio mesurée", "Le test n\u2019a pas trouvé de sortie GoXLR exploitable.")];
  var items = [], playing = [];
  outs.forEach(function (c) {
    if (c.timestamp_errors > 0) items.push(timestampItem(c));
    if (c.status === "silent" && !c.frames) return;
    if (!(c.frames > 0) || ["ok", "clipping", "dropouts"].indexOf(c.status) < 0) items.push(measurementFailureItem(c));
    else if (c.active_s > 0 || c.meter_db != null) playing.push(c);
  });
  if (!playing.length) items.push(echoItem("info", "Aucun son analysé en sortie", "Aucune sortie n\u2019a fourni de son exploitable. Lance une lecture et refais le test pour vérifier ses niveaux."));
  playing.forEach(function (c) {
    var lab = esc(HP_CH_LABELS[c.name] || c.name);
    if (c.status === "dropouts") items.push(echoItem("warn", lab + " : interruptions relevées", "Le test a détecté des interruptions sur cette voie de sortie."));
    var clip = c.status === "clipping" || c.overs > 0 || c.clipped > 2;
    items.push(clip ? echoItem("warn", lab + " sature", "Des échantillons atteignent ou dépassent 0 dB après l\u2019égaliseur : baisse le style ou le volume de l\u2019appli.")
      : echoItem("ok", lab + " : " + soundLevel(c), "Aucune saturation relevée pendant la mesure."));
  });
  return items;
}
function soundAt(s) { s = Math.round(s); return s < 60 ? s + " s" : Math.floor(s / 60) + " min " + (s % 60 ? (s % 60) + " s" : "").trim(); }
function hpBoost() {
  var eq = S.state.hp_eq, c = hpFilters(eq), mx = 0;
  if (!eq.enabled) return 0;
  for (var i = 0; i < 200; i++) mx = Math.max(mx, hpResp(c, 20 * Math.pow(1000, i / 199)));
  return mx;
}
/* baisse proportionnellement les bandes du style pour que la courbe totale ne monte pas au-dela de max dB */
function hpSoftBands(max) {
  var eq = S.state.hp_eq, b = eq.bands.slice();
  for (var s = 1; s >= 0; s -= .05) {
    b = eq.bands.map(function (v) { return v > 0 ? Math.round(v * s) : v; }); /* meme forme, en plus doux */
    var c = hpFilters(Object.assign({}, eq, {bands: b})), mx = 0;
    for (var i = 0; i < 200; i++) mx = Math.max(mx, hpResp(c, 20 * Math.pow(1000, i / 199)));
    if (mx <= max + .05) break;
  }
  return b;
}
function hpSoften() {
  var b = hpSoftBands(HP_SAFE_DB);
  return postModule("goxlr-mini", "hp-eq", {bands: b}).then(function (r) { if (!r) return null; toast("Basses adoucies : +" + HP_SAFE_DB + " dB au plus"); return refresh(); });
}
var HP_SAFE_DB = 10;
function paintSound() {
  var st = sheetState; if (!st || !st.sound) return;
  var b = st.body; b.innerHTML = "";
  b.appendChild(h("div", {"class": "diag-head"}, chipIco("audioLines", "#0071E3") + "<div><h2>Son qui crépite ?</h2><p class=\"muted-txt\">Rustudio cherche les coupures entre le PC et la GoXLR, et ce qui sature.</p></div>"));
  var r = st.res, t = h("div", {"class": "echo-sec"}, "<h4>Test</h4>");
  if (!inApp()) t.appendChild(echoItem("tip", "Dans l\u2019appli Rustudio", "Ouvre l\u2019appli Windows : c\u2019est elle qui écoute les voies GoXLR."));
  else if (r && r.running) {
    var left = Math.max(0, Math.ceil(r.secs - (Date.now() - r.t0) / 1000));
    t.appendChild(echoItem("info", "Test en cours, encore " + fmtDur(left), "Continue comme d\u2019habitude : joue, écoute de la musique. C\u2019est pendant l\u2019usage normal que les coupures se voient."));
  } else {
    if (r && r.error) t.appendChild(echoItem("warn", "Test impossible", esc(r.error)));
    var it = echoItem("info", r && r.loop ? "Refaire le test" : "30 secondes ou 5 minutes", "Rustudio écoute tes voies GoXLR et fait l\u2019aller-retour d\u2019un son aigu discret par le Stream Mix : tu ne l\u2019entends pas, Discord non plus. Pour un crépitement rare, prends le test long.", "Tester 30 s", function () { runSound(30); });
    var long = h("button", {"class": "pill", style: "margin-left:8px"}, "Test long, 5 min");
    long.addEventListener("click", function () { runSound(300); });
    it.querySelector(".pill").after(long);
    t.appendChild(it);
  }
  b.appendChild(t);
  if (r && r.loop) paintSoundResult(b, r);
  var day = st.day, drops = day && usbDropsItem(day.usb, st);
  if (day && (!day.unsupported || drops)) {
    var d = h("div", {"class": "echo-sec"}, "<h4>Ces dernières 24 heures</h4>");
    if (drops) d.appendChild(drops);
    var wi = windowsGlitchItem(day, false);
    if (wi) d.appendChild(wi);
    b.appendChild(d);
  }
  var boost = hpBoost();
  if (boost > HP_SAFE_DB + 2) {
    var e = h("div", {"class": "echo-sec"}, "<h4>Égaliseur du casque</h4>");
    e.appendChild(echoItem("warn", "Basses poussées de +" + Math.round(boost) + " dB", "Pour garder ces basses, le reste du son est baissé d\u2019autant, et le volume finit au maximum : sur les sons forts, l\u2019ampli casque et la membrane saturent et grésillent. Au-delà de +" + HP_SAFE_DB + " dB, ça s\u2019entend.", "Adoucir à +" + HP_SAFE_DB + " dB", function () { return hpSoften().then(paintSound); }));
    b.appendChild(e);
  }
}
function paintSoundResult(b, r) {
  var st = sheetState, lp = r.loop, ch = (r.check && r.check.channels) || [], win = r.win;
  var ins = ch.filter(function (c) { return c.kind === "input"; }), outs = ch.filter(function (c) { return c.kind === "output"; });
  var s = h("div", {"class": "echo-sec"}, "<h4>Transport du son</h4>");
  if (lp.status === "ok") s.appendChild(echoItem("ok", "PC → GoXLR → PC : aucun trou", "Un son de test a fait l\u2019aller-retour pendant " + fmtDur(Math.round(lp.analysed_s)) + " sans une seule coupure."));
  else if (lp.status === "glitches") s.appendChild(echoItem("warn", (lp.dropouts + lp.jumps) + " coupure" + (lp.dropouts + lp.jumps > 1 ? "s" : "") + " entre le PC et la GoXLR",
    "C\u2019est ce qu\u2019on entend comme des crépitements" + (lp.events.length ? " (première au bout de " + soundAt(lp.events[0].t) + ")" : "") + ". " + (usbAdvice(st.usb) || "Branche la GoXLR sur un port USB à l\u2019arrière du PC, directement sur la carte mère, sans hub ni rallonge, puis refais le test.")));
  else if (lp.status === "no-signal") s.appendChild(echoItem("tip", "Le son de test n\u2019est pas revenu", "La voie Sample est peut-être coupée, ou la GoXLR débranchée."));
  else if (lp.status === "test-late") s.appendChild(echoItem("tip", "Test perturbé", "Le PC était trop chargé pour jouer le son de test à temps : refais-le."));
  else s.appendChild(echoItem("tip", "Aller-retour impossible", esc(lp.error || "voies GoXLR absentes")));
  var wu = win && win.usb;
  if (wu && wu.available) s.appendChild(wu.count ? echoItem("warn", "Le pilote confirme " + wu.count + " coupure" + (wu.count > 1 ? "s" : "") + " pendant le test",
      dropLoss(wu) + "." + (dropHint(wu.events[0]) ? " Windows lançait alors : " + dropHint(wu.events[0]) + "." : ""))
    : wu.reading === false || !wu.streaming ? echoItem("info", "Aucune perte USB enregistrée", "Les compteurs sont actuellement arrêtés ou illisibles. Le résultat de l\u2019aller-retour ci-dessus reste à prendre en compte.")
    : echoItem("ok", "Aucune perte USB enregistrée pendant le test", "Aucune avance du compteur de pertes n\u2019a été relevée sur cette période."));
  if (lp.status !== "glitches" && st.usb && st.usb.found) {
    var adv = usbAdvice(st.usb);
    s.appendChild(adv ? echoItem("tip", "Port USB", adv.replace("Branche-la", "Si ça crépite un jour, branche-la").replace(", puis refais le test", "")) : echoItem("ok", "Port USB", "Relié directement au processeur, sans hub : le meilleur choix."));
  }
  inputLossItems(ins).forEach(function (it) { s.appendChild(it); });
  var wi = windowsGlitchItem(win, true);
  if (wi) s.appendChild(wi);
  b.appendChild(s);
  var n = h("div", {"class": "echo-sec"}, "<h4>Niveaux</h4>");
  outputLevelItems(outs).forEach(function (it) { n.appendChild(it); });
  var mic = ins.filter(function (c) { return c.name === "Chat Mic"; })[0];
  if (mic && mic.status === "clipping") n.appendChild(echoItem("warn", "Ton micro sature", "Il atteint 0 dB : baisse le gain dans la page Micro, ou recule un peu.", "Page Micro", function () { closeSheet(); go("mic"); }));
  b.appendChild(n);
}
function runSound(secs) {
  var st = sheetState; if (!st || !st.sound || (st.res && st.res.running)) return Promise.resolve();
  var t0 = Math.floor(Date.now() / 1000);
  st.res = {running: true, t0: Date.now(), secs: secs}; paintSound();
  clearInterval(st.timer);
  st.timer = setInterval(function () { if (sheetState !== st || !st.res || !st.res.running) { clearInterval(st.timer); return; } paintSound(); }, 1000);
  return postModule("goxlr-mini", "sound-test", {seconds: secs + 20}).then(function (r) {
    if (!r) throw new Error("Le moteur n’a pas pu démarrer le test audio.");
    return Promise.all([invoke("sound_check", {seconds: secs}), invoke("sound_loop_test", {seconds: secs})]);
  }).then(function (r) {
    return api("/api/glitches?since=" + t0).then(function (g) { return [r[0], r[1], g]; }, function () { return [r[0], r[1], null]; });
  }).then(function (r) { st.res = {check: r[0], loop: r[1], win: r[2]}; },
    function (e) { st.res = {error: e && e.message ? e.message : String(e || "erreur inconnue")}; })
    .then(function () {
      clearInterval(st.timer);
      postModule("goxlr-mini", "sound-test", {seconds: 0});
      return api("/api/glitches").then(function (g) { st.day = g; }, function () {});
    }).then(function () { paintSound(); });
}
function openEchoSheet() {
  closePop();
  var body = h("div");
  sheet({wide: true, echo: true, body: body, checks: null, echoRes: null, actions: [["Fermer", null, "primary"]]});
  var st = sheetState;
  postModule("goxlr-mini", "echo-check").then(function (r) { if (sheetState === st && r) { st.checks = r.checks || []; paintEcho(); } });
  if (inApp()) invoke("audio_defaults").then(onWinAudio, function () {});
  paintEcho();
}
function paintEcho() {
  var st = sheetState; if (!st || !st.echo) return;
  var b = st.body; b.innerHTML = "";
  b.appendChild(h("div", {"class": "diag-head"}, chipIco("shield", "#30D158") + "<div><h2>Anti-écho</h2><p class=\"muted-txt\">Tout ce qui peut faire entendre une voix en double, et un test pour le vérifier en vrai.</p></div>"));
  var g = h("div", {"class": "echo-sec"}, "<h4>GoXLR</h4>");
  if (!st.checks) g.appendChild(h("p", {"class": "muted-txt"}, "Vérification…"));
  (st.checks || []).forEach(function (c) {
    g.appendChild(echoItem(c.ok ? "ok" : c.lvl, esc(c.t), esc(c.d), c.fix && c.fix.label, c.fix && function () {
      return postModule("goxlr-mini", c.fix.action, c.fix.body).then(function (r) { return r ? postModule("goxlr-mini", "echo-check") : null; }).then(function (r) { if (r) { st.checks = r.checks; paintEcho(); refresh(); } });
    }));
  });
  b.appendChild(g);
  var w = h("div", {"class": "echo-sec"}, "<h4>Windows</h4>");
  if (!inApp()) w.appendChild(echoItem("tip", "Dans l\u2019appli Rustudio", "Ouvre l\u2019appli Windows pour vérifier le micro et la sortie que Windows donne à Discord et aux jeux."));
  else if (!WIN) w.appendChild(h("p", {"class": "muted-txt"}, "Lecture des périphériques…"));
  else if (!WIN.goxlr) w.appendChild(echoItem("tip", "Voies GoXLR absentes", "Windows ne voit pas encore « Chat Mic » : branche la GoXLR."));
  else {
    var k = winByKind(), capBad = k["capture-loop"] || k["capture-other"] || k["capture-none"], outBad = k["render-other"] || k["render-none"];
    w.appendChild(echoItem(capBad ? "warn" : "ok", "Micro de Windows", k["capture-loop"] ? "Windows enregistre " + winName(k["capture-loop"]) + " : tes amis entendent tout en double." : k["capture-other"] ? "Windows utilise " + winName(k["capture-other"]) + " au lieu de « Chat Mic »." : k["capture-none"] ? "Aucun micro par défaut." : "« Chat Mic » : c\u2019est bien ton micro XLR, traité par la GoXLR.", capBad && "Corriger", capBad && winFix));
    w.appendChild(echoItem(outBad ? "tip" : "ok", "Sortie de Windows", outBad ? "Le son part sur " + (winName(outBad) || "aucune sortie") + " au lieu de la GoXLR." : "« " + esc(WIN.render.console.name || "System") + " » et « " + esc(WIN.render.communications.name || "Chat") + " » pour les appels.", outBad && "Passer par la GoXLR", outBad && winFix));
  }
  b.appendChild(w);
  var d = h("div", {"class": "echo-sec"}, "<h4>Discord</h4>");
  d.appendChild(echoItem("tip", "Réglages conseillés", "Entrée « Chat Mic », sortie « Chat ». Avec un casque, coupe l\u2019annulation d\u2019écho et le réglage automatique du gain de Discord : ils écrasent ta voix sans rien apporter."));
  b.appendChild(d);
  var t = h("div", {"class": "echo-sec"}, "<h4>Test d\u2019écho</h4>"), r = st.echoRes;
  if (r && r.running) t.appendChild(echoItem("info", "Test en cours", esc(r.step || "…")));
  else if (r && r.error) t.appendChild(echoItem("warn", "Test impossible", esc(r.error), "Réessayer", runEcho));
  else if (r) t.appendChild(echoItem(r.level === "none" ? "ok" : r.level === "strong" ? "warn" : "tip", r.level === "none" ? "Aucun écho" : r.level === "strong" ? "Écho fort" : "Léger écho", r.text, "Refaire le test", runEcho));
  else t.appendChild(echoItem("info", "Vérifie en vrai, en 3 secondes", "Rustudio joue trois bips dans la voie Discord (tu les entends dans le casque) et écoute ton micro. Reste silencieux, et pas pendant un live.", "Lancer le test", runEcho));
  b.appendChild(t);
}
function runEcho() {
  var st = sheetState; if (!st || !st.echo) return Promise.resolve();
  if (S && S.state.mutes.Chat) { st.echoRes = {error: "La voie Chat (Discord) est coupée : réactive-la le temps du test."}; paintEcho(); return Promise.resolve(); }
  st.echoRes = {running: true, step: "Préparation…"}; paintEcho();
  return echoTest(function (s) { if (st.echoRes && st.echoRes.running) { st.echoRes.step = s; paintEcho(); } }).then(function (r) {
    var loop = S && S.state.routing.Chat.ChatMic;
    var txt = r.level === "none" ? "Ton micro n\u2019a rien capté des bips : tes amis ne s\u2019entendent pas."
      : r.level === "strong" ? "Les bips joués dans Discord reviennent dans ton micro (" + Math.max(0, Math.round(r.delay)) + " ms plus tard, " + Math.round(r.db) + " dB) : tes amis s\u2019entendent. " + (loop ? "Cause : Discord est renvoyé vers Discord (routage Chat → Chat Mic)." : "Vérifie le micro de Windows ci-dessus et le routage de Chat.")
      : "Ton micro capte un peu les bips (" + Math.round(r.db) + " dB) : le casque s\u2019entend dans le micro. Baisse un peu le casque ou éloigne le micro.";
    st.echoRes = {level: r.level, text: txt}; paintEcho();
  }, function (e) { st.echoRes = {error: e && e.message ? e.message : "erreur inconnue"}; paintEcho(); });
}
function fftInPlace(re, im, inv) {
  var n = re.length, i, j, k, t;
  for (i = 1, j = 0; i < n; i++) {
    var bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) { t = re[i]; re[i] = re[j]; re[j] = t; t = im[i]; im[i] = im[j]; im[j] = t; }
  }
  for (var len = 2; len <= n; len <<= 1) {
    var ang = 2 * Math.PI / len * (inv ? 1 : -1), wr = Math.cos(ang), wi = Math.sin(ang), half = len >> 1;
    for (i = 0; i < n; i += len) {
      var cr = 1, ci = 0;
      for (k = 0; k < half; k++) {
        var a = i + k, b2 = a + half, xr = re[b2] * cr - im[b2] * ci, xi = re[b2] * ci + im[b2] * cr;
        re[b2] = re[a] - xr; im[b2] = im[a] - xi; re[a] += xr; im[a] += xi;
        t = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = t;
      }
    }
  }
}
/* correlation croisee normalisee entre l'enregistrement et les bips joues : niveau et retard de l'echo */
function echoAnalyse(rec, ref, sr, offset) {
  var N = 1, i;
  while (N < rec.length + ref.length) N <<= 1;
  var ar = new Float64Array(N), ai = new Float64Array(N), br = new Float64Array(N), bi = new Float64Array(N);
  for (i = 0; i < rec.length; i++) ar[i] = rec[i];
  for (i = 0; i < ref.length; i++) br[i] = ref[i];
  fftInPlace(ar, ai, false); fftInPlace(br, bi, false);
  for (i = 0; i < N; i++) { var re = ar[i] * br[i] + ai[i] * bi[i], im = ai[i] * br[i] - ar[i] * bi[i]; ar[i] = re; ai[i] = im; }
  fftInPlace(ar, ai, true);
  var lo = Math.max(0, offset - Math.round(0.05 * sr)), hi = Math.max(lo, Math.min(rec.length - ref.length, offset + Math.round(0.8 * sr)));
  var best = 0, lag = lo;
  for (i = lo; i <= hi; i++) { var v = Math.abs(ar[i]) / N; if (v > best) { best = v; lag = i; } }
  var eref = 0, eseg = 0;
  for (i = 0; i < ref.length; i++) eref += ref[i] * ref[i];
  for (i = lag; i < lag + ref.length && i < rec.length; i++) eseg += rec[i] * rec[i];
  var ncc = best / Math.sqrt(eref * eseg + 1e-12), db = 20 * Math.log10(Math.max(1e-6, best / (eref + 1e-12)));
  return {level: ncc > 0.4 && db > -24 ? "strong" : ncc > 0.2 && db > -66 ? "light" : "none", ncc: ncc, db: db, delay: (lag - offset) / sr * 1000};
}
function echoSweeps(sr) {
  var n = Math.round(0.3 * sr), gap = Math.round(0.35 * sr), ref = new Float32Array(3 * n + 2 * gap), f0 = 400, f1 = 3200, T = n / sr, L = Math.log(f1 / f0);
  for (var k = 0; k < 3; k++) for (var i = 0; i < n; i++) {
    var t = i / sr, ph = 2 * Math.PI * f0 * T / L * (Math.exp(t / T * L) - 1), env = Math.min(1, i / (0.01 * sr), (n - i) / (0.01 * sr));
    ref[k * (n + gap) + i] = 0.25 * env * Math.sin(ph);
  }
  return ref;
}
function echoTest(onStep) {
  var md = navigator.mediaDevices;
  if (!md || !md.getUserMedia || !window.AudioContext) return Promise.reject(new Error("Le test d\u2019écho demande l\u2019appli Rustudio ou un navigateur récent."));
  var stream = null, cin = null, cout = null;
  function cleanup() {
    if (stream) stream.getTracks().forEach(function (t) { t.stop(); });
    if (cin) cin.close().catch(function () {}); if (cout) cout.close().catch(function () {});
  }
  onStep("Recherche de « Chat » et « Chat Mic »…");
  return md.getUserMedia({audio: true}).then(function (s) { s.getTracks().forEach(function (t) { t.stop(); }); return md.enumerateDevices(); }).then(function (devs) {
    var mic = devs.filter(function (d) { return d.kind === "audioinput" && /chat mic/i.test(d.label); })[0];
    var out = devs.filter(function (d) { return d.kind === "audiooutput" && /(^|[^a-z])chat([^a-z]|$)/i.test(d.label) && !/mic/i.test(d.label); })[0];
    if (!mic) throw new Error("Entrée « Chat Mic » introuvable : la GoXLR est-elle branchée ?");
    if (!out) throw new Error("Sortie « Chat » introuvable dans Windows.");
    return md.getUserMedia({audio: {deviceId: {exact: mic.deviceId}, echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1}})
      .then(function (s) { stream = s; return out; });
  }).then(function (out) {
    cin = new AudioContext();
    var sr = cin.sampleRate;
    cout = new AudioContext({sampleRate: sr});
    return (cout.setSinkId ? cout.setSinkId(out.deviceId) : Promise.reject(new Error("Sortie audio non sélectionnable ici."))).then(function () {
      var ref = echoSweeps(sr), buf = cout.createBuffer(1, ref.length, sr);
      buf.copyToChannel(ref, 0);
      var src = cin.createMediaStreamSource(stream), proc = cin.createScriptProcessor(4096, 1, 1), chunks = [], got = 0;
      proc.onaudioprocess = function (e) { var c = new Float32Array(e.inputBuffer.getChannelData(0)); chunks.push(c); got += c.length; };
      src.connect(proc); proc.connect(cin.destination);
      onStep("Silence : trois bips dans Discord, j\u2019écoute ton micro…");
      return new Promise(function (ok) {
        var offset = 0;
        setTimeout(function () { var p = cout.createBufferSource(); p.buffer = buf; p.connect(cout.destination); offset = got; p.start(); }, 500);
        setTimeout(function () { proc.disconnect(); src.disconnect(); ok({chunks: chunks, offset: offset, ref: ref, sr: sr}); }, 500 + ref.length / sr * 1000 + 1000);
      });
    });
  }).then(function (d) {
    onStep("Analyse…");
    var total = 0; d.chunks.forEach(function (c) { total += c.length; });
    var rec = new Float32Array(total), o = 0;
    d.chunks.forEach(function (c) { rec.set(c, o); o += c.length; });
    cleanup();
    return echoAnalyse(rec, d.ref, d.sr, d.offset);
  }).catch(function (e) { cleanup(); throw e; });
}
