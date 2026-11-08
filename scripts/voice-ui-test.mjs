// Empreinte vocale, carte « Ton micro » et fréquences de l'égaliseur du micro, avec les fonctions servies par le moteur.
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {runInNewContext} from "node:vm";
import {source as html} from "./ui-source.mjs";

function section(start, end) {
  const a = html.indexOf(start), b = html.indexOf(end, a);
  assert.ok(a >= 0 && b > a, "section présente : " + start);
  return html.slice(a, b);
}
const empreinte = readFileSync(new URL("../ui/js/goxlr/empreinte.js", import.meta.url), "utf8");
assert.ok(html.includes(empreinte), "le fichier testé est celui que charge l’interface");
const plain = v => JSON.parse(JSON.stringify(v));
const settle = async (n = 6) => { for (let i = 0; i < n; i++) await new Promise(r => setImmediate(r)); };

class El {
  constructor(tag) {
    this.tagName = String(tag).toUpperCase(); this.children = []; this.attrs = {}; this.events = {}; this.style = {};
    this.disabled = false; this.textContent = ""; this.title = ""; this._html = "";
    const cls = new Set();
    this.classList = {
      add: c => cls.add(c), remove: c => cls.delete(c), contains: c => cls.has(c),
      toggle: (c, on) => { if (on === undefined) on = !cls.has(c); if (on) cls.add(c); else cls.delete(c); return on; },
    };
  }
  setAttribute(k, v) { this.attrs[k] = String(v); if (k === "class") String(v).split(/\s+/).filter(Boolean).forEach(c => this.classList.add(c)); }
  getAttribute(k) { return this.attrs[k] ?? null; }
  appendChild(c) { this.children.push(c); return c; }
  remove() {}
  set innerHTML(v) { this._html = String(v); this.children = []; }
  get innerHTML() { return this._html; }
  set className(v) { this.attrs.class = v; }
  addEventListener(n, f) { (this.events[n] ||= []).push(f); }
  click() { for (const f of this.events.click || []) f({target: this}); }
  querySelector() { return null; }
  querySelectorAll() { return []; }
  focus() {}
  get isConnected() { return true; }
  getBoundingClientRect() { return {left: 0, right: 100, top: 0, bottom: 20, width: 100, height: 20}; }
}

const models = {    // extrait du catalogue du moteur (crates/core/src/mics.rs), mêmes champs
  "rode-nt1a": {brand: "RØDE", name: "NT1-A", type: "condenser", phantom: true, sensitivity_mv_pa: 25, self_noise_dba: 5, max_spl_db: 137, pattern: "cardioïde", distance_cm: [10, 20], start_gain_db: 30, tips: ["Parle face au point doré, à 10–20 cm."], bright: true, generic: false, source: "https://rode.com/en-us/products/nt1a"},
  "shure-sm7b": {brand: "Shure", name: "SM7B", type: "dynamic", phantom: false, sensitivity_mv_pa: 1.12, self_noise_dba: null, max_spl_db: 180, pattern: "cardioïde", distance_cm: [3, 10], start_gain_db: 57, tips: [], bright: false, generic: false, source: "https://pubs.shure.com/view/guide/SM7B/en-US.pdf"},
  "generic-condenser": {brand: "", name: "Autre micro statique (48 V)", type: "condenser", phantom: true, sensitivity_mv_pa: null, self_noise_dba: null, max_spl_db: null, pattern: "", distance_cm: [10, 20], start_gain_db: 30, tips: [], bright: false, generic: true, source: null},
  "generic-dynamic": {brand: "", name: "Autre micro dynamique", type: "dynamic", phantom: false, sensitivity_mv_pa: null, self_noise_dba: null, max_spl_db: null, pattern: "", distance_cm: [5, 15], start_gain_db: 50, tips: [], bright: false, generic: true, source: null},
};
// Modèle mesuré de l'égaliseur de la Mini, au format de S.meta.eq_model (crates/core/src/voicefit.rs, MINI_EQ)
const EQ_MODEL = {"90": {kind: "lowshelf", q: 0.94}, "250": {kind: "peak", q: 1.38}, "500": {kind: "peak", q: 1.32}, "1000": {kind: "peak", q: 1.48}, "3000": {kind: "peak", q: 1.35}, "8000": {kind: "highshelf", q: 0.96}};
const bandsKeys = ["90", "250", "500", "1000", "3000", "8000"];

