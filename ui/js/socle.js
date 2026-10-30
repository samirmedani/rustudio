// Socle commun : icônes, outils, API du moteur, suivi en direct, curseurs, fenêtres et navigation.
"use strict";

var ICONS = {
  pullRequest: "<circle cx=\"18\" cy=\"18\" r=\"3\" />\n  <circle cx=\"6\" cy=\"6\" r=\"3\" />\n  <path d=\"M13 6h3a2 2 0 0 1 2 2v7\" />\n  <line x1=\"6\" x2=\"6\" y1=\"9\" y2=\"21\" />",
  users: "<path d=\"M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2\" />\n  <path d=\"M16 3.128a4 4 0 0 1 0 7.744\" />\n  <path d=\"M22 21v-2a4 4 0 0 0-3-3.87\" />\n  <circle cx=\"9\" cy=\"7\" r=\"4\" />",
  batteryCharging: "<path d=\"m11 7-3 5h4l-3 5\" />\n  <path d=\"M14.856 6H16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2.935\" />\n  <path d=\"M22 14v-4\" />\n  <path d=\"M5.14 18H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h2.936\" />",
  battery: "<path d=\"M 22 14 L 22 10\" />\n  <rect x=\"2\" y=\"6\" width=\"16\" height=\"12\" rx=\"2\" />", /* icones Lucide (ISC), https://lucide.dev */
  grid: '<rect width="7" height="7" x="3" y="3" rx="1"/> <rect width="7" height="7" x="14" y="3" rx="1"/> <rect width="7" height="7" x="14" y="14" rx="1"/> <rect width="7" height="7" x="3" y="14" rx="1"/>',
  plus: '<path d="M5 12h14"/> <path d="M12 5v14"/>',
  mouse: '<rect x="5" y="2" width="14" height="20" rx="7"/> <path d="M12 6v4"/>',
  monitor: '<rect width="20" height="14" x="2" y="3" rx="2"/> <path d="M8 21h8"/> <path d="M12 17v4"/>',
  keyboard: '<path d="M10 8h.01"/> <path d="M12 12h.01"/> <path d="M14 8h.01"/> <path d="M16 12h.01"/> <path d="M18 8h.01"/> <path d="M6 8h.01"/> <path d="M7 16h10"/> <path d="M8 12h.01"/> <rect width="20" height="16" x="2" y="4" rx="2"/>',
  Mic: '<path d="M12 19v3"/> <path d="M19 10v2a7 7 0 0 1-14 0v-2"/> <rect x="9" y="2" width="6" height="13" rx="3"/>',
  Chat: '<path d="M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719"/>',
  Music: '<path d="M21 16V3a1 1 0 0 0-1.28-.96l-10 2.917a1 1 0 0 0-.72.96V19"/> <circle cx="18" cy="16" r="3"/> <circle cx="6" cy="19" r="3"/>',
  System: '<rect width="20" height="14" x="2" y="3" rx="2"/> <line x1="8" x2="16" y1="21" y2="21"/> <line x1="12" x2="12" y1="17" y2="21"/>',
  Game: '<line x1="6" x2="10" y1="11" y2="11"/> <line x1="8" x2="8" y1="9" y2="13"/> <line x1="15" x2="15.01" y1="12" y2="12"/> <line x1="18" x2="18.01" y1="10" y2="10"/> <path d="M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z"/>',
  Console: '<path d="M21 17a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v2a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-2Z"/> <path d="M6 15v-2"/> <path d="M12 15V9"/> <circle cx="12" cy="6" r="3"/>',
  LineIn: '<path d="M12 22v-5"/> <path d="M15 8V2"/> <path d="M17 8a1 1 0 0 1 1 1v4a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V9a1 1 0 0 1 1-1z"/> <path d="M9 8V2"/>',
  Sample: '<path d="M2 10v3"/> <path d="M6 6v11"/> <path d="M10 3v18"/> <path d="M14 8v7"/> <path d="M18 5v13"/> <path d="M22 10v3"/>',
  Headphones: '<path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3"/>',
  MicMonitor: '<path d="M6 8.5a6.5 6.5 0 1 1 13 0c0 6-6 6-6 10a3.5 3.5 0 1 1-7 0"/> <path d="M15 8.5a2.5 2.5 0 0 0-5 0v1a2 2 0 1 1 0 4"/>',
  LineOut: '<rect width="16" height="20" x="4" y="2" rx="2"/> <path d="M12 6h.01"/> <circle cx="12" cy="14" r="4"/> <path d="M12 14h.01"/>',
  BroadcastMix: '<path d="M16.247 7.761a6 6 0 0 1 0 8.478"/> <path d="M19.075 4.933a10 10 0 0 1 0 14.134"/> <path d="M4.925 19.067a10 10 0 0 1 0-14.134"/> <path d="M7.753 16.239a6 6 0 0 1 0-8.478"/> <circle cx="12" cy="12" r="2"/>',
  ChatMic: '<path d="M3 11h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-5Zm0 0a9 9 0 1 1 18 0m0 0v5a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3Z"/> <path d="M21 16v2a4 4 0 0 1-4 4h-5"/>',
  Sampler: '<path d="m12.296 3.464 3.02 3.956"/> <path d="M20.2 6 3 11l-.9-2.4c-.3-1.1.3-2.2 1.3-2.5l13.5-4c1.1-.3 2.2.3 2.5 1.3z"/> <path d="M3 11h18v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/> <path d="m6.18 5.276 3.1 3.899"/>',
  StreamMix2: '<path d="M4.9 16.1C1 12.2 1 5.8 4.9 1.9"/> <path d="M7.8 4.7a6.14 6.14 0 0 0-.8 7.5"/> <circle cx="12" cy="9" r="2"/> <path d="M16.2 4.8c2 2 2.26 5.11.8 7.47"/> <path d="M19.1 1.9a9.96 9.96 0 0 1 0 14.1"/> <path d="M9.5 18h5"/> <path d="m8 22 4-11 4 11"/>',
  mixer: '<path d="M10 8h4"/> <path d="M12 21v-9"/> <path d="M12 8V3"/> <path d="M17 16h4"/> <path d="M19 12V3"/> <path d="M19 21v-5"/> <path d="M3 14h4"/> <path d="M5 10V3"/> <path d="M5 21v-7"/>',
  routing: '<circle cx="6" cy="19" r="3"/> <path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15"/> <circle cx="18" cy="5" r="3"/>',
  light: '<circle cx="12" cy="12" r="4"/> <path d="M12 2v2"/> <path d="M12 20v2"/> <path d="m4.93 4.93 1.41 1.41"/> <path d="m17.66 17.66 1.41 1.41"/> <path d="M2 12h2"/> <path d="M20 12h2"/> <path d="m6.34 17.66-1.41 1.41"/> <path d="m19.07 4.93-1.41 1.41"/>',
  scenes: '<path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z"/> <path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12"/> <path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17"/>',
  more: '<circle cx="12" cy="12" r="1"/> <circle cx="19" cy="12" r="1"/> <circle cx="5" cy="12" r="1"/>',
  chev: '<path d="m6 9 6 6 6-6"/>',
  deviceClose: '<path d="m6 6 12 12M18 6 6 18"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  speaker: '<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/> <path d="M16 9a5 5 0 0 1 0 6"/> <path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>',
  speakerOff: '<path d="M11 4.702a.7.7 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.7.7 0 0 0 11 19.298z"/> <path d="m16.5 14.5 5-5"/> <path d="m16.5 9.5 5 5"/>',
  micOff: '<path d="M12 19v3"/> <path d="M15 9.34V5a3 3 0 0 0-5.68-1.33"/> <path d="M16.95 16.95A7 7 0 0 1 5 12v-2"/> <path d="M18.89 13.23A7 7 0 0 0 19 12v-2"/> <path d="m2 2 20 20"/> <path d="M9 9v3a3 3 0 0 0 5.12 2.12"/>',
  wand: '<path d="m21.64 3.64-1.28-1.28a1.21 1.21 0 0 0-1.72 0L2.36 18.64a1.21 1.21 0 0 0 0 1.72l1.28 1.28a1.2 1.2 0 0 0 1.72 0L21.64 5.36a1.2 1.2 0 0 0 0-1.72"/> <path d="m14 7 3 3"/> <path d="M5 6v4"/> <path d="M19 14v4"/> <path d="M10 2v2"/> <path d="M7 8H3"/> <path d="M21 16h-4"/> <path d="M11 3H9"/>',
  radar: '<path d="M19.07 4.93A10 10 0 0 0 6.99 3.34"/> <path d="M4 6h.01"/> <path d="M2.29 9.62A10 10 0 1 0 21.31 8.35"/> <path d="M16.24 7.76A6 6 0 1 0 8.23 16.67"/> <path d="M12 18h.01"/> <path d="M17.99 11.66A6 6 0 0 1 15.77 16.67"/> <circle cx="12" cy="12" r="2"/> <path d="m13.41 10.59 5.66-5.66"/>',
  leaf: '<path d="M11 20a10 10 0 0010-10 25.9 25.9 0 00-1.04-7.281 1 1 0 00-1.755-.325C15.833 5.5 13 5.5 9.8 6.1A7 7 0 0011 20"/> <path d="M2 21a5 5 0 012.911-4.544C7.613 15.212 8.351 15.24 11 13"/>',
  radio: '<path d="M5 16v2"/> <path d="M19 16v2"/> <rect width="20" height="8" x="2" y="8" rx="2"/> <path d="M18 12h.01"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/> <path d="m9 12 2 2 4-4"/>',
  circle: '<circle cx="12" cy="12" r="10"/>',
  save: '<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/> <path d="M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7"/> <path d="M7 3v4a1 1 0 0 0 1 1h7"/>',
  trash: '<path d="M10 11v6"/> <path d="M14 11v6"/> <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/> <path d="M3 6h18"/> <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  upload: '<path d="M12 3v12"/> <path d="m17 8-5-5-5 5"/> <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>',
  download: '<path d="M12 15V3"/> <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/> <path d="m7 10 5 5 5-5"/>',
  reset: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/> <path d="M3 3v5h5"/>',
  info: '<circle cx="12" cy="12" r="10"/> <path d="M12 16v-4"/> <path d="M12 8h.01"/>',
  bulb: '<path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"/> <path d="M9 18h6"/> <path d="M10 22h4"/>',
  warn: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/> <path d="M12 9v4"/> <path d="M12 17h.01"/>',
  ok: '<circle cx="12" cy="12" r="10"/> <path d="m16 9-5.5 5.5L8 12"/>',
  plus: '<path d="M5 12h14"/> <path d="M12 5v14"/>',
  refresh: '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/> <path d="M21 3v5h-5"/> <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/> <path d="M8 16H3v5"/>',
  settings: '<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/> <circle cx="12" cy="12" r="3"/>',
  activity: '<path d="M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2"/>',
  micVocal: '<path d="m11 7.601-5.994 8.19a1 1 0 0 0 .1 1.298l.817.818a1 1 0 0 0 1.314.087L15.09 12"/> <path d="M16.5 21.174C15.5 20.5 14.372 20 13 20c-2.058 0-3.928 2.356-6 2-2.072-.356-2.775-3.369-1.5-4.5"/> <circle cx="16" cy="7" r="5"/>',
  sparkles: '<path d="M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z"/> <path d="M20 2v4"/> <path d="M22 4h-4"/> <circle cx="4" cy="20" r="2"/>',
  audioLines: '<path d="M2 10v3"/> <path d="M6 6v11"/> <path d="M10 3v18"/> <path d="M14 8v7"/> <path d="M18 5v13"/> <path d="M22 10v3"/>',
  ear: '<path d="M6 8.5a6.5 6.5 0 1 1 13 0c0 6-6 6-6 10a3.5 3.5 0 1 1-7 0"/> <path d="M15 8.5a2.5 2.5 0 0 0-5 0v1a2 2 0 1 1 0 4"/>',
  volumeX: '<path d="M11 4.702a.7.7 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.7.7 0 0 0 11 19.298z"/> <path d="m16.5 14.5 5-5"/> <path d="m16.5 9.5 5 5"/>',
  stop: '<circle cx="12" cy="12" r="10"/> <rect x="9" y="9" width="6" height="6" rx="1"/>',
  usb: '<circle cx="10" cy="7" r="1"/> <circle cx="4" cy="20" r="1"/> <path d="M4.7 19.3 19 5"/> <path d="m21 3-3 1 2 2Z"/> <path d="M9.26 7.68 5 12l2 5"/> <path d="m10 14 5 2 3.5-3.5"/> <path d="m18 12 1-1 1 1-1 1Z"/>'
};
var S = null, L = null, V = 0, page = "devices", activeModule = null, built = {}, metaKey = "";
var pending = 0;

