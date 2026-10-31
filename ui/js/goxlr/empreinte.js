// GoXLR Mini : empreinte vocale (enregistrement de « Chat Mic », analyse par le moteur, profils sur mesure),
// carte « Ton micro » et égaliseur du micro avec ses vraies fréquences.
"use strict";

/* ---------- icônes de ces cartes (Lucide, ISC) ---------- */
ICONS.fingerprint = ICONS.fingerprint || '<path d="M12 10a2 2 0 0 0-2 2c0 1.02-.1 2.51-.26 4"/> <path d="M14 13.12c0 2.38 0 6.38-1 8.88"/> <path d="M17.29 21.02c.12-.6.43-2.3.5-3.02"/> <path d="M2 12a10 10 0 0 1 18-6"/> <path d="M2 16h.01"/> <path d="M21.8 16c.2-2 .131-5.354 0-6"/> <path d="M5 19.5C5.5 18 6 15 6 12a6 6 0 0 1 .34-2"/> <path d="M8.65 22c.21-.66.45-1.32.57-2"/> <path d="M9 6.8a6 6 0 0 1 9 5.2v2"/>';
ICONS.wind = ICONS.wind || '<path d="M12.8 19.6A2 2 0 1 0 14 16H2"/> <path d="M17.5 8a2.5 2.5 0 1 1 2 4H2"/> <path d="M9.8 4.4A2 2 0 1 1 11 8H2"/>';
ICONS.house = ICONS.house || '<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/> <path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>';
ICONS.zap = ICONS.zap || '<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/>';
ICONS.ruler = ICONS.ruler || '<path d="M21.3 15.3a2.4 2.4 0 0 1 0 3.4l-2.6 2.6a2.4 2.4 0 0 1-3.4 0L2.7 8.7a2.41 2.41 0 0 1 0-3.4l2.6-2.6a2.41 2.41 0 0 1 3.4 0Z"/> <path d="m14.5 12.5 2-2"/> <path d="m11.5 9.5 2-2"/> <path d="m8.5 6.5 2-2"/> <path d="m17.5 15.5 2-2"/>';
ICONS.waveform = ICONS.waveform || '<path d="M2 13a2 2 0 0 0 2-2V7a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0V4a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0v-4a2 2 0 0 1 2-2"/>';
ICONS.externalLink = ICONS.externalLink || '<path d="M15 3h6v6"/> <path d="M10 14 21 3"/> <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>';

/* ---------- textes et réglages ---------- */
var VP_TEXT = "Salut, c\u2019est moi ! Je règle mon micro pour que ma voix soit claire, posée et précise. Six chaussettes sèches sèchent sous le soleil. Pierre prépare un bon petit plat pour ses potes, puis part jouer. Quand je parle, chaque mot doit sonner juste.";
var VP_LOUD = "Allez, on y va, c\u2019est parti !";
var VP_DUR = {silence: 5, reading: 16, loud: 5};
var VP_STEPS = [["silence", "Silence"], ["reading", "Lecture"], ["loud", "Plus fort"], ["analyse", "Analyse"]];
var VP_PHASE = {
  prepare: ["settings", "Préparation", "Je mets tes traitements de côté et je dirige ton micro vers l\u2019enregistrement."],
  silence: ["volumeX", "Silence…", "Ne dis rien et ne touche à rien : j\u2019écoute ce que ton micro entend de ta pièce."],
  reading: ["audioLines", "Lis ce texte à voix haute", ""],
  loud: ["BroadcastMix", "Plus fort !", "Comme quand tu t\u2019enflammes en jeu : je vérifie que ta voix ne sature jamais."],
  analyse: ["sparkles", "J\u2019analyse ta voix…", "Timbre, sifflantes, plosives, bruit et résonance de ta pièce, puis chaque préréglage calculé pour toi."]
};
var VP_ERRORS = {
  "no-chat-mic": "Windows ne montre pas l\u2019entrée « Chat Mic » de la GoXLR. Vérifie qu\u2019elle est branchée et que « Chat Mic » est activé dans les paramètres Son de Windows.",
  denied: "L\u2019accès au micro a été refusé à Rustudio : je ne peux pas enregistrer « Chat Mic ».",
  audio: "L\u2019enregistrement de « Chat Mic » n\u2019a pas pu démarrer.",
  silent: "« Chat Mic » ne renvoie aucun son : Windows ne transmet pas l\u2019enregistrement.",
  muted: "Ton micro est coupé et n\u2019a pas pu être réactivé.",
  start: "Le moteur n\u2019a pas lancé l\u2019enregistrement.",
  empty: "L\u2019enregistrement est vide : « Chat Mic » n\u2019a rien transmis pendant l\u2019empreinte.",
  network: "Le moteur Rustudio ne répond pas : l\u2019enregistrement n\u2019a pas été analysé.",
  analyse: "L\u2019analyse de ta voix a échoué."
};
var VP_WARN = {
  too_quiet: "Ta voix arrivait faiblement : rapproche-toi du micro, puis refais ton empreinte.",
  too_short: "J\u2019ai entendu peu de parole : refais l\u2019empreinte en lisant tout le texte.",
  noisy: "Ta pièce est bruyante pour ce micro : le noise gate travaillera davantage.",
  clipped: "Ta voix a saturé pendant l\u2019enregistrement : le gain a été baissé pour la suite.",
  no_silence: "Le silence n\u2019a pas pu être mesuré : le bruit de ta pièce est estimé.",
  no_loud: "La phrase forte manque : la marge contre la saturation est estimée."
};
var VP_LATENCY_MS = 20;      // délai estimé entre la capsule et le traitement audio de la page
var VP_MARGIN_S = 1.5;       // marge gardée autour des étapes mesurées
var VP_MAX_BYTES = 7500000;  // le moteur accepte 8 Mo
var VP_MIN_FRAMES = 4800;    // moins de 0,1 s : rien n'a été enregistré
var VP_MAX_CAPTURE_S = 150;  // au-delà, plus rien n'est gardé (l'empreinte dure une trentaine de secondes)
var VP_TYPE_LABELS = {condenser: "Statique", dynamic: "Dynamique", jack: "Micro de casque"};

