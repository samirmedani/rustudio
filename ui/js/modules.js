// Espace personnel et catalogue des modules.
"use strict";

/* ---------- espace personnel et modules ---------- */
function moduleCredit(m) {
  var credit = h("div", {"class":"module-credit"});
  var origin = m.origin || m.status;
  credit.appendChild(h("span", {"class":"origin"}, svg(origin === "community" ? "users" : "shield") + (origin === "official" ? "Officiel Rustudio" : origin === "community" ? "Communautaire" : "Origine non précisée")));
  if (/^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$/i.test(m.author || "")) credit.appendChild(h("button", {onclick:function () { openUrl("https://github.com/" + m.author); }}, "Créé par " + esc(m.author)));
  if (origin === "community" && Number.isInteger(m.review_pr) && m.review_pr > 0) credit.appendChild(h("button", {onclick:function () { openUrl("https://github.com/samirmedani/rustudio/pull/" + m.review_pr); }, title:"Validation du mainteneur sur GitHub"}, svg("pullRequest") + " PR #" + m.review_pr + " validée"));
  return credit;
}
function moduleFeatures(m) {
  return m.capabilities ? m.capabilities.map(function (c) { return CAP_LABELS[c] || c; }) : (m.features || []);
}
function deviceCard(m, overview) {
  var status = moduleStatus(m.id), d = moduleRuntime(m.id);
  var card = h("div", {"class": "card device-card" + (!overview ? " device-selectable" : ""), "data-device": m.id});
  if (!overview) {
    card.tabIndex = 0;
    card.setAttribute("aria-label", "Ouvrir " + m.name);
    card.addEventListener("click", function (e) { if (!e.target.closest("button, a")) openDevice(m.id); });
    card.addEventListener("keydown", function (e) { if (e.target === card && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); openDevice(m.id); } });
  }
  card.innerHTML = '<div class="lib-head">' + chipIco(LIB_KIND[m.kind] || "usb", "#0071E3") + "<div><b>" + esc(m.name) + "</b><small>" + esc(m.vendor || "") + "</small></div></div>";
  card.appendChild(moduleCredit(m));
  card.appendChild(h("div", {"class": "device-status " + status.status, role: "status"}, "<i></i>" + esc(status.label) + (d.simulated ? " · Appareil simulé" : "")));
  if (status.detail) card.appendChild(h("p", {"class": "device-detail"}, esc(status.detail)));
  else if (status.status === "disconnected") card.appendChild(h("p", {"class": "device-detail"}, "Branche ton appareil pour le retrouver ici. Tes réglages restent disponibles."));
  if (m.summary) card.appendChild(h("p", {"class": "device-detail"}, esc(m.summary)));
  card.appendChild(h("div", {"class": "chips lib-feat"}, moduleFeatures(m).map(function (f) { return "<span>" + esc(f) + "</span>"; }).join("")));
  if (d.firmware) card.appendChild(h("small", {"class": "muted-txt"}, "Firmware " + esc(d.firmware)));
  var actions = h("div", {"class": "lib-act"});
  if (!overview) actions.appendChild(h("button", {"class": "pill primary", onclick: function () { openDevice(m.id); }}, "Ouvrir" + svg("chev")));
  card.appendChild(h("button", {"class": "device-remove", "aria-label": "Retirer " + m.name + " de mon espace", title: "Retirer de mon espace", onclick: function (e) { e.stopPropagation(); confirmRemoveDevice(m.id); }}, svg("deviceClose")));
  card.appendChild(actions);
  return card;
}
function renderDevices() {
  var root = $("p-devices");
  if (!built.devices) {
    root.innerHTML = "";
    var top = h("div", {"class": "workspace-top"}, "<p>Chaque appareil a son propre espace. Ajoute le matériel que tu possèdes ; les appareils connectés apparaissent automatiquement.</p>");
    top.appendChild(h("button", {"class": "pill primary", onclick: function () { go("modules"); }}, svg("plus") + "Ajouter un appareil"));
    root.appendChild(top); devicesUI.grid = h("div", {"class": "device-grid"}); root.appendChild(devicesUI.grid);
    root.appendChild(h("p", {"class": "workspace-note"}, "Retirer un appareil masque sa carte. Ses réglages et son fonctionnement restent en place."));
    devicesUI.key = ""; built.devices = true;
  }
  var mine = installedModules().filter(function (m) { return hasDevice(m.id); });
  var key = JSON.stringify([mine, mine.map(function (m) { return [moduleRuntime(m.id), moduleStatus(m.id)]; })]);
  if (devicesUI.key === key) return;
  devicesUI.key = key; devicesUI.grid.innerHTML = "";
  mine.forEach(function (m) { devicesUI.grid.appendChild(deviceCard(m)); });
  if (!mine.length) {
    var empty = h("div", {"class": "card workspace-empty", style: "grid-column:1/-1"}, chipIco("usb", "#0071E3") + "<h2>Aucun appareil ajouté</h2><p>Retrouve les modules disponibles et ajoute le matériel que tu possèdes.</p>");
    empty.appendChild(h("button", {"class": "pill primary", onclick: function () { go("modules"); }}, "Explorer les modules"));
    devicesUI.grid.appendChild(empty);
  }
}
var overviewKey = "";
function renderDeviceOverview() {
  var m = installedModule(activeModule); if (!m) return;
  var key = JSON.stringify([m, moduleRuntime(m.id), moduleStatus(m.id)]);
  if (overviewKey === key) return;
  overviewKey = key;
  var root = $("p-device"); root.innerHTML = ""; root.appendChild(deviceCard(m, true));
  if (!modulePages(m).length) root.appendChild(h("p", {"class": "workspace-note"}, "Aucun réglage disponible pour cet appareil dans cette version de l’interface."));
}
var lib = {data: null, q: "", filter:"all", grid: null, filters:[]};
var LIB_STATUS = {official: ["Officiel", "#176a35"], community: ["Communautaire", "#0071E3"], wanted: ["À contribuer", "#65656a"]};
var LIB_KIND = {"audio-interface": "mixer", mouse: "mouse", keyboard: "keyboard", monitor: "monitor", headset: "Headphones"};
function renderLibrary() {
  if (built.library) return;
  var root = $("p-modules"); root.innerHTML = "";
  var top = h("div", {"class": "lib-top"});
  var q = h("input", {"class": "inp", placeholder: "Chercher un appareil, une marque…", style: "flex:1"});
  q.value = lib.q; q.setAttribute("aria-label", "Chercher un module");
  q.addEventListener("input", function () { lib.q = q.value.trim().toLowerCase(); paintLibrary(); });
  top.appendChild(q);
  top.appendChild(h("button", {"class": "pill", onclick: function () {
    post("library-refresh").then(function (d) { if (d && d.modules) { lib.data = d; paintLibrary(); toast("Catalogue vérifié de cette version"); } });
  }}, svg("refresh") + "Actualiser"));
  top.appendChild(h("button", {"class": "pill", onclick: function () { openUrl((lib.data && lib.data.contribute_url) || "https://github.com/samirmedani/rustudio"); }}, svg("pullRequest") + "Proposer un module"));
  root.appendChild(top);
  var filters = h("div", {"class":"lib-filters", role:"group", "aria-label":"Origine des modules"});
  [["all","Tous"],["official","Officiels"],["community","Communautaires"],["wanted","À contribuer"]].forEach(function (spec) {
    var button = h("button", {"aria-pressed":String(lib.filter === spec[0]), onclick:function () { lib.filter = spec[0]; paintLibrary(); }}, spec[1]);
    lib.filters.push({key:spec[0],button:button}); filters.appendChild(button);
  }); root.appendChild(filters);
  lib.grid = h("div", {"class": "lib-grid"});
  root.appendChild(lib.grid);
  root.appendChild(h("p", {"class": "muted-txt lib-foot"}, "Catalogue livré avec cette version. Les contributions communautaires passent par une pull request GitHub, une analyse et la validation du mainteneur avant publication."));
  built.library = true;
  lib.paintKey = ""; paintLibrary();
  api("/api/library").then(function (d) { lib.data = d; paintLibrary(); return null; }).then(function (d) { if (d && d.modules) { lib.data = d; paintLibrary(); } })
    .catch(function () { toast("La bibliothèque n’a pas pu être chargée. Réessaie avec Actualiser."); });
}
function paintLibrary() {
  if (!lib.grid) return;
  var modules = installedModules().slice(), known = modules.map(function (m) { return m.id; });
  ((lib.data && lib.data.modules) || []).forEach(function (m) { if (m.status === "wanted" && known.indexOf(m.id) < 0) { modules.push(m); known.push(m.id); } });
  var key = JSON.stringify([modules, workspacePrefs.devices, lib.q, lib.filter]);
  if (lib.paintKey === key) return;
  lib.paintKey = key; lib.grid.innerHTML = "";
  lib.filters.forEach(function (f) { f.button.classList.toggle("on", f.key === lib.filter); f.button.setAttribute("aria-pressed", String(f.key === lib.filter)); });
  var mods = modules.filter(function (m) { return (lib.filter === "all" || (m.origin || m.status) === lib.filter) && (!lib.q || (m.name + " " + m.vendor + " " + m.kind + " " + (m.author || "") + " " + moduleFeatures(m).join(" ")).toLowerCase().indexOf(lib.q) >= 0); });
  mods.forEach(function (m) {
    var local = installedModule(m.id), st = LIB_STATUS[m.origin || m.status] || LIB_STATUS.wanted;
    var c = h("div", {"class": "card lib-card"});
    c.innerHTML = '<div class="lib-head">' + chipIco(LIB_KIND[m.kind] || "grid", st[1]) + "<div><b>" + esc(m.name) + "</b><small>" + esc(m.vendor || "") + '</small></div><span class="lib-badge" style="--c:' + st[1] + '">' + st[0] + "</span></div><p>" + esc(m.summary || "") + '</p><div class="chips lib-feat">' + moduleFeatures(m).map(function (f) { return "<span>" + esc(f) + "</span>"; }).join("") + "</div>";
    if (local) c.appendChild(moduleCredit(local));
    var row = h("div", {"class": "lib-act"});
    if (local) {
      if (hasDevice(m.id)) { row.appendChild(h("span", {"class": "lib-on"}, svg("check") + "Dans Mes appareils")); row.appendChild(h("button", {"class": "pill primary", onclick: function () { openDevice(m.id); }}, "Ouvrir")); }
      else row.appendChild(h("button", {"class": "pill primary", onclick: function () { if (addDevice(m.id)) toast(m.name + " ajouté à Mes appareils"); }}, "Ajouter à Mes appareils"));
    } else if (m.status === "wanted") row.appendChild(h("button", {"class": "pill", onclick: function () { openUrl((lib.data && lib.data.contribute_url) || "https://github.com/samirmedani/rustudio/blob/main/docs/MODULES.md"); }}, "Contribuer ce module"));
    else {
      row.appendChild(h("span", {"class": "muted-txt"}, "Absent de cette version"));
      row.appendChild(h("button", {"class": "pill", onclick: function () { openUrl("https://github.com/samirmedani/rustudio/tree/main/" + m.source); }}, "Voir le module"));
    }
    c.appendChild(row);
    lib.grid.appendChild(c);
  });
  if (!mods.length) {
    var empty = h("div", {"class":"card workspace-empty", style:"grid-column:1/-1"}, chipIco("users", "#0071E3") + "<h2>" + (lib.filter === "community" && !lib.q ? "La communauté commence ici" : "Aucun module ne correspond") + "</h2><p>" + (lib.filter === "community" && !lib.q ? "Aucun module communautaire n’est encore publié. Propose ton appareil sur GitHub : après analyse et validation du mainteneur, il pourra rejoindre une prochaine version." : "Tu peux proposer la prise en charge de ton appareil sur GitHub.") + "</p>");
    empty.appendChild(h("button", {"class":"pill primary", onclick:function () { openUrl("https://github.com/samirmedani/rustudio/blob/main/docs/COMMUNITY.md"); }}, svg("pullRequest") + "Proposer un module")); lib.grid.appendChild(empty);
  }
}