function harness(opts = {}) {
  const ids = new Map(), posts = [], events = [], toasts = [], sheets = [], fetches = [], timers = [], tracks = [], sliders = [];
  let now = 0, gum = 0;
  const doc = {createElement: t => new El(t), getElementById: id => ids.get(id) || null};
  const nodes = [];
  class FakeCtx {
    constructor() { this.sampleRate = 48000; this.closed = false; this.destination = {}; this.audioWorklet = {addModule: async () => { events.push("worklet"); }}; }
    resume() { return Promise.resolve(); }
    close() { this.closed = true; events.push("ctx-close"); return Promise.resolve(); }
    createMediaStreamSource() { return {connect() {}, disconnect() {}}; }
    createGain() { return {gain: {value: 1}, connect() {}, disconnect() {}}; }
    createScriptProcessor() { throw new Error("pas utilisé"); }
  }
  class FakeNode { constructor() { this.port = {onmessage: null}; nodes.push(this); } connect() {} disconnect() {} }
  const responses = Object.assign({"voice-print": {ok: true, job: 77}, "job-cancel": {ok: true}, "mic-listen": {ok: true}, "mic-ab": {ok: true}, mute: {ok: true}, "mic-preset": {ok: true}, "mic-model": {ok: true, mic: {type: "condenser", model: "rode-nt1a", gain_db: 30}}, dsp: {ok: true}}, opts.responses || {});
  const S = {sim: !!opts.sim, connected: true, v: 3,
    meta: {mic_models: models, mic_presets: {brut: {label: "Naturel"}, naturel: {label: "Léger"}, studio: {label: "Studio"}, radio: {label: "Broadcast"}},
      mic_type_preset: {condenser: "studio", dynamic: "radio", jack: "stream"}, eq_bands: bandsKeys,
      eq_ranges: {"90": [30, 90], "250": [100, 300], "500": [310, 800], "1000": [800, 2500], "3000": [2600, 5000], "8000": [5100, 18000]},
      eq_model: JSON.parse(JSON.stringify(EQ_MODEL)), ratios: [1, 1.1, 1.2, 1.4, 1.6, 1.8, 2, 2.5, 3.2, 4, 5.6, 8, 16, 32, 64]},
    state: {mic: {type: "condenser", gain_db: 30, model: "rode-nt1a"}, mutes: {Mic: !!opts.muted}, partial: {Mic: opts.partial || null}, fx: {eq: false, gate: false, comp: false, deesser: false}, mic_preset: "brut",
      dsp: {eq: {"90": 0, "250": 0, "500": 0, "1000": 0, "3000": 0, "8000": 0}, eq_freq: {"90": 90, "250": 250, "500": 500, "1000": 1000, "3000": 3000, "8000": 8000}, gate: {}, comp: {}, deesser: 0}, voice_print: null}};
  const devices = opts.devices || [{kind: "audioinput", label: "Micro (webcam)", deviceId: "cam"}, {kind: "audioinput", label: "Par défaut - Chat Mic (GoXLR Mini)", deviceId: "default"}, {kind: "audioinput", label: "Chat Mic (GoXLR Mini)", deviceId: "chat"}];
  const c = {
    document: doc, ICONS: {}, S, L: null, V: 3, pending: 0, built: {}, page: "mic", mic: {}, jobSeen: null, sheetState: null, popEl: null, LISTEN_LEVEL: 191, innerHeight: 720,
    Date: {now: () => (opts.epoch || 1.8e12) + now}, Math, JSON, Promise, Object, Array, String, Number, isFinite, Float32Array, ArrayBuffer, DataView, Uint8Array, Int16Array, Error,
    navigator: {mediaDevices: {
      enumerateDevices: async () => devices,
      getUserMedia: async constraints => { gum++; events.push("getUserMedia"); c.lastConstraints = constraints; const t = {stop() { t.stopped = true; events.push("track-stop"); }}; tracks.push(t); return {getTracks: () => [t]}; },
    }},
    AudioContext: FakeCtx, AudioWorkletNode: FakeNode, Blob: class { constructor(p, o) { this.parts = p; this.type = o && o.type; } },
    URL: {createObjectURL: () => "blob:vp", revokeObjectURL() {}},
    setTimeout: (fn, ms = 0) => { const id = timers.length + 1; timers.push({id, at: now + ms, fn}); return id; },
    clearTimeout: id => { const t = timers.find(x => x.id === id); if (t) t.cancelled = true; },
    setInterval: () => 0, clearInterval() {},
    inApp: () => !!opts.app,
    $: id => ids.get(id) || null,
    svg: n => "<svg data-i=\"" + n + "\"></svg>",
    chipIco: n => "<span class=\"chip-ico\">" + n + "</span>",
    esc: s => String(s).replace(/[&<>"]/g, ch => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;"})[ch]),
    clamp: (v, a, b) => Math.max(a, Math.min(b, v)),
    toast: (m, a) => toasts.push({m, a}),
    sheet: o => { if (c.sheetState && c.sheetState !== o && c.sheetState.onClose) c.sheetState.onClose(); c.sheetState = o; sheets.push(o); },
    closeSheet: () => { const o = c.sheetState; c.sheetState = null; if (o && o.onClose) o.onClose(); },
    postModule: async (id, action, body) => { posts.push([action, body]); events.push("post:" + action); return responses[action] === undefined ? {ok: true} : responses[action]; },
    refresh: async r => (r === null ? null : (events.push("refresh"), S)),
    api: async path => { events.push("get:" + path); return opts.voiceData || null; },
    fetch: async (path, o) => {
      if (String(path).endsWith("/voice-print")) {     // lancement de l'enregistrement (réponse du moteur gardée pour l'erreur)
        posts.push(["voice-print", JSON.parse(o.body)]); events.push("post:voice-print");
        if (opts.startFails) throw new Error("hors ligne");
        const rep = responses["voice-print"];
        return {ok: !!rep, status: rep ? 200 : 409, json: async () => rep || {error: "Une analyse est déjà en cours."}};
      }
      fetches.push({path, o}); events.push("fetch");
      if (opts.fetchFails) throw new Error("hors ligne");
      const reply = opts.fetchReply || {ok: true, print: {t: 1, mic_type: "condenser", model: "rode-nt1a"}, profiles: {studio: {label: "Studio", dsp: {eq: {}, gate: {}, comp: {}}, fx: {}, notes: [], chart: null}}, summary: {voice_type: "grave", f0_hz: 118}};
      return {ok: !opts.httpError, status: opts.httpError ? 500 : 200, json: async () => reply};
    },
    muteState: ch => S.state.mutes[ch] ? "all" : (S.state.partial[ch] || null),
    fxAny: () => Object.keys(S.state.fx).some(k => S.state.fx[k]),
    voiceIntro: () => events.push("voiceIntro"),
    throttle: fn => fn, openPop() {}, closePop() {}, drawEq() {}, renderMic() {},
    // curseur minimal : le test pousse une valeur (input) ou relâche (commit), comme la souris ou le clavier
    Slider: o => { let value = o.value; const api = {el: new El("div"), o, set(v) { value = v; }, get value() { return value; },
      input(v) { if (v !== value) { value = v; if (o.onInput) o.onInput(v); } }, commit() { if (o.onCommit) o.onCommit(value); }}; sliders.push(api); return api; },
    MIC_TYPE_LABELS: {dynamic: "Dynamique", condenser: "Statique +48 V", jack: "Jack 3,5 mm"},
    PRESET_ICONS: {}, PRESET_COLOURS: {},
  };
  runInNewContext(section("function h(tag,", "function chipIco("), c);
  runInNewContext(empreinte, c);
  return {
    c, S, posts, events, toasts, sheets, fetches, tracks, nodes, ids, sliders, gum: () => gum,
    async advance(ms) {
      now += ms;
      for (;;) {
        const due = timers.filter(t => !t.cancelled && !t.done && t.at <= now).sort((a, b) => a.at - b.at)[0];
        if (!due) break;
        due.done = true; due.fn(); await settle();
      }
      await settle();
    },
    // Envoie des échantillons au faux AudioWorklet, comme le ferait la capture de « Chat Mic ».
    audio(seconds, first) {
      const node = nodes.at(-1);
      assert.ok(node && node.port.onmessage, "le traitement audio écoute « Chat Mic »");
      const n = Math.round(seconds * 48000 / 2048);
      for (let i = 0; i < n; i++) { const d = new Float32Array(2048); d.fill(i % 2 ? 0.25 : -0.25); node.port.onmessage({data: {d, first}}); }
      return n * 2048;
    },
  };
}

