// Exerce les fonctions de l'interface servie, sans matériel ni dépendance DOM.
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {runInNewContext, Script} from "node:vm";

const html = readFileSync(new URL("../ui/index.html", import.meta.url), "utf8");
const script = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));
new Script(script); // Vérifie aussi la syntaxe de tout le JavaScript de l'interface.
function section(start, end) {
  const a = html.indexOf(start), b = html.indexOf(end, a);
  assert.ok(a >= 0 && b > a, `section présente : ${start}`);
  return html.slice(a, b);
}

function harness() {
  const ids = new Map(), listeners = {}, frames = [], messages = [], requests = [];
  let document;
  class Element {
    constructor(tag) {
      this.tagName = tag.toUpperCase(); this.children = []; this.parentElement = null;
      this.attrs = {}; this.events = {}; this.value = ""; this.disabled = false; this.inert = false;
      const classes = new Set();
      this.classList = {
        add: name => classes.add(name), remove: name => classes.delete(name),
        contains: name => classes.has(name),
        toggle: (name, on) => { if (on === undefined) on = !classes.has(name); if (on) classes.add(name); else classes.delete(name); return on; },
      };
      this.style = {setProperty() {}};
    }
    setAttribute(name, value) {
      this.attrs[name] = String(value);
      if (name === "id") { this.id = String(value); ids.set(this.id, this); }
      if (name === "inert") this.inert = true;
      if (name === "class") String(value).split(/\s+/).forEach(c => this.classList.add(c));
    }
    getAttribute(name) { return this.attrs[name] ?? null; }
    removeAttribute(name) { delete this.attrs[name]; }
    appendChild(child) { this.children.push(child); child.parentElement = this; return child; }
    set innerHTML(value) {
      this.children.forEach(c => { c.parentElement = null; }); this.children = []; this._html = value;
      // Seul le titre est nécessaire aux tests du focus et du nom accessible.
      const title = String(value).match(/<h2>([^<]*)<\/h2>/);
      if (title) { const h2 = new Element("h2"); h2.textContent = title[1]; this.appendChild(h2); }
    }
    get innerHTML() { return this._html || ""; }
    addEventListener(name, fn) { (this.events[name] ||= []).push(fn); }
    fire(name, props = {}) {
      const e = {target: this, button: 0, preventDefault() { this.defaultPrevented = true; }, ...props};
      for (const fn of this.events[name] || []) fn(e);
      return e;
    }
    click() { if (!this.disabled) this.fire("click"); }
    focus() { document.activeElement = this; }
    contains(node) { return node === this || this.children.some(c => c.contains(node)); }
    closest(selector) {
      assert.equal(selector, "[inert]");
      for (let e = this; e; e = e.parentElement) if (e.inert) return e;
      return null;
    }
    get isConnected() { return document.body.contains(this); }
    getClientRects() { return this.isConnected && this.style.display !== "none" ? [{}] : []; }
    getBoundingClientRect() { return {left: 0, top: 0, width: 100, height: 100}; }
    setPointerCapture() {}
    querySelectorAll(selector) {
      const all = this.children.flatMap(c => [c, ...c.querySelectorAll("*")]);
      if (selector === "*") return all;
      if (/^h2$|^input$|^button$/.test(selector)) return all.filter(c => c.tagName.toLowerCase() === selector);
      return all.filter(c => !c.disabled && (["BUTTON", "INPUT", "SELECT", "TEXTAREA"].includes(c.tagName) || c.attrs.tabindex === "0"));
    }
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
  }
  document = {
    createElement: tag => new Element(tag), getElementById: id => ids.get(id), activeElement: null,
    addEventListener: (name, fn) => { (listeners[name] ||= []).push(fn); },
  };
  document.body = new Element("body"); document.activeElement = document.body;
  for (const id of ["app", "toast", "sheet", "p-scenes"]) {
    const e = new Element("div"); e.setAttribute("id", id); document.body.appendChild(e);
  }
  const box = new Element("div"); box.setAttribute("id", "sheet-box"); ids.get("sheet").appendChild(box);
  const context = {
    document, performance: {now: () => 1000}, pending: 0, V: 1, S: {state: {name: "avant"}}, built: {}, sc: {},
    toast: (message, action, fn) => messages.push({message, action, fn}),
    render() {}, setOnline() {}, svg: () => "", chipIco: () => "", clamp: (v, a, b) => Math.max(a, Math.min(b, v)),
    setTimeout() {}, requestAnimationFrame: fn => frames.push(fn),
    fetch: async (path, options) => {
      requests.push({path, options});
      return {ok: true, json: async () => options?.method === "POST" ? {ok: true} : {v: 2, state: {name: "après"}}};
    },
  };
  runInNewContext(section("function h(tag,", "function chipIco("), context);
  context.$ = id => ids.get(id);
  context.buildAuto = () => context.h("div");
  runInNewContext(section("function api(path)", "var liveChanged ="), context);
  runInNewContext(section("function Slider(o)", "/* ---------- popover"), context);
  runInNewContext(section("function withUndo(", "/* ---------- navigation"), context);
  runInNewContext(section("var sheetState =", "var jobSeen ="), context);
  runInNewContext(section("function hpSaveSheet()", "function renderHpStyles()"), context);
  runInNewContext(section("function buildScenes()", "function sceneLabel("), context);
  return {
    context, document, messages, requests, ids,
    frame: () => { while (frames.length) frames.shift()(); },
    key: props => { const e = {preventDefault() { this.defaultPrevented = true; }, ...props}; for (const fn of listeners.keydown || []) fn(e); return e; },
  };
}
const settle = () => new Promise(resolve => setImmediate(resolve));

