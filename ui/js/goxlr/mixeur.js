// GoXLR Mini : page Mixeur.
"use strict";

/* ---------- mixeur ---------- */
var strips = {}, others = {};
var sendVolume = {};
function volumeSender(ch) {
  if (!sendVolume[ch]) sendVolume[ch] = throttle(function (v) { postModule("goxlr-mini", "volume", {channel: ch, value: v}); }, 45);
  return sendVolume[ch];
}
function buildMixer() {
  var root = $("p-mixer"); root.innerHTML = "";
  var grid = h("div", {"class": "strips"});
  S.meta.faders.forEach(function (f, i) {
    var st = h("div", {"class": "strip"});
    st.innerHTML = '<span class="fnum">FADER ' + (i + 1) + '</span>';
    var head = h("button", {"class": "strip-head", title: "Changer la voie de ce fader"});
    var sl = Slider({label: "Volume du fader " + (i + 1), format: function (v) { return pct(v) + " %"; }, vertical: true, min: 0, max: 255, step: 1, wheel: 5, settleMs: 120, smoothRemote: true,
      onInput: function (v) { var ch = S.state.faders[f]; S.state.volumes[ch] = v; volumeSender(ch)(v); paintStripVal(f); },
      onCommit: function (v) { postModule("goxlr-mini", "volume", {channel: S.state.faders[f], value: v}); }});
    var ghost = h("div", {"class": "ghost"}); sl.track.appendChild(ghost);
    var note = h("div", {"class": "phys-note"});
    var val = h("div", {"class": "val"});
    var mute = h("button", {"class": "mute-btn"});
    var cap = h("div", {"class": "mute-cap"});
    cap.addEventListener("click", function () { toggleMute(S.state.faders[f], S.state.mute_modes[f]); });
    mute.title = "Clic droit : couper partout (comme un appui long sur le bouton de l’appareil)";
    mute.addEventListener("click", function () { toggleMute(S.state.faders[f], S.state.mute_modes[f]); });
    mute.addEventListener("contextmenu", function (e) { e.preventDefault(); var ch = S.state.faders[f]; if (muteState(ch) !== "all") setMute(ch, true, "all"); });
    head.addEventListener("click", function () { openPop(head, channelPicker(f)); });
    st.appendChild(head); st.appendChild(sl.el); st.appendChild(note); st.appendChild(val); st.appendChild(mute); st.appendChild(cap);
    grid.appendChild(st);
    strips[f] = {el: st, head: head, sl: sl, ghost: ghost, note: note, val: val, mute: mute, cap: cap};
  });
  root.appendChild(h("p", {"class":"workspace-note",style:"margin:0 0 14px;color:var(--text2)"}, "Les curseurs règlent le volume. La Mini n’est pas motorisée : ses poignées restent en place. Bouger un fader physique reprend la main."));
  root.appendChild(grid);
  var card = h("div", {"class": "card others"});
  root.appendChild(card);
  others.card = card;
  built.mixer = true;
}
function channelPicker(f) {
  var box = h("div");
  box.appendChild(h("div", {"class": "ptitle"}, "Voie du fader " + (S.meta.faders.indexOf(f) + 1)));
  var g = h("div", {"class": "grid"});
  S.meta.channels.forEach(function (ch) {
    var where = S.meta.faders.filter(function (k) { return S.state.faders[k] === ch; })[0];
    var t = h("button", {"class": "tile" + (S.state.faders[f] === ch ? " cur" : "")},
      chipIco(ch, col(ch)) + LABELS[ch] + (where && where !== f ? "<em>F" + (S.meta.faders.indexOf(where) + 1) + "</em>" : ""));
    t.addEventListener("click", function () {
      closePop();
      if (S.state.faders[f] === ch) return;
      var old = S.state.faders[f];
      if (where) S.state.faders[where] = old;
      S.state.faders[f] = ch; render();
      postModule("goxlr-mini", "fader", {fader: f, channel: ch}).then(refresh);
    });
    g.appendChild(t);
  });
  box.appendChild(g);
  var i = S.meta.faders.indexOf(f);
  box.appendChild(h("div", {"class": "ptitle", style: "margin-top:6px"}, "Bouton mute de ce fader"));
  var mc = h("div", {"class": "mode-chips"});
  S.meta.mute_targets.forEach(function (t) {
    var b = h("button", {"class": S.state.mute_modes[f] === t ? "on" : ""}, MUTE_LABELS[t]);
    b.addEventListener("click", function () {
      var m = {}; m[f] = t; S.state.mute_modes[f] = t; closePop(); render();
      postModule("goxlr-mini", "settings", {mute_modes: m});
      toast("Bouton " + (i + 1) + " : " + MUTE_VERB[t].toLowerCase() + (t === "all" ? "" : " · appui long pour couper partout"));
    });
    mc.appendChild(b);
  });
  box.appendChild(mc);
  box.appendChild(h("div", {"class": "hint"}, "Appui court : selon ce choix. Appui long : coupe partout et fait clignoter le bouton."));
  return box;
}
function paintStripVal(f) {
  var s = strips[f], ch = S.state.faders[f];
  s.val.innerHTML = pct(S.state.volumes[ch]) + "<span>%</span>";
}
function paintStrip(f) {
  var s = strips[f], ch = S.state.faders[f], ms = muteState(ch), mode = S.state.mute_modes[f];
  var key = ch + "|" + ms + "|" + mode;
  if (s.key !== key) {
    s.sl.el.setAttribute("aria-label", "Volume " + LABELS[ch]);
    s.head.innerHTML = chipIco(ch, col(ch)) + '<span class="nm">' + LABELS[ch] + '<small>Fader ' + (S.meta.faders.indexOf(f) + 1) + ' · ' + (ms ? MUTED_TXT[ms].toLowerCase() : "actif") + '</small></span>' + svg("chev", ' class="chev"');
    s.mute.innerHTML = svg(ms ? "speakerOff" : "speaker");
    s.cap.textContent = ms ? MUTED_TXT[ms] : (hasInput(ch) ? MUTE_VERB[mode] : "Couper");
    s.cap.classList.toggle("on", ms === "all");
    s.cap.classList.toggle("part", !!ms && ms !== "all");
    s.mute.classList.toggle("on", ms === "all");
    s.mute.classList.toggle("part", !!ms && ms !== "all");
    s.el.classList.toggle("is-muted", ms === "all");
    s.el.classList.toggle("is-part", !!ms && ms !== "all");
    s.sl.colour(col(ch));
    s.key = key;
  }
  s.sl.set(S.state.volumes[ch]);
  paintStripVal(f);
}
function renderOthers() {
  var onF = S.meta.faders.map(function (f) { return S.state.faders[f]; });
  var list = S.meta.channels.filter(function (c) { return onF.indexOf(c) < 0; });
  var key = list.join(",");
  if (others.key !== key) {
    others.card.innerHTML = "";
    others.rows = {};
    list.forEach(function (ch) {
      var row = h("div", {"class": "orow"});
      row.innerHTML = chipIco(ch, col(ch)) + '<span class="nm">' + LABELS[ch] + '</span>';
      var sl = Slider({label: "Volume " + LABELS[ch], format: function (v) { return pct(v) + " %"; }, min: 0, max: 255, step: 1, wheel: 5, settleMs: 120, smoothRemote: true, colour: col(ch),
        onInput: function (v) { S.state.volumes[ch] = v; volumeSender(ch)(v); p.textContent = pct(v) + " %"; },
        onCommit: function (v) { postModule("goxlr-mini", "volume", {channel: ch, value: v}); }});
      var p = h("span", {"class": "pct"});
      var m = h("button", {"class": "ibtn", title: "Couper partout"});
      m.addEventListener("click", function () { toggleMute(ch, "all"); });
      row.appendChild(sl.el); row.appendChild(p); row.appendChild(m);
      others.card.appendChild(row);
      others.rows[ch] = {sl: sl, p: p, m: m};
    });
    others.key = key;
  }
  list.forEach(function (ch) {
    var r = others.rows[ch], ms = muteState(ch);
    r.sl.set(S.state.volumes[ch]);
    if (!r.sl.busy()) r.p.textContent = pct(S.state.volumes[ch]) + " %";
    r.m.innerHTML = svg(ms ? "speakerOff" : "speaker");
    r.m.classList.toggle("on", ms === "all");
    r.m.classList.toggle("part", !!ms && ms !== "all");
    r.m.title = ms ? MUTED_TXT[ms] + " · cliquer pour réactiver" : "Couper partout";
  });
}
function renderMixer() {
  if (!built.mixer) buildMixer();
  S.meta.faders.forEach(paintStrip);
  renderOthers();
  liveMixer();
}
function liveMixer() {
  if (!L || !built.mixer) return;
  S.meta.faders.forEach(function (f, i) {
    var s = strips[f], pos = L.live.fader_positions[i], ch = S.state.faders[f];
    if (pos == null) { s.ghost.classList.remove("show"); s.note.classList.remove("show"); return; }
    var off = Math.abs(pos - S.state.volumes[ch]) > 8 && !s.sl.busy();
    s.ghost.style.bottom = (pos / 255 * 100) + "%";
    s.ghost.classList.toggle("show", off);
    s.note.classList.toggle("show", off);
    if (off) s.note.textContent = "Niveau lu sur la GoXLR : " + pct(pos) + " %";
  });
}