/* ---------- outils sans interface ---------- */
{
  const {c} = harness();
  for (const [f, t] of [[90, "90 Hz"], [160, "160 Hz"], [999.6, "1 kHz"], [1000, "1 kHz"], [1500, "1,5 kHz"], [4500, "4,5 kHz"], [10800, "10,8 kHz"], [11000, "11 kHz"], [18000, "18 kHz"], [0, "—"], [NaN, "—"]]) {
    assert.equal(c.freqLabel(f), t, "libellé de " + f);
  }
  assert.equal(c.roundFreq(10834, 5100, 18000), 10800);
  assert.equal(c.roundFreq(87.4, 30, 90), 87);
  assert.equal(c.roundFreq(17990, 5100, 18000), 18000, "borne haute respectée");
  assert.equal(c.roundFreq(512), 510);

  const pcm = new DataView(c.vpToInt16([0, 1, -1, 0.5, -0.5, 2, -2, NaN]));
  assert.deepEqual([0, 1, 2, 3, 4, 5, 6, 7].map(i => pcm.getInt16(i * 2, true)), [0, 32767, -32768, 16384, -16384, 32767, -32768, 0], "PCM 16 bits saturé proprement");
  assert.deepEqual([pcm.getUint8(2), pcm.getUint8(3)], [0xff, 0x7f], "petit-boutiste");

  const x = new Float32Array(48000 * 40), T0 = 1.8e12;
  const tl = [{phase: "silence", start: T0 / 1000 + 4, end: T0 / 1000 + 9}, {phase: "reading", start: T0 / 1000 + 9, end: T0 / 1000 + 25}, {phase: "loud", start: T0 / 1000 + 25, end: T0 / 1000 + 30}];
  const cut = c.vpTrim(x, 48000, T0, tl);
  assert.ok(Math.abs(cut.samples.length - (31.5 - 2.5) * 48000) <= 2, "coupé à 1,5 s des étapes");
  assert.ok(Math.abs(cut.t0 - (T0 + 2500)) < 0.1, "t0 suit la coupe");
  assert.equal(c.vpTrim(x, 48000, T0, null).samples.length, x.length, "sans étapes : tout");
  assert.equal(c.vpTrim(x, 48000, T0, [{start: T0 / 1000 + 900, end: T0 / 1000 + 905}]).samples.length, x.length, "horloges en désaccord : tout plutôt que rien");
  assert.equal(c.vpTrim(new Float32Array(5e6), 48000, T0, null).samples.length, 3750000, "jamais plus de 7,5 Mo envoyés");

  const words = c.vpWords(c.VP_TEXT);
  assert.equal(words.length, 43);
  assert.equal(words[2].w, "moi\u00a0!", "la ponctuation reste avec son mot");
  assert.equal(c.vpWordAt(words, 0), 0);
  assert.equal(c.vpWordAt(words, 0.9), words.length, "le guide finit avant la fin de l’étape");
  let prev = 0;
  for (let f = 0; f <= 1; f += 0.01) { const i = c.vpWordAt(words, f); assert.ok(i >= prev); prev = i; }

  const at = (k, f0, g, q, f) => c.eqDb(c.eqCoefs(k, f0, g, q), f);
  assert.ok(Math.abs(at("peak", 3000, 6, 1.1, 3000) - 6) < 0.05, "cloche : gain au centre");
  assert.ok(Math.abs(at("peak", 3000, 6, 1.1, 300)) < 0.2, "cloche : neutre loin du centre");
  assert.ok(Math.abs(at("lowshelf", 75, -4, 0.7, 20) + 4) < 0.4 && Math.abs(at("lowshelf", 75, -4, 0.7, 2000)) < 0.1, "plateau grave");
  assert.ok(Math.abs(at("highshelf", 10500, -3, 0.7, 20000) + 3) < 0.6 && Math.abs(at("highshelf", 10500, -3, 0.7, 1000)) < 0.1, "plateau aigu");

  // bandes de la Mini telles que mesurées : plateau grave du second ordre, cloches à la fréquence réglée, plateau aigu
  const band = (b, f, g) => c.eqBand(c.micEqModel(b), f, g);
  const ls = band("90", 90, -9);
  assert.ok(Math.abs(ls(90) + 4.5) < 0.05, "plateau grave à −9 dB réglé à 90 Hz : mi-gain au coude");
  assert.ok(Math.abs(ls(45) + 9.41) < 0.05, "plateau grave à −9 dB réglé à 90 Hz : plein effet une octave dessous");
  assert.ok(Math.abs(ls(4000)) < 0.01, "plateau grave neutre dans les aigus");
  assert.ok(Math.abs(band("90", 90, 9)(20) - 9.18) < 0.05, "une remontée suit la même formule");
  const bell = band("250", 160, 6);
  let topHz = 60;
  for (let f = 60; f <= 400; f += 0.5) if (bell(f) > bell(topHz)) topHz = f;
  assert.ok(Math.abs(topHz - 160) <= 0.5 && Math.abs(bell(160) - 6) < 0.05, "cloche demandée à 160 Hz : sommet à 160 Hz (" + topHz + ")");
  const hs = band("8000", 8000, -6);
  assert.ok(Math.abs(hs(8000) + 3) < 0.1 && Math.abs(hs(18000) + 6) < 0.5 && Math.abs(hs(1000)) < 0.1, "plateau aigu en forme Q");
  const all = c.micEqResponse({dsp: {eq: {"90": -9, "250": 6, "500": 0, "1000": 0, "3000": 0, "8000": 0}, eq_freq: {"90": 90, "250": 160}}});
  assert.ok(Math.abs(all(130) - bell(130) - ls(130)) < 1e-9, "courbe totale : somme des bandes réglées");
  c.S.meta.eq_model = {"90": {kind: "inconnu", q: -1}};
  assert.equal(JSON.stringify(c.micEqModel("90")), JSON.stringify({kind: "peak", q: 1}), "modèle illisible : cloche, Q 1");
  c.S.meta.eq_model = JSON.parse(JSON.stringify(EQ_MODEL));

  assert.deepEqual(Array.from(c.vpSpecs(models["rode-nt1a"])), ["Statique · 48 V", "25 mV/Pa", "Bruit propre 5 dB(A)", "Cardioïde", "137 dB SPL max"]);
  assert.equal(c.vpSpecs(models["shure-sm7b"])[1], "1,12 mV/Pa");
  assert.equal(c.vpSpecs({type: "condenser", sensitivity_mv_pa: 20})[1], "20 mV/Pa", "aucun zéro perdu");
  assert.equal(c.vpSpecs({type: "dynamic", sensitivity_mv_pa: 1.6})[1], "1,6 mV/Pa");
  assert.equal(c.micName(models["rode-nt1a"]), "RØDE NT1-A");
  assert.equal(c.micName(models["generic-condenser"]), "Autre micro statique (48 V)");

  const facts = c.vpFacts(c.vpSummary({print: {f0_hz: 118}, summary: {voice_type: "grave", f0_hz: 118, snr_db: 41.6, sibilance: "marquées", plosives: 2, hum_hz: 50, room: "sèche"}}));
  assert.deepEqual(Array.from(facts, f => f.v), ["Grave · 118 Hz", "Marquées", "2 plosives", "42 dB sous ta voix", "Sèche", "50 Hz"]);
  assert.ok(facts[1].warn && facts[5].warn && !facts[0].warn);
  const set = c.vpSettings({fx: {eq: true, gate: true, comp: true, deesser: false}, dsp: {eq: {"90": -4, "250": 0, "8000": -3}, eq_freq: {"90": 75, "8000": 10500}, gate: {threshold: -50}, comp: {ratio: 8, threshold: -27, makeup: 5}, deesser: 40}});
  assert.equal(set[0].v, "\u22124\u00a0dB\u00a0à\u00a075\u00a0Hz · \u22123\u00a0dB\u00a0à\u00a010,5\u00a0kHz", "égaliseur en clair, sans coupure au milieu d’une bande");
  const low = c.vpSettings({fx: {eq: true}, dsp: {eq: {"250": -2, "500": -3}, eq_freq: {"250": 100, "500": 491}}});
  assert.equal(low[0].v, "\u22122\u00a0dB\u00a0à\u00a0100\u00a0Hz · \u22123\u00a0dB\u00a0à\u00a0491\u00a0Hz", "réglages des profils : fréquence réglée, comme les explications du moteur");
  assert.equal(set[2].v, "3,2:1 dès \u221227 dB, compensation +5 dB");
  assert.equal(set.length, 3, "un traitement coupé n’est pas annoncé");
}

