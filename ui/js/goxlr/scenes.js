// GoXLR Mini : scènes et conseils.
"use strict";

/* ---------- scenes et conseils ---------- */
var sc = {};
function buildScenes() {
  var root = $("p-scenes"); root.innerHTML = "";
  var grid = h("div", {"class": "scene-grid"});
  var left = h("div", {style: "min-width:0"});
  sc.grid = h("div", {"class": "scenes"});
  left.appendChild(sc.grid);
  var mine = h("div", {"class": "card mine"}, "<h3>Mes scènes</h3>");
  sc.mine = h("div", {"class": "mine-list"}); mine.appendChild(sc.mine);
  var save = h("div", {"class": "save-row"});
  sc.name = h("input", {placeholder: "Nom de la configuration actuelle", maxlength: "40"});
  var sb = h("button", {"class": "pill primary"}, svg("save") + "Enregistrer");
  sb.addEventListener("click", function () {
    var n = sc.name.value.trim(); if (!n) { sc.name.focus(); return; }
    sb.disabled = true;
    postModule("goxlr-mini", "scene-save", {name: n}).then(function (r) { if (r) { if (sc.name.value.trim() === n) sc.name.value = ""; toast("Scène « " + n + " » enregistrée"); return refresh(); } })
      .finally(function () { sb.disabled = false; });
  });
  sc.name.addEventListener("keydown", function (e) { if (e.key === "Enter") sb.click(); });
  save.appendChild(sc.name); save.appendChild(sb); mine.appendChild(save);
  left.appendChild(mine);
  grid.appendChild(left);
  var right = h("div", {style: "display:flex;flex-direction:column;gap:14px;min-height:0;height:100%"});
  right.appendChild(buildAuto());
  var adv = h("div", {"class": "card", style: "overflow:auto;flex:1;min-height:0"}, "<h3>" + svg("bulb", ' style="width:16px;height:16px;color:#FF9500"') + "Conseils</h3>");
  sc.adv = h("div"); adv.appendChild(sc.adv);
  right.appendChild(adv);
  grid.appendChild(right);
  root.appendChild(grid);
  built.scenes = true;
}
function sceneLabel(k) { return S.meta.scenes[k] ? S.meta.scenes[k].label : k; }
function sceneNames() { return Object.keys(S.meta.scenes).concat(Object.keys(S.state.user_scenes)); }
function appLabel(a) { var k = String(a).toLowerCase(); return S.meta.known_apps[k] || S.meta.known_apps[k + ".exe"] || a; }
function buildAuto() {
  var card = h("div", {"class": "card", style: "flex:none"});
  var top = h("div", {"class": "row-switch", style: "padding:0 0 6px"}, '<div><b>Scènes automatiques</b><small>Change de scène quand une appli s\u2019ouvre.</small></div>');
  sc.autoSw = h("button", {"class": "switch"});
  sc.autoSw.addEventListener("click", function () { saveAuto({enabled: !S.state.auto_scenes.enabled}); });
  top.appendChild(sc.autoSw); card.appendChild(top);
  sc.autoList = h("div", {"class": "auto-list"}); card.appendChild(sc.autoList);
  var add = h("div", {"class": "auto-add"});
  sc.autoApp = h("input", {"class": "inp", placeholder: "ex. obs64.exe", list: "apps-dl", maxlength: "64"});
  var dl = h("datalist", {id: "apps-dl"}); sc.appsDl = dl;
  sc.autoScene = h("select", {"class": "inp"});
  var plus = h("button", {"class": "pill primary", style: "padding:0;justify-content:center;height:34px;border-radius:10px", title: "Ajouter"}, svg("plus"));
  plus.addEventListener("click", function () {
    var app = sc.autoApp.value.trim(); if (!app) { sc.autoApp.focus(); return; }
    var rules = S.state.auto_scenes.rules.concat([{app: app, scene: sc.autoScene.value}]);
    sc.autoApp.value = "";
    saveAuto({rules: rules, enabled: true});
  });
  sc.autoApp.addEventListener("focus", loadApps);
  sc.autoApp.addEventListener("keydown", function (e) { if (e.key === "Enter") plus.click(); });
  add.appendChild(sc.autoApp); add.appendChild(sc.autoScene); add.appendChild(plus); card.appendChild(add); card.appendChild(dl);
  var fb = h("div", {style: "display:flex;align-items:center;gap:8px;margin-top:8px"}, '<span class="muted-txt" style="font-size:12px;white-space:nowrap">Quand elle se ferme</span>');
  sc.autoFb = h("select", {"class": "inp", style: "flex:1"});
  sc.autoFb.addEventListener("change", function () { saveAuto({fallback: sc.autoFb.value || null}); });
  fb.appendChild(sc.autoFb); card.appendChild(fb);
  sc.autoNote = h("div", {"class": "auto-note"}); card.appendChild(sc.autoNote);
  loadApps();
  return card;
}
function loadApps() {
  api("/api/processes").then(function (p) {
    sc.appsDl.innerHTML = "";
    p.known.forEach(function (a) { sc.appsDl.appendChild(h("option", {value: a.app}, esc(a.label))); });
    p.others.forEach(function (a) { sc.appsDl.appendChild(h("option", {value: a})); });
  }).catch(function () {});
}
function saveAuto(patch) {
  var a = S.state.auto_scenes;
  var body = {enabled: "enabled" in patch ? patch.enabled : a.enabled, rules: patch.rules || a.rules, fallback: "fallback" in patch ? patch.fallback : a.fallback};
  S.state.auto_scenes = body; sc.autoKey = null; renderAuto();
  return postModule("goxlr-mini", "auto-scenes", body).then(refresh);
}
function renderAuto() {
  var a = S.state.auto_scenes, names = sceneNames();
  var key = JSON.stringify([a, names, S.auto && S.auto.note]);
  if (sc.autoKey === key) return;
  sc.autoKey = key;
  sc.autoSw.classList.toggle("on", !!a.enabled);
  sc.autoList.innerHTML = "";
  if (!a.rules.length) sc.autoList.appendChild(h("p", {"class": "muted-txt", style: "font-size:12px;padding:2px"}, "Exemple : OBS ouvert → Streaming, Spotify → Écoute."));
  a.rules.forEach(function (r, idx) {
    var it = h("div", {"class": "auto-item"});
    it.appendChild(h("span", {title: r.app}, esc(appLabel(r.app)) + "<em>→</em>" + esc(sceneLabel(r.scene))));
    var del = h("button", {"class": "ibtn", title: "Retirer"}, svg("trash"));
    del.addEventListener("click", function () { saveAuto({rules: a.rules.filter(function (_, j) { return j !== idx; })}); });
    it.appendChild(del); sc.autoList.appendChild(it);
  });
  var cur = sc.autoScene.value;
  sc.autoScene.innerHTML = names.map(function (n) { return '<option value="' + esc(n) + '">' + esc(sceneLabel(n)) + "</option>"; }).join("");
  if (cur && names.indexOf(cur) >= 0) sc.autoScene.value = cur;
  sc.autoFb.innerHTML = '<option value="">Ne rien changer</option>' + names.map(function (n) { return '<option value="' + esc(n) + '">' + esc(sceneLabel(n)) + "</option>"; }).join("");
  sc.autoFb.value = a.fallback || "";
  var note = S.auto && S.auto.note;
  sc.autoNote.textContent = !a.enabled ? "Désactivées." : note ? (note.applied ? "Dernier changement : " + sceneLabel(note.scene) + " (" + appLabel(note.why.replace(/ (ouvert|déjà ouvert)$/, "")) + (/ouvert$/.test(note.why) ? " ouvert" : "") + ") à " + new Date(note.t * 1000).toLocaleTimeString("fr-FR", {hour: "2-digit", minute: "2-digit"}) + "." : "Au démarrage, " + appLabel(note.why.replace(/ déjà ouvert$/, "")) + " était déjà ouvert : scène gardée telle quelle.") : "J\u2019observe les applis toutes les 3 secondes.";
}
function renderScenes() {
  if (!built.scenes) buildScenes();
  var st = S.state, key = JSON.stringify([st.scene, Object.keys(st.user_scenes)]);
  if (sc.key !== key) {
    sc.grid.innerHTML = "";
    Object.keys(S.meta.scenes).forEach(function (k) {
      var d = S.meta.scenes[k], b = h("button", {"class": "scene" + (st.scene === k ? " on" : "")});
      var lane = S.meta.faders.map(function (f) { var ch = d.faders[f]; return '<span style="--c:' + col(ch) + '" title="' + LABELS[ch] + '">' + svg(ch) + "</span>"; }).join("");
      b.innerHTML = chipIco(SCENE_ICONS[k], SCENE_COLOURS[k]) + "<b>" + frLabel(d.label) + "</b><p>" + frLabel(d.desc) + '</p><div class="lane">' + lane + "</div>" + (st.scene === k ? '<span class="tag">ACTIVE</span>' : "");
      b.addEventListener("click", function () { withUndo("Scène " + frLabel(d.label) + " appliquée", function () { return postModule("goxlr-mini", "scene", {name: k}).then(refresh); }); });
      sc.grid.appendChild(b);
    });
    sc.mine.innerHTML = "";
    var names = Object.keys(st.user_scenes);
    if (!names.length) sc.mine.appendChild(h("p", {"class": "muted-txt", style: "padding:4px 2px"}, "Règle tout comme tu aimes, puis enregistre-le ici pour le retrouver en un clic."));
    names.forEach(function (n) {
      var it = h("div", {"class": "mine-item"});
      it.appendChild(h("span", {"class": "chip-ico", style: "--c:#0A84FF;width:28px;height:28px;border-radius:8px"}, svg("scenes")));
      var b = h("b"); b.textContent = n; it.appendChild(b);
      if (st.scene === n) it.appendChild(h("span", {"class": "muted-txt", style: "font-size:11px"}, "active"));
      it.appendChild(h("button", {"class": "pill", style: "height:28px;padding:0 12px", onclick: function () { withUndo("Scène « " + n + " » appliquée", function () { return postModule("goxlr-mini", "scene", {name: n}).then(refresh); }); }}, "Appliquer"));
      it.appendChild(h("button", {"class": "ibtn", title: "Supprimer", onclick: function () { postModule("goxlr-mini", "scene-delete", {name: n}).then(refresh); }}, svg("trash")));
      sc.mine.appendChild(it);
    });
    sc.key = key;
  }
  renderAuto();
  renderAdvice();
}
function advices() {
  var s = S.state, a = [];
  if (!S.connected) return [{lvl: "warn", t: "GoXLR déconnectée", d: (L && USB_TXT[L.usb]) || "Vérifie le câble USB. L\u2019appli se reconnecte toute seule."}];
  var voice = function () { voiceIntro(); return Promise.resolve(); };
  if (s.mutes.Mic) a.push({lvl: "warn", t: "Ton micro est coupé", d: "Personne ne t\u2019entend en ce moment.", label: "Réactiver", fn: function () { return postModule("goxlr-mini", "mute", {channel: "Mic", muted: false}); }});
  else if (s.partial.Mic) a.push({lvl: "warn", t: "Micro " + MUTED_TXT[s.partial.Mic].toLowerCase(), d: "Une partie de ton public ne t\u2019entend pas.", label: "Réactiver", fn: function () { return postModule("goxlr-mini", "mute", {channel: "Mic", muted: false}); }});
  if (L && L.live.mic_db <= -72 && !s.mutes.Mic) a.push({lvl: "warn", t: "Aucun son du micro", d: s.mic.type === "condenser" ? "Vérifie le câble XLR, puis lance « Régler ma voix »." : "Un micro statique ne marche qu\u2019avec le 48 V : « Régler ma voix » le détecte et l\u2019allume.", label: "Régler ma voix", fn: voice});
  else if (!micCal()) a.push({t: "Ta voix n\u2019est pas encore réglée", d: "20 secondes pour caler le gain sur ta voix. Aucun traitement n\u2019est allumé sans ton accord.", label: "Régler ma voix", fn: voice});
  if (s.routing.Chat.ChatMic) a.push({lvl: "warn", t: "Écho dans Discord", d: "Le chat est renvoyé vers le chat : tes amis s\u2019entendent eux-mêmes.", label: "Corriger", fn: function () { return postModule("goxlr-mini", "route", {input: "Chat", output: "ChatMic", enabled: false}); }});
  winAdvice(a);
  if (s.routing.System.BroadcastMix) a.push({t: "Tes notifications passent en live", d: "Les sons Windows sont envoyés au stream.", label: "Les retirer", fn: function () { return postModule("goxlr-mini", "route", {input: "System", output: "BroadcastMix", enabled: false}); }});
  if (s.mic.type === "dynamic" && s.mic.gain_db < 40 && !micCal()) a.push({t: "Gain bas pour un micro dynamique", d: "Il est à " + s.mic.gain_db + " dB ; un micro dynamique demande souvent 45 à 60 dB.", label: "Régler ma voix", fn: voice});
  var onF = S.meta.faders.map(function (f) { return s.faders[f]; });
  if (onF.indexOf("Mic") < 0) a.push({t: "Micro hors des faders", d: "Tu le coupes avec le bouton de toux ; son volume se règle dans Mixeur."});
  if (s.volumes.Headphones < 64) a.push({t: "Casque très bas", d: "Il est à " + pct(s.volumes.Headphones) + " %.", label: "Remonter à 80 %", fn: function () { return postModule("goxlr-mini", "volume", {channel: "Headphones", value: 204}); }});
  if (!a.length) a.push({lvl: "ok", t: "Tout est bien réglé", d: "Rien à signaler sur ta configuration."});
  return a;
}
function renderAdvice() {
  if (!S || activeModule !== "goxlr-mini") return;
  var a = advices(), key = JSON.stringify(a.map(function (x) { return x.t + x.d; }));
  var warn = a.filter(function (x) { return x.lvl !== "ok"; }).length;
  var badge = $("nav-badge"); if (badge) { badge.textContent = warn; badge.style.display = warn ? "" : "none"; }
  if (!built.scenes || sc.advKey === key) return;
  sc.adv.innerHTML = "";
  a.forEach(function (x) {
    var it = h("div", {"class": "advice"});
    it.innerHTML = chipIco(x.lvl === "ok" ? "ok" : x.lvl === "warn" ? "warn" : "bulb", x.lvl === "ok" ? "#30D158" : x.lvl === "warn" ? "#FF9F0A" : "#64D2FF");
    var body = h("div", {}, "<b>" + x.t + "</b><p>" + x.d + "</p>");
    if (x.label) body.appendChild(h("button", {"class": "pill", onclick: function () { x.fn().then(refresh); }}, x.label));
    it.appendChild(body); sc.adv.appendChild(it);
  });
  sc.advKey = key;
}