/* ---------- outils sans interface (testés par scripts/voice-ui-test.mjs) ---------- */
function freqLabel(f) {
  f = +f;
  if (!(f > 0)) return "—";
  var r = Math.round(f);
  if (r < 1000) return r + " Hz";
  return String(Math.round(f / 100) / 10).replace(".", ",") + " kHz";
}
function freqStep(f) { return f < 100 ? 1 : f < 1000 ? 5 : f < 10000 ? 50 : 100; }
function roundFreq(f, lo, hi) {
  var s = freqStep(f), r = Math.round(f / s) * s;
  return lo != null ? clamp(r, lo, hi) : r;
}
function numFr(v, d) { return (+v).toFixed(d || 0).replace(".", ","); }
function numTrim(v, d) { var t = (+v).toFixed(d || 0); if (d) t = t.replace(/\.?0+$/, ""); return t.replace(".", ","); }
function sgnDb(v) { v = Math.round(v); return (v > 0 ? "+" : v < 0 ? "\u2212" : "") + Math.abs(v) + " dB"; }
// Échantillons flottants (-1 à 1) vers PCM 16 bits petit-boutiste, saturés proprement.
function vpToInt16(x) {
  var n = x.length, buf = new ArrayBuffer(n * 2), dv = new DataView(buf);
  for (var i = 0; i < n; i++) {
    var v = +x[i];
    if (!(v === v)) v = 0;
    v = v > 1 ? 1 : v < -1 ? -1 : v;
    dv.setInt16(i * 2, v < 0 ? Math.round(v * 32768) : Math.round(v * 32767), true);
  }
  return buf;
}
function vpJoin(chunks, n) {
  var out = new Float32Array(n), o = 0;
  chunks.forEach(function (c) { var k = Math.min(c.length, n - o); if (k > 0) { out.set(k === c.length ? c : c.subarray(0, k), o); o += k; } });
  return out;
}
// Ne garde de l'enregistrement que les étapes mesurées, avec une marge : t0 suit la coupe.
function vpTrim(x, rate, t0, timeline) {
  var spans = (timeline || []).filter(function (s) { return s && isFinite(s.start) && isFinite(s.end) && s.end > s.start && s.start > 1e9; });
  var a = 0, b = x.length;
  if (spans.length && rate > 0) {
    var first = Math.min.apply(null, spans.map(function (s) { return s.start; }));
    var last = Math.max.apply(null, spans.map(function (s) { return s.end; }));
    a = clamp(Math.floor((first - VP_MARGIN_S - t0 / 1000) * rate), 0, x.length);
    b = clamp(Math.ceil((last + VP_MARGIN_S - t0 / 1000) * rate), a, x.length);
    if (b - a < rate) { a = 0; b = x.length; }   // horloges en désaccord : tout envoyer plutôt que rien
  }
  b = Math.min(b, a + Math.floor(VP_MAX_BYTES / 2));
  return {samples: x.subarray(a, b), t0: t0 + a / rate * 1000};
}
// Mots du texte à lire, avec leur poids de lecture (longueur, pause après la ponctuation).
function vpWords(text) {
  return String(text).replace(/ ([!?;:»])/g, "\u00a0$1").replace(/« /g, "«\u00a0").split(/ +/).filter(Boolean).map(function (w) {
    var letters = w.replace(/[^A-Za-zÀ-ÖØ-öø-ÿ\u2019']/g, "").length;
    return {w: w, weight: Math.max(2, letters) + 1.5, pause: /[.!?…]$/.test(w) ? 6 : /[,;:]$/.test(w) ? 3 : 0};
  });
}
// Mot à lire quand la lecture en est à f (0 à 1). Le guide finit un peu avant la fin, pour laisser respirer.
function vpWordAt(words, f) {
  var total = 0;
  words.forEach(function (x) { total += x.weight + x.pause; });
  var target = clamp(f / 0.9, 0, 1) * total, acc = 0;
  for (var i = 0; i < words.length; i++) { acc += words[i].weight + words[i].pause; if (acc > target) return i; }
  return words.length;
}
function vpMid(bands, vals) {
  var s = 0, n = 0;
  (bands || []).forEach(function (f, i) { var v = vals && vals[i]; if (f >= 500 && f <= 2000 && v != null && isFinite(v)) { s += v; n++; } });
  return n ? s / n : 0;
}
function vpRel(bands, vals, mid) { return (vals || []).map(function (v) { return v == null || !isFinite(v) ? null : v - mid; }); }
// Biquads RBJ (cloche, plateau grave, plateau aigu en forme Q) à 48 kHz, comme hpBiquad() de la page Casque.
function eqCoefs(kind, f0, g, q) {
  var fs = 48000, a = Math.pow(10, g / 40), w0 = 2 * Math.PI * Math.min(f0, fs * .49) / fs, cw = Math.cos(w0), al = Math.sin(w0) / (2 * (q > 0 ? q : 1)), s = 2 * Math.sqrt(a) * al;
  if (kind === "lowshelf") return [a * ((a + 1) - (a - 1) * cw + s), 2 * a * ((a - 1) - (a + 1) * cw), a * ((a + 1) - (a - 1) * cw - s), (a + 1) + (a - 1) * cw + s, -2 * ((a - 1) + (a + 1) * cw), (a + 1) + (a - 1) * cw - s];
  if (kind === "highshelf") return [a * ((a + 1) + (a - 1) * cw + s), -2 * a * ((a - 1) + (a + 1) * cw), a * ((a + 1) + (a - 1) * cw - s), (a + 1) - (a - 1) * cw + s, 2 * ((a - 1) - (a + 1) * cw), (a + 1) - (a - 1) * cw - s];
  return [1 + al * a, -2 * cw, 1 - al * a, 1 + al / a, -2 * cw, 1 - al / a];
}
function eqDb(c, f) {
  var w = 2 * Math.PI * f / 48000, c1 = Math.cos(w), s1 = -Math.sin(w), c2 = Math.cos(2 * w), s2 = -Math.sin(2 * w);
  var nr = c[0] + c[1] * c1 + c[2] * c2, ni = c[1] * s1 + c[2] * s2, dr = c[3] + c[4] * c1 + c[5] * c2, di = c[4] * s1 + c[5] * s2;
  return 10 * Math.log10(Math.max(1e-24, (nr * nr + ni * ni) / (dr * dr + di * di)));
}
function micEqFreq(band, st) { var ef = st && st.dsp && st.dsp.eq_freq, f = ef ? +ef[band] : 0; return f > 0 ? f : +band; }
// Modèle mesuré d'une bande de la Mini (S.meta.eq_model) : forme (cloche, plateau grave ou aigu) et largeur (Q).
// Chaque bande agit à la fréquence réglée.
function micEqModel(band) {
  var m = S && S.meta && S.meta.eq_model && S.meta.eq_model[band];
  var kind = m && (m.kind === "lowshelf" || m.kind === "highshelf") ? m.kind : "peak";
  return {kind: kind, q: (m && +m.q > 0) ? +m.q : 1};
}
// Réponse (dB, fonction de la fréquence) d'une bande réglée à f Hz et g dB, telle que la Mini la produit.
function eqBand(m, f, g) {
  if (!g || !(f > 0)) return function () { return 0; };
  var c = eqCoefs(m.kind, f, g, m.q);
  return function (x) { return eqDb(c, x); };
}
// Réponse de l'égaliseur du micro (dB) : fréquences et formes réelles des six bandes.
function micEqResponse(st, eq) {
  eq = eq || st.dsp.eq;
  var parts = [];
  (S.meta.eq_bands || []).forEach(function (b) {
    var g = +eq[b] || 0;
    if (g) parts.push(eqBand(micEqModel(b), micEqFreq(b, st), g));
  });
  return function (f) { var t = 0; for (var i = 0; i < parts.length; i++) t += parts[i](f); return t; };
}
function micName(m) {
  if (!m) return "";
  var b = String(m.brand || ""), n = String(m.name || "");
  return !b || m.generic || n.toLowerCase().indexOf(b.toLowerCase()) === 0 ? n : b + " " + n;
}
function micModelsReady() { return !!(S && S.meta && S.meta.mic_models && Object.keys(S.meta.mic_models).length); }
// Modèle affiché : celui de l'état s'il correspond au type réglé, sinon le micro générique de ce type.
function micModelKey() {
  var st = S.state, models = S.meta.mic_models || {}, k = st.mic.model;
  if (k && models[k] && models[k].type === st.mic.type) return k;
  var g = Object.keys(models).filter(function (x) { return models[x].generic && models[x].type === st.mic.type; })[0];
  return g || "";
}
function micModelInfo() { var k = micModelsReady() ? micModelKey() : ""; return k ? S.meta.mic_models[k] : null; }
// Modèle que tu as choisi, s'il correspond toujours au type réglé (le moteur l'oublie quand le type change).
function micChosen() {
  if (!micModelsReady()) return null;
  var st = S.state, m = st.mic.model ? S.meta.mic_models[st.mic.model] : null;
  return m && m.type === st.mic.type ? m : null;
}
function micDistance(m) { var d = m && m.distance_cm; return d && d.length === 2 ? d[0] + "–" + d[1] + " cm" : "10–15 cm"; }
function vpSpecs(m) {
  if (!m) return [];
  var out = [(VP_TYPE_LABELS[m.type] || m.type) + (m.phantom ? " · 48 V" : "")];
  if (m.sensitivity_mv_pa) out.push(numTrim(m.sensitivity_mv_pa, m.sensitivity_mv_pa < 10 ? 2 : 0) + " mV/Pa");
  if (m.self_noise_dba != null) out.push("Bruit propre " + numFr(m.self_noise_dba) + " dB(A)");
  if (m.pattern) out.push(String(m.pattern).charAt(0).toUpperCase() + String(m.pattern).slice(1));
  if (m.max_spl_db) out.push(numFr(m.max_spl_db) + " dB SPL max");
  return out;
}
// Résumé lisible de l'empreinte : celui du moteur, complété depuis l'empreinte brute si besoin.
function vpSummary(d) {
  var p = (d && d.print) || (S && S.state.voice_print) || null, s = (d && d.summary) || {}, lv = p && p.levels;
  var f0 = s.f0_hz != null ? +s.f0_hz : p && p.f0_hz != null ? +p.f0_hz : null;
  var sib = s.sibilance || null, ex = p && p.sibilance ? p.sibilance.excess_db : null;
  if (!sib && ex != null) sib = ex < 0 ? "faibles" : ex < 6 ? "normales" : ex < 12 ? "marquées" : "très marquées";
  return {
    type: s.voice_type || (f0 ? (f0 < 140 ? "grave" : f0 < 200 ? "médium" : "aiguë") : null), f0: f0,
    snr: s.snr_db != null ? +s.snr_db : lv ? lv.speech_p50_db - lv.noise_peak_db : null,
    sib: sib, sibHz: p && p.sibilance && p.sibilance.peak_hz, plo: s.plosives != null ? s.plosives : p && p.plosives ? p.plosives.count : null,
    hum: s.hum_hz != null ? s.hum_hz : p && p.hum ? p.hum.hz : null, room: s.room || null,
    rt60: s.rt60_ms != null ? s.rt60_ms : p && p.room && p.room.rt60_ms, warnings: s.warnings || (p && p.warnings) || []
  };
}
function vpFacts(sm) {
  var out = [], cap = function (t) { return t ? t.charAt(0).toUpperCase() + t.slice(1) : t; };
  if (sm.type || sm.f0) out.push({icon: "micVocal", k: "Ta voix", v: [cap(sm.type), sm.f0 ? Math.round(sm.f0) + " Hz" : ""].filter(Boolean).join(" · "), t: (sm.type ? "Voix " + sm.type : "Voix") + (sm.f0 ? " · " + Math.round(sm.f0) + " Hz" : "")});
  if (sm.sib) out.push({icon: "waveform", k: "Sifflantes", v: cap(sm.sib) + (sm.sibHz ? " · " + freqLabel(sm.sibHz) : ""), t: "Sifflantes " + sm.sib + (sm.sibHz ? " · " + freqLabel(sm.sibHz) : ""), warn: /marquées/.test(sm.sib)});
  if (sm.plo != null) {
    var n = +sm.plo, txt = typeof sm.plo === "string" && !(n === n) ? cap(sm.plo) : n === 0 ? "Aucune" : n + (n > 1 ? " plosives" : " plosive");
    out.push({icon: "wind", k: "Plosives", v: txt, t: n === 0 ? "Aucune plosive" : typeof sm.plo === "string" && !(n === n) ? "Plosives " + sm.plo : txt, warn: n >= 4});
  }
  if (sm.snr != null && isFinite(sm.snr)) out.push({icon: "volumeX", k: "Bruit de la pièce", v: Math.round(sm.snr) + " dB sous ta voix", t: "Bruit " + Math.round(sm.snr) + " dB sous ta voix", warn: sm.snr < 25});
  if (sm.room) out.push({icon: "house", k: "Pièce", v: cap(sm.room) + (sm.rt60 ? " · " + Math.round(sm.rt60) + " ms" : ""), t: "Pièce " + sm.room + (sm.rt60 ? " · " + Math.round(sm.rt60) + " ms" : "")});
  if (sm.hum) out.push({icon: "zap", k: "Ronflement", v: Math.round(sm.hum) + " Hz", t: "Ronflement " + Math.round(sm.hum) + " Hz", warn: true});
  return out;
}
// Réglages d'un profil, en clair : ce que la GoXLR va recevoir.
function vpSettings(p) {
  var d = p && p.dsp, fx = (p && p.fx) || {}, out = [];
  if (!d) return out;
  if (fx.eq && d.eq) {
    var parts = Object.keys(d.eq).filter(function (b) { return +d.eq[b]; }).map(function (b) {
      var f = d.eq_freq && +d.eq_freq[b] ? +d.eq_freq[b] : +b;
      return (sgnDb(+d.eq[b]) + " à " + freqLabel(f)).replace(/ /g, "\u00a0");
    });
    if (parts.length) out.push({icon: "mixer", t: "Égaliseur", v: parts.join(" · ")});
  }
  if (fx.gate && d.gate) out.push({icon: "volumeX", t: "Noise gate", v: "seuil " + sgnDb(d.gate.threshold)});
  if (fx.comp && d.comp && d.comp.ratio > 0) {
    var r = S && S.meta && S.meta.ratios ? S.meta.ratios[d.comp.ratio] : null;
    out.push({icon: "activity", t: "Compresseur", v: (r ? numTrim(r, r >= 10 ? 0 : 1) + ":1 " : "") + "dès " + sgnDb(d.comp.threshold) + (d.comp.makeup ? ", compensation " + sgnDb(d.comp.makeup) : "")});
  }
  if (fx.deesser && d.deesser > 0) out.push({icon: "waveform", t: "De-esser", v: d.deesser + " %"});
  return out;
}

/* ---------- graphiques ---------- */
function vpPath(c, pts) {
  if (!pts.length) return;
  c.moveTo(pts[0][0], pts[0][1]);
  for (var i = 1; i < pts.length - 1; i++) c.quadraticCurveTo(pts[i][0], pts[i][1], (pts[i][0] + pts[i + 1][0]) / 2, (pts[i][1] + pts[i + 1][1]) / 2);
  if (pts.length > 1) c.lineTo(pts[pts.length - 1][0], pts[pts.length - 1][1]);
}
// Spectres en dB relatifs (0 = médiums), fréquences en échelle logarithmique de 50 Hz à 16 kHz.
function vpDrawChart(cv, ch, o) {
  o = o || {};
  if (!cv || !cv.getContext || !cv.offsetWidth || !ch || !ch.bands) return;
  var w = cv.offsetWidth, ht = cv.offsetHeight, dpr = (typeof devicePixelRatio !== "undefined" && devicePixelRatio) || 1;
  cv.width = Math.round(w * dpr); cv.height = Math.round(ht * dpr);
  var c = cv.getContext("2d"); c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, w, ht);
  var mini = !!o.mini, F0 = 50, F1 = 16000, L = mini ? 2 : 34, R = mini ? 2 : 8, T = mini ? 6 : 10, B = mini ? 4 : 22, lo = o.lo != null ? o.lo : mini ? -36 : -42, hi = o.hi != null ? o.hi : mini ? 9 : 12;
  function X(f) { return L + Math.log(f / F0) / Math.log(F1 / F0) * (w - L - R); }
  function Y(db) { return T + (hi - clamp(db, lo, hi)) / (hi - lo) * (ht - T - B); }
  function pts(vals) {
    var out = [];
    ch.bands.forEach(function (f, i) { var v = vals && vals[i]; if (f >= F0 * .97 && f <= F1 * 1.03 && v != null && isFinite(v)) out.push([X(f), Y(v)]); });
    return out;
  }
  c.lineWidth = 1; c.font = "10.5px Inter Variable, Segoe UI, sans-serif"; c.textBaseline = "middle";
  if (!mini) {
    for (var g = Math.floor(hi / 6) * 6; g >= lo; g -= 6) {
      c.strokeStyle = g === 0 ? "rgba(0,0,0,.12)" : "rgba(0,0,0,.045)";
      c.beginPath(); c.moveTo(L, Y(g)); c.lineTo(w - R, Y(g)); c.stroke();
      if (g % 12 === 0) { c.fillStyle = "rgba(60,60,67,.5)"; c.textAlign = "right"; c.fillText(g > 0 ? "+" + g : String(g).replace("-", "\u2212"), L - 6, Y(g)); }
    }
    [[100, "100"], [200, "200"], [500, "500"], [1000, "1k"], [2000, "2k"], [5000, "5k"], [10000, "10k"]].forEach(function (t) {
      c.strokeStyle = "rgba(0,0,0,.045)"; c.beginPath(); c.moveTo(X(t[0]), T); c.lineTo(X(t[0]), ht - B); c.stroke();
      c.fillStyle = "rgba(60,60,67,.5)"; c.textAlign = "center"; c.fillText(t[1], X(t[0]), ht - B / 2 + 2);
    });
  }
  if (ch.target_lo && ch.target_hi && !mini) {
    var up = pts(ch.target_hi), dn = pts(ch.target_lo);
    if (up.length > 1 && dn.length > 1) {
      c.beginPath(); vpPath(c, up); dn.slice().reverse().forEach(function (p) { c.lineTo(p[0], p[1]); }); c.closePath();
      c.fillStyle = "rgba(0,113,227,.09)"; c.fill();
      c.setLineDash([3, 4]); c.strokeStyle = "rgba(0,113,227,.32)"; c.lineWidth = 1;
      c.beginPath(); vpPath(c, up); c.stroke(); c.beginPath(); vpPath(c, dn); c.stroke(); c.setLineDash([]);
    }
  }
  var noise = vpVisible(ch.noise, lo) ? pts(ch.noise) : [];
  if (noise.length > 1) {
    c.setLineDash([2, 4]); c.strokeStyle = "rgba(60,60,67,.42)"; c.lineWidth = 1.5;
    c.beginPath(); vpPath(c, noise); c.stroke(); c.setLineDash([]);
  }
  var voice = pts(ch.voice);
  if (voice.length > 1) {
    if (mini) {
      c.beginPath(); vpPath(c, voice); c.lineTo(voice[voice.length - 1][0], ht); c.lineTo(voice[0][0], ht); c.closePath();
      var vg = c.createLinearGradient(0, T, 0, ht); vg.addColorStop(0, "rgba(118,118,128,.26)"); vg.addColorStop(1, "rgba(118,118,128,.02)");
      c.fillStyle = vg; c.fill();
    }
    c.strokeStyle = "rgba(60,60,67,.5)"; c.lineWidth = mini ? 1.4 : 1.8;
    c.beginPath(); vpPath(c, voice); c.stroke();
  }
  var after = pts(ch.after), prog = o.progress == null ? 1 : clamp(o.progress, 0, 1);
  if (after.length > 1 && prog > 0) {
    c.save();
    c.beginPath(); c.rect(0, 0, L + (w - L - R) * prog + 1, ht); c.clip();
    var grad = c.createLinearGradient(L, 0, w - R, 0);
    grad.addColorStop(0, "#5E5CE6"); grad.addColorStop(.55, "#0071E3"); grad.addColorStop(1, "#32ADE6");
    c.strokeStyle = grad; c.lineWidth = mini ? 2 : 2.6; c.shadowColor = "rgba(0,113,227,.35)"; c.shadowBlur = mini ? 4 : 8;
    c.beginPath(); vpPath(c, after); c.stroke();
    c.restore();
  }
}
// Le bruit n'est tracé que s'il monte dans le graphique : plus bas, il ne gêne pas ta voix.
function vpVisible(vals, lo) { return (vals || []).some(function (v) { return v != null && isFinite(v) && v >= lo + 1; }); }
function vpRange(profiles) {
  var lo = 0, hi = 0;
  Object.keys(profiles || {}).forEach(function (k) {
    var ch = profiles[k] && profiles[k].chart;
    if (!ch || !ch.bands) return;
    ["voice", "after", "target_lo", "target_hi"].forEach(function (n) {
      (ch[n] || []).forEach(function (v, i) { var f = ch.bands[i]; if (v == null || !isFinite(v) || f < 50 || f > 16000) return; lo = Math.min(lo, v); hi = Math.max(hi, v); });
    });
  });
  return {lo: clamp(Math.floor((lo - 3) / 6) * 6, -48, -18), hi: clamp(Math.ceil((hi + 2) / 6) * 6, 6, 18)};
}
function vpAnimateChart(cv, ch, o) {
  var reduce = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (reduce || typeof requestAnimationFrame === "undefined") { vpDrawChart(cv, ch, o); return; }
  var t0 = null;
  function step(t) {
    if (t0 === null) t0 = t;
    var p = clamp((t - t0) / 750, 0, 1), e = 1 - Math.pow(1 - p, 3);
    vpDrawChart(cv, ch, Object.assign({}, o || {}, {progress: e}));
    if (p < 1 && cv.isConnected) requestAnimationFrame(step);
  }
  requestAnimationFrame(step);
}

/* ---------- lecture des réglages sur mesure (GET /voice), sans surcharger le moteur ---------- */
var voiceInfo = {data: null, key: null, busy: null, last: 0, timer: null, again: false};
function voiceInfoKey() { var st = S.state, p = st.voice_print; return JSON.stringify([p ? p.t : null, st.mic.type, st.mic.model || null, st.mic.gain_db]); }
function voiceInfoLoad(force) {
  if (!micModelsReady()) return Promise.resolve(null);
  var key = voiceInfoKey();
  if (!S.state.voice_print) { voiceInfo.data = null; voiceInfo.key = key; return Promise.resolve(null); }
  if (!force && key === voiceInfo.key) return Promise.resolve(voiceInfo.data);
  if (voiceInfo.busy) { voiceInfo.again = true; return voiceInfo.busy; }
  var wait = voiceInfo.last + 1000 - Date.now();
  if (!force && wait > 0) {
    clearTimeout(voiceInfo.timer);
    voiceInfo.timer = setTimeout(function () { voiceInfoLoad(); }, wait);
    return Promise.resolve(voiceInfo.data);
  }
  voiceInfo.key = key; voiceInfo.last = Date.now();
  voiceInfo.busy = api("/api/modules/goxlr-mini/voice").then(function (d) { return d && typeof d === "object" ? d : null; }, function () { return null; })
    .then(function (d) {
      voiceInfo.data = d; voiceInfo.busy = null;
      if (built.mic && page === "mic") renderMicVoice();
      if (voiceInfo.again) { voiceInfo.again = false; voiceInfoLoad(); }
      return d;
    });
  return voiceInfo.busy;
}
function voiceValid() {
  var p = S && S.state && S.state.voice_print;
  if (!p) return false;
  var d = voiceInfo.data;
  if (d && d.print && voiceInfo.key === voiceInfoKey() && typeof d.valid === "boolean") return d.valid;
  // même règle que le moteur : même type, et même modèle quand les deux sont connus
  return p.mic_type === S.state.mic.type && (!p.model || !S.state.mic.model || p.model === S.state.mic.model);
}

/* ---------- capture de « Chat Mic » (appli Windows) ---------- */
var VP_WORKLET = [
  "class RustudioVpTap extends AudioWorkletProcessor {",
  "  constructor() { super(); this.buf = new Float32Array(2048); this.n = 0; this.first = 0; }",
  "  process(inputs) {",
  "    var ch = inputs[0];",
  "    if (!ch || !ch.length || !ch[0].length) return true;",
  "    var n = ch[0].length;",
  "    if (!this.first) this.first = (typeof Date !== 'undefined' ? Date.now() : 0) - n / sampleRate * 1000;",
  "    for (var k = 0; k < n; k++) {",
  "      var s = 0;",
  "      for (var c = 0; c < ch.length; c++) s += ch[c][k];",
  "      this.buf[this.n++] = s / ch.length;",
  "      if (this.n === this.buf.length) { this.port.postMessage({d: this.buf, first: this.first}, [this.buf.buffer]); this.buf = new Float32Array(2048); this.n = 0; }",
  "    }",
  "    return true;",
  "  }",
  "}",
  "registerProcessor('rustudio-vp-tap', RustudioVpTap);"
].join("\n");
function vpErr(code, msg) { var e = new Error(msg || VP_ERRORS[code] || code); e.code = code; return e; }
function vpNewContext() {
  var Ctx = typeof AudioContext !== "undefined" ? AudioContext : typeof webkitAudioContext !== "undefined" ? webkitAudioContext : null;
  if (!Ctx) return null;
  var ctx = null;
  try { ctx = new Ctx({sampleRate: 48000, latencyHint: "interactive"}); } catch (e) { try { ctx = new Ctx(); } catch (e2) { return null; } }
  if (ctx.resume) ctx.resume().catch(function () {});     // créé pendant le clic : le son peut démarrer tout de suite
  return ctx;
}
function vpCloseCtx(s) { if (s && s.ctx && !s.cap && s.ctx.close) { s.ctx.close().catch(function () {}); s.ctx = null; } }
// Identifiant de l'entrée « Chat Mic » de la GoXLR, sans jamais retomber sur un autre micro.
function vpFindChatMic() {
  var md = typeof navigator !== "undefined" ? navigator.mediaDevices : null;
  if (!md || !md.enumerateDevices || !md.getUserMedia) return Promise.resolve(null);
  return md.enumerateDevices().then(function (list) {
    if (list.some(function (d) { return d.label; })) return list;
    return md.getUserMedia({audio: true}).then(function (s0) { s0.getTracks().forEach(function (t) { t.stop(); }); return md.enumerateDevices(); });
  }).then(function (list) {
    var mics = list.filter(function (d) { return d.kind === "audioinput" && /chat mic/i.test(d.label || ""); });
    var real = mics.filter(function (d) { return d.deviceId !== "default" && d.deviceId !== "communications"; });
    var m = real[0] || mics[0];
    return m ? m.deviceId : null;
  });
}
function vpScriptTap(cap, ctx, src, sink, push) {
  var sp = ctx.createScriptProcessor(4096, 2, 1);
  sp.onaudioprocess = function (e) {
    var b = e.inputBuffer, n = b.length, nc = b.numberOfChannels, m = new Float32Array(n);
    for (var c = 0; c < nc; c++) { var x = b.getChannelData(c); for (var k = 0; k < n; k++) m[k] += x[k] / nc; }
    push(m, Date.now() - n / cap.rate * 1000);
  };
  src.connect(sp); sp.connect(sink); cap.node = sp;
}
// Enregistre « Chat Mic » en mono, sans aucun traitement du navigateur. Résout dès le premier son reçu.
function vpCapture(deviceId, ctx) {
  var cap = {chunks: [], frames: 0, rate: 48000, t0: null, stopped: false, ctx: ctx, onFirst: null};
  if (!ctx) return Promise.reject(vpErr("audio"));
  var md = navigator.mediaDevices;
  return md.getUserMedia({audio: {deviceId: {exact: deviceId}, echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 2}})
    .then(function (stream) {
      cap.stream = stream;
      cap.rate = ctx.sampleRate || 48000;
      var src = ctx.createMediaStreamSource(stream), sink = ctx.createGain();
      sink.gain.value = 0; sink.connect(ctx.destination);         // rien n'est joué, mais le traitement tourne
      cap.src = src; cap.sink = sink;
      var push = function (d, first) {
        if (cap.stopped || cap.frames > cap.rate * VP_MAX_CAPTURE_S) return;   // fenêtre cachée : la mémoire reste bornée
        if (cap.t0 === null) cap.t0 = (first > 0 ? first : Date.now() - d.length / cap.rate * 1000) - VP_LATENCY_MS;
        cap.chunks.push(d); cap.frames += d.length;
        if (cap.onFirst) { var f = cap.onFirst; cap.onFirst = null; f(); }
      };
      if (ctx.audioWorklet && typeof AudioWorkletNode !== "undefined" && typeof Blob !== "undefined") {
        var url = URL.createObjectURL(new Blob([VP_WORKLET], {type: "text/javascript"}));
        return ctx.audioWorklet.addModule(url).then(function () {
          URL.revokeObjectURL(url);
          var node = new AudioWorkletNode(ctx, "rustudio-vp-tap", {numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1]});
          node.port.onmessage = function (e) { push(e.data.d, e.data.first); };
          src.connect(node); node.connect(sink); cap.node = node;
        }, function () { URL.revokeObjectURL(url); vpScriptTap(cap, ctx, src, sink, push); });
      }
      vpScriptTap(cap, ctx, src, sink, push);
    })
    .then(function () { return ctx.resume ? ctx.resume() : null; })
    .then(function () {
      return new Promise(function (resolve, reject) {
        if (cap.frames) return resolve(cap);
        var timer = setTimeout(function () { cap.onFirst = null; reject(vpErr("silent")); }, 4000);
        cap.onFirst = function () { clearTimeout(timer); resolve(cap); };
      });
    })
    .catch(function (e) {
      vpStopCapture(cap);
      throw e && e.code ? e : vpErr(e && (e.name === "NotAllowedError" || e.name === "SecurityError") ? "denied" : "audio");
    });
}
function vpStopCapture(cap) {
  if (!cap || cap.stopped) return;
  cap.stopped = true; cap.onFirst = null;
  try { if (cap.node) { if (cap.node.port) cap.node.port.onmessage = null; if (cap.node.onaudioprocess !== undefined) cap.node.onaudioprocess = null; cap.node.disconnect(); } } catch (e) {}
  try { if (cap.src) cap.src.disconnect(); } catch (e) {}
  if (cap.stream) cap.stream.getTracks().forEach(function (t) { t.stop(); });
  if (cap.ctx && cap.ctx.close) cap.ctx.close().catch(function () {});
}
function vpRecording(cap) { return {samples: vpJoin(cap.chunks, cap.frames), rate: cap.rate, t0: cap.t0 == null ? Date.now() : cap.t0}; }