/* ---------- pas de capture sans l'appli ---------- */
{
  const t = harness({app: false, sim: false});
  t.c.voicePrintIntro();
  assert.match(t.c.sheetState.title, /Ouvre l’appli Rustudio/);
  assert.equal(t.c.sheetState.actions.at(-1)[0], "Régler ma voix", "l’ancien réglage reste proposé");
  t.c.vpBegin(); await settle();
  assert.equal(t.gum(), 0, "aucune capture hors de l’appli");
  assert.ok(!t.posts.some(p => p[0] === "voice-print"), "aucun enregistrement lancé");
}

/* ---------- ordre : capture, puis analyse, puis envoi ---------- */
{
  const T0 = 1.8e12, t = harness({app: true, epoch: T0});
  t.c.vpBegin();
  await settle(10);
  assert.equal(t.gum(), 1);
  assert.equal(t.c.lastConstraints.audio.deviceId.exact, "chat", "« Chat Mic » réel, pas l’entrée par défaut");
  for (const k of ["echoCancellation", "noiseSuppression", "autoGainControl"]) assert.equal(t.c.lastConstraints.audio[k], false, "aucun traitement du navigateur : " + k);
  assert.ok(!t.posts.some(p => p[0] === "voice-print"), "l’analyse attend le premier son capturé");
  const frames = t.audio(30, T0 + 4);
  await settle(10);
  const iPost = t.events.indexOf("post:voice-print"), iGum = t.events.indexOf("getUserMedia");
  assert.ok(iGum >= 0 && iPost > iGum, "capture avant l’analyse");
  assert.deepEqual(plain(t.posts.find(p => p[0] === "voice-print")[1]), {silence_s: 5, reading_s: 16, loud_s: 5});
  assert.equal(t.c.jobSeen.kind, "voice-print");
  assert.ok(t.c.sheetState && t.c.sheetState.vpJob, "les étapes s’affichent");
  t.c.showVoicePrintJob({kind: "voice-print", running: true, phase: "reading", phase_end: T0 / 1000 + 20, progress: .5, step: "", retry: 0});
  assert.equal(t.fetches.length, 0);
  const tl = [{phase: "silence", start: T0 / 1000 + 4, end: T0 / 1000 + 9, gain_db: 30}, {phase: "reading", start: T0 / 1000 + 9, end: T0 / 1000 + 25, gain_db: 30}, {phase: "loud", start: T0 / 1000 + 25, end: T0 / 1000 + 30, gain_db: 30}];
  t.c.showVoicePrintJob({kind: "voice-print", running: false, progress: 1, result: {gain_db: 30, old_gain_db: 43, timeline: tl, audio: "wanted"}});
  t.c.showVoicePrintJob({kind: "voice-print", running: false, progress: 1, result: {gain_db: 30, old_gain_db: 43, timeline: tl, audio: "wanted"}});
  await t.advance(300);
  assert.equal(t.fetches.length, 1, "un seul envoi, même si la fin est vue deux fois");
  const f = t.fetches[0], q = new URLSearchParams(f.path.split("?")[1]);
  assert.ok(f.path.startsWith("/api/modules/goxlr-mini/voice-audio?"));
  assert.equal(q.get("rate"), "48000"); assert.equal(q.get("format"), "s16");
  assert.ok(Math.abs(+q.get("t0") - (T0 + 2500)) <= 1, "t0 : premier échantillon envoyé, retard de capture compris");
  assert.equal(f.o.method, "POST"); assert.equal(f.o.headers["X-Rustudio"], "1"); assert.equal(f.o.headers["Content-Type"], "application/octet-stream");
  assert.ok(f.o.body instanceof ArrayBuffer && f.o.body.byteLength % 2 === 0 && f.o.body.byteLength < frames * 2, "PCM 16 bits, coupé autour des étapes");
  assert.ok(t.tracks.every(x => x.stopped), "micro libéré avant l’envoi");
  assert.ok(t.events.indexOf("fetch") > t.events.indexOf("track-stop"));
  await settle(10);
  assert.ok(t.c.sheetState && t.c.sheetState.vpRes, "résultats affichés après l’analyse");
  assert.equal(t.c.pending, 0);
}