function $(id) { return document.getElementById(id); }
function svg(name, extra) {
  return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"' + (extra || "") + '>' + (ICONS[name] || "") + '</svg>';
}
function h(tag, attrs, html) {
  var e = document.createElement(tag);
  if (attrs) for (var k in attrs) {
    if (k === "style") e.style.cssText = attrs[k];
    else if (k.slice(0, 2) === "on") e.addEventListener(k.slice(2), attrs[k]);
    else e.setAttribute(k, attrs[k]);
  }
  if (html != null) e.innerHTML = html;
  return e;
}
function chipIco(name, colour) { return '<span class="chip-ico" style="--c:' + colour + '">' + svg(name) + '</span>'; }
function esc(s) { return String(s).replace(/[&<>"]/g, function (c) { return {"&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;"}[c]; }); }
function pct(v) { return Math.round(v * 100 / 255); }
function clamp(v, a, b) { return Math.max(a, Math.min(b, v)); }
function darken(hex, f) {
  var v = parseInt(hex, 16), r = Math.round(((v >> 16) & 255) * f), g = Math.round(((v >> 8) & 255) * f), b = Math.round((v & 255) * f);
  return ((1 << 24) + (r << 16) + (g << 8) + b).toString(16).slice(1).toUpperCase();
}
function throttle(fn, ms) {
  var last = 0, timer = null, args = null;
  return function () {
    args = arguments; var now = Date.now();
    clearTimeout(timer);
    if (now - last >= ms) { last = now; fn.apply(null, args); }
    else timer = setTimeout(function () { last = Date.now(); fn.apply(null, args); }, ms - (now - last));
  };
}

/* ---------- appli Windows (Tauri) ---------- */
var TAURI = window.__TAURI__ || null;
function inApp() { return !!(TAURI && TAURI.core && TAURI.core.invoke); }
function invoke(cmd, args) { return inApp() ? TAURI.core.invoke(cmd, args || {}) : Promise.reject(new Error("hors appli")); }

/* ---------- API ---------- */
function api(path) { return fetch(path, {cache: "no-store"}).then(function (r) { if (!r.ok) throw new Error("Lecture du moteur impossible"); return r.json(); }); }
function postRequest(path, body) {
  pending++;
  return fetch(path, {method: "POST", headers: {"Content-Type": "application/json", "X-Rustudio": "1"}, body: JSON.stringify(body || {})})
    .then(function (r) { return r.json().then(function (j) {
      if (!r.ok || !j || j.ok === false) { toast((j && j.error) || "Action impossible"); V = -1; return null; }
      return j;
    }); })
    .catch(function () { toast("Le moteur Rustudio ne répond pas"); V = -1; return null; })
    .finally(function () { pending--; });
}
function post(action, body) { return postRequest("/api/" + action, body); }
function postModule(id, action, body) {
  var m = installedModule(id);
  var globalActions = ["update-check", "update-install", "update-cancel", "update-ack", "update-remind", "library-refresh", "restart-engine"];
  if (globalActions.indexOf(action) < 0) {
    if (m && Array.isArray(m.actions) && m.actions.indexOf(action) >= 0) return postRequest("/api/modules/" + encodeURIComponent(id) + "/" + encodeURIComponent(action), body);
    // Compatibilité avec les moteurs qui précèdent les commandes par module.
    if (id === "goxlr-mini" && (m ? !Object.prototype.hasOwnProperty.call(m, "actions") : !(S && S.modules && Array.isArray(S.modules.installed)))) return post(action, body);
  }
  toast("Cette commande n’est pas disponible pour cet appareil.");
  V = -1;
  return Promise.resolve(null);
}
// Les actions échouées renvoient null : leurs suites ne doivent annoncer aucun succès.
function refresh(result) {
  if (result === null) return Promise.resolve(null);
  return api("/api/state").then(function (s) { S = s; V = s.v; render(); return s; })
    .catch(function () { setOnline(false, false); return result || null; });
}
var liveChanged = true, liveKey = "", bootId = null;
var liveTimer = null, liveRequest = null, liveWake = false;
document.addEventListener("visibilitychange", function () {
  document.documentElement.classList.toggle("asleep", document.hidden);
  if (!document.hidden) wakeLive();
});
document.documentElement.classList.toggle("asleep", document.hidden);
function wakeLive() {
  if (liveTimer !== null) { clearTimeout(liveTimer); liveTimer = null; }
  if (liveRequest) { liveWake = true; return liveRequest; }
  return liveLoop();
}
function liveLoop() {
  if (liveRequest) return liveRequest;
  if (liveTimer !== null) { clearTimeout(liveTimer); liveTimer = null; }
  // Le vumetre micro necessite un echange USB supplementaire : uniquement s'il est visible.
  var level = !document.hidden && activeModule === "goxlr-mini" && (page === "mic" || !!$("sheet-meter"));
  liveRequest = api(level ? "/api/live?level=1" : "/api/live").then(function (l) {
    if (bootId && l.boot && l.boot !== bootId) { location.reload(); return; }
    bootId = l.boot || bootId;
    L = l; setOnline(true, l.connected, l.health, l.usb);
    var k = l.live.fader_positions.join() + "|" + l.live.pressed.join() + "|" + l.live.mic_db;
    if (k !== liveKey) { liveKey = k; liveChanged = true; }
    if (l.v !== V && !pending) return refresh();
  }).catch(function () { setOnline(false, false); })
    .finally(function () {
      liveRequest = null;
      var delay = liveWake ? 0 : document.hidden ? 2000 : activeModule === "goxlr-mini" ? (page === "mixer" || page === "light" ? 20 : 50) : 250;
      liveWake = false; liveTimer = setTimeout(liveLoop, delay);
    });
  return liveRequest;
}

/* ---------- curseur generique ---------- */
function Slider(o) {
  var el = h("div", {"class": "sl " + (o.vertical ? "v" : "h"), tabindex: "0", role: "slider",
    "aria-label": o.label || "Réglage", "aria-orientation": o.vertical ? "vertical" : "horizontal", "aria-valuemin": o.min, "aria-valuemax": o.max});
  var track = h("div", {"class": "sl-track"}), fill = h("div", {"class": "sl-fill"}), knob = h("div", {"class": "sl-knob"});
  track.appendChild(fill); track.appendChild(knob); el.appendChild(track);
  var value = o.value != null ? o.value : o.min, dragging = false, lockUntil = 0, deferred = null, unlockTimer = null;
  function applyDeferred() {
    unlockTimer = null;
    if (dragging || deferred === null) return;
    var wait = lockUntil - performance.now();
    if (wait > 0) { unlockTimer = setTimeout(applyDeferred, Math.ceil(wait)); return; }
    var v = deferred; deferred = null;
    if (v !== value) { if (o.smoothRemote) el.classList.add("following"); value = v; paint(); }
  }
  function frac(v) { return (v - o.min) / (o.max - o.min); }
  function paint() {
    el.setAttribute("aria-valuenow", value);
    el.setAttribute("aria-valuetext", o.format ? o.format(value) : String(value));
    var p = (o.center != null ? frac(value) : frac(value)) * 100;
    if (o.vertical) { fill.style.height = p + "%"; knob.style.bottom = p + "%"; }
    else { fill.style.width = p + "%"; knob.style.left = p + "%"; }
  }
  function fromEvent(e) {
    var r = track.getBoundingClientRect();
    var f = o.vertical ? 1 - (e.clientY - r.top) / r.height : (e.clientX - r.left) / r.width;
    f = clamp(f, 0, 1);
    return clamp(Math.round((o.min + f * (o.max - o.min)) / o.step) * o.step, o.min, o.max);
  }
  function setLocal(v, commit) {
    el.classList.remove("following");
    v = clamp(v, o.min, o.max);
    if (unlockTimer !== null) { clearTimeout(unlockTimer); unlockTimer = null; }
    deferred = null;
    lockUntil = performance.now() + (o.settleMs == null ? 900 : o.settleMs);
    if (v !== value) { value = v; paint(); if (o.onInput) o.onInput(v); }
    if (commit && o.onCommit) o.onCommit(v);
  }
  track.addEventListener("pointerdown", function (e) { if (e.button !== 0) return; el.focus({preventScroll: true}); dragging = true; track.setPointerCapture(e.pointerId); el.classList.add("active"); setLocal(fromEvent(e)); });
  track.addEventListener("pointermove", function (e) { if (dragging) setLocal(fromEvent(e)); });
  function end() { if (!dragging) return; dragging = false; el.classList.remove("active"); setLocal(value, true); }
  track.addEventListener("pointerup", end); track.addEventListener("pointercancel", end);
  track.addEventListener("lostpointercapture", end);
  el.addEventListener("keydown", function (e) {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    var next = value;
    if (e.key === "ArrowUp" || e.key === "ArrowRight") next += o.step;
    else if (e.key === "ArrowDown" || e.key === "ArrowLeft") next -= o.step;
    else if (e.key === "PageUp") next += o.step * 10;
    else if (e.key === "PageDown") next -= o.step * 10;
    else if (e.key === "Home") next = o.min;
    else if (e.key === "End") next = o.max;
    else return;
    e.preventDefault(); setLocal(next, true);
  });
  el.addEventListener("wheel", function (e) {
    if (document.activeElement !== el || e.ctrlKey || e.deltaY === 0) return;
    e.preventDefault(); setLocal(value + (e.deltaY < 0 ? 1 : -1) * (o.wheel || o.step), true);
  }, {passive: false});
  if (o.smoothRemote) el.classList.add("smooth-live");
  if (o.colour) el.style.setProperty("--c", o.colour);
  paint();
  return {
    el: el, track: track,
    set: function (v) { deferred = v; if (unlockTimer === null) applyDeferred(); },
    colour: function (c) { el.style.setProperty("--c", c); },
    get value() { return value; },
    busy: function () { return dragging || performance.now() < lockUntil; }
  };
}

/* ---------- popover, toast, feuille ---------- */
var popEl = null, popAnchor = null;
function openPop(anchor, content, place) {
  closePop();
  popEl = h("div", {"class": "pop"});
  popEl.appendChild(content);
  document.body.appendChild(popEl);
  var r = anchor.getBoundingClientRect(), pw = popEl.offsetWidth, ph = popEl.offsetHeight;
  var x = r.left, y = r.bottom + 8;
  if (place === "up-left") { x = r.right - pw; y = r.top - ph - 10; popEl.style.transformOrigin = "bottom right"; }
  if (place === "down-right") { x = r.right - pw; popEl.style.transformOrigin = "top right"; }
  x = clamp(x, 10, innerWidth - pw - 10); y = clamp(y, 10, innerHeight - ph - 10);
  popEl.style.left = x + "px"; popEl.style.top = y + "px";
  popAnchor = anchor;
  requestAnimationFrame(function () { popEl && popEl.classList.add("on"); });
}
function closePop() {
  if (!popEl) return;
  var p = popEl; popEl = null; popAnchor = null;
  p.classList.remove("on"); setTimeout(function () { p.remove(); }, 180);
}
document.addEventListener("pointerdown", function (e) {
  if (popEl && !popEl.contains(e.target) && !(popAnchor && popAnchor.contains(e.target))) closePop();
});
var toastTimer = null;
function toast(msg, actionLabel, action) {
  var t = $("toast");
  t.innerHTML = "<span></span>";
  t.firstChild.textContent = msg;
  if (actionLabel) {
    var b = h("button", {onclick: function () { t.classList.remove("on"); action(); }}, actionLabel);
    t.appendChild(b);
  }
  t.classList.add("on");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(function () { t.classList.remove("on"); }, actionLabel ? 6000 : 3200);
}
function withUndo(msg, fn) {
  var before = JSON.parse(JSON.stringify(S.state));
  return fn().then(function (r) { if (r) toast(msg, "Annuler", function () { postModule("goxlr-mini", "profile", before).then(refresh); }); return r; });
}

/* ---------- navigation ---------- */
var WORKSPACE_KEY = "rustudio.workspace.v1", workspacePrefs = loadWorkspace();
var moduleNavKey = "", engineOnline = null, engineDevice = false, engineHealth = null, engineUsb = null;
var devicesUI = {grid: null, key: ""};
var MODULE_VIEWS = {"attack-shark-r5-ultra": ["mouse"], "goxlr-mini": ["mixer", "mic", "hp", "routing", "light", "scenes"]};
var CAP_LABELS = {dpi:"DPI", polling:"Fréquence", battery:"Batterie", sensor:"Capteur", mixer: "Mixeur", microphone: "Micro", "headphone-eq": "Égaliseur casque", routing: "Routage", lighting: "Éclairage", scenes: "Scènes"};
function loadWorkspace() {
  var empty = {devices: [], dismissed: []};
  try {
    var saved = JSON.parse(window.localStorage.getItem("rustudio.workspace.v1") || "null");
    if (!saved || typeof saved !== "object") return empty;
    function ids(a) { return Array.isArray(a) ? a.filter(function (id) { return typeof id === "string" && /^[a-z0-9-]{1,64}$/i.test(id); }).slice(0, 128) : []; }
    return {devices: ids(saved.devices), dismissed: ids(saved.dismissed)};
  } catch (_) { return empty; }
}
function saveWorkspace() { try { window.localStorage.setItem(WORKSPACE_KEY, JSON.stringify(workspacePrefs)); } catch (_) {} }
function installedModules() {
  return S && S.modules && Array.isArray(S.modules.installed) ? S.modules.installed : [];
}
function installedModule(id) { return installedModules().find(function (m) { return m.id === id; }) || null; }
function modulePages(m) {
  var supported = MODULE_VIEWS[m.id] || [];
  return (m.pages || []).filter(function (p) { return supported.indexOf(p.id) >= 0; });
}
function moduleRuntime(id) {
  var live = L && L.modules && Array.isArray(L.modules.devices) ? L.modules.devices : [];
  var saved = S && S.modules && Array.isArray(S.modules.devices) ? S.modules.devices : [];
  return live.find(function (d) { return d.id === id; }) || saved.find(function (d) { return d.id === id; }) || {id: id, connected: false, status: "disconnected"};
}
function moduleStatus(id) {
  var d = moduleRuntime(id), status = engineOnline === false ? "unavailable" : d.status;
  if (status === "ready" && !d.connected) status = "disconnected";
  var labels = {ready: "Connecté", disconnected: "Déconnecté", starting: "Connexion…", error: "Connexion impossible", blocked: "Pilote bloqué", unavailable: "Moteur indisponible"};
  return {status: labels[status] ? status : "disconnected", label: labels[status] || "Déconnecté", detail: status === "unavailable" ? "Les appareils seront relus dès que le moteur répondra." : d.error || ""};
}
function hasDevice(id) { return workspacePrefs.devices.indexOf(id) >= 0; }
function rememberDetected() {
  var changed = false;
  if (engineOnline === false) return;
  installedModules().forEach(function (m) {
    if (moduleRuntime(m.id).connected && !hasDevice(m.id) && workspacePrefs.dismissed.indexOf(m.id) < 0) {
      workspacePrefs.devices.push(m.id); changed = true;
    }
  });
  if (changed) saveWorkspace();
}
function addDevice(id, repaint) {
  if (!installedModule(id)) return false;
  if (!hasDevice(id)) workspacePrefs.devices.push(id);
  workspacePrefs.dismissed = workspacePrefs.dismissed.filter(function (x) { return x !== id; });
  saveWorkspace();
  if (repaint !== false) { renderDevices(); if (page === "modules") paintLibrary(); }
  return true;
}
function confirmRemoveDevice(id) {
  var m = installedModule(id); if (!m) return;
  var actions = [["Annuler", null]];
  if (id === "goxlr-mini") {
    actions.push(["Exporter la configuration", function () {
      api("/api/modules/goxlr-mini/state").then(function (data) {
        var url = URL.createObjectURL(new Blob([JSON.stringify(data.state, null, 2)], {type: "application/json"}));
        h("a", {href: url, download: "rustudio-config.json"}).click();
        setTimeout(function () { URL.revokeObjectURL(url); }, 10000);
        confirmRemoveDevice(id);
      }).catch(function () { toast("Export impossible. L’appareil est conservé."); });
    }]);
    actions.push(["Importer une configuration", function () { openDevice(id); $("import-file").click(); }]);
  }
  actions.push(["Retirer de mon espace", function () { removeDevice(id); }, "danger"]);
  sheet({icon: "usb", title: "Retirer " + esc(m.name) + " ?", text: "Ce retrait masque uniquement l’appareil : ses réglages restent conservés. Tu pourras le réajouter depuis Modules. Exporte une sauvegarde avant toute réinitialisation ou désinstallation : ces opérations peuvent supprimer ta configuration. Importer une configuration remplace les réglages actuels.", actions: actions});
}
function removeDevice(id) {
  var m = installedModule(id); if (!m) return;
  workspacePrefs.devices = workspacePrefs.devices.filter(function (x) { return x !== id; });
  if (workspacePrefs.dismissed.indexOf(id) < 0) workspacePrefs.dismissed.push(id);
  saveWorkspace();
  if (activeModule === id) go("devices"); else { renderDevices(); if (page === "modules") paintLibrary(); }
  toast(m.name + " retiré de ton espace");
}
function openDevice(id) {
  var m = installedModule(id); if (!m) return;
  var pages = modulePages(m);
  go(id + "/" + (pages.length ? pages[0].id : "overview"));
}
function workspaceRuntimeChanged() {
  rememberDetected();
  if (page === "devices") renderDevices();
  if (page === "modules") paintLibrary();
  if (page === "device" && activeModule) renderDeviceOverview();
}
var PAGES = {
  mouse: {title:"Ma souris", sub:"Sensibilité, précision et autonomie de ta R5 Ultra."},
  devices: {title: "Mes appareils", sub: "Ton matériel et ses réglages, réunis au même endroit."},
  modules: {title: "Modules", sub: "Choisis les modules qui correspondent à ton matériel."},
  device: {title: "Mon appareil", sub: "L’état de ton appareil et les fonctions de son module."},
  mixer: {title: "Mixeur", sub: "Tes quatre faders et toutes les voies, en direct."},
  mic: {title: "Micro", sub: "Détection, gain et traitement de ta voix."},
  hp: {title: "Casque", sub: "Égaliseur, correction de ton casque et profils."},
  routing: {title: "Routage", sub: "Choisis qui entend quoi : toi, ton stream et tes amis."},
  light: {title: "Éclairage", sub: "Couleurs, thèmes et animations de l'appareil."},
  scenes: {title: "Scènes", sub: "Des configurations complètes en un clic, et des conseils."}
};
function resolveRoute(path) {
  path = String(path || "devices").replace(/^#/, "");
  if (path === "library") path = "modules";
  if (path === "devices" || path === "modules") return {page: path, module: null, path: path};
  if (MODULE_VIEWS["goxlr-mini"].indexOf(path) >= 0) path = "goxlr-mini/" + path;
  var parts = path.split("/"), m = installedModule(parts[0]);
  if (m && parts.length === 2 && parts[1] === "overview") return {page: "device", module: m.id, path: path};
  if (m && parts.length === 2 && modulePages(m).some(function (p) { return p.id === parts[1]; })) return {page: parts[1], module: m.id, path: path};
  return {page: "devices", module: null, path: "devices"};
}
function renderModuleNav() {
  var m = installedModule(activeModule), key = JSON.stringify(m ? [m.id, m.name, modulePages(m)] : null);
  $("module-space").hidden = !m;
  if (key !== moduleNavKey) {
    moduleNavKey = key; $("module-nav").innerHTML = "";
    if (m) {
      $("module-name").textContent = m.name;
      $("module-nav").setAttribute("aria-label", m.name);
      var pages = modulePages(m);
      if (!pages.length) pages = [{id: "overview", label: "Vue d’ensemble", icon: "info"}];
      pages.forEach(function (p) {
        var route = m.id + "/" + p.id;
        var b = h("button", {"data-p": route, title: p.label, "aria-label": p.label}, svg(p.icon) + "<span>" + esc(p.label) + "</span>" + (m.id === "goxlr-mini" && p.id === "scenes" ? '<span class="badge" id="nav-badge" style="display:none"></span>' : ""));
        b.addEventListener("click", function () { go(route); }); $("module-nav").appendChild(b);
      });
    }
  }
  var route = activeModule ? activeModule + "/" + (page === "device" ? "overview" : page) : page;
  document.querySelectorAll("nav button").forEach(function (b) { var on = b.dataset.p === route; b.classList.toggle("on", on); if (on) b.setAttribute("aria-current", "page"); else b.removeAttribute("aria-current"); });
}
function go(path, replace) {
  var route = resolveRoute(path);
  var changed = page !== route.page || activeModule !== route.module;
  if (sheetState && (page !== route.page || activeModule !== route.module)) closeSheet();
  page = route.page; activeModule = route.module; closePop();
  if (changed && $("main-content")) $("main-content").scrollTop = 0;
  if (activeModule) addDevice(activeModule, false);
  if (location.hash !== "#" + route.path) history[replace ? "replaceState" : "pushState"](null, "", "#" + route.path);
  document.querySelectorAll(".page").forEach(function (e) { e.classList.toggle("on", e.id === "p-" + page); });
  renderModuleNav();
  var m = installedModule(activeModule);
  $("h-title").textContent = page === "device" && m ? m.name : PAGES[page].title; $("h-sub").textContent = PAGES[page].sub;
  $("h-sub").hidden = page === "mouse";
  $("hdr-mic").hidden = activeModule !== "goxlr-mini";
  setOnline(engineOnline, engineDevice, engineHealth, engineUsb);
  render();
}
var HEALTH_TXT = {
  absent: ["warn", "GoXLR éteinte ou débranchée. Elle sera reprise toute seule dès qu’elle revient."],
  error: ["warn", "Connexion à la GoXLR impossible pour l’instant. Nouvel essai automatique…"],
  blocked: ["", "Le pilote USB ne répond plus. Le moteur se relance tout seul dans quelques secondes…"],
  starting: ["info", "Recherche de la GoXLR…"]
};
// ce que Windows voit sur l'USB quand le pilote ne trouve pas la GoXLR
var USB_TXT = {
  none: "Windows ne voit pas la GoXLR sur l’USB. Si elle est allumée, son câble ne fait sans doute que charger : reprends le câble d’origine, sur un port à l’arrière du PC.",
  failed: "Un appareil USB n’arrive pas à se présenter à Windows, sans doute la GoXLR : essaie un autre câble ou un autre port.",
  full: "Une GoXLR complète est branchée : Rustudio pilote pour l’instant la GoXLR Mini (le module GoXLR est à contribuer).",
  mini: "Windows voit la GoXLR, mais son pilote ne la trouve pas encore. Si ça dure, débranche-la puis rebranche-la."
};
function setOnline(daemon, dev, health, usb) {
  engineOnline = daemon; engineDevice = dev; engineHealth = health; engineUsb = usb;
  var o = $("offline");
  var msg = null, kind = "";
  if (daemon === false) msg = "Le moteur Rustudio ne répond pas. Relance automatique en cours…";
  else if (activeModule === "goxlr-mini") {
    if (health && health !== "ok" && HEALTH_TXT[health]) { kind = HEALTH_TXT[health][0]; msg = (health === "absent" && USB_TXT[usb]) || HEALTH_TXT[health][1]; }
    else if (daemon && !dev) msg = "GoXLR déconnectée. Reconnexion automatique dès qu’elle revient…";
  }
  o.classList.toggle("on", !!msg);
  o.classList.toggle("warn", kind === "warn");
  o.classList.toggle("info", kind === "info");
  if (msg && $("offline-txt").textContent !== msg) $("offline-txt").textContent = msg;
  $("offline-diag").hidden = activeModule !== "goxlr-mini";
  $("conn-dot").classList.toggle("ok", daemon === true);
  $("conn-txt").textContent = daemon === false ? "Moteur arrêté" : daemon ? "Moteur connecté" : "Connexion…";
  if (S) workspaceRuntimeChanged();
}
