// Contrat de l'espace personnel : seuls les modules compilés peuvent être ouverts.
import assert from "node:assert/strict";
import {runInNewContext, Script} from "node:vm";

import {source as html} from "./ui-source.mjs";
new Script(html);
function section(start, end) {
  const a = html.indexOf(start), b = html.indexOf(end, a);
  assert.ok(a >= 0 && b > a, `section présente : ${start}`);
  return html.slice(a, b);
}
const goxlr = {id: "goxlr-mini", name: "GoXLR Mini", origin:"official", author:"samirmedani", source:"crates/goxlr", review_pr:null, vendor: "TC-Helicon", kind: "audio-interface", summary: "Ton interface audio.", capabilities: ["mixer", "microphone", "headphone-eq", "routing", "lighting", "scenes"], pages: [
  {id: "mixer", label: "Mixeur", icon: "mixer"}, {id: "mic", label: "Micro", icon: "Mic"},
  {id: "hp", label: "Casque", icon: "Headphones"}, {id: "routing", label: "Routage", icon: "routing"},
  {id: "light", label: "Éclairage", icon: "light"}, {id: "scenes", label: "Scènes", icon: "scenes"},
]};
const device = changes => ({id: goxlr.id, status: "disconnected", connected: false, simulated: false, firmware: null, error: null, ...changes});
const text = node => node.innerHTML + node.textContent + node.children.map(text).join("");

function harness(storage = new Map(), denied = false) {
  const ids = new Map(), commands = [], visits = [], messages = [];
  class Element {
    constructor(tag) {
      this.tagName = tag.toUpperCase(); this.children = []; this.parentElement = null;
      this.attrs = {}; this.dataset = {}; this.style = {}; this.hidden = false; this.innerHTML = ""; this.textContent = ""; this.events = {};
      const classes = new Set();
      this.classList = {add: name => classes.add(name), contains: name => classes.has(name), toggle: (name, on) => { if (on) classes.add(name); else classes.delete(name); }};
    }
    setAttribute(name, value) {
      this.attrs[name] = String(value);
      if (name === "id") { this.id = String(value); ids.set(this.id, this); }
      if (name.startsWith("data-")) this.dataset[name.slice(5)] = String(value);
      if (name === "class") String(value).split(/\s+/).forEach(c => this.classList.add(c));
    }
    getAttribute(name) { return this.attrs[name] ?? null; }
    removeAttribute(name) { delete this.attrs[name]; }
    set innerHTML(value) { this._html = value; this.children = []; }
    get innerHTML() { return this._html; }
    appendChild(child) { this.children.push(child); child.parentElement = this; return child; }
    addEventListener(name, fn) { (this.events[name] ||= []).push(fn); }
    querySelectorAll(selector) {
      const all = this.children.flatMap(c => [c, ...c.querySelectorAll("*")]);
      if (selector === "*") return all;
      if (selector === "nav button") return all.filter(c => c.tagName === "BUTTON" && c.parentElement?.tagName === "NAV");
      if (selector === ".page") return all.filter(c => c.classList.contains("page"));
      if (selector === "button") return all.filter(c => c.tagName === "BUTTON");
      throw new Error(`sélecteur inattendu : ${selector}`);
    }
  }
  const body = new Element("body"), document = {body, createElement: tag => new Element(tag), getElementById: id => ids.get(id), querySelectorAll: selector => body.querySelectorAll(selector)};
  function add(tag, id, parent = body) { const node = new Element(tag); node.setAttribute("id", id); parent.appendChild(node); return node; }
  for (const id of ["module-space", "module-name", "h-title", "h-sub", "hdr-mic", "offline", "offline-txt", "offline-diag", "conn-dot", "conn-txt"]) add("div", id);
  add("main", "main-content");
  const mainNav = add("nav", "main-nav"); add("nav", "module-nav");
  for (const route of ["devices", "modules"]) { const button = new Element("button"); button.setAttribute("data-p", route); mainNav.appendChild(button); }
  for (const page of ["devices", "modules", "device", "mixer", "mic", "hp", "routing", "light", "scenes"]) add("section", "p-" + page).classList.add("page");
  const location = {hash: "", reload() { throw new Error("rechargement inattendu"); }};
  const localStorage = {getItem: key => { if (denied) throw new Error("stockage refusé"); return storage.get(key) || null; }, setItem: (key, value) => { if (denied) throw new Error("stockage refusé"); storage.set(key, value); }};
  const context = {
    document, window: {localStorage}, location,
    history: {pushState: (_s, _t, path) => { location.hash = path; visits.push(path); }, replaceState: (_s, _t, path) => { location.hash = path; visits.push(path); }},
    S: {v: 1, state: {}, modules: {installed: [goxlr], devices: [device()]}}, L: null, V: 1, pending: 0,
    page: "devices", activeModule: null, built: {}, bootId: null, liveKey: "", liveChanged: false, sheetState: null,
    closeSheet() { const sheet = context.sheetState; context.sheetState = null; if (sheet?.onClose) sheet.onClose(); },
    closePop() {}, svg: name => `<svg data-icon="${name}"></svg>`, chipIco: () => "", toast: message => messages.push(message),
    post: (action, data) => { commands.push({action, data}); return Promise.resolve({ok: true}); },
    openUrl: url => visits.push(url), setTimeout() {},
    render() { context.rememberDetected(); context.renderModuleNav(); if (context.page === "devices") context.renderDevices(); },
  };
  runInNewContext(section("function h(tag,", "function chipIco("), context);
  runInNewContext(section("function esc(s)", "function pct("), context);
  context.$ = id => ids.get(id);
  runInNewContext(section("/* ---------- navigation", "/* ---------- mixeur"), context);
  runInNewContext(section("/* ---------- espace personnel et modules", "/* ---------- menu bas droite"), context);
  Object.assign(context, {liveTimer:null,liveRequest:null,liveWake:false});
  runInNewContext(section("function liveLoop()", "/* ---------- curseur"), context);
  return {context, ids, storage, commands, visits, messages};
}
const settle = () => new Promise(resolve => setImmediate(resolve));