for (const failure of ["HTTP", "réseau"]) {
  const h = harness(), c = h.context;
  c.fetch = async () => {
    if (failure === "réseau") throw new Error("hors ligne");
    return {ok: false, json: async () => ({error: "Nom refusé"})};
  };
  assert.equal(await c.withUndo("Scène appliquée", () => c.post("scene", {}).then(c.refresh)), null);
  assert.equal(c.pending, 0);
  assert.ok(h.messages.length && h.messages.every(m => !m.action && !m.message.includes("appliquée")), `${failure} : aucune fausse confirmation ni annulation`);
  assert.equal(c.V, -1, "une prochaine lecture du moteur corrige l'état optimiste");
  c.buildScenes(); c.sc.name.value = "Ma scène";
  const saveScene = h.ids.get("p-scenes").querySelector("button"); saveScene.click(); await settle();
  assert.equal(c.sc.name.value, "Ma scène", `${failure} : le nom de scène reste saisi`);
  assert.equal(saveScene.disabled, false, "la sauvegarde peut être réessayée");
  c.hpSaveSheet(); const current = c.sheetState;
  const input = current.body.querySelector("input"); input.value = "Mon casque";
  h.ids.get("sheet-box").querySelectorAll("button").at(-1).click(); await settle();
  assert.equal(c.sheetState, current, `${failure} : la fenêtre du profil reste ouverte`);
  assert.equal(input.value, "Mon casque");
  assert.ok(h.messages.every(m => !m.message.includes("enregistré")), `${failure} : aucune fausse sauvegarde`);
}

