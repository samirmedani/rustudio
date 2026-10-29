// Verifie les verdicts du diagnostic avec les fonctions reellement servies par le moteur.
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {runInNewContext} from "node:vm";

const html = readFileSync(new URL("../ui/index.html", import.meta.url), "utf8");
const start = html.indexOf("function fmtClock(");
const end = html.indexOf("function soundAt(", start);
assert.ok(start >= 0 && end > start, "fonctions du diagnostic presentes");
const context = {
  esc: s => String(s).replaceAll("<", "&lt;"),
  echoItem: (status, title, detail) => ({status, title, detail}),
  usbAdvice: () => null,
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
console.log("Diagnostic UI : repos, lecture, pilote absent, coupure micro et pistes verifies.");