/* ---------- simulateur : voix synthétique, sans capture ---------- */
{
  const t = harness({app: false, sim: true});
  t.c.vpBegin(); await settle(10);
  assert.equal(t.gum(), 0, "aucun micro ouvert en simulation hors appli");
  assert.ok(t.posts.some(p => p[0] === "voice-print"));
  t.c.showVoicePrintJob({kind: "voice-print", running: false, result: {timeline: [], audio: "wanted"}});
  await t.advance(10); await settle(10);
  assert.equal(t.fetches[0].path, "/api/modules/goxlr-mini/voice-audio?synthetic=1");
  assert.equal(t.fetches[0].o.body, undefined, "sans corps");
  assert.ok(t.c.sheetState.vpRes);
}
{
  const t = harness({app: true, sim: true, devices: [{kind: "audioinput", label: "Micro du Mac", deviceId: "mac"}]});
  t.c.vpBegin(); await settle(10);
  assert.equal(t.gum(), 0, "simulateur sans « Chat Mic » : jamais un autre micro");
  assert.ok(t.posts.some(p => p[0] === "voice-print"));
}

/* ---------- annulation ---------- */
{
  const T0 = 1.8e12, t = harness({app: true, epoch: T0});
  t.c.vpBegin(); await settle(10); t.audio(2, T0); await settle(10);
  const job = t.c.sheetState;
  job.actions[0][1]();
  await settle();
  assert.ok(t.posts.some(p => p[0] === "job-cancel"), "l’analyse du moteur est arrêtée");
  assert.ok(t.tracks.every(x => x.stopped), "le micro est libéré");
  assert.equal(t.c.sheetState, null);
  t.c.showVoicePrintJob({kind: "voice-print", running: false, error: "Analyse annulée."});
  await t.advance(400);
  assert.equal(t.fetches.length, 0, "rien n’est envoyé après une annulation");
  assert.equal(t.c.sheetState, null, "aucune erreur affichée pour une annulation voulue");
}