/* ---------- assistant : enregistrement guidé ---------- */
var vp = null;   // session d'empreinte en cours
function vpMode() { return inApp() ? "capture" : S && S.sim ? "synthetic" : null; }
function vpActive(s) { return !!s && !s.cancelled && (s.state === "starting" || s.state === "job" || s.state === "uploading"); }
function vpGone(s) { return s !== vp || s.cancelled; }
// Coupure qui empêche l'enregistrement : partout, ou de Discord (l'empreinte passe par « Chat Mic »).
// Une coupure du stream ou du casque reste telle quelle.
function vpMuteBlocks() { var ms = muteState("Mic"); return ms === "all" || ms === "chat" ? ms : null; }
function vpNoApp() {
  sheet({icon: "fingerprint", title: "Ouvre l\u2019appli Rustudio", text: "L\u2019empreinte vocale enregistre l\u2019entrée « Chat Mic » de ta GoXLR : elle se crée dans l\u2019appli Rustudio, sur le PC où la GoXLR est branchée. Ici, « Régler ma voix » cale déjà ton gain.",
    actions: [["Fermer", null], ["Régler ma voix", function () { voiceIntro(); }, "primary", true]]});
}
function voicePrintIntro() {
  if (!S || !S.connected) { toast("Branche la GoXLR pour créer ton empreinte"); return; }
  if (vpActive(vp)) return;
  var mode = vpMode();
  if (!mode) return vpNoApp();
  var m = micModelInfo(), muted = vpMuteBlocks();
  var body = h("div", {"class": "vp-intro"});
  body.innerHTML = '<div class="ring soft"><div class="ico">' + svg("fingerprint") + "</div></div><h2>Crée ton empreinte vocale</h2>" +
    "<p>Trente secondes en trois temps. Rustudio mesure ta voix et ta pièce, puis calcule chaque préréglage pour toi" + (m && !m.generic ? " et ton " + esc(micName(m)) : "") + ". Rien ne s\u2019active sans ton choix.</p>" +
    '<div class="vp-plan">' + [["volumeX", "Silence", VP_DUR.silence], ["audioLines", "Lecture", VP_DUR.reading], ["BroadcastMix", "Plus fort", VP_DUR.loud]].map(function (x, i) {
      return '<div><span class="n">' + (i + 1) + "</span>" + svg(x[0]) + "<b>" + x[1] + "</b><small>" + x[2] + " s</small></div>"; }).join("") + "</div>" +
    '<div class="vs-tips">' + [["ruler", "Place-toi comme en live, à " + micDistance(m) + " du micro, face à lui."], ["Headphones", "Garde ton casque et coupe ce qui fait du bruit : enceintes, ventilateur, musique."]]
      .map(function (t) { return "<div>" + svg(t[0]) + "<span>" + t[1] + "</span></div>"; }).join("") + "</div>";
  var notes = [];
  if (muted) notes.push(["micOff", (muted === "chat" ? "Ton micro est coupé de Discord" : "Ton micro est coupé") + " : Rustudio le rallume pour enregistrer ta voix."]);
  if (mode === "synthetic") notes.push(["info", "Appareil simulé : une voix de démonstration remplace l\u2019enregistrement."]);
  notes.forEach(function (n) { body.appendChild(h("div", {"class": "hp-note"}, svg(n[0]) + "<span>" + n[1] + "</span>")); });
  sheet({body: body, actions: [["Plus tard", null], [muted ? "Réactiver et commencer" : "Commencer", function () { vpBegin(); }, "primary", true]]});
}
function vpBegin() {
  if (vpActive(vp)) return;
  var mode = vpMode();
  if (!mode) return vpNoApp();
  var s = vp = {state: "starting", mode: mode, cap: null, ctx: null, job: null, result: null, upload: null};
  if (mode === "capture") s.ctx = vpNewContext();
  vpPaintJob(s, {running: true, phase: "prepare", progress: 0, step: "Je prépare l\u2019enregistrement…"});
  var unmute = vpMuteBlocks() ? postModule("goxlr-mini", "mute", {channel: "Mic", muted: false, target: "all"}).then(function (r) { if (!r) throw vpErr("muted"); return refresh(r); }) : Promise.resolve();
  unmute.then(function () { return mode === "capture" ? vpFindChatMic() : null; })
    .then(function (id) {
      if (vpGone(s)) return null;
      if (mode === "capture" && !id) {
        if (!S.sim) throw vpErr("no-chat-mic");
        s.mode = "synthetic"; vpCloseCtx(s);
        return null;
      }
      if (mode === "capture") return vpCapture(id, s.ctx).then(function (cap) { s.cap = cap; if (vpGone(s)) vpStopCapture(cap); });
      return null;
    })
    .then(function () {
      if (vpGone(s)) return null;
      return vpStart();
    })
    .then(function (r) {
      if (vpGone(s)) return;
      if (!r || !r.ok) throw vpErr("start");
      s.state = "job"; s.jobId = r.job;
      jobSeen = {kind: "voice-print", id: r.job};
    })
    .catch(function (e) {
      vpStopCapture(s.cap); vpCloseCtx(s);
      if (vpGone(s)) return;
      vpFail(s, e);
    });
}
// Lance l'enregistrement côté moteur en gardant la raison d'un refus (« Une analyse est déjà en cours », micro coupé…)
// pour la feuille d'erreur : un toast resterait caché sous elle.
function vpStart() {
  pending++;
  return fetch("/api/modules/goxlr-mini/voice-print", {method: "POST", headers: {"Content-Type": "application/json", "X-Rustudio": "1"},
    body: JSON.stringify({silence_s: VP_DUR.silence, reading_s: VP_DUR.reading, loud_s: VP_DUR.loud})})
    .then(function (r) {
      return r.json().catch(function () { return null; }).then(function (j) {
        pending--;
        if (!r.ok || !j || j.ok === false || j.error) { V = -1; throw vpErr("start", j && j.error ? String(j.error) : null); }
        return j;
      });
    }, function () { pending--; V = -1; throw vpErr("network", "Le moteur Rustudio ne répond pas : l\u2019enregistrement n\u2019a pas démarré."); });
}
// Appelé par showJob (menu.js) à chaque image tant que l'analyse est suivie.
function showVoicePrintJob(j) {
  var s = vp;
  if (!s || s.cancelled) return;
  s.job = j;
  if (j.running) { if (s.state === "job") vpPaintJob(s, j); return; }
  if (s.state !== "job") return;
  if (j.error) { vpStopCapture(s.cap); vpFail(s, vpErr("job", j.error)); return; }
  s.result = j.result || {};
  vpUpload(s);
}
function vpUpload(s) {
  s.state = "uploading";
  vpPaintJob(s, {running: true, phase: "analyse", step: s.cap ? "J\u2019envoie ton enregistrement au moteur…" : "Voix de démonstration…"});
  var settle = s.cap ? new Promise(function (res) { setTimeout(res, 300); }) : Promise.resolve();   // la fin arrive avec un léger retard
  return settle.then(function () {
    if (vpGone(s)) return null;
    var path = "/api/modules/goxlr-mini/voice-audio";
    if (s.cap) {
      vpStopCapture(s.cap);
      var rec = vpRecording(s.cap);
      if (rec.samples.length < VP_MIN_FRAMES) throw vpErr("empty");
      var cut = vpTrim(rec.samples, rec.rate, rec.t0, s.result && s.result.timeline);
      s.upload = {path: path + "?rate=" + Math.round(rec.rate) + "&t0=" + Math.round(cut.t0) + "&format=s16", body: vpToInt16(cut.samples)};
    } else s.upload = {path: path + "?synthetic=1", body: null};
    return vpSend(s);
  }).catch(function (e) { if (!vpGone(s)) vpFail(s, e); });
}
function vpSend(s) {
  s.state = "uploading";
  vpPaintJob(s, {running: true, phase: "analyse", step: "Je calcule tes profils sur mesure…"});
  pending++;
  var opts = {method: "POST", headers: {"Content-Type": "application/octet-stream", "X-Rustudio": "1"}};
  if (s.upload.body) opts.body = s.upload.body;
  return fetch(s.upload.path, opts)
    .then(function (r) {
      return r.json().catch(function () { return null; }).then(function (j) {
        if (!r.ok || !j || j.error || j.ok === false) throw vpErr("analyse", j && j.error ? String(j.error) : null);
        return j;
      });
    }, function () { throw vpErr("network"); })
    .then(function (j) {
      pending--;
      if (vpGone(s)) return;
      s.state = "done";
      // même contenu que GET /voice : empreinte, profils, résumé et conseils d'installation
      var data = {print: j.print || null, profiles: j.profiles || {}, summary: j.summary || null, notes: Array.isArray(j.notes) ? j.notes : [],
        valid: j.valid !== false, mic: j.mic || S.state.mic};
      voiceInfo.data = data;
      return refresh().then(function () {
        voiceInfo.key = voiceInfoKey();
        if (!vpGone(s)) vpShowResults(data, null, s.result);
      });
    }, function (e) { pending--; V = -1; throw e; });
}
function vpFail(s, e) {
  s.state = "error";
  var code = (e && e.code) || "audio", msg = (e && e.message) || VP_ERRORS.audio;
  var canResend = !!s.upload && (code === "network" || code === "analyse");
  var actions = [["Fermer", null]];
  if (canResend) actions.push(["Renvoyer l\u2019enregistrement", function () { s.cancelled = false; vp = s; vpSend(s).catch(function (x) { if (!vpGone(s)) vpFail(s, x); }); }, "", true]);
  if (code === "no-chat-mic" || code === "denied" || code === "silent" || code === "audio") actions.push(["Régler ma voix", function () { voiceIntro(); }, "", true]);
  actions.push(["Recommencer", function () { voicePrintIntro(); }, "primary", true]);
  sheet({icon: "warn", title: code === "job" ? "Empreinte interrompue" : "Empreinte impossible", text: esc(msg), vpFail: true, actions: actions});
}
function vpCancel(fromClose) {
  var s = vp;
  if (!s || s.cancelled) return;
  s.cancelled = true;
  vpStopCapture(s.cap); vpCloseCtx(s);
  if (s.state === "job") postModule("goxlr-mini", "job-cancel");
  s.state = "cancelled";
  if (!fromClose && sheetState && sheetState.vpJob) closeSheet();
}
// Ligne d'état du moteur : utile pendant la préparation, l'analyse ou une reprise ; masquée quand elle redit le titre.
var VP_SAME_AS_TITLE = {silence: /^silence/i, reading: /^lis (le|ce) texte/i, loud: /^plus fort/i};
function vpStatusText(ph, step, retry) {
  step = String(step || "").trim();
  if (!step || (+retry > 0 && ph === "reading")) return step;
  return VP_SAME_AS_TITLE[ph] && VP_SAME_AS_TITLE[ph].test(step) ? "" : step;
}
function vpHelp(ph) {
  if (ph === "reading") return "À ton volume habituel, à " + micDistance(micModelInfo()) + " du micro. Suis le surlignage sans te presser.";
  return VP_PHASE[ph][2];
}
function vpJobUi(s) {
  if (sheetState && sheetState.vpJob && sheetState.session === s) return sheetState.ui;
  var ui = {}, body = h("div", {"class": "vp-job"});
  ui.steps = h("div", {"class": "vs-steps"});
  ui.stepEls = VP_STEPS.map(function (x, i) { var e = h("span", {}, "<i>" + (i + 1) + "</i>" + x[1]); ui.steps.appendChild(e); return e; });
  ui.ring = h("div", {"class": "ring"}, '<svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="44" fill="none" stroke="rgba(0,0,0,.08)" stroke-width="7"/><circle class="vp-ring-p" cx="50" cy="50" r="44" fill="none" stroke="#0071E3" stroke-width="7" stroke-linecap="round" stroke-dasharray="276.5" stroke-dashoffset="276.5" style="transition:stroke-dashoffset .2s"/></svg>');
  ui.ico = h("div", {"class": "ico"}); ui.ring.appendChild(ui.ico);
  ui.title = h("h2", {"aria-live": "polite"}); ui.help = h("p");   // chaque étape est annoncée aux lecteurs d'écran
  ui.wordInfo = vpWords(VP_TEXT);
  ui.read = h("div", {"class": "vp-read", "aria-label": "Texte à lire"}, ui.wordInfo.map(function (x) { return '<span class="w">' + esc(x.w) + "</span>"; }).join(" "));
  ui.wordEls = ui.read.querySelectorAll ? ui.read.querySelectorAll(".w") : [];
  ui.loud = h("div", {"class": "vp-loud"}, "« " + esc(VP_LOUD) + " »");
  ui.meter = h("div", {"class": "sheet-meter"}, '<div class="meter" id="sheet-meter">' + new Array(35).join("<i></i>") + "</div>");
  ui.status = h("div", {"class": "vs-status", role: "status", "aria-live": "polite"});
  [ui.steps, ui.ring, ui.title, ui.help, ui.read, ui.loud, ui.meter, ui.status].forEach(function (e) { body.appendChild(e); });
  sheet({wide: true, job: true, vpJob: true, session: s, ui: ui, body: body, actions: [["Annuler", function () { vpCancel(); }, "", true]],
    onClose: function () { if (vp === s && vpActive(s)) vpCancel(true); }});
  return ui;
}
function vpPaintJob(s, j) {
  if (vpGone(s)) return;
  var ui = vpJobUi(s), ph = VP_PHASE[j.phase] ? j.phase : "prepare";
  var idx = VP_STEPS.map(function (x) { return x[0]; }).indexOf(ph);
  ui.stepEls.forEach(function (e, i) {
    var st = idx < 0 ? "" : i < idx ? "done" : i === idx ? "on" : "";
    if (e._st === st) return;
    e._st = st; e.className = st;
    e.innerHTML = "<i>" + (st === "done" ? svg("check") : i + 1) + "</i>" + VP_STEPS[i][1];
  });
  var dur = VP_DUR[ph] || 0, end = +j.phase_end || 0;
  var left = dur && end ? clamp(end - Date.now() / 1000, 0, dur) : null;
  var spin = left === null;
  ui.ring.classList.toggle("vp-spin", spin);
  var rp = ui.ring.querySelector ? ui.ring.querySelector(".vp-ring-p") : null;
  if (rp) { var off = spin ? "200" : (276.5 * left / dur).toFixed(1); if (rp._o !== off) { rp._o = off; rp.setAttribute("stroke-dashoffset", off); } }
  var ico = spin ? svg(VP_PHASE[ph][0]) : '<span class="num"><b>' + Math.max(1, Math.ceil(left)) + "<small>s</small></b></span>";
  if (ui.ico._h !== ico) { ui.ico._h = ico; ui.ico.innerHTML = ico; }
  if (ui.ph !== ph) { ui.ph = ph; ui.title.textContent = VP_PHASE[ph][1]; ui.help.textContent = vpHelp(ph); }
  ui.read.classList.toggle("on", ph === "reading");
  ui.loud.classList.toggle("on", ph === "loud");
  ui.meter.style.display = ph === "analyse" ? "none" : "";
  if (ph === "reading") vpPaintWords(ui, left === null ? 0 : (dur - left) / dur);
  var step = vpStatusText(ph, j.step, j.retry), warn = (+j.retry || 0) > 0 && ph === "reading";
  if (ui.status.textContent !== step) ui.status.textContent = step;
  ui.status.classList.toggle("warn", warn);
}
function vpPaintWords(ui, f) {
  var els = ui.wordEls;
  if (!els || !els.length) return;
  var i = vpWordAt(ui.wordInfo, f);
  if (ui.wordIdx === i) return;
  ui.wordIdx = i;
  for (var k = 0; k < els.length; k++) els[k].className = "w" + (k < i ? " done" : k === i ? " now" : "");
}