{
  const h = harness(), c = h.context;
  await c.withUndo("Scène appliquée", () => c.post("scene", {}).then(c.refresh));
  assert.equal(h.messages.at(-1).action, "Annuler", "succès : annulation disponible");
  c.fetch = async (_path, options) => { if (!options?.method) throw new Error("lecture interrompue"); return {ok: true, json: async () => ({ok: true})}; };
  await c.withUndo("Thème appliqué", () => c.post("theme", {}).then(c.refresh));
  assert.equal(h.messages.at(-1).message, "Thème appliqué", "une lecture interrompue n'efface pas une écriture réussie");
  c.fetch = async (_path, options) => ({ok: true, json: async () => options?.method ? {ok: true} : {v: 3, state: {}}});
  c.buildScenes(); c.sc.name.value = "Ma scène";
  h.ids.get("p-scenes").querySelector("button").click(); await settle();
  assert.equal(c.sc.name.value, ""); assert.match(h.messages.at(-1).message, /enregistrée/);
  c.hpSaveSheet(); c.sheetState.body.querySelector("input").value = "Mon casque";
  h.ids.get("sheet-box").querySelectorAll("button").at(-1).click(); await settle();
  assert.equal(c.sheetState, null); assert.match(h.messages.at(-1).message, /enregistré/);
}

{
  const h = harness(), c = h.context, committed = [];
  const slider = c.Slider({label: "Gain du micro", min: 0, max: 10, step: 1, value: 4, format: v => v + " dB", onCommit: v => committed.push(v)});
  h.ids.get("app").appendChild(slider.el);
  assert.equal(slider.el.getAttribute("role"), "slider");
  assert.equal(slider.el.getAttribute("aria-label"), "Gain du micro");
  const ignored = slider.el.fire("wheel", {deltaY: -100});
  assert.equal(slider.value, 4); assert.ok(!ignored.defaultPrevented, "le survol laisse défiler la page et ne change pas le son");
  slider.el.focus();
  assert.ok(slider.el.fire("keydown", {key: "ArrowUp"}).defaultPrevented);
  assert.equal(slider.value, 5); assert.equal(slider.el.getAttribute("aria-valuetext"), "5 dB");
  slider.el.fire("keydown", {key: "End"}); assert.equal(slider.value, 10);
  slider.el.fire("keydown", {key: "ArrowUp"}); assert.equal(slider.value, 10, "borne haute respectée");
  slider.el.fire("keydown", {key: "Home"}); assert.equal(slider.value, 0);
  slider.el.fire("keydown", {key: "PageUp"}); assert.equal(slider.value, 10);
  slider.el.fire("keydown", {key: "ArrowDown", ctrlKey: true}); assert.equal(slider.value, 10);
  assert.ok(slider.el.fire("wheel", {deltaY: 100}).defaultPrevented); assert.equal(slider.value, 9);
  assert.ok(committed.length >= 5, "le clavier et la molette ciblée transmettent le réglage");
}

{
  const h = harness(), c = h.context, app = h.ids.get("app"), box = h.ids.get("sheet-box");
  const opener = c.h("button"); app.appendChild(opener); opener.focus();
  c.sheet({title: "Premier écran", actions: [["Précédent"], ["Suivant"]]}); h.frame();
  assert.equal(app.inert, true); assert.equal(h.ids.get("toast").inert, false, "les erreurs restent annonçables dans une fenêtre");
  assert.equal(h.ids.get("sheet").getAttribute("aria-hidden"), "false");
  assert.equal(box.getAttribute("aria-label"), "Premier écran");
  const buttons = box.querySelectorAll("button");
  assert.equal(h.document.activeElement, buttons[0]);
  buttons.at(-1).focus(); assert.ok(h.key({key: "Tab"}).defaultPrevented);
  assert.equal(h.document.activeElement, buttons[0], "Tab reste dans la fenêtre");
  assert.ok(h.key({key: "Tab", shiftKey: true}).defaultPrevented);
  assert.equal(h.document.activeElement, buttons.at(-1), "Maj+Tab boucle aussi");
  c.sheet({title: "Deuxième écran", actions: [["Fermer"]]}); h.frame();
  c.closeSheet();
  assert.equal(app.inert, false); assert.equal(h.ids.get("sheet").inert, true);
  assert.equal(h.document.activeElement, opener, "le remplacement d'une fenêtre conserve son point de retour");
}
console.log("Interactions UI : erreurs sans faux succès, saisies conservées, annulation, curseurs clavier, molette ciblée et focus des fenêtres vérifiés.");
