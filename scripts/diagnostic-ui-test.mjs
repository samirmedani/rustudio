// Verifie les verdicts du diagnostic avec les fonctions reellement servies par le moteur.
import assert from "node:assert/strict";
import {runInNewContext} from "node:vm";

import {source as html} from "./ui-source.mjs";
const start = html.indexOf("function soundLevel(");
const end = html.indexOf("function soundAt(", start);
assert.ok(start >= 0 && end > start, "fonctions du diagnostic presentes");
const context = {
  esc: s => String(s).replaceAll("<", "&lt;"),
  echoItem: (status, title, detail) => ({status, title, detail}),
  usbAdvice: () => null,
  HP_CH_LABELS: {},
};
runInNewContext(html.slice(start, end), context);
const base = {available: true, reading: true, streaming: true, count: 0, out_ms: 0, in_ms: 0, events: [], top: []};
const render = changes => context.usbDropsItem({...base, ...changes}, {});
assert.equal(render({}).status, "ok");
assert.equal(render({streaming: false}).status, "info", "repos sans son : pas un test reussi");
assert.equal(render({reading: false}).status, "info", "compteurs devenus illisibles : pas un test reussi");
assert.equal(render({available: false}), null, "pilote incompatible : aucun verdict invente");
const event = {t: 1000, out_ms: 0, in_ms: 2, hint: [{name: "<programme>.exe", n: 1}]};
const mic = render({count: 1, in_ms: 2, events: [event]});
assert.equal(mic.status, "warn");
assert.match(mic.detail, /2 ms vers le PC/, "une coupure micro seule garde sa duree");
assert.match(mic.detail, /pas une cause prouvée/, "coincidence de lancement : pas une accusation");
assert.ok(!mic.detail.includes("<programme>"), "le nom d'un programme n'est jamais du HTML");
// Historique écrit par une ancienne version : repères internes de veille des écrans mêlés aux programmes.
const screen = {t: 2000, out_ms: 3, in_ms: 3, hint: [{name: "@ecrans-eteints", n: 1}]};
const old = render({count: 5, out_ms: 9, in_ms: 9, events: [screen, event], top: [{name: "@ecrans-eteints", count: 3}, {name: "svchost.exe", count: 2}]});
assert.ok(!old.detail.includes("@"), "aucun repère interne affiché");
assert.ok(!/écran|veille/i.test(old.detail), "le diagnostic ne parle plus de la veille des écrans");
assert.ok(!old.detail.includes("lançait :"), "un repère seul n'est pas présenté comme un programme lancé");
assert.match(old.detail, /« svchost\.exe », 2 coupures sur 5/, "le programme le plus fréquent reste signalé");
assert.match(old.detail, /<\/p><p class="drop-list">/, "la liste détaillée garde son propre paragraphe");

const win = {enabled: true, readable: true, total: 0, was_enabled: true};
assert.equal(context.windowsGlitchItem(win, false).status, "ok");
assert.equal(context.windowsGlitchItem({...win, enabled: false}, false).status, "info");
assert.equal(context.windowsGlitchItem({...win, readable: false, error: "<refus>"}, true).status, "info");
assert.equal(context.windowsGlitchItem({...win, total: null}, true).status, "info");
assert.equal(context.windowsGlitchItem({...win, was_enabled: false}, false).status, "info");
assert.equal(context.windowsGlitchItem({...win, total: 2, kinds: {driver: 2}}, true).status, "warn");
assert.ok(!context.windowsGlitchItem({...win, readable: false, error: "<refus>"}, false).detail.includes("<refus>"));

const failed = {name: "Chat Mic", status: "error", error: "<lecture refusée>"};
const good = {name: "VOD", status: "ok", frames: 48000};
const loss = {name: "Broadcast Stream Mix", status: "dropouts", frames: 47000, gap_ms: 20};
assert.ok(context.inputLossItems([failed]).every(i => i.status !== "ok"), "entree refusee : aucun feu vert");
assert.ok(context.inputLossItems([{name: "Chat Mic", frames: 0, status: "silent"}]).every(i => i.status !== "ok"));
assert.ok(context.inputLossItems([]).every(i => i.status !== "ok"));
assert.ok(context.inputLossItems([{...good, timestamp_errors: 1}]).every(i => i.status !== "ok"), "positions invalides : pas de validation des trous");
const partial = context.inputLossItems([failed, good, loss]);
assert.ok(partial.some(i => i.status === "info" && i.title.includes("Chat Mic")));
assert.ok(partial.some(i => i.status === "warn" && i.title.includes("Stream Mix")));
const green = partial.find(i => i.status === "ok");
assert.match(green.detail, /VOD/);
assert.ok(!green.detail.includes("Chat Mic") && !green.detail.includes("Stream Mix"), "seules les entrees effectivement mesurees sont validees");
assert.ok(!partial.some(i => i.detail.includes("<lecture refusée>")), "erreurs echappees");

assert.ok(context.outputLevelItems([{...failed, name: "Game", meter_db: -1}]).every(i => i.status !== "ok"), "un vumetre ne remplace pas une capture echouee");
assert.ok(context.outputLevelItems([{name: "System", status: "silent", frames: 0}]).every(i => i.status !== "ok"));
const both = context.outputLevelItems([{...loss, name: "Music", active_s: 1, peak_db: 1, clipped: 10, overs: 5}]);
assert.ok(both.some(i => i.title.includes("interruptions")) && both.some(i => i.title.includes("sature")), "pertes et saturation restent toutes deux visibles");
assert.ok(both.every(i => i.status === "warn"));
console.log("Diagnostic UI : repos, pertes, mesures echouees ou partielles, journal inaccessible, saturation et echappement verifies.");