/* ---------- résultats : analyse, profils sur mesure, écoute ---------- */
function vpProfileOrder(profiles) {
  var order = Object.keys((S.meta && S.meta.mic_presets) || {}).filter(function (k) { return profiles[k]; });
  Object.keys(profiles).forEach(function (k) { if (order.indexOf(k) < 0) order.push(k); });
  return order;
}
function vpApplied(r) { return r.applied && r.applied !== "brut" ? r.applied : null; }
function vpShowResults(data, previewKey, jobResult) {
  if (!data || !data.profiles || !Object.keys(data.profiles).length) { toast("L\u2019analyse de ta voix n\u2019est pas disponible"); return; }
  var st = S.state, profiles = data.profiles, order = vpProfileOrder(profiles);
  var applied = fxAny() && profiles[st.mic_preset] ? st.mic_preset : null;
  var rec = S.meta.mic_type_preset ? S.meta.mic_type_preset[st.mic.type] : null;
  var pick = [previewKey, applied, rec].concat(order).filter(function (k) { return k && profiles[k] && k !== "brut"; })[0] || order[0];
  var res = jobResult || null, r = {data: data, res: res, order: order, preview: pick, applied: applied, busy: false, range: vpRange(profiles)};
  var sm = vpSummary(data), m = micChosen();
  var body = h("div", {"class": "vp-res"});
  var sub = [m ? micName(m) : VP_TYPE_LABELS[st.mic.type] || "", "gain " + st.mic.gain_db + " dB"];
  if (res && res.old_gain_db != null && res.gain_db != null && res.old_gain_db !== res.gain_db) sub[1] = "gain réglé de " + res.old_gain_db + " à " + res.gain_db + " dB";
  if (sm.snr != null && isFinite(sm.snr)) sub.push(Math.round(sm.snr) + " dB au-dessus du bruit de ta pièce");
  body.innerHTML = '<div class="diag-head">' + chipIco("fingerprint", "#5E5CE6") + "<div><h2>Ton empreinte vocale</h2><p class=\"muted-txt\">" + esc(sub.filter(Boolean).join(" · ")) + "</p></div></div>";
  // En tête, ce qui fausse la mesure (saturation, ronflement, pièce) ; les conseils d'installation viennent en bas.
  var notes = (Array.isArray(data.notes) ? data.notes : []).filter(function (n) { return n && n.t; });
  var alerts = notes.filter(function (n) { return n.lvl === "warn"; }).map(function (n) { return n.t; });
  ((res && res.notes) || []).forEach(function (n) { if (n && n.t && n.lvl === "warn") alerts.push(n.t); });
  (sm.warnings || []).forEach(function (w) { if (VP_WARN[w]) alerts.push(VP_WARN[w]); });
  var seen = {}, sat = false;
  alerts.filter(function (t) {
    var x = /satur/i.test(t);
    if (seen[t] || (x && sat)) return false;
    seen[t] = true; sat = sat || x;
    return true;
  }).slice(0, 3).forEach(function (t) { body.appendChild(h("div", {"class": "advice vp-warn"}, chipIco("warn", "#FF9F0A") + "<div><p>" + esc(t) + "</p></div>")); });
  var tips = notes.filter(function (n) { return n.lvl !== "warn"; }).slice(0, 4);
  var top = h("div", {"class": "vp-top"});
  var box = h("div", {"class": "vp-chart-box"}), plot = h("div", {"class": "vp-plot"});
  r.canvas = h("canvas", {"class": "vp-chart", role: "img"});
  plot.appendChild(r.canvas); box.appendChild(plot);
  r.legend = h("div", {"class": "vp-legend"});
  box.appendChild(r.legend);
  top.appendChild(box);
  var stats = h("div", {"class": "vp-stats"});
  vpFacts(sm).slice(0, 5).forEach(function (x) {
    stats.appendChild(h("div", {"class": x.warn ? "warn" : "", title: x.t}, "<small>" + svg(x.icon) + esc(x.k) + "</small><b>" + esc(x.v) + "</b>"));
  });
  top.appendChild(stats);
  body.appendChild(top);
  // Choisir un profil le montre (courbe, réglages, raisons) ; seul « Appliquer » ou l'écoute le met en place.
  var sec = h("div", {"class": "vp-sec"}, '<h4 id="vp-profs-t">' + svg("sparkles") + "Tes profils sur mesure</h4>");
  var grid = h("div", {"class": "vp-profs", role: "radiogroup", "aria-labelledby": "vp-profs-t"});
  r.cards = {};
  order.forEach(function (k) {
    var p = profiles[k], b = h("button", {"class": "vp-prof", type: "button", role: "radio"}, chipIco(PRESET_ICONS[k] || "sparkles", PRESET_COLOURS[k] || "#0071E3") + "<span><b>" + esc(p.label || k) + "</b><small></small></span>");
    b.addEventListener("click", function () { if (r.preview !== k) { r.preview = k; r.hover = null; vpPaintRes(r); } });
    b.addEventListener("keydown", function (e) { vpProfKey(r, k, e); });
    b.addEventListener("pointerenter", function () { if (r.hover !== k) { r.hover = k; vpPaintChart(r); } });
    b.addEventListener("pointerleave", function () { if (r.hover) { r.hover = null; vpPaintChart(r); } });
    grid.appendChild(b); r.cards[k] = b;
  });
  sec.appendChild(grid);
  r.note = h("div", {"class": "hp-note vp-preview"});
  sec.appendChild(r.note);
  body.appendChild(sec);
  r.what = h("div", {"class": "vp-sec"});
  body.appendChild(r.what);
  if (tips.length) {
    var ts = h("div", {"class": "vp-sec"}, "<h4>" + svg("bulb") + "Pour ton installation</h4>");
    tips.forEach(function (n) {
      var ok = n.lvl === "ok";
      ts.appendChild(h("div", {"class": "advice"}, chipIco(ok ? "ok" : "bulb", ok ? "#30D158" : "#64D2FF") + "<div><p>" + esc(n.t) + "</p></div>"));
    });
    body.appendChild(ts);
  }
  sheet({wide: true, body: body, vpRes: r, actions: [
    ["Écouter et comparer", function () { vpCompare(r); }, "", true],
    ["Appliquer", function () { if (r.preview && r.preview !== r.applied) vpApply(r, r.preview); else closeSheet(); }, "primary", true]]});
  var acts = $("sheet-box") && $("sheet-box").querySelectorAll ? $("sheet-box").querySelectorAll(".sheet-actions .pill") : [];
  r.primary = acts.length ? acts[acts.length - 1] : null;
  vpPaintRes(r, true);
}
// Flèches entre les profils, comme un groupe de boutons radio.
function vpProfKey(r, k, e) {
  var i = r.order.indexOf(k), n = r.order.length, j = -1;
  if (e.key === "ArrowRight" || e.key === "ArrowDown") j = (i + 1) % n;
  else if (e.key === "ArrowLeft" || e.key === "ArrowUp") j = (i - 1 + n) % n;
  else if (e.key === "Home") j = 0;
  else if (e.key === "End") j = n - 1;
  if (j < 0) return;
  e.preventDefault();
  var nk = r.order[j];
  r.preview = nk; r.hover = null; vpPaintRes(r);
  if (r.cards[nk].focus) r.cards[nk].focus();
}
function vpPaintRes(r, first) {
  var profiles = r.data.profiles, p = profiles[r.preview] || {}, applied = r.applied;
  Object.keys(r.cards).forEach(function (k) {
    var b = r.cards[k], on = k === applied, prev = k === r.preview;
    b.classList.toggle("on", on); b.classList.toggle("prev", prev);
    b.setAttribute("aria-checked", prev ? "true" : "false");
    b.setAttribute("tabindex", prev ? "0" : "-1");
    var sm = b.querySelector ? b.querySelector("small") : null;
    if (sm) sm.textContent = on ? "Appliqué" : k === "brut" ? "Sans traitement" : "Sur mesure";
  });
  var label = p.label || r.preview, cur = applied && profiles[applied] ? profiles[applied].label || applied : null;
  // La note reste à sa place (pas de saut de la feuille) : aperçu tant que rien n'est appliqué, confirmation ensuite.
  var done = r.preview === applied;
  var note = done ? "« " + esc(label) + " » est appliqué : compare-le à ta voix brute avec « Écouter et comparer »." :
    "Aperçu de « " + esc(label) + " » : " + (cur ? "« " + esc(cur) + " » reste en place tant que tu n\u2019appliques pas." :
    fxAny() ? "ton réglage personnalisé reste en place tant que tu n\u2019appliques pas." : "rien ne change tant que tu n\u2019appliques pas. Ta voix reste naturelle.");
  r.note.classList.toggle("ok", done);
  r.note.innerHTML = svg(done ? "ok" : "info") + "<span>" + note + "</span>";
  var rows = vpSettings(p), why = (p.notes || []).filter(function (n) { return n && n.t; }).slice(0, 12);
  r.what.innerHTML = "<h4>" + svg("wand") + (r.preview === applied ? "Ce que Rustudio a réglé" : "Ce que Rustudio réglera") + " avec « " + esc(label) + " »</h4>";
  if (rows.length) r.what.appendChild(h("div", {"class": "vp-set"}, rows.map(function (x) { return "<div>" + svg(x.icon) + "<b>" + esc(x.t) + "</b><span>" + esc(x.v) + "</span></div>"; }).join("")));
  if (!rows.length && r.preview === "brut") r.what.appendChild(h("p", {"class": "muted-txt vp-none"}, "Aucun traitement : ta voix telle que ton micro la capte."));
  if (why.length) {
    r.what.appendChild(h("p", {"class": "vp-why-t"}, "Pourquoi"));
    r.what.appendChild(h("ul", {"class": "vp-why"}, why.map(function (n) {
      var lv = n.lvl === "warn" ? "warn" : n.lvl === "tip" ? "tip" : "ok";
      return '<li class="' + lv + '">' + svg(lv === "warn" ? "warn" : lv === "tip" ? "bulb" : "check") + "<span>" + esc(n.t) + "</span></li>";
    }).join("")));
  }
  if (r.primary) {
    var t = r.preview !== applied ? "Appliquer « " + label + " »" : "Terminé";
    if (r.primary.textContent !== t) r.primary.textContent = t;
    r.primary.disabled = !!r.busy;
  }
  vpPaintChart(r, first);
}
function vpPaintChart(r, animate) {
  var k = r.hover || r.preview, p = r.data.profiles[k] || {}, ch = p.chart;
  if (r.legend) r.legend.innerHTML = '<span><i class="lg-voice"></i>Ta voix</span><span><i class="lg-after"></i>Avec « ' + esc(p.label || k) + " »</span>" +
    (ch && ch.target_lo ? '<span><i class="lg-zone"></i>Zone visée</span>' : "") + (ch && vpVisible(ch.noise, r.range.lo) ? '<span><i class="lg-noise"></i>Bruit de ta pièce</span>' : "");
  if (r.canvas) r.canvas.setAttribute("aria-label", "Spectre de ta voix, avant et après le profil « " + (p.label || k) + " »");
  if (!ch || !r.canvas) return;
  if (typeof requestAnimationFrame === "undefined") return;
  if (animate) requestAnimationFrame(function () { vpAnimateChart(r.canvas, ch, r.range); });
  else vpDrawChart(r.canvas, ch, r.range);
}
function vpApply(r, k) {
  if (r.busy) return Promise.resolve(false);
  r.busy = true;
  if (r.primary) r.primary.disabled = true;
  return postModule("goxlr-mini", "mic-preset", {name: k}).then(function (x) {
    if (!x) return false;
    r.applied = k;
    return refresh(x).then(function () { return true; });
  }).then(function (ok) {
    r.busy = false;
    if (sheetState && sheetState.vpRes === r) vpPaintRes(r);
    return ok;
  });
}
// Compare le profil montré : appliqué d'abord s'il ne l'est pas, puis écoute avec ou sans traitement.
function vpCompare(r) {
  var k = r.preview && r.preview !== "brut" ? r.preview : vpApplied(r);
  if (!k) { toast("Choisis d\u2019abord un profil à comparer"); return; }
  (r.applied === k ? Promise.resolve(true) : vpApply(r, k)).then(function (ok) { if (ok) vpListenSheet(r.data, k, r.res); });
}
function vpListenSheet(data, key, jobResult) {
  var p = (data.profiles || {})[key] || {}, label = p.label || key;
  var o = {key: key, label: label, bypass: false, seen: false, timer: null, ended: false};
  var body = h("div", {"class": "vp-listen"});
  o.ring = h("div", {"class": "ring"}, '<svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="44" fill="none" stroke="rgba(0,0,0,.08)" stroke-width="7"/><circle class="vp-ring-p" cx="50" cy="50" r="44" fill="none" stroke="#0071E3" stroke-width="7" stroke-linecap="round" stroke-dasharray="276.5" stroke-dashoffset="0" style="transition:stroke-dashoffset .2s"/></svg>');
  o.ico = h("div", {"class": "ico"}, svg("ear")); o.ring.appendChild(o.ico);
  body.appendChild(o.ring);
  body.appendChild(h("h2", {}, "Écoute et compare"));
  body.appendChild(h("p", {}, "Parle normalement : tu t\u2019entends en direct dans ton casque. Bascule pour entendre ce que change « " + esc(label) + " »."));
  var seg = h("div", {"class": "seg vp-ab", role: "group", "aria-label": "Comparer avant et après"});
  o.raw = h("button", {type: "button", "aria-pressed": "false"}, svg("circle") + "Sans traitement");
  o.fit = h("button", {type: "button", "aria-pressed": "true", "class": "on"}, svg("sparkles") + "Avec « " + esc(label) + " »");
  o.raw.addEventListener("click", function () { vpBypass(o, true); });
  o.fit.addEventListener("click", function () { vpBypass(o, false); });
  seg.appendChild(o.raw); seg.appendChild(o.fit); body.appendChild(seg);
  o.hint = h("p", {"class": "vp-ab-hint"}, "");
  body.appendChild(o.hint);
  vpListenPaint(o);
  o.again = h("button", {"class": "pill", type: "button", style: "display:none;margin:14px auto 0"}, svg("refresh") + "Relancer l\u2019écoute");
  o.again.addEventListener("click", function () { vpListenStart(o); });
  body.appendChild(o.again);
  sheet({body: body, vpListen: o, actions: [["Retour à l\u2019analyse", function () { vpShowResults(data, key, jobResult); }, "", true], ["Terminé", null, "primary"]],
    onClose: function () { vpListenStop(o); }});
  vpListenStart(o);
  o.timer = setInterval(function () { if (!sheetState || sheetState.vpListen !== o) { clearInterval(o.timer); return; } vpListenTick(o); }, 200);
}
function vpListenStart(o) {
  o.seen = false; o.ended = false; o.again.style.display = "none";
  return postModule("goxlr-mini", "mic-listen", {seconds: 30, level: typeof LISTEN_LEVEL !== "undefined" ? LISTEN_LEVEL : 191}).then(function (r) { if (r) o.started = true; return r; });
}
function vpBypass(o, on) {
  if (o.bypass === on) return Promise.resolve(true);
  return postModule("goxlr-mini", "mic-ab", {bypass: on}).then(function (r) {
    if (!r) return false;
    o.bypass = on; vpListenPaint(o);
    return true;
  });
}
function vpListenPaint(o) {
  o.raw.classList.toggle("on", o.bypass); o.fit.classList.toggle("on", !o.bypass);
  o.raw.setAttribute("aria-pressed", o.bypass ? "true" : "false"); o.fit.setAttribute("aria-pressed", o.bypass ? "false" : "true");
  o.hint.textContent = o.bypass ? "Ta voix brute, sans aucun traitement." : "Ta voix avec « " + o.label + " », comme l\u2019entendent tes amis.";
}
function vpListenTick(o) {
  var left = L && L.listening > 0 ? L.listening : 0, ab = !!(L && L.ab);
  if (left) o.seen = true;
  if (o.seen && !left && !o.ended) { o.ended = true; o.again.style.display = ""; if (o.bypass) vpBypass(o, false); }
  if (o.bypass && !ab && o.seen && L && L.ab !== undefined) { o.bypass = false; vpListenPaint(o); }   // fin du contournement côté moteur
  var rp = o.ring.querySelector ? o.ring.querySelector(".vp-ring-p") : null;
  if (rp) rp.setAttribute("stroke-dashoffset", (276.5 * (1 - Math.min(1, left / 30))).toFixed(1));
  var ico = left ? '<span class="num"><b>' + Math.ceil(left) + "<small>s</small></b></span>" : svg("ear");
  if (o.ico._h !== ico) { o.ico._h = ico; o.ico.innerHTML = ico; }
}
function vpListenStop(o) {
  clearInterval(o.timer);
  if (o.stopped) return;
  o.stopped = true;
  if (o.bypass) postModule("goxlr-mini", "mic-ab", {bypass: false});
  if (o.started || (L && L.listening > 0)) postModule("goxlr-mini", "mic-listen", {seconds: 0});
}
function vpOpenResults() {
  var show = function (d) { if (d && d.profiles) vpShowResults(d); else toast("L\u2019analyse de ta voix n\u2019est pas disponible pour l\u2019instant"); };
  if (voiceInfo.data && voiceInfo.data.profiles && voiceInfo.key === voiceInfoKey()) show(voiceInfo.data);
  else voiceInfoLoad(true).then(show);
}
function vpForget() {
  // Le moteur recalcule aussitôt le préréglage en cours : s'il est allumé, ta voix change, il faut le dire.
  var st = S.state, cur = voiceValid() && fxAny() && st.mic_preset !== "custom" && S.meta.mic_presets[st.mic_preset] ? S.meta.mic_presets[st.mic_preset].label : null;
  var text = cur ? "Les préréglages reprennent leurs réglages de base, « " + esc(cur) + " » compris : ta voix change tout de suite. Tu pourras refaire une empreinte quand tu veux."
    : "Les préréglages reprennent leurs réglages de base. Ton réglage actuel ne change pas, et tu pourras refaire une empreinte quand tu veux.";
  sheet({icon: "fingerprint", title: "Oublier ton empreinte ?", text: text,
    actions: [["Annuler", null], ["Oublier", function () {
      postModule("goxlr-mini", "voice-forget", {}).then(function (r) { if (r) { voiceInfo.data = null; toast("Empreinte oubliée"); } return refresh(r); });
    }, "danger"]]});
}