{
  const h = harness(), c = h.context;
  assert.equal(c.resolveRoute("").path, "devices");
  assert.equal(c.resolveRoute("library").path, "modules", "ancien lien Bibliothèque conservé");
  assert.equal(c.resolveRoute("mixer").path, "goxlr-mini/mixer", "ancien lien audio conservé");
  assert.equal(c.resolveRoute("goxlr-mini/mic").page, "mic");
  h.ids.get("main-content").scrollTop = 180;
  c.go("goxlr-mini/mic");
  assert.equal(h.ids.get("main-content").scrollTop, 0, "une nouvelle page retrouve son en-tête");
  for (const route of ["inconnu", "fantome/mixer", "goxlr-mini/inconnu", "goxlr-mini/mixer/autre"]) assert.equal(c.resolveRoute(route).path, "devices", `route inconnue sûre : ${route}`);
  c.setOnline(true, false, "absent", "none"); c.go("devices");
  assert.ok(!h.ids.get("offline").classList.contains("on"), "une GoXLR absente ne bloque pas l'accueil");
  assert.equal(h.ids.get("hdr-mic").hidden, true); assert.equal(h.ids.get("module-space").hidden, true);
  assert.equal(h.ids.get("conn-txt").textContent, "Moteur connecté");
  c.go("mic");
  assert.equal(c.activeModule, goxlr.id); assert.equal(h.ids.get("hdr-mic").hidden, false);
  assert.equal(h.ids.get("module-space").hidden, false); assert.equal(h.ids.get("module-nav").children.length, 6);
  assert.ok(h.ids.get("offline").classList.contains("on"), "alerte GoXLR limitée à son espace");
  assert.equal(h.ids.get("p-mic").classList.contains("on"), true);
  c.go("modules"); assert.equal(h.ids.get("hdr-mic").hidden, true);
  c.setOnline(false, false);
  assert.ok(h.ids.get("offline").classList.contains("on"), "l'erreur moteur reste globale");
  assert.match(h.ids.get("offline-txt").textContent, /Rustudio/);
  assert.equal(h.commands.length, 0, "naviguer ne modifie jamais l'audio");
}

