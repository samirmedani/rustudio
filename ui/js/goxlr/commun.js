// GoXLR Mini : vocabulaire, couleurs et actions partagées par ses pages.
"use strict";

var LABELS = {Mic:"Micro", LineIn:"Entrée ligne", Console:"Console", System:"Système", Game:"Jeu", Chat:"Chat", Sample:"Samples", Music:"Musique", Headphones:"Casque", MicMonitor:"Retour micro", LineOut:"Sortie ligne"};
var IN_MAP = {Microphone:"Mic", LineIn:"LineIn", Console:"Console", System:"System", Game:"Game", Chat:"Chat", Music:"Music", Samples:"Sample"};
var OUT_LABELS = {Headphones:"Casque", BroadcastMix:"Stream", ChatMic:"Discord / chat", Sampler:"VOD", LineOut:"Enceintes", StreamMix2:"Stream 2"};
var OUT_HINT = {Headphones:"Ce que tu entends", BroadcastMix:"Ce que ton stream entend", ChatMic:"Ce que tes amis entendent", Sampler:"Ce qui est enregistré en VOD", LineOut:"Sortie ligne", StreamMix2:"Second mix"};
var MUTE_LABELS = {all: "Partout", stream: "Stream", chat: "Discord", phones: "Casque", lineout: "Enceintes"};
var MUTE_VERB = {all: "Couper", stream: "Couper du stream", chat: "Couper de Discord", phones: "Couper du casque", lineout: "Couper des enceintes"};
var MUTED_TXT = {all: "Coupé", stream: "Coupé du stream", chat: "Coupé de Discord", phones: "Coupé du casque", lineout: "Coupé des enceintes"};
var MIC_MODES = [["toggle", "Bascule"], ["hold", "Maintenir = coupé"], ["ptt", "Maintenir = parler"]];
var MIC_TYPE_LABELS = {dynamic:"Dynamique", condenser:"Statique +48 V", jack:"Jack 3,5 mm"};
var PRESET_ICONS = {studio:"micVocal", naturel:"audioLines", radio:"radio", stream:"BroadcastMix", podcast:"Mic", "anti-bruit":"shield", brut:"leaf"};
var PRESET_COLOURS = {studio:"#5E5CE6", naturel:"#64D2FF", radio:"#FF9F0A", stream:"#BF5AF2", podcast:"#FF375F", "anti-bruit":"#8E8E93", brut:"#30D158"};
var FX_INFO = [["eq", "Égaliseur", "Ajuste les graves, les médiums et les aigus de ta voix."],
  ["gate", "Noise gate", "Baisse le micro entre tes phrases (clavier, ventilo). Trop fort, il coupe tes fins de mots."],
  ["comp", "Compresseur", "Rapproche les passages forts et faibles. Trop fort, ta voix paraît écrasée."],
  ["deesser", "De-esser", "Adoucit les « s » et les « ch » qui sifflent."]];
function fxAny() { var f = S.state.fx || {}; return FX_INFO.some(function (x) { return f[x[0]]; }); }
function fxSet(k, v) {
  var patch = {}; patch[k] = v; S.state.fx[k] = v; renderMic();
  postModule("goxlr-mini", "fx", patch).then(refresh);
}
function monitorSet(on) {
  var st = S.state; st.routing.Microphone.Headphones = on; renderMic();
  var p = postModule("goxlr-mini", "route", {input: "Microphone", output: "Headphones", enabled: on});
  if (on && st.volumes.MicMonitor < 26) p = p.then(function (r) { return r ? postModule("goxlr-mini", "volume", {channel: "MicMonitor", value: 102}) : null; });
  p.then(refresh);
}
var SCENE_ICONS = {streaming:"BroadcastMix", "stream-safe":"shield", gaming:"Game", discord:"Chat", podcast:"Mic", ecoute:"Headphones"};
var SCENE_COLOURS = {streaming:"#BF5AF2", "stream-safe":"#30D158", gaming:"#30D158", discord:"#5E5CE6", podcast:"#FF375F", ecoute:"#FF9F0A"};
var ANIM_LABELS = {"none":"Aucune", "retro-rainbow":"Arc-en-ciel rétro", "rainbow-dark":"Arc-en-ciel doux", "rainbow-bright":"Arc-en-ciel vif", "simple":"Respiration", "ripple":"Ondulation"};
var STYLE_LABELS = {"two-colour":"Deux couleurs", "gradient":"Dégradé", "meter":"Vumètre", "gradient-meter":"Dégradé + vumètre"};
var PALETTE = ["FF453A","FF9F0A","FFD60A","30D158","63E6BE","64D2FF","0A84FF","5E5CE6","BF5AF2","FF375F","FF6482","AC8E68","FFFFFF","D1D1D6","8E8E93","00E5FF","FF00E5","7B61FF"];
var RATIO_LABEL = function (i) { var r = S ? S.meta.ratios[i] : 1; return (r >= 10 ? r.toFixed(0) : r.toFixed(1)).replace(".", ",") + ":1"; };

function muteState(ch) { return S.state.mutes[ch] ? "all" : (S.state.partial[ch] || null); }
function hasInput(ch) { return !!S.meta.channel_inputs[ch]; }
function setMute(ch, muted, target) {
  target = target || "all";
  if (!muted) { S.state.mutes[ch] = false; S.state.partial[ch] = null; }
  else if (target === "all" || !hasInput(ch)) { S.state.mutes[ch] = true; S.state.partial[ch] = null; }
  else { S.state.mutes[ch] = false; S.state.partial[ch] = target; }
  render();
  return postModule("goxlr-mini", "mute", {channel: ch, muted: muted, target: target});
}
function toggleMute(ch, target) { return setMute(ch, !muteState(ch), target); }
function col(ch) { return "#" + uiHex(S ? S.meta.channel_colours[ch] : "8E8E93"); }
function uiHex(hx) {     // une voie blanche ou presque reste lisible sur le fond clair
  var v = parseInt(hx, 16), l = (0.2126 * (v >> 16 & 255) + 0.7152 * (v >> 8 & 255) + 0.0722 * (v & 255)) / 255;
  return l > 0.82 ? "8E8E93" : hx;
}