/* ---------- page Micro : carte « Ton micro » ---------- */
function buildMicModelCard() {
  var c = h("div", {"class": "card mm-card"});
  c.innerHTML = "<h3>" + svg("Mic", ' style="width:16px;height:16px;color:#FF453A"') + "Ton micro</h3>";
  var sel = h("select", {"class": "inp mm-select", "aria-label": "Modèle de ton micro"});
  // Au clavier, chaque flèche change la liste : on attend la fin du choix avant de toucher au 48 V et au gain.
  sel.addEventListener("change", function () {
    var mm = mic.model; mm.preview = sel.value; mm.key = null; renderMicModel();
    clearTimeout(mm.timer);
    mm.timer = setTimeout(function () { var k = mm.preview; micModelSet(k || null).then(function () { if (mm.preview === k) { mm.preview = null; mm.key = null; renderMicModel(); } }); }, 450);
  });
  c.appendChild(sel);
  var specs = h("div", {"class": "mm-specs"}), tips = h("div", {"class": "mm-tips"});
  c.appendChild(specs); c.appendChild(tips);
  mic.model = {card: c, sel: sel, specs: specs, tips: tips, cat: "", key: null, preview: null, timer: null};
  return c;
}
function micModelSet(key) {
  var models = S.meta.mic_models || {}, m = key ? models[key] : null, before = S.state.mic.type;
  return postModule("goxlr-mini", "mic-model", {model: key}).then(function (r) {
    if (!r) { if (mic.model) mic.model.key = null; return refresh(null).then(function () { renderMicModel(); return null; }); }
    if (m) {
      var g = r.mic && r.mic.gain_db != null ? r.mic.gain_db : null;
      toast(micName(m) + (m.phantom && before !== "condenser" ? " : alimentation 48 V activée" : "") + (g != null ? (m.phantom && before !== "condenser" ? ", gain " : " : gain de départ ") + g + " dB" : ""));
    }
    return refresh(r);
  });
}
function renderMicModel() {
  var mm = mic.model;
  if (!mm || !micModelsReady()) return;
  var models = S.meta.mic_models, keys = Object.keys(models), cat = keys.join("|");
  if (mm.cat !== cat) {
    mm.cat = cat; mm.sel.innerHTML = "";
    mm.sel.appendChild(h("option", {value: "", disabled: "disabled"}, "Choisis ton micro…"));
    var brands = {}, generic = [];
    keys.forEach(function (k) { var m = models[k]; if (m.generic) generic.push(k); else (brands[m.brand || "Autres"] = brands[m.brand || "Autres"] || []).push(k); });
    Object.keys(brands).sort(function (a, b) { return a.localeCompare(b, "fr"); }).forEach(function (b) {
      var g = h("optgroup", {label: b});
      brands[b].sort(function (x, y) { return micName(models[x]).localeCompare(micName(models[y]), "fr"); })
        .forEach(function (k) { g.appendChild(h("option", {value: k}, esc(micName(models[k])))); });
      mm.sel.appendChild(g);
    });
    if (generic.length) {
      var gg = h("optgroup", {label: "Autre micro"});
      generic.forEach(function (k) { gg.appendChild(h("option", {value: k}, esc(models[k].name))); });
      mm.sel.appendChild(gg);
    }
  }
  // Rien de choisi : la liste invite à choisir, sans supposer un modèle à ta place.
  var st = S.state, key = mm.preview != null && models[mm.preview] ? mm.preview : micChosen() ? st.mic.model : "", m = key ? models[key] : null;
  if (mm.sel.value !== key) mm.sel.value = key;
  mm.sel.classList.toggle("empty", !key);
  var sig = JSON.stringify([key, st.mic.type]);
  if (mm.key === sig) return;
  mm.key = sig;
  var specs = m ? vpSpecs(m) : [(VP_TYPE_LABELS[st.mic.type] || st.mic.type) + (st.mic.type === "condenser" ? " · 48 V" : "")];
  var src = m && /^https:\/\//.test(m.source || "") ? m.source : null;
  mm.specs.innerHTML = specs.map(function (t, i) { return "<span" + (i === 0 ? ' class="kind"' : "") + ">" + esc(t) + "</span>"; }).join("") +
    (src ? '<button type="button" class="mm-src" title="Ouvrir la fiche officielle d\u2019où viennent ces chiffres">' + svg("externalLink") + "Fiche " + esc(m.brand || "officielle") + "</button>" : "");
  var link = src && mm.specs.querySelector ? mm.specs.querySelector(".mm-src") : null;
  if (link) link.addEventListener("click", function () { openUrl(src); });
  var tips = (m && m.tips) || [];
  mm.tips.innerHTML = tips.length ? tips.map(function (t, i) { return "<div>" + svg(i === 0 ? "ruler" : "bulb") + "<span>" + esc(t) + "</span></div>"; }).join("")
    : '<div class="mm-empty">' + svg("info") + "<span>Choisis ton modèle : le gain de départ, l\u2019alimentation et tes préréglages s\u2019adaptent à lui.</span></div>";
}

