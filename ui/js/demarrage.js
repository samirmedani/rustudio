// Rendu général et démarrage : à charger en dernier.
"use strict";

/* ---------- rendu ---------- */
function render() {
  if (!S) return;
  if (activeModule && !installedModule(activeModule)) { go("devices", true); return; }
  var meta = S.meta || {}, key = JSON.stringify([meta.outputs, meta.channels]);
  if (key !== metaKey) { built = {}; metaKey = key; }
  rememberDetected(); renderModuleNav();
  var showSim = S.sim && !/[?&]capture/.test(location.search);       // captures de la documentation : sans le badge
  $("conn-fw").innerHTML = activeModule === "goxlr-mini" ? (S.firmware ? "Firmware " + esc(S.firmware) : "") + (showSim ? '<br><span class="sim-badge">APPAREIL SIMULÉ</span>' : "") : "";
  renderUpdate();
  $("hdr-mic").hidden = activeModule !== "goxlr-mini";
  if (page === "devices") renderDevices();
  if (page === "modules") renderLibrary();
  if (page === "mouse") renderMouse();
  if (page === "device") renderDeviceOverview();
  var m = installedModule(activeModule);
  $("h-eye").textContent = m ? m.name : page === "modules" ? "À chaque matériel son module" : "Ton espace personnel";
  var creditKey = JSON.stringify(m && [m.id,m.origin,m.author,m.review_pr]);
  if (render.creditKey !== creditKey) { render.creditKey = creditKey; $("h-origin").innerHTML = ""; if (m) $("h-origin").appendChild(moduleCredit(m)); }
  if (activeModule === "goxlr-mini") {
  var mm = $("hdr-mic"), mms = muteState("Mic");
  mm.innerHTML = svg(mms ? "micOff" : "Mic") + (mms ? (mms === "all" ? "Micro coupé" : "Micro " + MUTED_TXT[mms].toLowerCase()) : "Micro actif");
  mm.classList.toggle("danger", mms === "all");
  mm.classList.toggle("warn", !!mms && mms !== "all");
  if (page === "mixer") renderMixer();
  if (page === "mic") renderMic();
  if (page === "hp") renderHp();
  if (page === "routing") renderRouting();
  if (page === "light") renderLight();
  if (page === "scenes") renderScenes(); else renderAdvice();
  var sn = S.state.scene;
  $("h-eye").textContent = m.name + " · " + (showSim ? "Appareil simulé · " : "") + (sn && sn !== "custom" ? "Scène " + sceneLabel(sn) : "Réglage personnalisé");
  }
}
var lastDev = 0;
function frame(t) {
  if (L && S && activeModule === "goxlr-mini") {
    var target = L.live.mic_db == null ? -72.2 : L.live.mic_db;
    meterShown = target > meterShown ? target : meterShown + (target - meterShown) * 0.12;
    if (target > meterPeak || t - peakT > 1200) { meterPeak = target; peakT = t; }
    if (target > -3) clipT = t;
    if (page === "mic" && built.mic) {
      meterPaint(document.querySelector("#p-mic .meter"), meterShown);
      var dbt = meterShown <= -72 ? "—" : Math.round(meterShown) + " dB";
      if ($("mic-db").textContent !== dbt) $("mic-db").textContent = dbt;
      $("mic-clip").classList.toggle("on", t - clipT < 1500);
    }
    var sm = $("sheet-meter"); if (sm) meterPaint(sm, meterShown);
    if (page === "mic" || sheetState) liveListen();
    if (liveChanged) {
      if (page === "mixer") liveMixer();
      if (page === "light" && built.light && t - lastDev > 33) { paintDevicePreview(); lastDev = t; liveChanged = false; }
      if (page !== "light") liveChanged = false;
    }
    liveJob();
  }
  requestAnimationFrame(frame);
}
function init() {
  document.querySelectorAll("#main-nav button").forEach(function (b) {
    b.insertAdjacentHTML("afterbegin", svg(b.dataset.i));
    b.addEventListener("click", function () { go(b.dataset.p); });
  });
  $("fab").innerHTML = svg("more");
  $("fab").addEventListener("click", function () { if (popEl) closePop(); else openMenu(); });
  $("hdr-mic").addEventListener("click", function () { if (!S || activeModule !== "goxlr-mini") return; toggleMute("Mic", "all"); });
  $("offline-diag").addEventListener("click", openDiag);
  document.querySelector("main").addEventListener("scroll", closePop, {passive: true});
  $("ver-check").innerHTML = svg("refresh");
  $("ver-check").addEventListener("click", checkUpdate);
  $("upd-btn").addEventListener("click", openUpdateSheet);
  $("ver-txt").addEventListener("click", openHistorySheet);
  $("ver-txt").setAttribute("role", "button");
  $("ver-txt").setAttribute("tabindex", "0");
  $("ver-txt").addEventListener("keydown", function (e) { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openHistorySheet(); } });
  $("ver-state").addEventListener("click", function () { if (S && S.update && S.update.status === "available") openUpdateSheet(); });
  $("import-file").addEventListener("change", function (e) {
    var f = e.target.files[0]; if (!f) return;
    f.text().then(function (t) { var p = JSON.parse(t); return withUndo("Configuration importée", function () { return postModule("goxlr-mini", "profile", p).then(refresh); }); })
      .catch(function () { toast("Fichier illisible"); });
    e.target.value = "";
  });
  document.addEventListener("keydown", function (e) { if (e.key === "Escape") { closePop(); if (sheetState && !sheetState.job) closeSheet(); } });
  document.addEventListener("contextmenu", function (e) { if (e.target.tagName !== "INPUT") e.preventDefault(); });
  window.addEventListener("hashchange", function () { go(location.hash.slice(1), true); });
  refresh().then(function () { go(location.hash.slice(1) || "devices", true); });
  winInit();
  liveLoop();
  requestAnimationFrame(frame);
}
init();