/* ---------- erreurs sans faux succès ---------- */
{
  const t = harness({app: true, responses: {"voice-print": null}});
  t.c.vpBegin(); await settle(10); t.audio(1, 1.8e12); await settle(10);
  assert.equal(t.c.sheetState.title, "Empreinte impossible");
  assert.equal(t.c.sheetState.text, "Une analyse est déjà en cours.", "la raison du moteur, pas un message générique");
  assert.equal(t.c.pending, 0);
  assert.equal(t.c.V, -1, "l’état sera relu");
  assert.ok(t.tracks.every(x => x.stopped), "refus du moteur : micro libéré");
  assert.equal(t.fetches.length, 0);
}
{
  const t = harness({app: false, sim: true, startFails: true});
  t.c.vpBegin(); await settle(10);
  assert.equal(t.c.sheetState.title, "Empreinte impossible");
  assert.match(t.c.sheetState.text, /ne répond pas/, "moteur injoignable : dit tel quel");
  assert.equal(t.c.pending, 0);
}
{
  const t = harness({app: true, sim: false, devices: [{kind: "audioinput", label: "Micro (webcam)", deviceId: "cam"}]});
  t.c.vpBegin(); await settle(10);
  assert.equal(t.gum(), 0, "pas de « Chat Mic » : aucun autre micro n’est enregistré");
  assert.match(t.c.sheetState.text, /Chat Mic/);
  assert.ok(t.c.sheetState.actions.some(a => a[0] === "Régler ma voix"));
}
for (const kind of ["http", "réseau"]) {
  const T0 = 1.8e12, t = harness({app: true, epoch: T0, httpError: kind === "http", fetchFails: kind === "réseau", fetchReply: {error: "Enregistrement trop court"}});
  t.c.vpBegin(); await settle(10); t.audio(28, T0); await settle(10);
  t.c.showVoicePrintJob({kind: "voice-print", running: false, result: {timeline: [], audio: "wanted"}});
  await t.advance(300); await settle(10);
  assert.equal(t.c.sheetState.title, "Empreinte impossible", kind + " : erreur affichée");
  assert.ok(!t.c.sheetState.vpRes, kind + " : aucun résultat inventé");
  if (kind === "http") assert.match(t.c.sheetState.text, /Enregistrement trop court/, "message du moteur");
  assert.ok(t.c.sheetState.actions.some(a => /Renvoyer/.test(a[0])), kind + " : renvoi possible sans tout refaire");
  assert.equal(t.c.V, -1, kind + " : l’état sera relu");
  assert.equal(t.c.pending, 0);
  assert.ok(!t.toasts.some(x => /réglée|appliqué/.test(x.m)));
  const resend = t.c.sheetState.actions.find(a => /Renvoyer/.test(a[0]))[1];
  resend(); await settle(10);
  assert.equal(t.fetches.length, 2, kind + " : le même enregistrement est renvoyé");
  assert.equal(t.fetches[1].o.body, t.fetches[0].o.body);
}
{
  const T0 = 1.8e12, t = harness({app: true, epoch: T0});
  t.c.vpBegin(); await settle(10); t.audio(2, T0); await settle(10);
  t.c.showVoicePrintJob({kind: "voice-print", running: false, error: "Ton micro est coupé."});
  await t.advance(400);
  assert.equal(t.c.sheetState.title, "Empreinte interrompue");
  assert.match(t.c.sheetState.text, /coupé/);
  assert.equal(t.fetches.length, 0);
  assert.ok(t.tracks.every(x => x.stopped));
}
{
  const t = harness({app: true, muted: true});
  t.c.vpBegin(); await settle(10);
  assert.deepEqual(plain(t.posts[0]), ["mute", {channel: "Mic", muted: false, target: "all"}], "micro réactivé avant l’enregistrement");
}
{
  const t = harness({app: true, partial: "chat"});
  t.c.voicePrintIntro();
  assert.match(t.c.sheetState.body.innerHTML + JSON.stringify(t.c.sheetState.body.children.map(x => x._html)), /coupé de Discord/, "coupé de Discord : annoncé");
  t.c.vpBegin(); await settle(10);
  assert.deepEqual(plain(t.posts[0]), ["mute", {channel: "Mic", muted: false, target: "all"}], "coupé de Discord : « Chat Mic » ne reçoit rien, le micro est rallumé");
}
{
  const t = harness({app: true, partial: "stream"});
  t.c.vpBegin(); await settle(10); t.audio(1, 1.8e12); await settle(10);
  assert.ok(!t.posts.some(p => p[0] === "mute"), "coupé du stream seulement : la coupure reste, ta voix ne part pas en live");
  assert.ok(t.posts.some(p => p[0] === "voice-print"));
}