/* ---------- page Micro : carte « Empreinte vocale » ---------- */
function buildVoiceCard() {
  var c = h("div", {"class": "card vp-card"});
  c.innerHTML = "<h3>" + svg("fingerprint", ' style="width:16px;height:16px;color:#5E5CE6"') + 'Empreinte vocale<small class="vp-when"></small></h3>';
  var body = h("div", {"class": "vp-body"});
  c.appendChild(body);
  mic.vp = {card: c, body: body, when: c.querySelector ? c.querySelector(".vp-when") : null, key: null, canvas: null};
  return c;
}
function renderVoiceCard() {
  var v = mic.vp;
  if (!v || !S) return;
  var st = S.state, p = st.voice_print, d = voiceInfo.data, valid = voiceValid();
  // Courbe « après » seulement pour le profil vraiment appliqué : sinon ta voix seule, telle qu'elle sort.
  var applied = fxAny() && d && d.profiles && d.profiles[st.mic_preset] ? st.mic_preset : null, prof = applied ? d.profiles[applied] : null;
  var key = JSON.stringify([p ? p.t : null, valid, d ? d.summary : null, prof ? prof.chart : null, applied, st.mic.type, st.mic.model || null, vpMode()]);
  if (v.key === key) return;
  v.key = key; v.body.innerHTML = ""; v.canvas = null;
  if (v.when) v.when.textContent = p && p.t ? "Mesurée le " + new Date(p.t * 1000).toLocaleDateString("fr-FR") : "";
  if (!p) {
    v.body.appendChild(h("p", {"class": "vp-lead"}, "Trente secondes pour que Rustudio connaisse ta voix : un silence, un court texte, une phrase plus forte. Il mesure ton timbre, tes « s », tes « p » et le bruit de ta pièce, puis calcule chaque préréglage pour ta voix et ton micro."));
    v.body.appendChild(h("div", {"class": "vp-plan"}, [["volumeX", "Silence", VP_DUR.silence], ["audioLines", "Lecture", VP_DUR.reading], ["BroadcastMix", "Plus fort", VP_DUR.loud]].map(function (x, i) {
      return '<div><span class="n">' + (i + 1) + "</span>" + svg(x[0]) + "<b>" + x[1] + "</b><small>" + x[2] + " s</small></div>"; }).join("")));
    v.body.appendChild(h("button", {"class": "pill primary vp-cta", type: "button", onclick: voicePrintIntro}, svg("fingerprint") + "Créer mon empreinte (30 s)"));
    return;
  }
  if (!valid) {
    var made = p.model && S.meta.mic_models[p.model] ? micName(S.meta.mic_models[p.model]) : VP_TYPE_LABELS[p.mic_type] || "un autre micro";
    var now = micChosen();
    v.body.appendChild(h("div", {"class": "hp-note warn"}, svg("warn") + "<span>Mesurée avec " + esc(made) + ". Refais-la avec " + esc(now && !now.generic ? "ton " + micName(now) : "ton micro actuel") + " pour des préréglages justes.</span>"));
    var row = h("div", {"class": "btn-row"});
    row.appendChild(h("button", {"class": "pill primary", type: "button", onclick: voicePrintIntro}, svg("refresh") + "Refaire mon empreinte"));
    row.appendChild(h("button", {"class": "pill", type: "button", onclick: vpForget}, svg("trash") + "Oublier"));
    v.body.appendChild(row);
    return;
  }
  var sum = h("div", {"class": "vp-sum"}), fig = h("div", {"class": "vp-fig"});
  v.canvas = h("canvas", {"class": "vp-mini", role: "img", "aria-label": prof ? "Spectre de ta voix, avant et avec « " + (prof.label || applied) + " »" : "Spectre de ta voix"});
  fig.appendChild(v.canvas);
  fig.appendChild(h("div", {"class": "vp-legend vp-mini-cap"}, '<span><i class="lg-voice"></i>Ta voix</span>' + (prof ? '<span><i class="lg-after"></i>' + esc(prof.label || applied) + "</span>" : "")));
  sum.appendChild(fig);
  sum.appendChild(h("div", {"class": "vp-facts"}, vpFacts(vpSummary(d)).slice(0, 5).map(function (x) {
    return '<div class="vp-fact' + (x.warn ? " warn" : "") + '">' + svg(x.icon) + "<span>" + esc(x.t) + "</span></div>"; }).join("")));
  v.body.appendChild(sum);
  var acts = h("div", {"class": "btn-row"});
  acts.appendChild(h("button", {"class": "pill primary", type: "button", onclick: vpOpenResults}, svg("activity") + "Voir l\u2019analyse"));
  acts.appendChild(h("button", {"class": "pill", type: "button", onclick: voicePrintIntro}, svg("refresh") + "Refaire"));
  acts.appendChild(h("button", {"class": "pill vp-forget", type: "button", title: "Oublier mon empreinte", "aria-label": "Oublier mon empreinte", onclick: vpForget}, svg("trash")));
  v.body.appendChild(acts);
  var any = d && d.profiles ? Object.keys(d.profiles).map(function (k) { return d.profiles[k].chart; }).filter(Boolean)[0] : null;
  v.chart = prof && prof.chart ? prof.chart : any ? {bands: any.bands, voice: any.voice, noise: any.noise} : vpPrintChart(p);
  var rg = v.chart ? vpRange({x: {chart: v.chart}}) : null;
  if (rg && typeof requestAnimationFrame !== "undefined") requestAnimationFrame(function () { vpDrawChart(v.canvas, v.chart, {mini: true, lo: rg.lo, hi: rg.hi}); });
}
function vpPrintChart(p) {
  if (!p || !p.bands || !p.voice_db) return null;
  var mid = vpMid(p.bands, p.voice_db);
  return {bands: p.bands, voice: vpRel(p.bands, p.voice_db, mid), noise: p.noise_db ? vpRel(p.bands, p.noise_db, mid) : null};
}
// Badges « Sur mesure » des préréglages, carte de l'empreinte, et lecture des réglages calculés.
function renderMicVoice() {
  if (!built.mic || !micModelsReady()) return;
  renderMicModel();
  renderVoiceCard();
  var fit = voiceValid();
  Object.keys(mic.presetBtns || {}).forEach(function (k) {
    var b = mic.presetBtns[k], tg = b.querySelector ? b.querySelector(".fit-tag") : null, want = fit && k !== "brut";
    if (want && !tg) b.appendChild(h("span", {"class": "fit-tag"}, svg("sparkles") + "Sur mesure"));
    if (!want && tg) tg.remove();
  });
  var hint = $("preset-hint");
  if (hint && fit && S.state.mic_preset !== "custom") hint.innerHTML = svg("sparkles", ' class="fit-ico"') + "Calculés sur ta voix";
}

