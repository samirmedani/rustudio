// GoXLR Mini : assistant « Régler ma voix ».
"use strict";

/* ---------- assistant « Régler ma voix » ---------- */
function micCal() { var c = S && S.state.mic_cal; return c && c.type === S.state.mic.type ? c : null; }
function dbTxt(v) { v = Math.round(v); return (v < 0 ? "\u2212" : "") + Math.abs(v) + " dB"; }
var VOICE_READ = "« Salut tout le monde, bienvenue sur le live ! Aujourd\u2019hui je teste mon nouveau réglage de micro. Je parle comme d\u2019habitude, et quand c\u2019est drôle, je ris franchement. »";
var VOICE_STEPS = [["detect", "Micro"], ["voice", "Ta voix"], ["silence", "Silence"]];
var VOICE_PHASE = {
  detect: ["radar", "Dis quelques mots", "J\u2019essaie l\u2019entrée XLR, avec et sans l\u2019alimentation 48 V, puis le jack."],
  voice: ["audioLines", "Parle comme en live", "Lis ce texte à voix haute, à ton volume habituel :"],
  silence: ["volumeX", "Silence…", "Ne parle plus et laisse le son du casque comme d\u2019habitude : je mesure ce que le micro entend entre tes phrases."],
  apply: ["sparkles", "Presque fini", "Le gain se cale sur tes mesures."]
};
var MIC_TYPE_LONG = {condenser: "Statique · 48 V", dynamic: "Dynamique", jack: "Jack 3,5 mm"};
function voiceIntro() {
  if (!S || !S.connected) { toast("Branche la GoXLR pour régler ta voix"); return; }
  if (S.state.mutes.Mic) { toast("Ton micro est coupé", "Réactiver", function () { postModule("goxlr-mini", "mute", {channel: "Mic", muted: false}).then(refresh); }); return; }
  var body = h("div");
  body.innerHTML = '<div class="ring soft"><div class="ico">' + svg("sparkles") + "</div></div><h2>Régler ma voix</h2>" +
    "<p>Vingt secondes : je trouve ton micro, j\u2019écoute ta voix puis le silence de ta pièce. Ensuite je règle le gain pour toi. Aucun traitement n\u2019est allumé : tu choisis ensuite ceux que tu veux.</p>" +
    '<div class="vs-tips">' + [["Mic", "Place-toi comme en live, à 10–15 cm du micro, face à lui."], ["Headphones", "Garde ton casque et le son du PC comme d\u2019habitude."], ["audioLines", "Parle à ton volume normal quand je te le demande."]]
      .map(function (t) { return "<div>" + svg(t[0]) + "<span>" + t[1] + "</span></div>"; }).join("") + "</div>";
  sheet({body: body, actions: [["Plus tard", null], ["Commencer", function () { startJob("voice-setup", {}); }, "primary"]]});
}
function showVoiceJob(j) {
  if (!j.running) {
    if (j.error) { sheet({icon: "warn", title: "Réglage interrompu", text: esc(j.error), actions: [["Fermer", null], ["Recommencer", function () { startJob("voice-setup", {}); }, "primary"]]}); return; }
    return showVoiceResult(j.result || {});
  }
  var ph = VOICE_PHASE[j.phase] ? j.phase : "detect", p = j.progress || 0;
  if (!(sheetState && sheetState.voice)) {
    var body = h("div");
    body.innerHTML = '<div class="vs-steps" id="vs-steps">' + VOICE_STEPS.map(function (s, i) { return '<span><i data-n="' + (i + 1) + '">' + (i + 1) + "</i>" + s[1] + "</span>"; }).join("") + "</div>" +
      '<div class="ring"><svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="44" fill="none" stroke="rgba(0,0,0,.08)" stroke-width="7"/><circle id="ring-p" cx="50" cy="50" r="44" fill="none" stroke="#0071E3" stroke-width="7" stroke-linecap="round" stroke-dasharray="276.5" stroke-dashoffset="276.5" style="transition:stroke-dashoffset .2s"/></svg><div class="ico" id="vs-ico"></div></div>' +
      '<h2 id="vs-title"></h2><p id="vs-help"></p><div class="vs-read" id="vs-read">' + VOICE_READ + "</div>" +
      '<div class="sheet-meter"><div class="meter" id="sheet-meter">' + new Array(35).join("<i></i>") + '</div></div><div class="vs-status" id="vs-status"></div>';
    sheet({job: true, voice: true, body: body, actions: [["Annuler", function () { postModule("goxlr-mini", "job-cancel"); }]]});
  }
  var idx = ph === "apply" ? 3 : VOICE_STEPS.map(function (s) { return s[0]; }).indexOf(ph);
  document.querySelectorAll("#vs-steps span").forEach(function (e, i) {
    var st = i < idx ? "done" : i === idx ? "on" : "";
    if (e._st === st) return;
    e._st = st; e.className = st;
    var n = e.querySelector("i"); n.innerHTML = st === "done" ? svg("check") : n.dataset.n;
  });
  // anneau : avancement de l'etape ; secondes restantes pendant l'ecoute de la voix et du silence
  var span = {detect: [0, .3, 0], voice: [.32, .78, 10], silence: [.78, .95, 4.7], apply: [.95, 1, 0]}[ph];
  var f = clamp((p - span[0]) / (span[1] - span[0]), 0, 1);
  var rp = $("ring-p"); if (rp) rp.setAttribute("stroke-dashoffset", 276.5 * (1 - f));
  var ico = $("vs-ico"), left = span[2] ? Math.max(1, Math.ceil((1 - f) * span[2])) : 0;
  var icoHtml = left ? '<span class="num"><b>' + left + "<small>s</small></b></span>" : svg(VOICE_PHASE[ph][0]);
  if (ico._h !== icoHtml) { ico._h = icoHtml; ico.innerHTML = icoHtml; }
  if ($("vs-title").textContent !== VOICE_PHASE[ph][1]) { $("vs-title").textContent = VOICE_PHASE[ph][1]; $("vs-help").textContent = VOICE_PHASE[ph][2]; }
  $("vs-read").classList.toggle("on", ph === "voice");
  if ($("vs-status").textContent !== (j.step || "")) $("vs-status").textContent = j.step || "";
}
function showVoiceResult(r) {
  if (!r.cal) { closeSheet(); return; }
  var c = r.cal, preset = S.meta.mic_presets[r.preset], fx = r.fx || S.state.fx || {};
  var ratio = (r.comp.ratio_x >= 10 ? r.comp.ratio_x.toFixed(0) : r.comp.ratio_x.toFixed(1)).replace(".", ",") + ":1";
  var cells = [["Micro", MIC_TYPE_LONG[r.type] || r.type, ""], ["Gain", r.gain_db + " dB", "crêtes de ta voix vers \u221212 dB"],
    ["Ta voix", dbTxt(c.voice_db), "crêtes à " + dbTxt(c.voice_peak_db)], ["Bruit de la pièce", dbTxt(c.noise_peak_db), Math.round(r.snr_db) + " dB sous ta voix"],
    ["Noise gate", fx.gate ? dbTxt(r.gate.threshold) : "Coupé", (fx.gate ? "" : "prêt : ") + "seuil " + dbTxt(r.gate.threshold) + ", " + Math.abs(r.gate.attenuation_db) + " dB de moins"],
    ["Compresseur", fx.comp ? dbTxt(r.comp.threshold) : "Coupé", (fx.comp ? "" : "prêt : ") + ratio + " · compensation +" + r.comp.makeup + " dB"]];
  var anyFx = Object.keys(fx).some(function (k) { return fx[k]; });
  var body = h("div");
  body.innerHTML = '<div class="diag-head">' + chipIco("ok", "#30D158") + "<div><h2>Ta voix est réglée</h2><p class=\"muted-txt\">" + (anyFx ? "Préréglage " + esc(frLabel(preset ? preset.label : r.preset)) + ", calé sur ta voix et ta pièce." : "Gain calé sur ta voix. Aucun traitement allumé : ta voix reste naturelle, allume ceux que tu veux sur la page Micro.") + " Écoute-toi pour vérifier.</p></div></div>" +
    '<div class="diag-grid vs-grid">' + cells.map(function (x) { return "<div><small>" + x[0] + "</small><b>" + esc(x[1]) + "</b>" + (x[2] ? "<em>" + esc(x[2]) + "</em>" : "") + "</div>"; }).join("") + "</div>";
  var notes = h("div", {"class": "vs-notes"});
  (r.notes || []).forEach(function (n) {
    var it = h("div", {"class": "advice"});
    it.innerHTML = chipIco(n.lvl === "ok" ? "ok" : n.lvl === "warn" ? "warn" : "bulb", n.lvl === "ok" ? "#30D158" : n.lvl === "warn" ? "#FF9F0A" : "#64D2FF");
    var b = h("div", {}, "<p>" + esc(n.t) + "</p>");
    if (n.fix) {
      var btn = h("button", {"class": "pill"}, esc(n.fix.label));
      btn.addEventListener("click", function () {
        postModule("goxlr-mini", n.fix.action, n.fix.body).then(function (x) { if (x && x.ok) { btn.disabled = true; btn.innerHTML = svg("check") + "Corrigé"; refresh(); } });
      });
      b.appendChild(btn);
    }
    it.appendChild(b); notes.appendChild(it);
  });
  body.appendChild(notes);
  sheet({wide: true, body: body, actions: [["M\u2019écouter", listenSheet, "", true], ["Terminé", null, "primary"]]});
  refresh();
}
/* « M'écouter » : enregistrer puis reecouter (ce qu'entendent Discord et le stream), ou retour en direct */
var LISTEN_LEVEL = 191, REC_SECONDS = 8, lsn = null;
function micBlocked() {
  if (!S || !S.connected) { toast("Branche la GoXLR pour t\u2019écouter"); return true; }
  if (S.state.mutes.Mic) { toast("Ton micro est coupé", "Réactiver", function () { postModule("goxlr-mini", "mute", {channel: "Mic", muted: false}).then(refresh); }); return true; }
  return false;
}
function listenSheet() {
  if (L && L.listening > 0) { postModule("goxlr-mini", "mic-listen", {seconds: 0}); return; }     // retour en direct en cours : l'arreter
  if (micBlocked()) return;
  var body = h("div");
  lsn = {body: body, mode: "idle"};
  var me = lsn;
  sheet({body: body, listen: true, actions: [], onClose: function () { stopListen(me); }});
  paintListen();
}
function stopListen(o) {
  o = o || lsn; if (!o) return;
  o.cancelled = true;
  clearInterval(o.timer);
  if (o.mr && o.mr.state !== "inactive") try { o.mr.stop(); } catch (e) {}
  if (o.stream) o.stream.getTracks().forEach(function (t) { t.stop(); });
  if (o.audio) { o.audio.pause(); o.audio = null; }
  if (o.mode === "live" && L && L.listening > 0) postModule("goxlr-mini", "mic-listen", {seconds: 0});
  if (o.url) URL.revokeObjectURL(o.url);
  if (o === lsn) lsn = null;
}
function ringHtml(inner) {
  return '<div class="ring"><svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="44" fill="none" stroke="rgba(0,0,0,.08)" stroke-width="7"/><circle id="ring-p" cx="50" cy="50" r="44" fill="none" stroke="#0071E3" stroke-width="7" stroke-linecap="round" stroke-dasharray="276.5" stroke-dashoffset="276.5" style="transition:stroke-dashoffset .2s"/></svg><div class="ico" id="ls-ico">' + inner + "</div></div>";
}
function lsButtons(list) {
  var row = h("div", {"class": "ls-actions"});
  list.forEach(function (a) { row.appendChild(h("button", {"class": "pill " + (a[2] || ""), onclick: a[1]}, a[0])); });
  return row;
}
function paintListen(msg) {
  var o = lsn; if (!o) return;
  var b = o.body; b.innerHTML = "";
  if (o.mode === "idle" || o.mode === "done") {
    var done = o.mode === "done";
    b.innerHTML = '<div class="ring soft"><div class="ico">' + svg(done ? "ok" : "ear") + "</div></div><h2>" + (done ? "Voilà ta voix" : "Écoute ta voix") + "</h2><p>" +
      (msg ? esc(msg) : done ? "C\u2019est ce qu\u2019entendent tes amis sur Discord et ton stream. Si elle sonne loin ou résonne, rapproche-toi du micro (10 à 15 cm)."
        : "Enregistre-toi quelques secondes, puis réécoute : tu entends ta voix traitée, comme tes amis sur Discord et ton stream.") + "</p>";
    var acts = [["Fermer", function () { closeSheet(); }], ["En direct", startLive]];
    if (done) acts.push(["Réécouter", playBack], ["Refaire", startRecording, "primary"]);
    else acts.push(["Enregistrer " + REC_SECONDS + " s", startRecording, "primary"]);
    b.appendChild(lsButtons(acts));
  } else if (o.mode === "rec") {
    b.innerHTML = ringHtml("") + "<h2>Parle comme en live</h2><p>Lis ce texte à voix haute :</p>" + '<div class="vs-read on">' + VOICE_READ + "</div>" +
      '<div class="sheet-meter"><div class="meter" id="sheet-meter">' + new Array(35).join("<i></i>") + "</div></div>";
    b.appendChild(lsButtons([["Annuler", function () { var x = lsn; stopListen(x); listenSheet(); }]]));
  } else if (o.mode === "play") {
    b.innerHTML = ringHtml(svg("audioLines")) + "<h2>Réécoute…</h2><p>Ta voix telle que la reçoivent Discord et ton stream.</p>";
    b.appendChild(lsButtons([["Arrêter", function () { if (lsn && lsn.audio) lsn.audio.pause(); lsn.mode = "done"; paintListen(); }]]));
  } else if (o.mode === "live") {
    b.innerHTML = ringHtml("") + "<h2>Tu t\u2019entends en direct</h2><p>Avec un casque ouvert, ta vraie voix couvre une partie du retour : coupe la musique et monte le volume si besoin.</p>";
    var f = h("div", {"class": "field ls-field"}, '<div class="field-top">Volume du retour<span id="ls-vol"></span></div>');
    var sl = Slider({label: "Volume du retour micro", format: function (v) { return pct(v) + " %"; }, min: 64, max: 255, step: 1, value: o.level, colour: "#FF6961",
      onInput: function (v) { o.level = v; $("ls-vol").textContent = pct(v) + " %"; },
      onCommit: function (v) { o.level = v; postModule("goxlr-mini", "mic-listen", {seconds: Math.max(1, (L && L.listening) || 30), level: v}); }});
    f.appendChild(sl.el); b.appendChild(f);
    b.appendChild(lsButtons([["Arrêter", function () { postModule("goxlr-mini", "mic-listen", {seconds: 0}); lsn.mode = "idle"; paintListen(); }, "primary"]]));
    $("ls-vol").textContent = pct(o.level) + " %";
  }
}
function startLive() {
  if (micBlocked() || !lsn) return;
  lsn.mode = "live"; lsn.level = lsn.level || LISTEN_LEVEL; lsn.liveSeen = false;
  paintListen();
  postModule("goxlr-mini", "mic-listen", {seconds: 30, level: lsn.level});
}
function chatMicStream() {
  var md = navigator.mediaDevices;
  if (!md || !md.getUserMedia || !window.MediaRecorder) return Promise.reject(new Error("L\u2019enregistrement n\u2019est pas disponible ici."));
  var want = {echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 2};
  return md.enumerateDevices().then(function (list) {
    if (list.some(function (d) { return d.label; })) return list;
    return md.getUserMedia({audio: true}).then(function (s0) { s0.getTracks().forEach(function (t) { t.stop(); }); return md.enumerateDevices(); });
  }).then(function (list) {
    var mic = list.filter(function (d) { return d.kind === "audioinput" && /chat mic/i.test(d.label); })[0];
    if (!mic && !S.sim) throw new Error("Windows ne montre pas l\u2019entrée « Chat Mic » de la GoXLR.");
    if (mic) want.deviceId = {exact: mic.deviceId};
    return md.getUserMedia({audio: want});
  });
}
function startRecording() {
  if (micBlocked() || !lsn) return;
  var o = lsn;
  if (o.audio) { o.audio.pause(); o.audio = null; }
  o.mode = "rec"; o.cancelled = false; paintListen();
  chatMicStream().then(function (stream) {
    if (o.cancelled) { stream.getTracks().forEach(function (t) { t.stop(); }); return; }
    o.stream = stream;
    var mime = MediaRecorder.isTypeSupported("audio/webm;codecs=opus") ? "audio/webm;codecs=opus" : "";
    var mr = new MediaRecorder(stream, mime ? {mimeType: mime, audioBitsPerSecond: 256000} : {});
    var chunks = [];
    mr.ondataavailable = function (e) { if (e.data && e.data.size) chunks.push(e.data); };
    mr.onstop = function () {
      stream.getTracks().forEach(function (t) { t.stop(); });
      clearInterval(o.timer);
      if (o.cancelled || lsn !== o) return;
      if (o.url) URL.revokeObjectURL(o.url);
      o.url = URL.createObjectURL(new Blob(chunks, {type: mr.mimeType || "audio/webm"}));
      playBack();
    };
    o.mr = mr; mr.start(); o.t0 = performance.now();
    o.timer = setInterval(function () {
      var t = (performance.now() - o.t0) / 1000, left = Math.max(0, REC_SECONDS - t);
      var rp = $("ring-p"); if (rp) rp.setAttribute("stroke-dashoffset", 276.5 * (1 - Math.min(1, t / REC_SECONDS)));
      var ico = $("ls-ico"), html = '<span class="num"><b>' + Math.max(1, Math.ceil(left)) + "<small>s</small></b></span>";
      if (ico && ico._h !== html) { ico._h = html; ico.innerHTML = html; }
      if (left <= 0 && mr.state === "recording") mr.stop();
    }, 100);
  }).catch(function (e) {
    if (lsn !== o) return;
    o.mode = "idle"; paintListen("Impossible d\u2019enregistrer : " + ((e && e.message) || e));
  });
}
function playBack() {
  var o = lsn; if (!o || !o.url) return;
  if (o.audio) o.audio.pause();
  var a = new Audio(o.url);
  o.audio = a; o.mode = "play"; paintListen();
  a.ontimeupdate = function () { var rp = $("ring-p"); if (rp && a.duration && isFinite(a.duration)) rp.setAttribute("stroke-dashoffset", 276.5 * (1 - a.currentTime / a.duration)); };
  a.onended = function () { if (lsn === o && o.mode === "play") { o.mode = "done"; paintListen(); } };
  a.play().catch(function () { if (lsn === o) { o.mode = "done"; paintListen("Lecture impossible : clique « Réécouter »."); } });
}
function liveListen() {
  var left = L && L.listening > 0 ? Math.ceil(L.listening) : 0;
  var t = left ? svg("stop") + "Arrêter · " + left + " s" : svg("ear") + "M\u2019écouter";
  document.querySelectorAll(".listen-btn").forEach(function (b) { if (b._t !== t) { b._t = t; b.innerHTML = t; b.classList.toggle("on", !!left); } });
  if (lsn && lsn.mode === "live" && sheetState && sheetState.listen) {
    if (left) lsn.liveSeen = true;
    else if (lsn.liveSeen) { lsn.mode = "idle"; paintListen(); return; }
    var rp = $("ring-p"); if (rp) rp.setAttribute("stroke-dashoffset", 276.5 * (1 - Math.min(1, (L.listening || 0) / 30)));
    var ico = $("ls-ico"), html = left ? '<span class="num"><b>' + left + "<small>s</small></b></span>" : svg("ear");
    if (ico && ico._h !== html) { ico._h = html; ico.innerHTML = html; }
  }
}
