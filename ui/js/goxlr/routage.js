// GoXLR Mini : page Routage.
"use strict";

/* ---------- routage ---------- */
var rt = {};
var SHORTCUTS = [
  {t: "VOD sans musique", d: "Ta VOD reçoit le stream, sans la musique (anti-DMCA).", get: function (s) { return s.vod.mode === "no-music"; },
   apply: function (on) { S.state.vod.mode = on ? "no-music" : "off"; return postModule("goxlr-mini", "settings", {vod: {mode: S.state.vod.mode}}).then(refresh); }},
  {t: "Musique hors du stream", d: "Anti-DMCA : la musique reste dans ton casque.", get: function (s) { return !s.routing.Music.BroadcastMix; }, set: function (on) { return [["Music", "BroadcastMix", !on]]; }},
  {t: "Discord entend ta musique", d: "Partage ce que tu écoutes avec tes amis.", get: function (s) { return s.routing.Music.ChatMic; }, set: function (on) { return [["Music", "ChatMic", on]]; }},
  {t: "Notifications hors du stream", d: "Les sons Windows ne passent pas en live.", get: function (s) { return !s.routing.System.BroadcastMix; }, set: function (on) { return [["System", "BroadcastMix", !on]]; }},
  {t: "Le stream entend Discord", d: "Tes viewers entendent tes amis.", get: function (s) { return s.routing.Chat.BroadcastMix; }, set: function (on) { return [["Chat", "BroadcastMix", on]]; }},
  {t: "T'entendre dans le casque", d: "Retour de ta voix, réglable dans Micro.", get: function (s) { return s.routing.Microphone.Headphones; }, set: function (on) { return [["Microphone", "Headphones", on]]; }}
];
function buildRouting() {
  var root = $("p-routing"); root.innerHTML = "";
  var grid = h("div", {"class": "route-grid"});
  var card = h("div", {"class": "card", style: "padding:20px 22px"});
  var t = h("table", {"class": "mx"});
  var head = "<tr><th></th>";
  S.meta.outputs.forEach(function (o) { head += '<th title="' + OUT_HINT[o] + '">' + chipIco(o, "#8E8E93") + OUT_LABELS[o] + "</th>"; });
  t.innerHTML = head + "</tr>";
  rt.nodes = {};
  S.meta.inputs.forEach(function (i) {
    var ch = IN_MAP[i], tr = h("tr");
    tr.appendChild(h("td", {}, '<span class="rname">' + chipIco(ch, col(ch)) + LABELS[ch] + "</span>"));
    S.meta.outputs.forEach(function (o) {
      var td = h("td"), n = h("button", {"class": "node", style: "--c:" + col(ch), title: LABELS[ch] + " → " + OUT_LABELS[o]}, svg("check", ' stroke-width="3"'));
      n.addEventListener("click", function () {
        if (vodAuto(o)) { toast("La VOD suit le stream sans la musique. Désactive « VOD sans musique » pour la régler à la main."); return; }
        var v = !S.state.routing[i][o]; S.state.routing[i][o] = v; renderRouting(); postModule("goxlr-mini", "route", {input: i, output: o, enabled: v});
      });
      td.appendChild(n); tr.appendChild(td); rt.nodes[i + "|" + o] = n;
    });
    t.appendChild(tr);
  });
  card.appendChild(t);
  var lg = h("div", {"class": "legend"});
  S.meta.outputs.forEach(function (o) { lg.appendChild(h("span", {}, "<b>" + OUT_LABELS[o] + "</b>" + OUT_HINT[o].charAt(0).toLowerCase() + OUT_HINT[o].slice(1))); });
  card.appendChild(lg);
  grid.appendChild(card);
  var side = h("div", {"class": "card"}, "<h3>Raccourcis</h3>");
  rt.sc = [];
  SHORTCUTS.forEach(function (sc) {
    var row = h("div", {"class": "row-switch"}, "<div><b>" + sc.t + "</b><small>" + sc.d + "</small></div>");
    var sw = h("button", {"class": "switch"});
    sw.addEventListener("click", function () {
      var on = !sc.get(S.state);
      if (sc.apply) { sc.apply(on); renderRouting(); return; }
      sc.set(on).forEach(function (c) { S.state.routing[c[0]][c[1]] = c[2]; postModule("goxlr-mini", "route", {input: c[0], output: c[1], enabled: c[2]}); });
      renderRouting();
    });
    row.appendChild(sw); side.appendChild(row); rt.sc.push({sc: sc, sw: sw});
  });
  grid.appendChild(side);
  root.appendChild(grid);
  built.routing = true;
}
function renderRouting() {
  if (!built.routing) buildRouting();
  S.meta.inputs.forEach(function (i) {
    S.meta.outputs.forEach(function (o) {
      var n = rt.nodes[i + "|" + o], auto = vodAuto(o);
      var on = auto ? (i !== "Music" && !!S.state.routing[i].BroadcastMix) : !!S.state.routing[i][o];
      n.classList.toggle("on", on);
      n.classList.toggle("auto", auto);
    });
  });
  rt.sc.forEach(function (x) { x.sw.classList.toggle("on", !!x.sc.get(S.state)); });
}
function vodAuto(o) { return S.state.vod.mode === "no-music" && o === S.meta.vod_output; }
