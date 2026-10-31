// GoXLR Mini : page Micro.
"use strict";

/* ---------- micro ---------- */
var mic = {};
function buildMic() {
  var root = $("p-mic"); root.innerHTML = "";
  mic.model = null; mic.vp = null;
  var grid = h("div", {"class": "mic-grid"});
  var left = h("div", {style: "display:flex;flex-direction:column;gap:16px"});
  if (micModelsReady()) left.appendChild(buildMicModelCard());
  var inCard = h("div", {"class": "card"});
  inCard.innerHTML = '<h3>' + svg("Mic", ' style="width:16px;height:16px;color:#FF453A"') + 'Entrée micro<small id="mic-state"></small></h3>';
  var meter = h("div", {"class": "meter"});
  for (var i = 0; i < 34; i++) meter.appendChild(h("i"));
  var db = h("div", {"class": "meter-db"}, '<span><b id="mic-db">—</b><span class="clip" id="mic-clip">SATURE</span></span><span>-72 dB · 0 dB</span>');
  inCard.appendChild(meter); inCard.appendChild(db);
  var typeField = h("div", {"class": "field"}, '<div class="field-top">Type de micro</div>');
  var seg = h("div", {"class": "seg"});
  S.meta.mic_types.forEach(function (t) {
    var b = h("button", {"data-t": t}, MIC_TYPE_LABELS[t]);
    b.addEventListener("click", function () {
      if (t === "condenser" && S.state.mic.type !== "condenser") toast("Alimentation fantôme 48 V activée pour le micro statique");
      postModule("goxlr-mini", "mic", {type: t, suggest_gain: true}).then(refresh);
    });
    seg.appendChild(b);
  });
  typeField.appendChild(seg);
  inCard.appendChild(typeField);
  var gainField = h("div", {"class": "field"}, '<div class="field-top">Gain<span id="gain-val"></span></div>');
  var gainSend = throttle(function (v) { postModule("goxlr-mini", "mic", {gain_db: v}); }, 60);
  mic.gain = Slider({label: "Gain du micro", format: function (v) { return v + " dB"; }, min: 0, max: 72, step: 1, colour: "#FF453A", onInput: function (v) { S.state.mic.gain_db = v; $("gain-val").textContent = v + " dB"; gainSend(v); }, onCommit: function (v) { postModule("goxlr-mini", "mic", {gain_db: v}); }});
  gainField.appendChild(mic.gain.el);
  inCard.appendChild(gainField);
  var row = h("div", {"class": "btn-row"});
  row.appendChild(h("button", {"class": "pill listen-btn", title: "Enregistre ta voix et réécoute-la, ou écoute-toi en direct", onclick: listenSheet}, svg("ear") + "M\u2019écouter"));
  row.appendChild(h("button", {"class": "pill primary", onclick: voiceIntro}, svg("sparkles") + "Régler ma voix"));
  inCard.appendChild(row);
  inCard.appendChild(h("div", {"class": "cal-line", id: "cal-line"}));
  inCard.appendChild(h("button", {"class": "pill", style: "width:100%;margin-top:12px", onclick: openEchoSheet}, svg("shield") + "Anti-écho : tout vérifier en 3 secondes"));
  left.appendChild(inCard);
  var muteCard = h("div", {"class": "card"});
  var ms = h("div", {"class": "row-switch", style: "padding-top:0"}, '<div><b>Micro coupé</b><small id="mic-mute-sub">Coupé partout : personne ne t\u2019entend.</small></div>');
  mic.muteSw = h("button", {"class": "switch"});
  mic.muteSw.addEventListener("click", function () { toggleMute("Mic", "all"); });
  ms.appendChild(mic.muteSw); muteCard.appendChild(ms);
  var mbf = h("div", {"class": "field", style: "margin-top:8px"}, '<div class="field-top">Bouton micro de l\u2019appareil</div>');
  mic.mbSeg = h("div", {"class": "seg"});
  MIC_MODES.forEach(function (m) {
    var b = h("button", {"data-m": m[0]}, m[1]);
    b.addEventListener("click", function () {
      S.state.mic_button.mode = m[0]; renderMic();
      postModule("goxlr-mini", "settings", {mic_button: {mode: m[0]}}).then(refresh);
      if (m[0] === "ptt") toast("Micro coupé au repos : garde le bouton appuyé pour parler");
    });
    mic.mbSeg.appendChild(b);
  });
  mbf.appendChild(mic.mbSeg);
  var tgRow = h("div", {style: "display:flex;align-items:center;gap:10px;margin-top:10px"}, '<span class="muted-txt" style="font-size:12px;white-space:nowrap">Il coupe</span>');
  mic.mbTarget = h("div", {"class": "chips"});
  [["all", "Partout"], ["stream", "Du stream"], ["chat", "De Discord"]].forEach(function (t) {
    var b = h("button", {"data-t": t[0]}, t[1]);
    b.addEventListener("click", function () { S.state.mic_button.target = t[0]; renderMic(); postModule("goxlr-mini", "settings", {mic_button: {target: t[0]}}).then(refresh); });
    mic.mbTarget.appendChild(b);
  });
  tgRow.appendChild(mic.mbTarget);
  mbf.appendChild(tgRow);
  var bl = h("div", {"class": "row-switch", style: "padding-bottom:0"}, '<div><b>Clignoter quand il est coupé</b><small>Impossible d\u2019oublier que tu es muet.</small></div>');
  mic.blinkSw = h("button", {"class": "switch"});
  mic.blinkSw.addEventListener("click", function () { var v = !S.state.mic_button.blink; S.state.mic_button.blink = v; renderMic(); postModule("goxlr-mini", "settings", {mic_button: {blink: v}}); });
  bl.appendChild(mic.blinkSw); mbf.appendChild(bl);
  muteCard.appendChild(mbf);
  var monSw = h("div", {"class": "row-switch", style: "padding-bottom:0"}, '<div><b>M\u2019entendre dans le casque</b><small id="mon-sub"></small></div>');
  mic.monSw = h("button", {"class": "switch"});
  mic.monSw.addEventListener("click", function () { monitorSet(!S.state.routing.Microphone.Headphones); });
  monSw.appendChild(mic.monSw); muteCard.appendChild(monSw);
  var mon = h("div", {"class": "field", id: "mon-field"}, '<div class="field-top">Volume du retour<span id="mon-val"></span></div>');
  mic.mon = Slider({label: "Volume du retour micro", format: function (v) { return pct(v) + " %"; }, min: 0, max: 255, step: 1, colour: "#FF6961", onInput: function (v) { S.state.volumes.MicMonitor = v; $("mon-val").textContent = pct(v) + " %"; volumeSender("MicMonitor")(v); }, onCommit: function (v) { postModule("goxlr-mini", "volume", {channel: "MicMonitor", value: v}); }});
  mon.appendChild(mic.mon.el); muteCard.appendChild(mon);
  left.appendChild(muteCard);
  grid.appendChild(left);

  var right = h("div", {style: "display:flex;flex-direction:column;gap:16px;min-width:0"});
  var fxc = h("div", {"class": "card"}, '<h3>Traitements de ta voix<small id="fx-hint"></small></h3>');
  mic.fxSw = {};
  FX_INFO.forEach(function (x) {
    var row = h("div", {"class": "row-switch"}, "<div><b>" + x[1] + "</b><small>" + x[2] + "</small></div>");
    var sw = h("button", {"class": "switch", "aria-label": x[1]});
    sw.addEventListener("click", function () { fxSet(x[0], !S.state.fx[x[0]]); });
    row.appendChild(sw); fxc.appendChild(row); mic.fxSw[x[0]] = sw;
  });
  // Les interrupteurs des traitements terminent la colonne de gauche, à côté de leurs réglages fins :
  // les deux colonnes ont ainsi la même hauteur, et l'ordre reste le même sur une seule colonne.
  left.appendChild(fxc);
  if (micModelsReady()) right.appendChild(buildVoiceCard());
  var pc = h("div", {"class": "card"}, '<h3>Préréglages de voix<small id="preset-hint"></small></h3>');
  var pg = h("div", {"class": "presets"});
  mic.presetBtns = {};
  Object.keys(S.meta.mic_presets).forEach(function (k) {
    var p = S.meta.mic_presets[k];
    var b = h("button", {"class": "preset"}, chipIco(PRESET_ICONS[k], PRESET_COLOURS[k]) + "<b>" + frLabel(p.label) + "</b><p>" + frLabel(p.desc) + "</p>");
    b.addEventListener("click", function () {
      var fit = k !== "brut" && voiceValid();
      withUndo("Préréglage " + frLabel(p.label) + (fit ? " sur mesure" : "") + " appliqué", function () { return postModule("goxlr-mini", "mic-preset", {name: k}).then(refresh); });
    });
    pg.appendChild(b); mic.presetBtns[k] = b;
  });
  pc.appendChild(pg);
  pc.appendChild(h("p", {id: "preset-tip", "class": "muted-txt", style: "margin-top:12px;font-size:12px;line-height:1.45"}));
  right.appendChild(pc);

  var fine = h("div", {"class": "card", style: "flex:1"});
  var tabs = h("div", {"class": "tabs"});
  var panes = h("div");
  mic.tabs = {};
  fine.appendChild(h("div", {"class": "fx-off", id: "fx-off"}));
  [["eq", "Égaliseur"], ["gate", "Noise gate"], ["comp", "Compresseur"], ["more", "De-esser & bip"]].forEach(function (t, i) {
    var b = h("button", {}, t[1]);
    b.addEventListener("click", function () { showTab(t[0]); });
    tabs.appendChild(b); mic.tabs[t[0]] = {btn: b};
  });
  fine.appendChild(tabs); fine.appendChild(panes);
  // egaliseur : la courbe reelle (frequence et forme de chaque bande), puis un curseur de gain par bande
  var eq = h("div", {"class": "mic-eq"});
  var graph = h("div", {"class": "eq-graph"});
  var cv = h("canvas", {role: "img", "aria-label": "Courbe de l\u2019égaliseur du micro"}); graph.appendChild(cv); mic.eqCanvas = cv;
  eq.appendChild(graph);
  var bands = h("div", {"class": "eq-bands"});
  mic.eq = {};
  var dspSend = throttle(function (patch) { postModule("goxlr-mini", "dsp", patch); }, 80);
  S.meta.eq_bands.forEach(function (b, i) {
    var wrap = h("div", {"class": "eq-band"});
    var lab = h("b");
    var sl = Slider({label: "Égaliseur micro " + freqLabel(+b), format: function (v) { return v + " dB"; }, vertical: true, min: -9, max: 9, step: 1, onInput: function (v) { S.state.dsp.eq[b] = v; lab.textContent = (v > 0 ? "+" : "") + v + " dB"; drawEq(); var p = {eq: {}}; p.eq[b] = v; dspSend(p); }});
    var freq = h("button", {"class": "eq-freq", type: "button", title: "Changer la fréquence de cette bande"}, freqLabel(+b));
    freq.addEventListener("click", function () { openEqFreq(b, freq); });
    var hot = function (on) { var was = mic.eqHot; mic.eqHot = on ? b : mic.eqHot === b ? null : mic.eqHot; if (was !== mic.eqHot) drawEq(); };
    [sl.el, freq].forEach(function (e) {
      e.addEventListener("pointerenter", function () { hot(true); });
      e.addEventListener("pointerleave", function () { if (document.activeElement !== sl.el && document.activeElement !== freq) hot(false); });
      e.addEventListener("focus", function () { hot(true); });
      e.addEventListener("blur", function () { hot(false); });
    });
    wrap.appendChild(lab); wrap.appendChild(sl.el);
    wrap.appendChild(freq);
    bands.appendChild(wrap); mic.eq[b] = {sl: sl, lab: lab, freq: freq};
  });
  eq.appendChild(bands);
  mic.tabs.eq.pane = eq;
  // gate
  var gate = h("div");
  gate.appendChild(h("p", {"class": "muted-txt", style: "margin-bottom:6px"}, "Baisse le micro quand tu ne parles pas. Garde le seuil bien sous ta voix calme."));
  mic.gateThr = fineSlider(gate, "Seuil", -59, 0, function (v) { return v + " dB"; }, function (v) { S.state.dsp.gate.threshold = v; dspSend({gate: {threshold: v}}); });
  mic.gateAtt = fineSlider(gate, "Réduction", 0, 100, function (v) { return v + " %"; }, function (v) { S.state.dsp.gate.attenuation = v; dspSend({gate: {attenuation: v}}); });
  mic.tabs.gate.pane = gate;
  // compresseur
  var comp = h("div");
  comp.appendChild(h("p", {"class": "muted-txt", style: "margin-bottom:6px"}, "Égalise ta voix : les passages forts sont adoucis, les faibles remontés."));
  mic.compThr = fineSlider(comp, "Seuil", -40, 0, function (v) { return v + " dB"; }, function (v) { S.state.dsp.comp.threshold = v; dspSend({comp: {threshold: v}}); });
  mic.compRatio = fineSlider(comp, "Ratio", 0, 14, RATIO_LABEL, function (v) { S.state.dsp.comp.ratio = v; dspSend({comp: {ratio: v}}); });
  mic.compMake = fineSlider(comp, "Gain de compensation", -6, 24, function (v) { return (v > 0 ? "+" : "") + v + " dB"; }, function (v) { S.state.dsp.comp.makeup = v; dspSend({comp: {makeup: v}}); });
  mic.tabs.comp.pane = comp;
  // de-esser et bip
  var more = h("div");
  mic.deess = fineSlider(more, "De-esser (adoucit les « s »)", 0, 100, function (v) { return v + " %"; }, function (v) { S.state.dsp.deesser = v; dspSend({deesser: v}); });
  mic.bleep = fineSlider(more, "Volume du bip (bouton bleep)", -36, 0, function (v) { return v + " dB"; }, function (v) { S.state.bleep_db = v; dspSend({bleep_db: v}); });
  mic.tabs.more.pane = more;
  ["eq", "gate", "comp", "more"].forEach(function (k) { panes.appendChild(mic.tabs[k].pane); });
  right.appendChild(fine);
  grid.appendChild(right);
  root.appendChild(grid);
  showTab(mic.tab || "eq");
  built.mic = true;
}
function frLabel(s) { return s; }
function fineSlider(parent, label, min, max, fmt, onInput) {
  var f = h("div", {"class": "field"});
  var top = h("div", {"class": "field-top"}, label);
  var v = h("span"); top.appendChild(v); f.appendChild(top);
  var sl = Slider({label: label, format: fmt, min: min, max: max, step: 1, colour: "var(--accent)", onInput: function (x) { v.textContent = fmt(x); onInput(x); }});
  f.appendChild(sl.el); parent.appendChild(f);
  return {set: function (x) { sl.set(x); if (!sl.busy()) v.textContent = fmt(x); }};
}
function showTab(k) {
  mic.tab = k;
  Object.keys(mic.tabs).forEach(function (t) { mic.tabs[t].btn.classList.toggle("on", t === k); mic.tabs[t].pane.style.display = t === k ? "" : "none"; });
  fxOffNote();
  if (k === "eq") requestAnimationFrame(drawEq);
}
function fxOffNote() {
  var el = $("fx-off"); if (!el || !S) return;
  var key = {eq: "eq", gate: "gate", comp: "comp", more: "deesser"}[mic.tab || "eq"], info = FX_INFO.filter(function (x) { return x[0] === key; })[0];
  var off = !S.state.fx[key];
  el.style.display = off ? "" : "none";
  if (off) el.innerHTML = svg("info") + "<span>" + info[1] + " coupé : ces réglages ne s\u2019appliquent pas.</span>";
  if (off && !el.querySelector("button")) el.appendChild(h("button", {"class": "pill", onclick: function () { fxSet(key, true); }}, "Allumer"));
}
// Courbe réelle de l'égaliseur du micro : fréquences et formes des six bandes mesurées sur la Mini, de 30 Hz à 18 kHz.
// Chaque point se pose là où sa bande agit (sommet de la cloche, coude du plateau), comme les libellés des bandes.
function drawEq() {
  var cv = mic.eqCanvas; if (!cv || !cv.offsetWidth || !S) return;
  var w = cv.offsetWidth, ht = cv.offsetHeight, dpr = devicePixelRatio || 1;
  cv.width = Math.round(w * dpr); cv.height = Math.round(ht * dpr);
  var c = cv.getContext("2d"); c.setTransform(dpr, 0, 0, dpr, 0, 0);
  var st = S.state, on = !!(st.fx && st.fx.eq), F0 = 30, F1 = 18000, L = 10, R = 10, top = 12, bot = ht - 20, span = 12;
  function X(f) { return L + Math.log(f / F0) / Math.log(F1 / F0) * (w - L - R); }
  function Y(db) { return top + (span - clamp(db, -span, span)) / (2 * span) * (bot - top); }
  c.lineWidth = 1; c.font = "10.5px Inter Variable, Segoe UI, sans-serif"; c.textBaseline = "middle"; c.textAlign = "center";
  [-9, 0, 9].forEach(function (g) { c.strokeStyle = g ? "rgba(0,0,0,.05)" : "rgba(0,0,0,.12)"; c.beginPath(); c.moveTo(L, Y(g)); c.lineTo(w - R, Y(g)); c.stroke(); });
  [[100, "100 Hz"], [1000, "1 kHz"], [10000, "10 kHz"]].forEach(function (t) {
    c.strokeStyle = "rgba(0,0,0,.05)"; c.beginPath(); c.moveTo(X(t[0]), top); c.lineTo(X(t[0]), bot); c.stroke();
    c.fillStyle = "rgba(60,60,67,.5)"; c.fillText(t[1], X(t[0]), bot + 10);
  });
  // forme du spectre de ta voix (empreinte valide), en fond, sur sa propre échelle : son sommet touche le haut du graphique
  var p = voiceValid() && st.voice_print;
  if (p && p.bands && p.voice_db) {
    var mid = vpMid(p.bands, p.voice_db), vs = [], vals = [];
    p.bands.forEach(function (f, i) { var v = p.voice_db[i]; if (f >= F0 && f <= F1 && v != null && isFinite(v)) vals.push([f, v - mid]); });
    var vtop = Math.max.apply(null, vals.map(function (x) { return x[1]; }).concat([0])), vbot = vtop - 42;
    vals.forEach(function (x) { vs.push([X(x[0]), top + 4 + (vtop - clamp(x[1], vbot, vtop)) / (vtop - vbot) * (bot - top - 4)]); });
    if (vs.length > 1) {
      c.beginPath(); vpPath(c, vs); c.lineTo(vs[vs.length - 1][0], bot); c.lineTo(vs[0][0], bot); c.closePath();
      var vg = c.createLinearGradient(0, top, 0, bot); vg.addColorStop(0, "rgba(118,118,128,.15)"); vg.addColorStop(1, "rgba(118,118,128,.02)");
      c.fillStyle = vg; c.fill();
      c.fillStyle = "rgba(60,60,67,.45)"; c.textAlign = "left"; c.fillText("Ta voix", L + 4, top + 6); c.textAlign = "center";
    }
  }
  var resp = micEqResponse(st), pts = [];
  for (var x = L; x <= w - R + 0.5; x += 2) pts.push([x, Y(resp(F0 * Math.pow(F1 / F0, (x - L) / (w - L - R))))]);
  c.beginPath(); c.moveTo(pts[0][0], Y(0)); pts.forEach(function (q) { c.lineTo(q[0], q[1]); }); c.lineTo(pts[pts.length - 1][0], Y(0)); c.closePath();
  c.fillStyle = on ? "rgba(0,113,227,.1)" : "rgba(118,118,128,.08)"; c.fill();
  c.beginPath(); pts.forEach(function (q, i) { if (i) c.lineTo(q[0], q[1]); else c.moveTo(q[0], q[1]); });
  if (on) {
    var grad = c.createLinearGradient(L, 0, w - R, 0);
    grad.addColorStop(0, "#5856D6"); grad.addColorStop(.5, "#0071E3"); grad.addColorStop(1, "#32ADE6");
    c.lineWidth = 2.5; c.strokeStyle = grad; c.shadowColor = "rgba(0,113,227,.3)"; c.shadowBlur = 8; c.stroke(); c.shadowBlur = 0;
  } else { c.setLineDash([5, 4]); c.lineWidth = 2; c.strokeStyle = "rgba(60,60,67,.38)"; c.stroke(); c.setLineDash([]); }
  S.meta.eq_bands.forEach(function (b) {
    var f = micEqFreq(b, st), x = X(f), y = Y(resp(f)), hot = mic.eqHot === b || (mic.freqBand === b && !!popEl);
    if (hot) {
      c.setLineDash([3, 3]); c.strokeStyle = "rgba(0,113,227,.45)"; c.lineWidth = 1;
      c.beginPath(); c.moveTo(x, top); c.lineTo(x, bot); c.stroke(); c.setLineDash([]);
      var t = freqLabel(f) + " · " + sgnDb(+st.dsp.eq[b] || 0), tw = c.measureText(t).width + 14, tx = clamp(x - tw / 2, 2, w - tw - 2), ty = y < top + 26 ? y + 14 : y - 26;
      c.fillStyle = "rgba(255,255,255,.95)"; c.strokeStyle = "rgba(0,113,227,.3)";
      c.beginPath(); if (c.roundRect) c.roundRect(tx, ty, tw, 18, 9); else c.rect(tx, ty, tw, 18); c.fill(); c.stroke();
      c.fillStyle = "#0058b0"; c.fillText(t, tx + tw / 2, ty + 9.5);
    }
    c.beginPath(); c.arc(x, y, hot ? 6 : 4.5, 0, Math.PI * 2);
    c.fillStyle = "#fff"; c.fill(); c.lineWidth = 2; c.strokeStyle = on ? "#0071E3" : "rgba(60,60,67,.45)"; c.stroke();
  });
}
function renderMic() {
  if (!built.mic) buildMic();
  var st = S.state;
  document.querySelectorAll("#p-mic .seg button").forEach(function (b) { b.classList.toggle("on", b.dataset.t === st.mic.type); });
  mic.gain.set(st.mic.gain_db); if (!mic.gain.busy()) $("gain-val").textContent = st.mic.gain_db + " dB";
  var mms = muteState("Mic");
  mic.muteSw.classList.toggle("on", mms === "all");
  $("mic-mute-sub").textContent = mms === "all" ? "Coupé partout : personne ne t\u2019entend." : mms ? MUTED_TXT[mms] + " : les autres t\u2019entendent encore." : "Actif. Le bouton de l\u2019appareil fait pareil.";
  mic.mbSeg.querySelectorAll("button").forEach(function (b) { b.classList.toggle("on", b.dataset.m === st.mic_button.mode); });
  mic.mbTarget.querySelectorAll("button").forEach(function (b) { b.classList.toggle("on", b.dataset.t === st.mic_button.target); });
  mic.blinkSw.classList.toggle("on", !!st.mic_button.blink);
  var monOn = !!st.routing.Microphone.Headphones;
  mic.monSw.classList.toggle("on", monOn);
  $("mon-sub").textContent = monOn ? "Tu entends ta voix dans le casque, en direct." : "Coupé : tu n\u2019entends pas ta voix dans le casque.";
  $("mon-field").style.display = monOn ? "" : "none";
  mic.mon.set(st.volumes.MicMonitor); if (!mic.mon.busy()) $("mon-val").textContent = pct(st.volumes.MicMonitor) + " %";
  FX_INFO.forEach(function (x) { mic.fxSw[x[0]].classList.toggle("on", !!st.fx[x[0]]); });
  var on = FX_INFO.filter(function (x) { return st.fx[x[0]]; }).map(function (x) { return x[1].toLowerCase(); });
  // Les interrupteurs montrent déjà lesquels sont allumés : le titre donne le compte, sans pousser « Traitements de ta voix » à la ligne.
  var fxHint = $("fx-hint");
  fxHint.textContent = !on.length ? "Aucun : ta voix naturelle" : on.length === FX_INFO.length ? "Tous actifs" : on.length + (on.length > 1 ? " actifs" : " actif") + " sur " + FX_INFO.length;
  fxHint.title = on.length ? "Actifs : " + on.join(", ") : "";
  fxOffNote();
  var rec = S.meta.mic_type_preset[st.mic.type];
  Object.keys(mic.presetBtns).forEach(function (k) {
    var b = mic.presetBtns[k]; b.classList.toggle("on", fxAny() ? st.mic_preset === k : k === "brut");
    var tag = b.querySelector(".tag");
    if (k === rec && !tag) b.appendChild(h("span", {"class": "tag"}, "CONSEILLÉ"));
    if (k !== rec && tag) tag.remove();
  });
  $("preset-hint").textContent = st.mic_preset === "custom" ? "Réglage personnalisé" : "";
  S.meta.eq_bands.forEach(function (b) { var e = mic.eq[b], v = st.dsp.eq[b]; e.sl.set(v); if (!e.sl.busy()) e.lab.textContent = (v > 0 ? "+" : "") + v + " dB"; });
  mic.gateThr.set(st.dsp.gate.threshold); mic.gateAtt.set(st.dsp.gate.attenuation);
  mic.compThr.set(st.dsp.comp.threshold); mic.compRatio.set(st.dsp.comp.ratio); mic.compMake.set(st.dsp.comp.makeup);
  mic.deess.set(st.dsp.deesser); mic.bleep.set(st.bleep_db);
  $("mic-state").textContent = mms ? MUTED_TXT[mms] : MIC_TYPE_LABELS[st.mic.type];
  var HINTS = {dynamic: "<b>Micro dynamique</b> (SM7B, PodMic…) : il demande 45 à 60 dB de gain. « Régler ma voix » le trouve pour toi.",
    condenser: "<b>Micro statique</b> : il reçoit le 48 V et capte toute la pièce. Si le bruit de fond te gêne, allume seulement le noise gate, puis écoute-toi.",
    jack: "<b>Micro de casque</b> : 20 à 30 dB suffisent souvent. Le préréglage Stream le rend plus clair."};
  mic.hintTxt = HINTS[st.mic.type];
  $("preset-hint").innerHTML = st.mic_preset === "custom" ? "Réglage personnalisé" : "";
  var ph = $("preset-tip"); if (ph) ph.innerHTML = HINTS[st.mic.type];
  var cal = micCal(), cl = $("cal-line");
  var calHtml = cal ? svg("ok") + "<span>Voix mesurée le " + frDate(cal.t * 1000) + " : bruit de la pièce " + dbTxt(cal.noise_peak_db + st.mic.gain_db - cal.gain_db) + ", ta voix " + Math.round(cal.voice_db - cal.noise_peak_db) + " dB au-dessus.</span>"
    : svg("info") + "<span>Pas encore mesurée : « Régler ma voix » cale le gain sur ta voix en 20 secondes et prépare des traitements doux, sans les allumer.</span>";
  if (cl && cl._h !== calHtml) { cl._h = calHtml; cl.innerHTML = calHtml; cl.classList.toggle("ok", !!cal); }
  renderEqFreqs();
  if (micModelsReady()) { renderMicVoice(); voiceInfoLoad(); }
  if (mic.tab === "eq") drawEq();
}
var meterShown = -72.2, meterPeak = -72.2, peakT = 0, clipT = -1e9;
function meterPaint(container, db) {
  if (!container) return;
  var bars = container.children, n = bars.length;
  var lit = Math.round(clamp((db + 72.2) / 72.2, 0, 1) * n);
  var peakIdx = Math.round(clamp((meterPeak + 72.2) / 72.2, 0, 1) * n) - 1;
  var key = lit + "," + peakIdx;
  if (container._k === key) return;
  container._k = key;
  for (var i = 0; i < n; i++) {
    var f = i / n, c = f > .88 ? "#FF3B30" : f > .72 ? "#FFCC00" : "#34C759";
    bars[i].style.background = (i < lit || i === peakIdx && peakIdx > 0) ? c : "rgba(0,0,0,.07)";
  }
}