{
  const h = harness(), c = h.context, closed = [];
  c.go("goxlr-mini/mic");
  const recording = {onClose: () => closed.push([c.page, c.activeModule])}; c.sheetState = recording;
  c.go("goxlr-mini/mic");
  assert.equal(c.sheetState, recording); assert.equal(closed.length, 0, "la même route ne ferme pas une analyse en cours");
  c.go("devices");
  assert.equal(c.sheetState, null); assert.equal(closed.length, 1);
  assert.deepEqual(closed[0], ["mic", "goxlr-mini"], "le nettoyage de l'enregistrement précède le changement de contexte");
  c.S.modules.installed.push({id: "future-mouse", name: "Ma souris", kind: "mouse", capabilities: [], pages: []});
  c.go("goxlr-mini/overview"); c.sheetState = {onClose: () => closed.push([c.page, c.activeModule])};
  c.go("future-mouse/overview");
  assert.equal(c.sheetState, null); assert.deepEqual(closed[1], ["device", "goxlr-mini"], "changer d'appareil ferme aussi la fenêtre sur une page de même type");
}

{
  const h = harness(), c = h.context;
  c.S.modules.devices = [device({status: "ready", connected: true, firmware: "1.2.3"})];
  c.setOnline(true, true, "ok", "mini");
  assert.equal(c.hasDevice(goxlr.id), true, "un appareil connecté est ajouté automatiquement");
  assert.match(text(h.ids.get("p-devices")), /Connecté/);
  assert.match(text(h.ids.get("p-devices")), /1\.2\.3/);
  const stored = JSON.parse(h.storage.get("rustudio.workspace.v1")); assert.deepEqual(stored.devices, [goxlr.id]);
  c.S.modules.devices = [device()]; c.workspaceRuntimeChanged();
  assert.equal(c.hasDevice(goxlr.id), true, "l'appareil reste dans l'espace après débranchement");
  assert.match(text(h.ids.get("p-devices")), /Déconnecté/);
  for (const [status, label] of [["starting", "Connexion…"], ["error", "Connexion impossible"], ["blocked", "Pilote bloqué"]]) {
    c.L = {modules: {devices: [device({status})]}}; c.workspaceRuntimeChanged();
    assert.equal(c.moduleStatus(goxlr.id).label, label); assert.ok(text(h.ids.get("p-devices")).includes(label));
  }
  c.L = {modules: {devices: [device({status: "ready", connected: true})]}};
  c.removeDevice(goxlr.id); c.workspaceRuntimeChanged();
  assert.equal(c.hasDevice(goxlr.id), false, "un retrait explicite empêche la réapparition automatique");
  const reopened = harness(h.storage); reopened.context.S.modules.devices = [device({status: "ready", connected: true})]; reopened.context.rememberDetected();
  assert.equal(reopened.context.hasDevice(goxlr.id), false, "le retrait persiste au redémarrage");
  assert.equal(reopened.context.addDevice(goxlr.id), true);
  assert.equal(harness(h.storage).context.hasDevice(goxlr.id), true, "l'ajout manuel persiste aussi");
  assert.equal(h.commands.length, 0, "ajout et retrait ne touchent pas au son");
}

