// GoXLR Mini : page Éclairage.
"use strict";

/* ---------- eclairage ---------- */
var lt = {};
function buildLight() {
  var root = $("p-light"); root.innerHTML = "";
  var grid = h("div", {"class": "light-grid"});
  var left = h("div", {style: "display:flex;flex-direction:column;gap:16px"});
  var dev = h("div", {"class": "card device"}); lt.dev = dev; lt.previewKey = null; left.appendChild(dev);
  var styleCard = h("div", {"class": "card"}, "<h3>Barres des faders</h3>");
  lt.styles = h("div", {"class": "seg"});
  S.meta.fader_styles.forEach(function (k) {
    var b = h("button", {"data-k": k}, STYLE_LABELS[k]);
    b.addEventListener("click", function () { S.meta.faders.forEach(function (f) { S.state.lighting.fader_style[f] = k; }); renderLight(); postModule("goxlr-mini", "lighting", {fader_style: k}); });
    lt.styles.appendChild(b);
  });
  styleCard.appendChild(lt.styles);
  styleCard.appendChild(h("h3", {style: "margin-top:16px"}, "Animation"));
  lt.anims = h("div", {"class": "chips"});
  S.meta.animations.forEach(function (k) {
    var b = h("button", {"data-k": k}, ANIM_LABELS[k] || k);
    b.addEventListener("click", function () { S.state.lighting.animation = k; renderLight(); postModule("goxlr-mini", "lighting", {animation: k}); });
    lt.anims.appendChild(b);
  });
  styleCard.appendChild(lt.anims);
  var animSend = throttle(function (p) { postModule("goxlr-mini", "lighting", p); }, 120);
  lt.mod1 = fineSlider(styleCard, "Amplitude", 0, 100, function (v) { return v + " %"; }, function (v) { S.state.lighting.mod1 = v; animSend({mod1: v}); });
  lt.mod2 = fineSlider(styleCard, "Vitesse", 0, 100, function (v) { return v + " %"; }, function (v) { S.state.lighting.mod2 = v; animSend({mod2: v}); });
  left.appendChild(styleCard);
  grid.appendChild(left);
  var right = h("div", {style: "display:flex;flex-direction:column;gap:16px;min-width:0"});
  var th = h("div", {"class": "card"}, "<h3>Thèmes</h3>");
  lt.themes = h("div", {"class": "themes"}); th.appendChild(lt.themes); right.appendChild(th);
  var cu = h("div", {"class": "card", style: "flex:1"}, "<h3>Personnaliser<small>Clique une couleur pour la changer</small></h3>");
  lt.custom = h("div"); cu.appendChild(lt.custom); right.appendChild(cu);
  grid.appendChild(right);
  root.appendChild(grid);
  built.light = true;
}
function colourPop(anchor, current, onPick) {
  var box = h("div");
  var sws = h("div", {"class": "swatches"});
  PALETTE.forEach(function (c) {
    var b = h("button", {"class": "sw" + (c === current ? " cur" : ""), style: "--c:#" + c});
    b.addEventListener("click", function () { closePop(); onPick(c); });
    sws.appendChild(b);
  });
  box.appendChild(sws);
  var cus = h("label", {"class": "sw-custom"}, "Autre couleur");
  var inp = h("input", {type: "color", value: "#" + current.toLowerCase()});
  inp.addEventListener("change", function () { closePop(); onPick(inp.value.slice(1).toUpperCase()); });
  cus.appendChild(inp); box.appendChild(cus);
  openPop(anchor, box);
}
function renderLight() {
  if (!built.light) buildLight();
  var st = S.state, li = st.lighting, c = st.colours;
  paintDevicePreview();
  lt.styles.querySelectorAll("button").forEach(function (b) { b.classList.toggle("on", b.dataset.k === li.fader_style.A); });
  lt.anims.querySelectorAll("button").forEach(function (b) { b.classList.toggle("on", b.dataset.k === li.animation); });
  lt.mod1.set(li.mod1); lt.mod2.set(li.mod2);
  var key = JSON.stringify([li.theme, S.meta.themes]);
  if (lt.themesKey !== key) {
    lt.themes.innerHTML = "";
    Object.keys(S.meta.themes).forEach(function (k) {
      var t = S.meta.themes[k], b = h("button", {"class": "theme" + (li.theme === k ? " on" : "")});
      b.innerHTML = '<div class="bars">' + t.preview.map(function (x) { return '<i style="background:linear-gradient(0deg,#' + darken(x, .25) + ',#' + x + ')"></i>'; }).join("") + "</div><b>" + frLabel(t.label) + "</b>";
      b.addEventListener("click", function () { withUndo("Thème " + frLabel(t.label) + " appliqué", function () { return postModule("goxlr-mini", "theme", {name: k}).then(refresh); }); });
      lt.themes.appendChild(b);
    });
    lt.themesKey = key;
  }
  var ckey = JSON.stringify([c, st.faders]);
  if (lt.customKey !== ckey) {
    lt.custom.innerHTML = "";
    S.meta.faders.forEach(function (f, i) {
      var ch = st.faders[f], top = c["FadeMeter" + (i + 1)][1];
      customRow("Fader " + (i + 1) + " · " + LABELS[ch], [top], function (nc) {
        return [["FadeMeter" + (i + 1), [darken(nc, .16), nc]], ["Fader" + (i + 1) + "Mute", [nc, null]]];
      });
    });
    customRow("Bouton micro", [c.MicrophoneMute[0]], function (nc) { return [["MicrophoneMute", [nc, null]]]; });
    customRow("Logo X et éclairage", [c.Accent[0]], function (nc) { return [["Accent", [nc, nc]], ["Global", [nc, nc]], ["Bleep", [nc, null]]]; });
    lt.customKey = ckey;
  }
}
function customRow(label, colours, toTargets) {
  var row = h("div", {"class": "custom-row"}, '<span class="lbl">' + label + "</span>");
  var sws = h("div", {"class": "sws"});
  colours.forEach(function (cur) {
    var b = h("button", {"class": "swb", style: "--c:#" + cur});
    b.addEventListener("click", function () {
      colourPop(b, cur, function (nc) {
        toTargets(nc).forEach(function (t) {
          var cols = S.state.colours[t[0]]; if (t[1][0]) cols[0] = t[1][0]; if (t[1][1]) cols[1] = t[1][1];
          postModule("goxlr-mini", "colour", {target: t[0], colours: t[1]});
        });
        S.state.lighting.theme = "custom"; lt.customKey = null; lt.themesKey = null; renderLight();
      });
    });
    sws.appendChild(b);
  });
  row.appendChild(sws); lt.custom.appendChild(row);
}
function paintDevicePreview() {
  var st = S.state, pos = L ? L.live.fader_positions : [null, null, null, null];
  var bleep = !!L && L.live.pressed.indexOf("Bleep") >= 0;
  var key = JSON.stringify([st.colours, st.faders, st.mutes, bleep]);
  if (lt.previewKey !== key) {
    lt.dev.innerHTML = deviceSvg(); lt.previewKey = key;
    lt.previewFaders = lt.dev.querySelectorAll("[data-preview-fader]"); lt.previewValues = [];
  }
  lt.previewFaders.forEach(function (node) {
    var i = Number(node.getAttribute("data-preview-fader")), ch = st.faders[S.meta.faders[i]];
    var value = pos[i] == null ? st.volumes[ch] : pos[i];
    if (lt.previewValues[i] !== value) {
      lt.previewValues[i] = value;
      node.style.transform = "translateY(" + (176 - value / 255 * 112) + "px)";
    }
  });
}
function deviceSvg() {
  var st = S.state, c = st.colours, pos = L ? L.live.fader_positions : [null, null, null, null];
  var o = '<svg viewBox="0 0 440 300"><defs><filter id="bl" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="6"/></filter>' +
    '<linearGradient id="body" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2a2a2f"/><stop offset="1" stop-color="#121215"/></linearGradient></defs>';
  o += '<ellipse cx="220" cy="282" rx="190" ry="10" fill="#' + c.Global[0] + '" opacity=".35" filter="url(#bl)"/>';
  o += '<rect x="14" y="16" width="412" height="262" rx="26" fill="url(#body)" stroke="rgba(255,255,255,.1)"/>';
  o += '<g transform="translate(206,30)"><path d="M0 0l14 14M14 0L0 14" stroke="#' + c.Accent[0] + '" stroke-width="4" stroke-linecap="round"/><path d="M0 0l14 14M14 0L0 14" stroke="#' + c.Accent[0] + '" stroke-width="5" opacity=".7" filter="url(#bl)"/></g>';
  S.meta.faders.forEach(function (f, i) {
    var x = 46 + i * 80, ch = st.faders[f], muted = st.mutes[ch], fm = c["FadeMeter" + (i + 1)], mb = c["Fader" + (i + 1) + "Mute"][0];
    var g = "g" + i;
    o += '<linearGradient id="' + g + '" x1="0" y1="1" x2="0" y2="0"><stop offset="0" stop-color="#' + fm[0] + '"/><stop offset="1" stop-color="#' + fm[1] + '"/></linearGradient>';
    for (var k = 0; k < 12; k++) {
      var y = 182 - k * 11.5;
      o += '<rect x="' + (x + 32) + '" y="' + y + '" width="8" height="7" rx="2" fill="' + (muted ? "#1d1d20" : "url(#" + g + ")") + '"/>';
    }
    o += '<rect x="' + (x + 12) + '" y="58" width="6" height="134" rx="3" fill="#050506"/>';
    var p = pos[i] == null ? st.volumes[ch] : pos[i];
    var ky = 176 - p / 255 * 112;
    o += '<g class="preview-fader" data-preview-fader="' + i + '" style="transform:translateY(' + ky + 'px)"><rect x="' + (x - 2) + '" y="0" width="34" height="22" rx="5" fill="#3a3a40" stroke="rgba(255,255,255,.18)"/><rect x="' + (x + 2) + '" y="10" width="26" height="2" rx="1" fill="rgba(255,255,255,.55)"/></g>';
    o += '<rect x="' + (x - 2) + '" y="214" width="48" height="26" rx="8" fill="#' + mb + '" opacity="' + (muted ? 1 : .22) + '"/>';
    if (muted) o += '<rect x="' + (x - 2) + '" y="214" width="48" height="26" rx="8" fill="#' + mb + '" opacity=".6" filter="url(#bl)"/>';
    o += '<text x="' + (x + 22) + '" y="258" fill="rgba(255,255,255,.45)" font-size="10" text-anchor="middle" font-family="Segoe UI, sans-serif">' + LABELS[ch] + '</text>';
  });
  var mm = c.MicrophoneMute[0], bl = c.Bleep[0], micMuted = st.mutes.Mic;
  o += '<rect x="372" y="64" width="38" height="62" rx="10" fill="#' + mm + '" opacity="' + (micMuted ? 1 : .22) + '"/>';
  o += '<text x="391" y="140" fill="rgba(255,255,255,.45)" font-size="9" text-anchor="middle" font-family="Segoe UI, sans-serif">MICRO</text>';
  o += '<rect x="372" y="152" width="38" height="38" rx="10" fill="#' + bl + '" opacity="' + (L && L.live.pressed.indexOf("Bleep") >= 0 ? 1 : .22) + '"/>';
  o += '<text x="391" y="204" fill="rgba(255,255,255,.45)" font-size="9" text-anchor="middle" font-family="Segoe UI, sans-serif">BLEEP</text>';
  return o + "</svg>";
}
