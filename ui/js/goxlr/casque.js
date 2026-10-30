// GoXLR Mini : page Casque (égaliseur Equalizer APO).
"use strict";

/* ---------- casque (egaliseur Equalizer APO) ---------- */
var hp = {};
var HP_CH_LABELS = {Game: "Jeu", Music: "Musique", System: "Système", Chat: "Chat", Sample: "Samples"};
var HP_STYLE_ICONS = {neutre: "circle", basses: "speaker", jeu: "Game", voix: "Chat", film: "BroadcastMix", musique: "Music", doux: "leaf"};
var HP_STYLE_COLOURS = {neutre: "#8E8E93", basses: "#FF375F", jeu: "#30D158", voix: "#5E5CE6", film: "#FF9F0A", musique: "#BF5AF2", doux: "#64D2FF"};
function hpBiquad(kind, f0, g, q) {
  var fs = 48000, a = Math.pow(10, g / 40), w0 = 2 * Math.PI * Math.min(f0, fs * .49) / fs, cw = Math.cos(w0), al = Math.sin(w0) / (2 * q), s = 2 * Math.sqrt(a) * al;
  if (kind === "PK") return [1 + al * a, -2 * cw, 1 - al * a, 1 + al / a, -2 * cw, 1 - al / a];
  if (kind === "LSC") return [a * ((a + 1) - (a - 1) * cw + s), 2 * a * ((a - 1) - (a + 1) * cw), a * ((a + 1) - (a - 1) * cw - s), (a + 1) + (a - 1) * cw + s, -2 * ((a - 1) + (a + 1) * cw), (a + 1) + (a - 1) * cw - s];
  return [a * ((a + 1) + (a - 1) * cw + s), -2 * a * ((a - 1) + (a + 1) * cw), a * ((a + 1) + (a - 1) * cw - s), (a + 1) - (a - 1) * cw + s, 2 * ((a - 1) - (a + 1) * cw), (a + 1) - (a - 1) * cw - s];
}
function hpDb(c, f) {
  var w = 2 * Math.PI * f / 48000, c1 = Math.cos(w), s1 = -Math.sin(w), c2 = Math.cos(2 * w), s2 = -Math.sin(2 * w);
  var nr = c[0] + c[1] * c1 + c[2] * c2, ni = c[1] * s1 + c[2] * s2, dr = c[3] + c[4] * c1 + c[5] * c2, di = c[4] * s1 + c[5] * s2;
  return 10 * Math.log10(Math.max(1e-24, (nr * nr + ni * ni) / (dr * dr + di * di)));
}
function hpFilters(eq, part) {
  var f = [];
  if (part !== "style") (S.meta.headphones[eq.headphone] || []).forEach(function (x) { f.push(x); });
  if (part !== "corr") S.meta.hp_bands.forEach(function (b, i) { if (eq.bands[i]) f.push(["PK", b, eq.bands[i], S.meta.hp_q]); });
  return f.map(function (x) { return hpBiquad(x[0], x[1], x[2], x[3]); });
}
function hpResp(coefs, f) { var t = 0; for (var i = 0; i < coefs.length; i++) t += hpDb(coefs[i], f); return t; }
function hpPreamp(eq) {
  var c = hpFilters(eq), mx = 0;
  for (var i = 0; i < 200; i++) mx = Math.max(mx, hpResp(c, 20 * Math.pow(1000, i / 199)));
  return mx > 0 ? -(mx + .3) : 0;
}
function hpStatus() {
  var st = S.hp_eq || {endpoints: {}}, eq = S.state.hp_eq;
  var miss = eq.channels.filter(function (c) { return st.endpoints[c] === false; });
  var absent = eq.channels.filter(function (c) { return st.endpoints[c] == null; });
  return {st: st, eq: eq, needSetup: !st.apo || miss.length > 0, absent: absent.length === eq.channels.length && eq.channels.length > 0};
}
function hpSetup() {
  postModule("goxlr-mini", "hp-eq-setup", {}).then(function (r) { if (r && r.ok) { toast("Installation de l\u2019égaliseur…"); refresh(); } });
}
function buildHp() {
  var root = $("p-hp"); root.innerHTML = "";
  var grid = h("div", {"class": "mic-grid"});
  var left = h("div", {style: "display:flex;flex-direction:column;gap:16px"});
  var c1 = h("div", {"class": "card"});
  var rs = h("div", {"class": "row-switch", style: "padding-top:0"}, '<div><b>Égaliseur du casque</b><small id="hp-status"></small></div>');
  hp.sw = h("button", {"class": "switch"});
  hp.sw.addEventListener("click", function () {
    var x = hpStatus(), v = !x.eq.enabled;
    if (v && x.needSetup) { hpSetup(); return; }
    x.eq.enabled = v; renderHp(); postModule("goxlr-mini", "hp-eq", {enabled: v}).then(refresh);
  });
  rs.appendChild(hp.sw); c1.appendChild(rs);
  hp.setup = h("div", {"class": "hp-setup"}); c1.appendChild(hp.setup);
  left.appendChild(c1);
  var c2 = h("div", {"class": "card"}, "<h3>" + svg("Headphones", ' style="width:16px;height:16px;color:#0071E3"') + "Ton casque</h3>");
  hp.model = h("select", {"class": "inp", style: "width:100%"});
  hp.model.appendChild(h("option", {value: ""}, "Aucune correction"));
  Object.keys(S.meta.headphones).forEach(function (n) { hp.model.appendChild(h("option", {value: n}, esc(n))); });
  hp.model.addEventListener("change", function () { S.state.hp_eq.headphone = hp.model.value || null; renderHp(); postModule("goxlr-mini", "hp-eq", {headphone: hp.model.value || null}).then(refresh); });
  c2.appendChild(hp.model);
  c2.appendChild(h("p", {"class": "muted-txt", style: "margin-top:10px;font-size:12.5px;line-height:1.45"}, "Correction AutoEq : ton casque ramené à la courbe de référence Harman, d\u2019après les mesures d\u2019oratory1990. Les styles s\u2019ajoutent par-dessus."));
  left.appendChild(c2);
  var c3 = h("div", {"class": "card"}, "<h3>Options</h3>");
  var f = h("div", {"class": "field", style: "margin-top:0"}, '<div class="field-top">Appliquer à</div>');
  hp.ch = h("div", {"class": "chips"});
  S.meta.hp_channels.forEach(function (c) {
    var b = h("button", {"data-c": c}, HP_CH_LABELS[c] || c);
    b.addEventListener("click", function () {
      var eq = S.state.hp_eq, i = eq.channels.indexOf(c);
      if (i >= 0) eq.channels.splice(i, 1); else eq.channels.push(c);
      eq.channels = S.meta.hp_channels.filter(function (x) { return eq.channels.indexOf(x) >= 0; });
      renderHp(); postModule("goxlr-mini", "hp-eq", {channels: eq.channels}).then(refresh);
    });
    hp.ch.appendChild(b);
  });
  f.appendChild(hp.ch); c3.appendChild(f);
  var lv = h("div", {"class": "row-switch", style: "margin-top:8px"}, "<div><b>Couper pendant le live</b><small>Ton stream reçoit aussi le son égalisé : quand OBS est ouvert, l\u2019égaliseur se coupe tout seul.</small></div>");
  hp.live = h("button", {"class": "switch"});
  hp.live.addEventListener("click", function () { var v = !S.state.hp_eq.pause_live; S.state.hp_eq.pause_live = v; renderHp(); postModule("goxlr-mini", "hp-eq", {pause_live: v}); });
  lv.appendChild(hp.live); c3.appendChild(lv);
  hp.pre = h("p", {"class": "muted-txt", style: "margin-top:6px;font-size:12.5px;line-height:1.45"}); c3.appendChild(hp.pre);
  hp.remove = h("button", {"class": "pill", style: "margin-top:12px;height:30px;font-size:12.5px", onclick: function () {
    sheet({icon: "Headphones", title: "Retirer l\u2019égaliseur ?", text: "Les voies GoXLR retrouvent leurs effets Windows d\u2019origine. Le son coupe 2 secondes. Tes profils sont gardés.",
      actions: [["Annuler", null], ["Retirer", function () { postModule("goxlr-mini", "hp-eq-remove", {}).then(refresh); }, "danger"]]});
  }}, "Retirer l\u2019égaliseur de Windows");
  c3.appendChild(hp.remove);
  left.appendChild(c3);
  grid.appendChild(left);

  var right = h("div", {style: "display:flex;flex-direction:column;gap:16px;min-width:0"});
  var c4 = h("div", {"class": "card"}, '<h3>Style<small id="hp-style-hint"></small></h3>');
  hp.styles = h("div", {"class": "presets"}); c4.appendChild(hp.styles);
  right.appendChild(c4);
  var c5 = h("div", {"class": "card"}, '<h3>Réglage fin<small id="hp-fine-hint"></small></h3>');
  var eqb = h("div", {"class": "eq hp10"});
  hp.cv = h("canvas"); eqb.appendChild(hp.cv);
  var bands = h("div", {"class": "eq-bands"});
  hp.bands = [];
  var send = throttle(function () { postModule("goxlr-mini", "hp-eq", {bands: S.state.hp_eq.bands}); }, 90);
  S.meta.hp_bands.forEach(function (b, i) {
    var wrap = h("div", {"class": "eq-band"}), lab = h("b");
    var sl = Slider({label: "Égaliseur casque " + b + " Hz", format: function (v) { return v + " dB"; }, vertical: true, min: -12, max: 12, step: 1, onInput: function (v) {
      var eq = S.state.hp_eq; eq.bands[i] = v; eq.style = null; lab.textContent = (v > 0 ? "+" : "") + v; drawHp(); renderHpStyles(); hpPreText(); send();
    }, onCommit: function () { postModule("goxlr-mini", "hp-eq", {bands: S.state.hp_eq.bands}).then(refresh); }});
    wrap.appendChild(lab); wrap.appendChild(sl.el);
    wrap.appendChild(h("small", {}, b >= 1000 ? (b / 1000) + "k" : String(b)));
    bands.appendChild(wrap); hp.bands.push({sl: sl, lab: lab});
  });
  eqb.appendChild(bands); c5.appendChild(eqb);
  var row = h("div", {"class": "btn-row"});
  row.appendChild(h("button", {"class": "pill", onclick: function () { postModule("goxlr-mini", "hp-eq", {style: "neutre"}).then(refresh); }}, svg("reset") + "Remettre à plat"));
  row.appendChild(h("button", {"class": "pill primary", onclick: hpSaveSheet}, svg("save") + "Enregistrer comme profil"));
  c5.appendChild(row);
  right.appendChild(c5);
  grid.appendChild(right);
  root.appendChild(grid);
  built.hp = true;
  hp.stylesKey = "";
}
function hpSaveSheet() {
  var body = h("div");
  body.innerHTML = '<div class="diag-head">' + chipIco("save", "#0071E3") + "<div><h2>Enregistrer ce réglage</h2><p class=\"muted-txt\">Il rejoint tes styles, en un clic.</p></div></div>";
  var inp = h("input", {"class": "inp", placeholder: "Nom du profil (ex. FPS, Soirée film)", maxlength: "40", style: "width:100%;margin-top:16px"});
  body.appendChild(inp);
  var saving = false;
  function save() {
    var n = inp.value.trim(); if (!n) { inp.focus(); return; } if (saving) return;
    saving = true;
    postModule("goxlr-mini", "hp-eq-save", {name: n}).then(function (r) {
      if (!r) return;
      if (sheetState && sheetState.body === body) closeSheet();
      toast("Profil « " + n + " » enregistré"); return refresh();
    }).finally(function () { saving = false; });
  }
  inp.addEventListener("keydown", function (e) { if (e.key === "Enter") save(); });
  sheet({wide: true, body: body, actions: [["Annuler", null], ["Enregistrer", save, "primary", true]]});
  setTimeout(function () { inp.focus(); }, 50);
}
function renderHpStyles() {
  var eq = S.state.hp_eq, key = JSON.stringify([Object.keys(eq.custom), eq.style]);
  if (hp.stylesKey === key) return;
  hp.stylesKey = key; hp.styles.innerHTML = "";
  function card(id, label, desc, icon, colour, removable) {
    var b = h("button", {"class": "preset" + (eq.style === id ? " on" : "")}, chipIco(icon, colour) + "<b>" + esc(label) + "</b><p>" + esc(desc) + "</p>");
    b.addEventListener("click", function () { postModule("goxlr-mini", "hp-eq", {style: id}).then(refresh); });
    if (removable) {
      var x = h("span", {"class": "preset-x", title: "Supprimer ce profil"}, svg("trash"));
      x.addEventListener("click", function (e) { e.stopPropagation(); postModule("goxlr-mini", "hp-eq-delete", {name: label}).then(function (r) { if (r) { toast("Profil « " + label + " » supprimé"); refresh(); } }); });
      b.appendChild(x);
    }
    hp.styles.appendChild(b);
  }
  Object.keys(S.meta.hp_styles).forEach(function (k) { var s = S.meta.hp_styles[k]; card(k, s.label, s.desc, HP_STYLE_ICONS[k] || "circle", HP_STYLE_COLOURS[k] || "#8E8E93"); });
  Object.keys(eq.custom).forEach(function (n) { card("custom:" + n, n, "Ton profil.", "save", "#0071E3", true); });
}
function hpPreText() {
  var pre = hpPreamp(S.state.hp_eq), boost = pre < 0 ? -pre - .3 : 0, key = Math.round(boost * 10);
  if (hp.preKey === key) return;
  hp.preKey = key; hp.pre.innerHTML = "";
  if (boost > HP_SAFE_DB + 2) {
    hp.pre.appendChild(h("span", {"class": "hp-err"}, "Basses poussées de +" + Math.round(boost) + " dB : le reste du son est baissé d\u2019autant et, volume au maximum, le casque grésille sur les sons forts. "));
    hp.pre.appendChild(h("button", {"class": "pill", style: "margin-top:8px;height:30px", onclick: function () { hpSoften(); }}, "Adoucir à +" + HP_SAFE_DB + " dB"));
  } else hp.pre.textContent = pre < 0 ? "Volume baissé de " + (-pre).toFixed(1).replace(".", ",") + " dB avant l\u2019égaliseur : jamais de saturation." : "Aucune baisse de volume nécessaire.";
}
function drawHp() {
  var cv = hp.cv; if (!cv || !cv.offsetWidth) return;
  var w = cv.offsetWidth, ht = cv.offsetHeight, dpr = devicePixelRatio || 1;
  cv.width = w * dpr; cv.height = ht * dpr;
  var c = cv.getContext("2d"); c.scale(dpr, dpr);
  var top = 22, bot = ht - 30;
  var cr = cv.getBoundingClientRect(), tr = hp.bands[0].sl.track.getBoundingClientRect();
  if (tr.height) { top = tr.top - cr.top; bot = tr.bottom - cr.top; }
  var mid = (top + bot) / 2, span = 12;
  function y(db) { return mid - clamp(db, -15, 15) / span * (bot - top) / 2; }
  c.strokeStyle = "rgba(0,0,0,.07)"; c.lineWidth = 1;
  [-12, -6, 0, 6, 12].forEach(function (g) { c.beginPath(); c.moveTo(0, y(g)); c.lineTo(w, y(g)); c.stroke(); });
  var f0 = 31 / Math.SQRT2, f1 = 16000 * Math.SQRT2;
  function fAt(x) { return f0 * Math.pow(f1 / f0, x / w); }
  var eq = S.state.hp_eq, corr = hpFilters(eq, "corr"), all = hpFilters(eq);
  if (corr.length) {
    c.beginPath(); c.setLineDash([4, 4]);
    for (var x = 0; x <= w; x += 3) { var yy = y(hpResp(corr, fAt(x))); if (x === 0) c.moveTo(x, yy); else c.lineTo(x, yy); }
    c.strokeStyle = "rgba(60,60,67,.35)"; c.lineWidth = 1.5; c.stroke(); c.setLineDash([]);
  }
  var grad = c.createLinearGradient(0, 0, w, 0);
  grad.addColorStop(0, "#FF375F"); grad.addColorStop(.5, "#5E5CE6"); grad.addColorStop(1, "#32ADE6");
  c.beginPath();
  for (var x2 = 0; x2 <= w; x2 += 3) { var y2 = y(hpResp(all, fAt(x2))); if (x2 === 0) c.moveTo(x2, y2); else c.lineTo(x2, y2); }
  c.lineWidth = 2.5; c.strokeStyle = grad; c.shadowColor = "rgba(94,92,230,.3)"; c.shadowBlur = 8; c.stroke();
  c.shadowBlur = 0; c.lineTo(w, mid); c.lineTo(0, mid); c.closePath();
  var fg = c.createLinearGradient(0, top, 0, bot); fg.addColorStop(0, "rgba(94,92,230,.14)"); fg.addColorStop(.5, "rgba(94,92,230,.02)"); fg.addColorStop(1, "rgba(94,92,230,.14)");
  c.fillStyle = fg; c.fill();
}
function renderHp() {
  if (!built.hp) buildHp();
  var x = hpStatus(), st = x.st, eq = x.eq;
  hp.sw.classList.toggle("on", eq.enabled && !x.needSetup);
  var msg, setupHtml = "";
  var setup = st.setup;
  if (setup && setup.running) { msg = setup.step; setupHtml = '<div class="hp-note"><span class="spin"></span><span>' + esc(setup.step || "") + "</span></div>"; }
  else if (x.needSetup) {
    msg = eq.enabled ? "À finir d\u2019installer." : "Coupé.";
    setupHtml = '<div class="hp-note info">' + svg("info") + "<span>" + (st.apo ? "Equalizer APO est là : il reste à le brancher sur tes voies GoXLR. Le son de Windows coupe 2 secondes."
      : "Un clic installe Equalizer APO, l\u2019égaliseur libre de référence sous Windows, et le branche sur tes voies GoXLR. Le son coupe 2 secondes.") +
      (setup && setup.error ? '<br><b class="hp-err">' + esc(setup.error) + "</b>" : "") + "</span></div>";
  } else if (!eq.enabled) msg = "Coupé.";
  else if (x.absent) msg = "En attente : GoXLR débranchée.";
  else if (st.live && eq.pause_live) msg = "En pause : un logiciel de live est ouvert, ton stream reçoit le son sans égaliseur.";
  else if (!eq.channels.length) msg = "Choisis au moins une voie dans Options.";
  else msg = "Actif sur " + eq.channels.map(function (c) { return HP_CH_LABELS[c]; }).join(", ").toLowerCase() + ".";
  if (st.sim) msg += " (simulé)";
  $("hp-status").textContent = msg;
  if (hp.setup._h !== setupHtml) {
    hp.setup._h = setupHtml; hp.setup.innerHTML = setupHtml;
    if (x.needSetup && !(setup && setup.running)) hp.setup.appendChild(h("button", {"class": "pill primary", style: "margin-top:12px;width:100%;height:38px", onclick: hpSetup}, svg("download") + (st.apo ? "Brancher l\u2019égaliseur" : "Installer et activer")));
  }
  if (hp.model.value !== (eq.headphone || "")) hp.model.value = eq.headphone || "";
  hp.ch.querySelectorAll("button").forEach(function (b) {
    var c = b.dataset.c, on = eq.channels.indexOf(c) >= 0;
    b.classList.toggle("on", on);
    b.title = st.endpoints[c] == null ? "Voie absente de Windows (GoXLR débranchée)" : "";
  });
  hp.live.classList.toggle("on", !!eq.pause_live);
  hp.remove.style.display = Object.keys(st.endpoints || {}).some(function (c) { return st.endpoints[c]; }) ? "" : "none";
  renderHpStyles();
  S.meta.hp_bands.forEach(function (b, i) { var e = hp.bands[i], v = eq.bands[i]; e.sl.set(v); if (!e.sl.busy()) e.lab.textContent = (v > 0 ? "+" : "") + v; });
  var styleName = eq.style && S.meta.hp_styles[eq.style] ? S.meta.hp_styles[eq.style].label : eq.style && eq.style.indexOf("custom:") === 0 ? eq.style.slice(7) : "Réglage perso";
  $("hp-fine-hint").textContent = styleName + (eq.headphone ? " · correction " + eq.headphone : "");
  $("hp-style-hint").textContent = eq.style ? "" : "Réglage perso";
  hpPreText();
  drawHp();
}