/* ---------- résultats : alertes en tête, aperçu sans rien appliquer, application explicite ---------- */
{
  const t = harness({app: true});
  const deep = e => (e._html || "") + e.children.map(deep).join("");
  const prof = (label, notes) => ({label, dsp: {eq: {"90": -9, "250": -2}, eq_freq: {"90": 90, "250": 100}, gate: {threshold: -41}, comp: {ratio: 3, threshold: -23, makeup: 2}, deesser: 56},
    fx: {eq: true, gate: true, comp: true, deesser: true}, notes, chart: null, fitted: true});
  const data = {print: {t: 1, mic_type: "condenser", model: "rode-nt1a"}, valid: true,
    summary: {voice_type: "grave", f0_hz: 118, snr_db: 28, sibilance: "marquées", plosives: 4, warnings: ["clipped", "too_quiet"]},
    notes: [{lvl: "warn", t: "Ta voix a saturé pendant l’enregistrement : refais l’empreinte."}, {lvl: "tip", t: "4 « p » ont soufflé : filtre anti-pop à 5 cm."}],
    profiles: {studio: prof("Studio", [{lvl: "ok", t: "−2 dB à 100 Hz : grondement sous ta voix."}, {lvl: "ok", t: "Sifflantes marquées : de-esser à 56 %."}]), radio: prof("Broadcast", [])}};
  t.c.vpShowResults(data, null, {gain_db: 39, old_gain_db: 43, notes: [{lvl: "warn", t: "Ta voix saturait : gain baissé de 43 à 39 dB."}, {lvl: "ok", t: "Gain réglé à 39 dB."}]});
  const o = t.c.sheetState, r = o.vpRes, kids = o.body.children;
  const alerts = kids.filter(k => k.classList.contains("vp-warn")).map(deep);
  assert.equal(alerts.length, 2, "une seule alerte de saturation, plus la voix trop faible");
  assert.match(alerts[0], /a saturé/); assert.match(alerts[1], /faiblement/);
  const tips = deep(kids.at(-1));
  assert.match(tips, /Pour ton installation/); assert.match(tips, /anti-pop/); assert.doesNotMatch(tips, /satur/, "l’alerte n’est pas répétée en bas");
  assert.match(o.body._html, /RØDE NT1-A · gain réglé de 43 à 39 dB · 28 dB au-dessus du bruit de ta pièce/);
  assert.equal(r.preview, "studio", "profil conseillé pour un micro statique, montré d’abord");
  assert.match(r.note._html, /rien ne change tant que tu n’appliques pas\. Ta voix reste naturelle/);
  assert.match(deep(r.what), /−2\u00a0dB\u00a0à\u00a0100\u00a0Hz/, "réglages : fréquence réglée");
  assert.match(deep(r.what), /Pourquoi/); assert.match(deep(r.what), /de-esser à 56 %/, "toutes les raisons du moteur");
  r.cards.radio.click(); await settle();
  assert.equal(r.preview, "radio");
  assert.equal(r.cards.radio.getAttribute("aria-checked"), "true");
  assert.equal(r.cards.studio.getAttribute("tabindex"), "-1", "un seul arrêt de tabulation dans le groupe");
  assert.ok(!t.posts.some(p => p[0] === "mic-preset"), "choisir un profil le montre sans rien appliquer");
  let prevented = false;
  for (const f of r.cards.radio.events.keydown) f({key: "ArrowLeft", preventDefault() { prevented = true; }});
  assert.equal(r.preview, "studio", "flèches : profil précédent"); assert.ok(prevented);
  o.actions[1][1](); await settle();
  assert.deepEqual(plain(t.posts.filter(p => p[0] === "mic-preset")), [["mic-preset", {name: "studio"}]], "« Appliquer » met le profil en place");
  assert.equal(r.applied, "studio");
  assert.match(r.note._html, /« Studio » est appliqué/, "la note confirme sur place, sans faire sauter la feuille");
  r.cards.radio.click(); await settle();
  assert.match(r.note._html, /« Studio » reste en place tant que tu n’appliques pas/);
  o.actions[0][1](); await settle(10);
  assert.deepEqual(plain(t.posts.filter(p => p[0] === "mic-preset").at(-1)), ["mic-preset", {name: "radio"}], "écouter et comparer : le profil montré, pas l’ancien");
  assert.equal(t.c.sheetState.vpListen.key, "radio");
}

/* ---------- ligne d'état de l'enregistrement : sans redire le titre ---------- */
{
  const {c} = harness();
  assert.equal(c.vpStatusText("silence", "Silence…", 0), "");
  assert.equal(c.vpStatusText("reading", "Lis le texte à voix haute", 0), "");
  assert.equal(c.vpStatusText("loud", "Plus fort !", 0), "");
  assert.equal(c.vpStatusText("prepare", "Je prépare l’enregistrement…", 0), "Je prépare l’enregistrement…");
  assert.equal(c.vpStatusText("reading", "Relis le texte, même voix", 1), "Relis le texte, même voix", "une reprise se voit");
  assert.equal(c.vpStatusText("reading", "Ta voix sature : gain baissé à 29 dB, on recommence la lecture.", 0), "Ta voix sature : gain baissé à 29 dB, on recommence la lecture.");
}

/* ---------- comparaison avant / après ---------- */
{
  const t = harness({app: true});
  t.c.L = {listening: 0, ab: false};
  t.c.vpListenSheet({profiles: {studio: {label: "Studio"}}}, "studio");
  await settle();
  assert.deepEqual(plain(t.posts.at(-1)), ["mic-listen", {seconds: 30, level: 191}]);
  const o = t.c.sheetState.vpListen;
  await t.c.vpBypass(o, true);
  assert.deepEqual(plain(t.posts.at(-1)), ["mic-ab", {bypass: true}]);
  assert.equal(o.raw.getAttribute("aria-pressed"), "true");
  await t.c.vpBypass(o, false);
  assert.deepEqual(plain(t.posts.at(-1)), ["mic-ab", {bypass: false}]);
  await t.c.vpBypass(o, true);
  t.c.closeSheet(); await settle();
  assert.deepEqual(plain(t.posts.slice(-2)), [["mic-ab", {bypass: false}], ["mic-listen", {seconds: 0}]], "fermer remet le traitement et coupe le retour");
  t.c.closeSheet();
  assert.equal(t.posts.filter(p => p[0] === "mic-listen" && p[1].seconds === 0).length, 1, "un seul arrêt");
}
{
  const t = harness({app: true, responses: {"mic-ab": null}});
  t.c.L = {listening: 20, ab: false};
  t.c.vpListenSheet({profiles: {studio: {label: "Studio"}}}, "studio");
  const o = t.c.sheetState.vpListen;
  assert.equal(await t.c.vpBypass(o, true), false);
  assert.equal(o.bypass, false, "un refus ne fait pas croire à l’écoute sans traitement");
}