/* ---------- égaliseur du micro : fréquence de chaque bande ---------- */
// Le curseur parcourt les fréquences permises par la Mini ; les libellés montrent où la bande agit vraiment.
function openEqFreq(band, anchor) {
  var range = S.meta.eq_ranges && S.meta.eq_ranges[band];
  if (!range || !S.state.dsp.eq_freq) return;
  var lo = +range[0], hi = +range[1], def = clamp(+band, lo, hi);
  function toPos(f) { return Math.round(1000 * Math.log(clamp(f, lo, hi) / lo) / Math.log(hi / lo)); }
  function toF(p) { return roundFreq(lo * Math.pow(hi / lo, p / 1000), lo, hi); }
  var shown = freqLabel;
  var box = h("div", {"class": "freq-pop"});
  var top = h("div", {"class": "field-top"}, "Fréquence de la bande");
  var val = h("span"); top.appendChild(val); box.appendChild(top);
  // Une touche qui ne bouge rien n'envoie rien : sinon le préréglage en cours deviendrait « personnalisé » pour rien.
  var sent = micEqFreq(band, S.state);
  function commit(f) { if (f === sent) return; sent = f; var x = {eq_freq: {}}; x.eq_freq[band] = f; postModule("goxlr-mini", "dsp", x).then(refresh); }
  var send = throttle(function (f) { var p = {eq_freq: {}}; p.eq_freq[band] = f; sent = f; postModule("goxlr-mini", "dsp", p); }, 120);
  function set(f) { S.state.dsp.eq_freq[band] = f; val.textContent = shown(f); renderEqFreqs(); drawEq(); }
  var sl = Slider({label: "Fréquence de la bande " + (S.meta.eq_bands.indexOf(band) + 1), min: 0, max: 1000, step: 10, value: toPos(micEqFreq(band, S.state)), colour: "var(--accent)",
    format: function (p) { return shown(toF(p)); },
    onInput: function (p) { var f = toF(p); set(f); send(f); },
    onCommit: function (p) { commit(toF(p)); }});
  box.appendChild(sl.el);
  box.addEventListener("keydown", function (e) { if (e.key === "Escape") { closePop(); if (anchor.focus) anchor.focus({preventScroll: true}); } });
  var foot = h("div", {"class": "freq-foot"}, "<small>" + shown(lo) + " – " + shown(hi) + "</small>");
  var reset = h("button", {"class": "pill", type: "button"}, "Par défaut · " + shown(def));
  reset.addEventListener("click", function () { sl.set(toPos(def)); set(def); commit(def); });
  foot.appendChild(reset); box.appendChild(foot);
  val.textContent = shown(micEqFreq(band, S.state));
  mic.freqBand = band;
  var r = anchor.getBoundingClientRect ? anchor.getBoundingClientRect() : null;
  openPop(anchor, box, r && r.bottom > innerHeight * 0.6 ? "up-left" : undefined);
  drawEq();
  setTimeout(function () { if (sl.el.isConnected) sl.el.focus({preventScroll: true}); }, 30);
}
function renderEqFreqs() {
  if (!mic.eq) return;
  var st = S.state, ok = !!(S.meta.eq_ranges && st.dsp.eq_freq);
  S.meta.eq_bands.forEach(function (b, i) {
    var e = mic.eq[b];
    if (!e || !e.freq) return;
    var f = micEqFreq(b, st), t = freqLabel(f);
    if (e.freq.textContent !== t) e.freq.textContent = t;
    e.freq.classList.toggle("moved", Math.abs(f - +b) > 0.5);
    e.freq.disabled = !ok;
    e.freq.title = ok ? "Changer la fréquence de cette bande" : "";
    e.freq.setAttribute("aria-label", "Bande " + (i + 1) + " : " + t + (ok ? ". Changer la fréquence" : ""));
    e.sl.el.setAttribute("aria-label", "Égaliseur micro " + t);
  });
}