{
  const h = harness(), c = h.context;
  c.lib.grid = h.ids.get("p-modules");
  c.lib.data = {modules: [{...goxlr, name: "Faux nom distant", builtin: true}, {id: "fantome", name: "Module fantôme", vendor: "Externe", kind: "mouse", status: "official", builtin: true, features: [], source: "crates/fantome"}, {id: "a-ecrire", name: "À écrire", status: "wanted", builtin: true, features: []}]};
  c.paintLibrary();
  assert.equal(c.addDevice("fantome"), false); assert.equal(c.resolveRoute("fantome/mixer").path, "devices");
  const cards = c.lib.grid.children;
  assert.ok(text(cards[0]).includes("GoXLR Mini") && !text(cards[0]).includes("Faux nom distant"), "le registre livré prime sur les déclarations distantes");
  assert.match(text(cards[0]), /Ajouter à Mes appareils/);
  assert.equal(cards.length, 2, "une fiche tierce non livrée ne doit même pas être affichée");
  assert.ok(!text(c.lib.grid).includes("Module fantôme"));
  assert.match(text(cards[0]), /Officiel Rustudio/); assert.match(text(cards[0]), /Créé par samirmedani/);
  assert.match(text(cards[1]), /À contribuer/); assert.match(text(cards[1]), /Contribuer ce module/);
  c.addDevice(goxlr.id); assert.match(text(c.lib.grid.children[0]), /Ouvrir|Ajouter à Mes appareils/);
  c.page = "modules"; c.paintLibrary(); assert.match(text(c.lib.grid.children[0]), /Dans Mes appareils/);
}

{
  const h = harness(), c = h.context;
  c.lib.grid = h.ids.get("p-modules"); c.lib.filter = "community";
  c.lib.data = {modules:[{id:"imposteur",name:"Fausse validation",status:"community",author:"autre",review_pr:42,builtin:true}]};
  c.paintLibrary();
  assert.match(text(c.lib.grid), /Aucun module communautaire n’est encore publié/);
  assert.ok(!text(c.lib.grid).includes("Fausse validation"));
  c.S.modules.installed.push({...goxlr,id:"module-test",name:"Module de test",origin:"community",author:"contributeur",review_pr:42});
  c.paintLibrary();
  assert.match(text(c.lib.grid), /Communautaire/); assert.match(text(c.lib.grid), /Créé par contributeur/);
  assert.match(text(c.lib.grid), /PR #42 validée/);
  assert.ok(!text(c.lib.grid).includes("Officiel Rustudio"));
  c.lib.filter = "official"; c.paintLibrary(); assert.match(text(c.lib.grid), /GoXLR Mini/);
  assert.ok(!text(c.lib.grid).includes("Module de test"));
}

{
  const h = harness(), c = h.context;
  c.addDevice(goxlr.id); c.setOnline(true, false, "absent", "none");
  c.api = async path => {
    assert.equal(path, "/api/live", "l'accueil ne demande pas le vumètre audio");
    return {boot: "test", v: 1, connected: false, health: "blocked", usb: "mini", live: {fader_positions: [], pressed: [], mic_db: -72}, modules: {devices: [device({status: "blocked", error: "Pilote sans réponse"})]}};
  };
  c.refresh = () => { throw new Error("v inchangé : aucune relecture nécessaire"); };
  c.liveLoop(); await settle();
  assert.equal(c.V, 1); assert.match(text(h.ids.get("p-devices")), /Pilote bloqué/);
  assert.match(text(h.ids.get("p-devices")), /Pilote sans réponse/);
}

{
  const h = harness(new Map(), true), c = h.context;
  assert.doesNotThrow(() => c.addDevice(goxlr.id)); assert.equal(c.hasDevice(goxlr.id), true);
  assert.doesNotThrow(() => c.removeDevice(goxlr.id)); assert.equal(c.hasDevice(goxlr.id), false, "le stockage refusé conserve un espace utilisable en mémoire");
  c.S.modules.installed.push({id: "future-mouse", name: "Ma souris", kind: "mouse", capabilities: ["dpi"], pages: [{id: "dpi", label: "Sensibilité"}]});
  c.openDevice("future-mouse");
  assert.equal(c.page, "device"); assert.equal(c.activeModule, "future-mouse");
  assert.equal(h.ids.get("hdr-mic").hidden, true); assert.equal(h.ids.get("module-nav").children.length, 1);
  assert.ok(!text(h.ids.get("module-nav")).includes("Mixeur"), "un autre appareil n'hérite pas des pages GoXLR");
}
console.log("Modules UI : routes, états live, espace persistant, ajout/retrait sans effet audio, catalogue non exécutable et contexte par appareil vérifiés.");
