// Historique des versions, retour à une version précédente et « Aide et retours », avec les fonctions servies.
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {runInNewContext} from "node:vm";

const read = name => readFileSync(new URL("../ui/js/" + name, import.meta.url), "utf8");
const fence = "\u0060\u0060\u0060";
const context = {
  esc: s => String(s).replace(/[&<>"]/g, c => ({"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;"})[c]),
  svg: name => "<svg data-i=\"" + name + "\"></svg>",
  chipIco: name => "<span class=\"chip-ico\">" + name + "</span>",
  document: {addEventListener() {}},
  inApp: () => false,
  toast() {},
  S: null,
};
runInNewContext(read("mises-a-jour.js") + "\n" + read("aide.js"), context);
const c = context;

const versions = ["5.6.12", "5.6.11", "5.6.10", "5.6.9", "5.6.8"].map((v, i) => ({
  version: v, published: "2026-10-31T10:00:00Z", notes: "**Nouveau**\n- dans la " + v, installed: i === 1, newer: i === 0, revertible: i > 1, source: "github"}));
const hist = {current: "5.6.11", versions, installable: true, error: null};

let view = c.histView(hist, false);
assert.equal(view.shown.length, 3, "trois versions dépliées");
assert.equal(view.hidden, 2);
assert.equal(c.histView(hist, true).shown.length, 5, "toutes une fois dépliées");
assert.equal(c.histFoldLabel(2, false), "Afficher les 2 versions précédentes");
assert.equal(c.histFoldLabel(1, false), "Afficher la version précédente");
assert.equal(c.histFoldLabel(2, true), "Masquer les versions précédentes");

const installed = c.histCard(versions[1], true), newer = c.histCard(versions[0], true), older = c.histCard(versions[2], true);
assert.match(installed, /Installée/);
assert.ok(!installed.includes("hist-back"), "pas de retour vers la version installée");
assert.match(newer, /Disponible/);
assert.ok(!newer.includes("hist-back"), "une version plus récente passe par la mise à jour");
assert.match(older, /data-v="5\.6\.10"/, "retour proposé vers une version précédente");
assert.match(older, /<li>dans la 5\.6\.10<\/li>/, "notes rendues");
assert.ok(!c.histCard(versions[2], false).includes("hist-back"), "aucun retour pendant une installation ou hors Windows");
assert.ok(!c.histCard({...versions[2], version: "<b>x</b>"}, true).includes("<b>x</b>"), "version échappée");

assert.match(c.histNote({...hist, error: "offline"}), /notes intégrées/);
assert.match(c.histNote({...hist, error: "rate-limited"}), /limite les recherches/);
assert.match(c.histNote({...hist, installable: false, why_not: "dev"}), /Version de développement/);
assert.equal(c.histNote(hist), "");

const confirm = c.revertText("5.6.10", "5.6.12");
for (const part of ["5.6.10", "5.6.12", "SHA-256", "copie", "redémarre"]) assert.ok(confirm.includes(part), "confirmation : " + part);

const support = {
  version: "5.6.12", windows: "Windows 11 25H2 (build 26200.7019)", repo: "samirmedani/rustudio",
  bug_url: "https://github.com/samirmedani/rustudio/issues/new?template=bug.yml&version=5.6.12",
  ideas_url: "https://github.com/samirmedani/rustudio/discussions/categories/ideas",
  questions_url: "https://github.com/samirmedani/rustudio/discussions/categories/q-a",
  device_url: "https://github.com/samirmedani/rustudio/issues/new?template=appareil.yml",
  report: {rustudio: "5.6.12", appareils: [{module: "goxlr-mini", connecte: true}, {module: "attack-shark-r5-ultra", connecte: false, etat: "asleep"}],
    journal: ["GoXLR connectée : série [masqué]"]},
};
const win = {render: {console: {name: "System"}}, capture: {console: {name: "Chat Mic"}}, issues: [{kind: "render-other"}]};
const diag = c.diagnosticText(support, win);
assert.ok(diag.startsWith("Diagnostic de Rustudio"));
assert.ok(diag.includes(fence + "json") && diag.trim().endsWith(fence), "bloc json prêt à coller");
const parsed = JSON.parse(diag.slice(diag.indexOf("{"), diag.lastIndexOf("}") + 1));
assert.deepEqual(parsed.windows_audio, {sortie: "System", micro: "Chat Mic", alertes: ["render-other"]});
assert.equal(c.devicesLine(support.report), "GoXLR Mini, Attack Shark R5 Ultra");

const prompt = c.agentPrompt(support, win);
for (const part of [support.bug_url, "Rustudio 5.6.12", "Windows 11 25H2", "C:\\ProgramData\\Rustudio\\rustudio.log", "Ne change rien",
  "gh issue list --repo samirmedani/rustudio", "Anonymise tout", "seulement avec mon accord", "GoXLR Utility"]) {
  assert.ok(prompt.includes(part), "prompt : " + part);
}
assert.ok(!/S2\d{6,}/.test(prompt), "aucun numéro de série");

const items = c.helpItems(support);
assert.deepEqual(Array.from(items, i => i.title), ["Signaler un problème", "Enquêter avec ton agent IA", "Proposer une idée", "Poser une question", "Demander un appareil", "Historique des versions"]);
const links = items.flatMap(i => i.acts.map(a => a[1])).filter(x => !["diag", "prompt", "history"].includes(x));
assert.ok(links.length === 4 && links.every(u => u.startsWith("https://github.com/samirmedani/rustudio/")), "liens vers le dépôt seulement");
assert.equal(items[0].acts[1][1], support.bug_url, "formulaire de bug prérempli");

console.log("Aide et versions UI : historique replié, retour proposé seulement vers une version précédente, textes hors ligne, confirmation, diagnostic, prompt et liens vérifiés.");
