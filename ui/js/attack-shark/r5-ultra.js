// Attack Shark R5 Ultra : tableau de bord de la souris.
"use strict";

/* ---------- R5 Ultra : controles propres a la souris ---------- */
var mouseUI = null, mouseState = null, mouseLastState = null, mouseLoading = false, mouseBusy = false, mouseReadAt = 0;
var mouseDraft = {}, mouseDraftProfile = null, mouseReadEpoch = 0, mouseNotice = "", mouseSaving = false;
function mouseValue(name) {
  if (Object.prototype.hasOwnProperty.call(mouseDraft, name)) return mouseDraft[name];
  var known = mouseState || mouseLastState;
  if (!known) return null;
  var dpi = /^dpi-(\d+)-([xy])$/.exec(name);
  return dpi ? known.stages[Number(dpi[1])][dpi[2]] : known[name];
}
function mouseEdit(name, value) {
  if (!mouseState || mouseBusy || (mouseDraftProfile !== null && mouseDraftProfile !== mouseState.profile)) return;
  mouseLastState = mouseState;
  var dpi = /^dpi-(\d+)-([xy])$/.exec(name);
  var actual = dpi ? mouseState.stages[Number(dpi[1])][dpi[2]] : mouseState[name];
  var equal = dpi ? value !== "" && Number(value) === actual : value === actual;
  if (equal) delete mouseDraft[name]; else mouseDraft[name] = value;
  mouseDraftProfile = Object.keys(mouseDraft).length ? mouseState.profile : null;
  mouseNotice = ""; paintMouse();
}
function mouseDiscard() {
  if (mouseBusy) return;
  mouseDraft = {}; mouseDraftProfile = null; mouseNotice = ""; paintMouse();
}
function mouseChanges() {
  if (!mouseState) throw new Error("Réveille ou reconnecte la souris avant d’enregistrer.");
  if (mouseDraftProfile !== null && mouseDraftProfile !== mouseState.profile) throw new Error("Le profil a changé. Reviens au profil modifié ou annule tes modifications.");
  var changes = {}, stages = mouseState.stages.map(function (v) { return {x:v.x,y:v.y}; });
  Object.keys(mouseDraft).forEach(function (name) {
    var value = mouseDraft[name], dpi = /^dpi-(\d+)-([xy])$/.exec(name);
    if (dpi) {
      var index = Number(dpi[1]), n = Number(value);
      if (!stages[index] || value === "" || !Number.isInteger(n) || n < 50 || n > mouseState.dpi_max || n % 50 !== 0) throw new Error("Choisis des DPI entre 50 et " + mouseState.dpi_max + ", par pas de 50.");
      stages[index][dpi[2]] = n;
    } else if (value !== mouseState[name]) changes[name] = value;
  });
  if (JSON.stringify(stages) !== JSON.stringify(mouseState.stages)) changes.stages = stages;
  if (mouseState.wired && changes.polling_hz > 1000) throw new Error("Le mode USB est limité à 1 000 Hz. Choisis une fréquence compatible.");
  return changes;
}
function mouseSave() {
  if (mouseBusy) return Promise.resolve(null);
  var changes;
  try { changes = mouseChanges(); } catch (e) { toast(e.message); return Promise.resolve(null); }
  if (!Object.keys(changes).length) { mouseDiscard(); return Promise.resolve(null); }
  var profile = mouseDraftProfile;
  mouseBusy = true; mouseSaving = true; mouseReadEpoch++; mouseNotice = ""; paintMouse();
  return postModule("attack-shark-r5-ultra", "settings", {profile:profile,changes:changes}).then(function (r) {
    if (r && r.ok && r.state) {
      mouseState = r.state; mouseLastState = r.state; mouseDraft = {}; mouseDraftProfile = null; toast("Modifications enregistrées");
    } else mouseNotice = "Enregistrement interrompu. Tes modifications sont conservées.";
    return r;
  }).finally(function () { mouseBusy = false; mouseSaving = false; mouseReadAt = 0; paintMouse(); loadMouse(); });
}
var MOUSE_EMPTY = {
  asleep: ["Ta R5 Ultra est en veille", "Bouge-la ou clique une fois : ses réglages s’afficheront ici automatiquement."],
  disconnected: ["R5 Ultra introuvable", "Branche son récepteur 2,4 GHz ou son câble USB : Rustudio la retrouve tout seul."],
  error: ["Lecture impossible", "Rustudio réessaie automatiquement."],
  starting: ["Recherche de la souris…", "Lecture de ses réglages en cours."],
};
function renderMouse() {
  if (!mouseUI) {
    var root = $("p-mouse"); root.innerHTML = "";
    var toolbar = h("div", {"class":"mouse-toolbar"});
    var status = h("div", {"class":"mouse-status", role:"status"}); toolbar.appendChild(status);
    var refreshBtn = h("button", {"class":"pill", onclick:function () {
      if (mouseBusy) return;
      mouseBusy = true; paintMouse();
      postModule("attack-shark-r5-ultra", "refresh").then(function (r) {
        if (r) { mouseState = r.state; mouseLastState = r.state; }
      }).finally(function () { mouseBusy = false; mouseReadAt = 0; loadMouse(); paintMouse(); });
    }}, svg("refresh") + "Actualiser");
    var actions = h("div", {"class":"mouse-actions"}); actions.appendChild(refreshBtn);
    var discard = h("button", {"class":"pill", hidden:"", onclick:mouseDiscard}, "Annuler");
    var save = h("button", {"class":"pill primary", hidden:"", onclick:mouseSave}, svg("save"));
    var saveLabel = h("span", {}, "Enregistrer"); save.appendChild(saveLabel);
    actions.appendChild(discard); actions.appendChild(save); toolbar.appendChild(actions); root.appendChild(toolbar);
    var empty = h("div", {"class":"card mouse-empty", role:"status", hidden:""});
    var emptyTitle = h("b", {}), emptyText = h("p", {});
    empty.appendChild(h("span", {"class":"mouse-empty-ico", "aria-hidden":"true"}, svg("mouse"))); empty.appendChild(emptyTitle); empty.appendChild(emptyText); root.appendChild(empty);
    var stats = {}, strip = h("div", {"class":"mouse-stats", "aria-label":"État de la souris"});
    [["battery","battery","Batterie"],["dpi","mouse","Sensibilité active"],["polling","activity","Fréquence"],["link","usb","Connexion"]].forEach(function (spec) {
      var card = h("div", {"class":"card mouse-stat", "data-stat":spec[0]});
      var label = h("div", {"class":"mouse-stat-label"}, svg(spec[1]) + esc(spec[2]));
      var value = h("strong", {}, "—"), detail = h("small", {}, "En attente de lecture");
      card.appendChild(label); card.appendChild(value); card.appendChild(detail); strip.appendChild(card);
      stats[spec[0]] = {card:card,label:label,value:value,detail:detail};
    });
    var battery = h("progress", {"class":"mouse-battery", max:"100", value:"0", "aria-label":"Charge de la batterie"});
    battery.hidden = true; stats.battery.card.appendChild(battery); root.appendChild(strip);
    var dashboard = h("div", {"class":"mouse-dashboard"}); root.appendChild(dashboard);
    var dpi = h("section", {"class":"card mouse-panel", "aria-labelledby":"mouse-dpi-title"}, '<h2 id="mouse-dpi-title">' + svg("mouse") + 'Sensibilité</h2><p>Paliers X / Y · Pas de 50 DPI</p>');
    var activeRow = h("div", {"class":"mouse-active"});
    var active = h("select", {id:"mouse-active-stage", "class":"inp", "aria-label":"Palier DPI actif"});
    active.addEventListener("change", function () { mouseEdit("active_stage", Number(active.value)); });
    activeRow.appendChild(h("label", {"for":"mouse-active-stage"}, "Palier actif")); activeRow.appendChild(active); dpi.appendChild(activeRow);
    dpi.appendChild(h("div", {"class":"mouse-dpi-head", "aria-hidden":"true"}, '<span>N°</span><span>Axe X · DPI</span><span>Axe Y · DPI</span>'));
    var rows = h("div", {"class":"mouse-dpi"}); dpi.appendChild(rows);
    dpi.appendChild(h("p", {"class":"mouse-help"}, "Modifie tes paliers, puis clique sur Enregistrer pour les appliquer.")); dashboard.appendChild(dpi);
    var settings = h("section", {"class":"card mouse-panel", "aria-labelledby":"mouse-sensor-title"}, '<h2 id="mouse-sensor-title">' + svg("settings") + 'Capteur et autonomie</h2><p>Choisis tes réglages, puis clique sur Enregistrer.</p>');
    var fields = h("div", {"class":"mouse-settings"}); settings.appendChild(fields);
    var controls = {};
    function select(name, label, options) {
      var wrap = h("label", {"class":"mouse-field"}, "<span>" + label + "</span>");
      var el = h("select", {"class":"inp", "aria-label":label});
      options.forEach(function (o) { el.appendChild(h("option", {value:String(o[0])}, o[1])); });
      el.addEventListener("change", function () { mouseEdit(name, Number(el.value)); });
      wrap.appendChild(el); fields.appendChild(wrap); controls[name] = el;
    }
    select("polling_hz", "Fréquence de la souris", [125,250,500,1000,2000,4000,8000].map(function (n) { return [n,n.toLocaleString("fr-FR") + " Hz"]; }));
    select("lod_mm", "Hauteur de décrochage", [[0.7,"0,7 mm"],[1,"1 mm"],[2,"2 mm"]]);
    select("debounce_ms", "Délai anti-double clic", Array.from({length:31}, function (_,i) { return [i,i + " ms"]; }));
    select("sleep_seconds", "Mise en veille", [[30,"30 secondes"],[60,"1 minute"],[120,"2 minutes"],[300,"5 minutes"],[600,"10 minutes"],[65535,"Désactivée"]]);
    var toggles = h("div", {"class":"mouse-toggles"}); settings.appendChild(toggles);
    [["angle_snap","Correction des lignes","routing"],["motion_sync","Synchronisation du capteur","refresh"],["ripple_control","Lissage des mouvements","activity"]].forEach(function (spec) {
      var row = h("div", {"class":"mouse-toggle"}, "<span>" + svg(spec[2]) + esc(spec[1]) + "</span>");
      var button = h("button", {"class":"switch", role:"switch", "aria-label":spec[1], "aria-checked":"false", onclick:function () { if (mouseState) mouseEdit(spec[0], !mouseValue(spec[0])); }});
      row.appendChild(button); toggles.appendChild(row); controls[spec[0]] = button;
    });
    settings.appendChild(h("p", {"class":"mouse-help"}, "USB : jusqu’à 1 000 Hz · Récepteur compatible : jusqu’à 8 000 Hz.")); dashboard.appendChild(settings);
    root.appendChild(h("p", {"class":"workspace-note"}, "Branche le câble USB ou le récepteur 2,4 GHz. Bluetooth, macros, remappage des boutons et firmware ne sont pas pris en charge."));
    mouseUI = {empty:empty, emptyTitle:emptyTitle, emptyText:emptyText, strip:strip, dashboard:dashboard, status:status, active:active, rows:rows, controls:controls, refresh:refreshBtn, root:root, count:0, stats:stats, battery:battery, toolbar:toolbar, save:save, saveLabel:saveLabel, discard:discard};
  }
  paintMouse(); loadMouse();
}
function loadMouse() {
  if (mouseLoading || mouseBusy || Date.now() - mouseReadAt < 1000) return;
  mouseLoading = true; mouseReadAt = Date.now(); var epoch = mouseReadEpoch;
  api("/api/modules/attack-shark-r5-ultra/state").then(function (data) {
    if (epoch !== mouseReadEpoch) return;
    mouseState = data.device.connected ? data.state : null;
    if (mouseState) mouseLastState = mouseState;
    if (!data.device.connected) mouseUI.status.textContent = data.device.error || "Souris déconnectée";
    paintMouse();
  }).catch(function () { if (epoch === mouseReadEpoch) { mouseState = null; mouseUI.status.textContent = "Lecture impossible. Réessaie avec Actualiser."; } }).finally(function () { mouseLoading = false; paintMouse(); });
}
function paintMouse() {
  if (!mouseUI) return;
  var u = mouseUI, live = mouseState, s = mouseState || mouseLastState, dirty = Object.keys(mouseDraft).length > 0;
  var conflict = !!live && mouseDraftProfile !== null && mouseDraftProfile !== s.profile;
  u.save.hidden = !dirty; u.discard.hidden = !dirty;
  u.save.disabled = mouseBusy || !live || conflict; u.discard.disabled = mouseBusy;
  u.saveLabel.textContent = mouseSaving ? "Enregistrement…" : "Enregistrer";
  u.toolbar.classList.toggle("pending", dirty);
  u.refresh.disabled = mouseBusy;
  u.refresh.classList.toggle("working", mouseBusy);
  u.active.disabled = !live || mouseBusy || conflict;
  Object.keys(u.controls).forEach(function (key) { u.controls[key].disabled = !live || mouseBusy || conflict; });
  u.rows.querySelectorAll("input,button").forEach(function (el) { el.disabled = !live || mouseBusy || conflict; });
  // Aucun réglage lu : pas de formulaire rempli de valeurs par défaut qui ressembleraient aux vrais réglages.
  u.empty.hidden = !!s; u.strip.hidden = !s; u.dashboard.hidden = !s;
  if (!s) {
    u.status.textContent = "";  // le message de la carte suffit ; la ligne garde Actualiser à droite
    var run = moduleRuntime("attack-shark-r5-ultra"), text = MOUSE_EMPTY[run.status] || MOUSE_EMPTY.starting;
    u.emptyTitle.textContent = text[0];
    u.emptyText.textContent = run.status === "error" && run.error ? run.error : text[1];
    u.empty.className = "card mouse-empty " + (run.status || "starting");
    return;
  }
  if (live) {
  var sim = moduleRuntime("attack-shark-r5-ultra").simulated;
  var status = mouseBusy ? (mouseSaving ? "Enregistrement des modifications…" : "Lecture de la souris…") : conflict ? "Profil différent : reviens au profil modifié ou annule." : mouseNotice || (dirty ? "Modifications non enregistrées" : sim ? "Appareil simulé · Réglages de démonstration" : "Souris connectée");
  if (u.status.textContent !== status) u.status.textContent = status;
  }
  // En veille : les dernières valeurs lues restent visibles, atténuées.
  var stage = s.stages[s.active_stage - 1];
  u.stats.battery.value.textContent = s.battery + " %";
  u.stats.battery.detail.textContent = s.charging ? "En charge" : s.battery <= 20 ? "Batterie faible" : s.wired ? "Alimentée en USB" : "Sur batterie";
  u.stats.battery.card.classList.toggle("low", s.battery <= 20);
  var batteryIcon = s.charging ? "batteryCharging" : "battery";
  if (u.stats.battery.icon !== batteryIcon) { u.stats.battery.label.innerHTML = svg(batteryIcon) + "Batterie"; u.stats.battery.icon = batteryIcon; }
  u.battery.hidden = false; u.battery.value = s.battery;
  u.stats.dpi.value.textContent = stage.x === stage.y ? stage.x.toLocaleString("fr-FR") : stage.x.toLocaleString("fr-FR") + " / " + stage.y.toLocaleString("fr-FR");
  u.stats.dpi.card.classList.toggle("split", stage.x !== stage.y);
  u.stats.dpi.detail.textContent = (stage.x === stage.y ? "DPI" : "DPI X / Y") + " · Palier " + s.active_stage + " / " + s.stages.length;
  u.stats.polling.value.textContent = s.polling_hz.toLocaleString("fr-FR") + " Hz";
  u.stats.polling.detail.textContent = "Fréquence enregistrée";
  u.stats.link.value.textContent = s.wired ? "USB" : "2,4 GHz";
  var linkIcon = s.wired ? "usb" : "BroadcastMix";
  if (u.stats.link.icon !== linkIcon) { u.stats.link.label.innerHTML = svg(linkIcon) + "Connexion"; u.stats.link.icon = linkIcon; }
  u.stats.link.detail.textContent = "Profil " + s.profile + " · FW " + s.firmware;
  Object.keys(u.stats).forEach(function (key) {
    u.stats[key].card.classList.toggle("stale", !live);
    if (!live) u.stats[key].detail.textContent = "Dernière valeur lue";
  });
  if (u.count !== s.stages.length) {
    u.count = s.stages.length; u.active.innerHTML = ""; u.rows.innerHTML = "";
    s.stages.forEach(function (_, i) {
      u.active.appendChild(h("option", {value:String(i+1)}, "Palier " + (i+1)));
      var row = h("div", {"class":"mouse-dpi-row"}, "<b>" + (i+1) + "</b>");
      ["x","y"].forEach(function (axis) {
        var input = h("input", {type:"number", "class":"inp", min:"50", max:String(s.dpi_max), step:"50", "aria-label":"DPI " + axis.toUpperCase() + " palier " + (i+1), "data-stage":String(i), "data-axis":axis});
        input.addEventListener("input", function () { mouseEdit("dpi-" + i + "-" + axis, input.value); }); row.appendChild(input);
      });
      u.rows.appendChild(row);
    });
  }
  u.active.value = String(mouseValue("active_stage"));
  u.rows.querySelectorAll("input").forEach(function (el) {
    if (document.activeElement !== el) el.value = mouseValue("dpi-" + el.dataset.stage + "-" + el.dataset.axis);
    el.max = s.dpi_max;
  });
  u.rows.querySelectorAll(".mouse-dpi-row").forEach(function (row, i) {
    var edited = ["x","y"].some(function (axis) { return Object.prototype.hasOwnProperty.call(mouseDraft, "dpi-" + i + "-" + axis); });
    row.classList.toggle("active", i + 1 === s.active_stage); row.classList.toggle("dirty", edited);
    row.classList.toggle("pending-active", i + 1 === Number(mouseValue("active_stage")) && i + 1 !== s.active_stage);
  });
  Object.keys(u.controls).forEach(function (key) {
    var el = u.controls[key], value = String(mouseValue(key));
    if (el.tagName === "BUTTON") { el.classList.toggle("on", !!mouseValue(key)); el.setAttribute("aria-checked", value); return; }
    if (!Array.from(el.options).some(function (o) { return o.value === value; })) el.appendChild(h("option", {value:value}, value + (key === "sleep_seconds" ? " secondes" : "")));
    el.value = value;
    if (key === "polling_hz") Array.from(el.options).forEach(function (o) { o.disabled = s.wired && Number(o.value) > 1000; });
  });
}
