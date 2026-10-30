// Mises à jour proposées par le moteur.
"use strict";

/* ---------- mises a jour ---------- */
var UPD_PHASE = {downloading: "Téléchargement", verifying: "Vérification de l’empreinte…", extracting: "Préparation…", applying: "Installation : Rustudio redémarre, l’interface revient toute seule…"};
var UPD_WHY = {dev: "Version de développement (compilée depuis les sources) : installe la nouvelle version depuis la page de la release.",
  os: "La mise à jour intégrée fonctionne sous Windows seulement.",
  readonly: "Le dossier d’installation n’est pas modifiable : installe depuis la page de la release."};
var updShown = {};
function openUrl(u) {
  if (!/^https:\/\//.test(u)) return;
  if (inApp()) invoke("open_external", {url: u}).catch(function () { toast("Lien impossible à ouvrir"); });
  else window.open(u, "_blank");
}
function frDate(iso) { var d = new Date(iso); return isNaN(d) ? "" : d.toLocaleDateString("fr-FR"); }
function mdInline(t) {
  return esc(t).replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>").replace(/\*([^*]+)\*/g, "<i>$1</i>")
    .replace(/\[([^\]]+)\]\((https:\/\/[^)\s]+)\)/g, '<a href="$2" data-ext="1">$1</a>');
}
function mdRender(src) {
  var out = [], list = null, code = null, prev = "";     // prev : "li", "p" ou "" (ligne vide, titre...)
  function close() { if (list) { out.push("</" + list + ">"); list = null; } }
  function extend(tag, txt) { var i = out.length - 1; out[i] = out[i].slice(0, -(tag.length + 3)) + " " + mdInline(txt) + "</" + tag + ">"; }
  String(src || "").replace(/\r/g, "").split("\n").forEach(function (line) {
    if (/^```/.test(line)) { if (code === null) { close(); code = []; } else { out.push("<pre><code>" + esc(code.join("\n")) + "</code></pre>"); code = null; } prev = ""; return; }
    if (code !== null) { code.push(line); return; }
    // suite d'un point de liste (ligne indentee) ou d'un paragraphe (texte colle) : meme bloc, comme sur GitHub
    if (prev === "li" && /^\s{2,}\S/.test(line)) { extend("li", line.trim()); return; }
    if (prev === "p" && line.trim() && !/^(#{1,4}\s|\s*[-*]\s|\s*\d+[.)]\s|>)/.test(line)) { extend("p", line.trim()); return; }
    var m;
    if ((m = line.match(/^#{1,4}\s+(.*)/))) { close(); out.push("<h4>" + mdInline(m[1]) + "</h4>"); prev = ""; }
    else if ((m = line.match(/^\s*[-*]\s+(.*)/))) { if (list !== "ul") { close(); out.push("<ul>"); list = "ul"; } out.push("<li>" + mdInline(m[1]) + "</li>"); prev = "li"; }
    else if ((m = line.match(/^\s*\d+[.)]\s+(.*)/))) { if (list !== "ol") { close(); out.push("<ol>"); list = "ol"; } out.push("<li>" + mdInline(m[1]) + "</li>"); prev = "li"; }
    else if ((m = line.match(/^>\s?(.*)/))) { close(); out.push("<p><i>" + mdInline(m[1]) + "</i></p>"); prev = ""; }
    else if (line.trim()) { close(); out.push("<p>" + mdInline(line) + "</p>"); prev = "p"; }
    else { close(); prev = ""; }
  });
  close();
  if (code !== null) out.push("<pre><code>" + esc(code.join("\n")) + "</code></pre>");
  return out.join("") || "<p>Pas de notes pour cette version.</p>";
}
document.addEventListener("click", function (e) {
  var a = e.target.closest && e.target.closest("a[data-ext]");
  if (a) { e.preventDefault(); openUrl(a.getAttribute("href")); }
});
function renderUpdate() {
  var u = S.update; if (!u) return;
  $("ver-txt").textContent = "v" + u.current;
  $("ver-txt").title = "Rustudio " + u.current;
  var st = $("ver-state"), busy = u.phase && u.phase !== "failed", avail = u.status === "available" && u.latest;
  st.className = "ver-state" + (avail ? " avail" : "") + (busy ? " busy" : "");
  if (busy) st.innerHTML = "<i></i>" + (u.phase === "downloading" && u.progress != null ? "Téléchargement " + Math.round(u.progress * 100) + " %" : "Installation…");
  else if (u.phase === "failed") st.innerHTML = "<i style=\"background:var(--red)\"></i>Échec de la mise à jour";
  else if (avail) st.innerHTML = "<i></i>" + esc(u.latest.version) + " proposée";
  else if (!u.release_build) st.innerHTML = "<i style=\"background:var(--text3)\"></i>développement";
  else if (u.status === "up-to-date") st.innerHTML = "<i></i>À jour";
  else st.innerHTML = "";
  if (st.lastChild && st.lastChild.nodeType === 3) { var sp = document.createElement("span"); sp.textContent = st.lastChild.textContent; st.replaceChild(sp, st.lastChild); }
  st.title = avail ? "Mise à jour proposée : Rustudio " + u.latest.version : "";
  $("ver-check").classList.toggle("spin-on", u.status === "checking");
  $("upd-card").style.display = avail && !busy ? "" : "none";
  if (avail) $("upd-txt").textContent = "Rustudio " + u.latest.version + (u.latest.published ? ", du " + frDate(u.latest.published) : "") + ". Installe-la quand tu veux.";
  if (sheetState && sheetState.update) paintUpdateSheet();
  if (u.installed && !updShown.installed) {
    updShown.installed = true;
    var ins = u.installed;
    if (ins.ok && ins.version === u.current) toast("Rustudio " + ins.version + " est installé", "Nouveautés", function () { if (u.latest) openUpdateSheet(); });
    else if (!ins.ok) toast("La mise à jour vers " + ins.version + " a échoué : retour à " + ins.from_version);
    post("update-ack");
  }
  if (avail && !busy && u.latest.version !== u.reminded && !updShown[u.latest.version] && !sheetState && !(L && L.job)) {
    updShown[u.latest.version] = true;
    post("update-remind", {version: u.latest.version});
    toast("Mise à jour proposée : Rustudio " + u.latest.version, "Voir", openUpdateSheet);
  }
}
function checkUpdate() {
  $("ver-check").classList.add("spin-on");
  post("update-check").then(function (r) {
    if (!r || !r.update) { $("ver-check").classList.remove("spin-on"); return; }
    S.update = r.update;
    var res = r.update.result;
    if (res === "found") openUpdateSheet();
    else toast(res === "up-to-date" ? "Rustudio est à jour" : res === "rate-limited" ? "GitHub limite les recherches : réessaie plus tard" : "Recherche impossible : réessaie plus tard");
    renderUpdate();
  });
}
var NOTE_TAGS = {"nouveau": ["sparkles", "#0A84FF", "Nouveau"], "nouveautés": ["sparkles", "#0A84FF", "Nouveau"],
  "amélioré": ["activity", "#5E5CE6", "Amélioré"], "corrigé": ["check", "#30D158", "Corrigé"], "corrections": ["check", "#30D158", "Corrigé"],
  "sécurité": ["shield", "#FF9F0A", "Sécurité"], "à savoir": ["info", "#8E8E93", "À savoir"]};
function notesRender(md) {
  var src = String(md || "").replace(/^\*\*([^*]+)\*\*\s*$/gm, "#### $1");
  return mdRender(src).replace(/<h4>([^<]+)<\/h4>/g, function (m, t) {
    var k = NOTE_TAGS[t.trim().toLowerCase()];
    return k ? '<h4 class="ntag" style="--c:' + k[1] + '">' + svg(k[0]) + k[2] + "</h4>" : "<h4>" + t + "</h4>";
  });
}
function openUpdateSheet() {
  var u = S.update; if (!u || !u.latest) return;
  var body = h("div");
  sheet({wide: true, update: true, body: body, actions: []});
  paintUpdateSheet();
}
function paintUpdateSheet() {
  var u = S.update, b = sheetState.body, rel = u.latest || {};
  var key = JSON.stringify([u.phase, Math.round((u.progress || 0) * 50), u.error, rel.version, u.installable, u.status]);
  if (sheetState.key === key) return;
  sheetState.key = key;
  var newer = u.status === "available", busy = u.phase && u.phase !== "failed";
  var html = '<div class="upd-hero">' + chipIco(newer ? "sparkles" : "ok", newer ? "#0A84FF" : "#30D158") + '<div><div class="eyebrow">' + (newer ? "Mise à jour proposée" : "À jour") + "</div><h2>Rustudio " + esc(newer ? rel.version : u.current) + "</h2><p class=\"muted-txt\">" + (newer ? "Tu as la " + esc(u.current) + (rel.published ? " · publiée le " + frDate(rel.published) : "") : "Tu as la dernière version.") + "</p></div></div>";
  html += '<div class="notes rel">' + notesRender(rel.notes) + "</div>";
  if (busy) {
    var pct = u.phase === "downloading" && u.progress != null ? Math.round(u.progress * 100) : null;
    html += '<div class="upd-progress"><div class="bar' + (pct == null ? " indet" : "") + '"><i style="width:' + (pct || 0) + '%"></i></div><small>' + UPD_PHASE[u.phase] + (pct != null ? " " + pct + " %" : "") + "</small></div>";
  } else if (u.phase === "failed" && u.error) {
    html += '<div class="upd-fail">' + esc(u.error.msg) + " L’ancienne version reste en place.</div>";
  } else if (newer && !u.installable) {
    html += '<div class="upd-why">' + (UPD_WHY[u.why_not] || "") + "</div>";
  } else if (newer) {
    html += '<p class="upd-confirm">' + svg("info") + "<span>Rustudio redémarre quelques secondes et tes réglages sont gardés. Si la nouvelle version ne répond pas, l’ancienne revient toute seule.</span></p>";
  }
  b.innerHTML = html;
  var acts = $("sheet-box").querySelector(".sheet-actions");
  acts.innerHTML = "";
  function btn(label, fn, cls, keep) { var x = h("button", {"class": "pill " + (cls || "")}, label); x.addEventListener("click", function () { if (!keep) closeSheet(); fn && fn(); }); acts.appendChild(x); }
  if (busy) {
    if (u.phase !== "applying") btn("Annuler", function () { post("update-cancel"); }, "", true);
    return;
  }
  btn("Voir sur GitHub", function () { openUrl(u.release_page); }, "", true);
  if (u.phase === "failed") {
    btn("Fermer", null);
    btn("Réessayer", function () { post("update-install").then(refresh); }, "primary", true);
  } else if (newer) {
    btn("Plus tard", null);
    if (u.installable) btn("Mettre à jour maintenant", function () { post("update-install").then(refresh); }, "primary", true);
    else btn("Ouvrir la page de la release", function () { openUrl(u.release_page); }, "primary");
  } else btn("Fermer", null, "primary");
}