/* ---------- carte « Ton micro » et fréquences de l'égaliseur ---------- */
{
  const t = harness();
  assert.equal(t.c.micModelKey(), "rode-nt1a");
  t.S.state.mic.type = "dynamic";
  assert.equal(t.c.micModelKey(), "generic-dynamic", "le type réglé prime : micro générique du même type");
  t.S.state.mic.model = null; t.S.state.mic.type = "jack";
  assert.equal(t.c.micModelKey(), "", "aucun modèle connu");
  t.S.state.mic.type = "condenser"; t.S.state.mic.model = "rode-nt1a";
  t.c.voiceInfo.data = null;
  t.S.state.voice_print = {t: 5, mic_type: "condenser", model: "rode-nt1a"};
  assert.equal(t.c.voiceValid(), true);
  t.S.state.mic.model = "generic-condenser";
  assert.equal(t.c.voiceValid(), false, "empreinte d’un autre micro");

  const eq = {};
  for (const b of bandsKeys) eq[b] = {freq: t.c.document.createElement("button"), sl: {el: t.c.document.createElement("div")}};
  t.c.mic.eq = eq;
  Object.assign(t.S.state.dsp.eq_freq, {"90": 75, "8000": 10500});
  t.c.renderEqFreqs();
  assert.deepEqual(bandsKeys.map(b => eq[b].freq.textContent), ["75 Hz", "250 Hz", "500 Hz", "1 kHz", "3 kHz", "10,5 kHz"], "libellé : fréquence réglée de chaque bande");
  t.S.state.dsp.eq_freq["250"] = 100;
  t.c.renderEqFreqs();
  assert.equal(eq["250"].freq.textContent, "100 Hz", "même fréquence que les explications du moteur");
  assert.ok(eq["250"].freq.classList.contains("moved"));
  t.S.state.dsp.eq_freq["250"] = 250;
  assert.ok(eq["8000"].freq.classList.contains("moved") && !eq["500"].freq.classList.contains("moved"), "fréquence déplacée signalée");
  assert.equal(eq["8000"].freq.getAttribute("aria-label"), "Bande 6 : 10,5 kHz. Changer la fréquence");
  assert.equal(eq["8000"].sl.el.getAttribute("aria-label"), "Égaliseur micro 10,5 kHz");
  delete t.S.meta.eq_ranges;
  t.c.renderEqFreqs();
  assert.equal(eq["90"].freq.disabled, true, "sans plages connues, la fréquence ne se règle pas");
  t.S.meta.eq_ranges = {"90": [30, 90]};
  assert.equal(t.c.micEqFreq("90", t.S.state), 75);
  assert.equal(t.c.micEqFreq("250", {dsp: {}}), 250, "moteur plus ancien : fréquence de base");
}

/* ---------- fréquence d'une bande : seul un vrai changement part au moteur ---------- */
{
  const t = harness();
  const eq = {};
  for (const b of bandsKeys) eq[b] = {freq: t.c.document.createElement("button"), sl: {el: t.c.document.createElement("div")}};
  t.c.mic.eq = eq;
  t.c.openEqFreq("250", eq["250"].freq);
  const sl = t.sliders.at(-1), dsp = () => t.posts.filter(p => p[0] === "dsp").map(p => plain(p[1]));
  assert.equal(sl.o.format(sl.value), "250 Hz", "valeur montrée : fréquence réglée");
  sl.commit(); await settle();
  assert.deepEqual(dsp(), [], "touche sans effet (bout de course) : rien n’est envoyé, le préréglage reste intact");
  sl.input(0); await settle();
  assert.deepEqual(dsp(), [{eq_freq: {"250": 100}}], "déplacement : fréquence demandée envoyée");
  assert.equal(eq["250"].freq.textContent, "100 Hz");
  sl.commit(); await settle();
  assert.equal(dsp().length, 1, "relâcher sur la valeur déjà envoyée : pas de doublon");
  sl.input(1000); sl.commit(); await settle();
  assert.deepEqual(dsp().at(-1), {eq_freq: {"250": 300}});
}

/* ---------- choix du modèle au clavier : un seul changement de 48 V et de gain ---------- */
{
  const t = harness();
  const card = t.c.buildMicModelCard(), mm = t.c.mic.model;
  assert.ok(card && mm.sel, "carte construite");
  t.c.renderMicModel();
  assert.equal(mm.sel.value, "rode-nt1a");
  const pick = v => { mm.sel.value = v; for (const f of mm.sel.events.change || []) f({target: mm.sel}); };
  pick("shure-sm7b"); pick("generic-condenser"); pick("rode-nt1a");
  assert.equal(t.posts.filter(p => p[0] === "mic-model").length, 0, "rien n’est envoyé pendant le défilement");
  await t.advance(450);
  assert.deepEqual(plain(t.posts.filter(p => p[0] === "mic-model")), [["mic-model", {model: "rode-nt1a"}]], "seul le choix final part");
  pick("shure-sm7b");
  assert.equal(mm.specs.innerHTML.includes("1,12 mV/Pa"), true, "caractéristiques montrées tout de suite");
  assert.match(mm.specs.innerHTML, /Fiche Shure/, "lien vers la fiche officielle d’où viennent les chiffres");
}
{
  const t = harness();
  t.S.state.mic.model = null;
  t.c.buildMicModelCard(); t.c.renderMicModel();
  const mm = t.c.mic.model;
  assert.equal(mm.sel.value, "", "aucun modèle choisi : la liste invite à choisir, sans en supposer un");
  assert.ok(mm.sel.classList.contains("empty"));
  assert.match(mm.specs.innerHTML, /Statique · 48 V/, "le type réglé reste affiché");
  assert.match(mm.tips.innerHTML, /Choisis ton modèle/);
  assert.equal(t.c.micChosen(), null);
  t.S.state.mic.model = "shure-sm7b";
  assert.equal(t.c.micChosen(), null, "modèle d’un autre type que celui réglé : ignoré");
}

console.log("Empreinte UI : modèle mesuré de l’égaliseur, pas de capture sans appli, capture avant analyse puis envoi, PCM 16 bits et coupe, voix synthétique en simulation, annulation, erreurs sans faux succès, résultats et aperçu sans application, comparaison avant/après, modèle et fréquences vérifiés.");
