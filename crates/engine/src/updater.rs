//! Mises a jour depuis les releases GitHub du projet (comme Wispad) : rien ne s'installe sans ton clic.
//! Taille et empreinte SHA-256 verifiees, chaque redirection controlee, retour arriere si la nouvelle version
//! ne repond pas. L'echange des fichiers se fait dans une tache a part (apply-update).
//! Retour a une version precedente : seulement a la demande, vers une release publiee de ce depot (5.0.0 ou plus
//! recente), avec les memes verifications ; c'est le moteur de la version choisie qui fait l'echange.
use crate::config::{now_epoch, Config, VERSION};
use crate::controller::Ctl;
use crate::log::log;
use crate::logf;
use rustudio_core::release::{parse_release, Names, Release};
use rustudio_core::semver;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const UPDATE_ASSET: &str = "Rustudio.zip";
pub const UPDATE_HASH_ASSET: &str = "Rustudio.sha256";
pub const UPDATE_TASK: &str = "Rustudio Update";
pub const REQUIRED_PAYLOAD: [&str; 3] = ["rustudio-engine.exe", "Rustudio.exe", "manifest.json"];
pub const MAX_ZIP: u64 = 400 * 1024 * 1024;
const FIRST_CHECK: f64 = 5.0;
const MANUAL_CHECK_INTERVAL: f64 = 5.0;
const EVERY: f64 = 6.0 * 3600.0;
const OFFLINE_RETRY: f64 = 30.0 * 60.0;
/// Plus ancienne version proposee au retour : l'echange des fichiers (plan.json) et l'etat des reglages n'ont pas
/// change depuis, ses propres verifications reprennent la main sans perte.
pub const DOWNGRADE_FLOOR: &str = "5.0.0";
/// Notes de chaque version, embarquees : l'historique s'affiche aussi hors ligne.
pub const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
/// Liste des releases relue au plus toutes les 10 minutes (sauf demande explicite).
const HISTORY_TTL: f64 = 600.0;
/// Copies de l'etat gardees avant un retour a une version precedente.
const STATE_BACKUPS: usize = 5;

/// Sections « ## x.y.z » du CHANGELOG, dans l'ordre du fichier : (version, notes). « ## À venir » est ignore.
pub fn changelog_sections(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut cur: Option<(String, String)> = None;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            out.extend(cur.take());
            let v = h.trim().trim_start_matches('[').split([']', ' ']).next().unwrap_or("").to_string();
            cur = semver::parse(&v).is_some().then(|| (v, String::new()));
            continue;
        }
        if let Some((_, n)) = cur.as_mut() {
            n.push_str(line);
            n.push('\n');
        }
    }
    out.extend(cur);
    out.into_iter().map(|(v, n)| (v, n.trim().to_string())).collect()
}

fn newest_first(a: &str, b: &str) -> std::cmp::Ordering {
    if semver::lt(a, b) {
        std::cmp::Ordering::Greater
    } else if semver::lt(b, a) {
        std::cmp::Ordering::Less
    } else {
        std::cmp::Ordering::Equal
    }
}

/// Notes de la derniere version, suivies de celles des versions sautees (de la plus recente a la plus ancienne) :
/// qui passe de la 5.0 a la 5.2 lit aussi ce que la 5.1 apportait.
pub fn compose_notes(latest: &str, older: &[(String, String)]) -> String {
    let mut s = latest.trim_end().to_string();
    for (v, n) in older {
        if !n.trim().is_empty() {
            s.push_str(&format!("\n\n#### Aussi dans la {v}\n\n{}", n.trim()));
        }
    }
    s.chars().take(60_000).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    let d = Sha256::digest(data);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn read_limited(r: &mut dyn Read, limit: u64) -> Result<Vec<u8>, ()> {
    let mut out = vec![];
    r.take(limit + 1).read_to_end(&mut out).map_err(|_| ())?;
    if out.len() as u64 > limit {
        return Err(());
    }
    Ok(out)
}

pub fn read_json(path: &Path) -> Option<Value> {
    let t = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok().filter(|v| v.is_object())
}

#[derive(Debug)]
pub struct UpdateError {
    pub code: &'static str,
    pub msg: String,
}
fn uerr(code: &'static str, msg: impl Into<String>) -> UpdateError {
    UpdateError { code, msg: msg.into() }
}

/// Ou chercher les releases (modifiable pour les tests).
#[derive(Clone, Debug)]
pub struct UpdCfg {
    pub repo: String,
    pub api: String,
    pub prefix: String,
    pub hosts: Vec<String>,
}
impl UpdCfg {
    pub fn from_env() -> UpdCfg {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let repo = env("RUSTUDIO_UPDATE_REPO").unwrap_or_else(|| "samirmedani/rustudio".into());
        let mut hosts: Vec<String> = ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com",
            "github-releases.githubusercontent.com"].iter().map(|s| s.to_string()).collect();
        if let Some(h) = env("RUSTUDIO_UPDATE_HOSTS") {
            hosts.extend(h.split(',').filter(|s| !s.is_empty()).map(String::from));
        }
        UpdCfg {
            api: env("RUSTUDIO_UPDATE_API").unwrap_or_else(|| "https://api.github.com".into()).trim_end_matches('/').to_string(),
            prefix: env("RUSTUDIO_UPDATE_PREFIX").unwrap_or_else(|| format!("https://github.com/{repo}/releases/download/")),
            repo,
            hosts,
        }
    }
}

#[derive(Default)]
struct St {
    status: String,
    result: Option<String>,
    latest: Option<Release>,
    checked_at: Option<f64>,
    next_at: f64,
    manual_at: f64,
    phase: Option<String>,
    progress: Option<f64>,
    error: Option<Value>,
    cancel: bool,
    installed: Option<Value>,
    /// retour demande vers cette version precedente (None : mise a jour vers la derniere)
    target: Option<String>,
    /// releases publiees (heure de lecture, de la plus recente a la plus ancienne)
    history: Option<(f64, Vec<Release>)>,
}

type Launcher = Box<dyn Fn(&Path, &Path) -> Result<(), String> + Send + Sync>;

pub struct Updater {
    pub ucfg: Mutex<UpdCfg>,
    data_dir: PathBuf,
    base_dir: PathBuf,
    port: u16,
    pub release_build: bool,
    st: Mutex<St>,
    changes: AtomicU64,
    pub wake: crate::api::Event,
    started: Mutex<f64>,
    pub apply_timeout: Mutex<Duration>,
    pub launcher: Mutex<Option<Launcher>>,
    pub force_installable: AtomicBool,
    pub current: Mutex<String>,
    /// prochaine tentative de menage de la mise a jour de GoXLR Mini 4.x (infini : rien a faire)
    legacy_next: Mutex<f64>,
}

#[derive(Debug, Clone)]
pub struct UpdPeek {
    pub status: String,
    pub result: Option<String>,
    pub phase: Option<String>,
    pub error: Option<Value>,
    pub latest: Option<String>,
    pub next_at: f64,
    pub installed: Option<Value>,
}

enum Fetch {
    Release(Release),
    Unusable,
    RateLimited(Option<f64>),
    Offline,
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host).split(':').next().unwrap_or("").to_lowercase()
}

impl Updater {
    pub fn new(cfg: &Config) -> Updater {
        let manifest = read_json(&cfg.manifest_path());
        let release_build = manifest.as_ref().and_then(|m| m.get("release")).and_then(|v| v.as_bool()).unwrap_or(false);
        let u = Updater {
            ucfg: Mutex::new(UpdCfg::from_env()),
            data_dir: cfg.data_dir.clone(),
            base_dir: cfg.base_dir.clone(),
            port: cfg.port,
            release_build,
            st: Mutex::new(St { status: "idle".into(), next_at: now_epoch() + FIRST_CHECK, ..Default::default() }),
            changes: AtomicU64::new(0),
            wake: crate::api::Event::new(),
            started: Mutex::new(now_epoch()),
            apply_timeout: Mutex::new(Duration::from_secs(90)),
            launcher: Mutex::new(None),
            force_installable: AtomicBool::new(false),
            current: Mutex::new(VERSION.to_string()),
            legacy_next: Mutex::new(if cfg!(windows) { now_epoch() + 180.0 } else { f64::INFINITY }),
        };
        u.load_result();
        u
    }
    fn st(&self) -> std::sync::MutexGuard<'_, St> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn changes(&self) -> u64 {
        self.changes.load(Ordering::Relaxed)
    }
    fn bump(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }
    fn current(&self) -> String {
        self.current.lock().unwrap().clone()
    }
    /// Version qui tourne (modifiable par les tests).
    pub fn current_version(&self) -> String {
        self.current()
    }
    fn set(&self, f: impl FnOnce(&mut St)) {
        f(&mut self.st());
        self.bump();
    }

    pub fn installable(&self) -> (bool, Option<&'static str>) {
        if self.force_installable.load(Ordering::Relaxed) {
            return (true, None);
        }
        if !self.release_build {
            return (false, Some("dev"));
        }
        if !cfg!(windows) {
            return (false, Some("os"));
        }
        let probe = self.base_dir.join(".ecriture-test");
        if std::fs::write(&probe, "ok").and_then(|_| std::fs::remove_file(&probe)).is_err() {
            return (false, Some("readonly"));
        }
        (true, None)
    }

    pub fn snapshot(&self, reminded: Option<Value>) -> Value {
        let (ok, why) = self.installable();
        let repo = self.ucfg.lock().unwrap().repo.clone();
        let s = self.st();
        let page = s.latest.as_ref().map(|l| l.url.clone()).unwrap_or_else(|| format!("https://github.com/{repo}/releases/latest"));
        json!({"current": self.current(), "release_build": self.release_build, "status": s.status, "result": s.result,
               "latest": s.latest, "checked_at": s.checked_at, "phase": s.phase, "progress": s.progress, "error": s.error,
               "installable": ok, "why_not": why, "repo": repo, "repo_url": format!("https://github.com/{repo}"),
               "release_page": page, "installed": s.installed, "target": s.target, "floor": DOWNGRADE_FLOOR,
               "check_wait": (s.manual_at + MANUAL_CHECK_INTERVAL - now_epoch()).max(0.0).ceil(), "reminded": reminded})
    }

    pub fn brief(&self) -> Value {
        let s = self.st();
        json!({"status": s.status, "latest": s.latest.as_ref().map(|l| l.version.clone()), "phase": s.phase})
    }

    pub fn check(&self, manual: bool) -> Result<(), String> {
        let before = {
            let mut s = self.st();
            if s.status == "checking" {
                return Ok(());
            }
            if manual {
                let wait = s.manual_at + MANUAL_CHECK_INTERVAL - now_epoch();
                if wait > 0.0 {
                    return Err(format!("Recherche trop rapprochée : réessaie dans {} s.", wait.ceil() as i64));
                }
                s.manual_at = now_epoch();
            }
            std::mem::replace(&mut s.status, "checking".into())
        };
        self.bump();
        let mut r = self.fetch();
        if let Fetch::Release(info) = &mut r {
            if semver::lt(&self.current(), &info.version) {
                info.notes = self.notes_since(info);
            }
        }
        let now = now_epoch();
        let keep = |b: &str, fallback: &str| if b != "idle" && b != "checking" { b.to_string() } else { fallback.to_string() };
        let mut found = None;
        {
            let mut s = self.st();
            s.checked_at = Some(now);
            match r {
                Fetch::Release(info) => {
                    let newer = semver::lt(&self.current(), &info.version);
                    s.status = if newer { "available" } else { "up-to-date" }.into();
                    s.result = Some(if newer { "found" } else { "up-to-date" }.into());
                    s.next_at = now + EVERY;
                    if newer {
                        found = Some(info.version.clone());
                    }
                    s.latest = Some(info);
                }
                Fetch::Unusable => {
                    s.status = keep(&before, "up-to-date");
                    s.result = Some("up-to-date".into());
                    s.next_at = now + EVERY;
                }
                Fetch::RateLimited(retry) => {
                    s.status = keep(&before, "idle");
                    s.result = Some("rate-limited".into());
                    s.next_at = now + (6.0 * 3600.0f64).min(300f64.max(retry.unwrap_or(3600.0)));
                }
                Fetch::Offline => {
                    s.status = keep(&before, "idle");
                    s.result = Some("offline".into());
                    s.next_at = now + OFFLINE_RETRY;
                }
            }
        }
        self.bump();
        if let Some(v) = found {
            logf!("Mise à jour disponible : {v}");
        }
        Ok(())
    }

    fn agent(&self, timeout: u64, redirects: u32) -> ureq::Agent {
        crate::net::agent(Duration::from_secs(timeout), redirects, &format!("Rustudio/{}", self.current()))
    }

    fn fetch(&self) -> Fetch {
        let c = self.ucfg.lock().unwrap().clone();
        let url = format!("{}/repos/{}/releases/latest", c.api, c.repo);
        match self.agent(8, 5).get(&url).set("Accept", "application/vnd.github+json").call() {
            Ok(resp) => {
                let mut r = resp.into_reader();
                let Ok(body) = read_limited(&mut r, 4_000_000) else { return Fetch::Offline };
                let Ok(data) = serde_json::from_slice::<Value>(&body) else { return Fetch::Offline };
                let names = Names { repo: &c.repo, asset: UPDATE_ASSET, hash_asset: UPDATE_HASH_ASSET, download_prefix: &c.prefix };
                match parse_release(&data, &names) {
                    Some(rel) => Fetch::Release(rel),
                    None => Fetch::Unusable,
                }
            }
            Err(ureq::Error::Status(404, _)) => Fetch::Unusable,
            Err(ureq::Error::Status(code, resp)) if code == 403 || code == 429 => {
                let retry = resp.header("Retry-After").and_then(|v| v.parse::<f64>().ok()).or_else(|| {
                    resp.header("X-RateLimit-Reset").and_then(|v| v.parse::<f64>().ok()).map(|t| t - now_epoch())
                });
                Fetch::RateLimited(retry)
            }
            Err(_) => Fetch::Offline,
        }
    }

    /// Notes des releases publiees entre la version installee et la derniere (une requete de plus, seulement quand
    /// une mise a jour est proposee) ; en cas d'echec, les notes de la derniere seule.
    fn notes_since(&self, latest: &Release) -> String {
        let cur = self.current();
        let Ok(list) = self.fetch_list() else { return latest.notes.clone() };
        self.st().history = Some((now_epoch(), list.clone()));
        let older: Vec<(String, String)> = list
            .into_iter()
            .filter(|r| semver::lt(&cur, &r.version) && semver::lt(&r.version, &latest.version))
            .map(|r| (r.version, r.notes))
            .collect();
        compose_notes(&latest.notes, &older)
    }

    /// Releases publiees de ce depot (ni brouillon ni pre-version), de la plus recente a la plus ancienne.
    fn fetch_list(&self) -> Result<Vec<Release>, &'static str> {
        let c = self.ucfg.lock().unwrap().clone();
        let url = format!("{}/repos/{}/releases?per_page=50", c.api, c.repo);
        match self.agent(8, 5).get(&url).set("Accept", "application/vnd.github+json").call() {
            Ok(resp) => {
                let mut rd = resp.into_reader();
                let body = read_limited(&mut rd, 12_000_000).map_err(|_| "offline")?;
                let Ok(Value::Array(list)) = serde_json::from_slice::<Value>(&body) else { return Err("offline") };
                let names = Names { repo: &c.repo, asset: UPDATE_ASSET, hash_asset: UPDATE_HASH_ASSET, download_prefix: &c.prefix };
                let mut v: Vec<Release> = list.iter().filter_map(|d| parse_release(d, &names)).collect();
                v.sort_by(|a, b| newest_first(&a.version, &b.version));
                v.dedup_by(|a, b| a.version == b.version);
                Ok(v)
            }
            Err(ureq::Error::Status(404, _)) => Ok(vec![]),
            Err(ureq::Error::Status(code, _)) if code == 403 || code == 429 => Err("rate-limited"),
            Err(_) => Err("offline"),
        }
    }

    /// La release d'une version precise, relue sur GitHub au moment du clic (adresse et empreinte a jour).
    fn fetch_tag(&self, version: &str) -> Result<Release, String> {
        let c = self.ucfg.lock().unwrap().clone();
        let url = format!("{}/repos/{}/releases/tags/v{version}", c.api, c.repo);
        match self.agent(8, 5).get(&url).set("Accept", "application/vnd.github+json").call() {
            Ok(resp) => {
                let mut rd = resp.into_reader();
                let body = read_limited(&mut rd, 4_000_000).map_err(|_| "GitHub ne répond pas : vérifie ta connexion.".to_string())?;
                let data = serde_json::from_slice::<Value>(&body).map_err(|_| "Réponse de GitHub illisible.".to_string())?;
                let names = Names { repo: &c.repo, asset: UPDATE_ASSET, hash_asset: UPDATE_HASH_ASSET, download_prefix: &c.prefix };
                parse_release(&data, &names)
                    .filter(|r| r.version == version)
                    .ok_or_else(|| format!("La {version} n’est pas une version publiée de Rustudio."))
            }
            Err(ureq::Error::Status(404, _)) => Err(format!("La {version} est introuvable dans les releases de Rustudio.")),
            Err(ureq::Error::Status(code, _)) if code == 403 || code == 429 => Err("GitHub limite les recherches : réessaie plus tard.".into()),
            Err(_) => Err("GitHub ne répond pas : vérifie ta connexion.".into()),
        }
    }

    /// Historique des versions : releases publiees (lues sur GitHub, au plus toutes les 10 minutes) et, hors ligne,
    /// les notes embarquees des versions deja parues. Une version plus ancienne publiee avec son archive peut etre
    /// reinstallee (« revertible »), jusqu'a DOWNGRADE_FLOOR.
    pub fn history(&self, refresh: bool) -> Value {
        let fresh = self.st().history.as_ref().map(|(t, _)| now_epoch() - t < HISTORY_TTL).unwrap_or(false);
        let mut error: Option<&str> = None;
        if refresh || !fresh {
            match self.fetch_list() {
                Ok(list) => self.st().history = Some((now_epoch(), list)),
                Err(e) => error = Some(e),
            }
        }
        let cur = self.current();
        let (ok, why) = self.installable();
        let (remote, fetched_at) = {
            let s = self.st();
            (s.history.as_ref().map(|(_, l)| l.clone()), s.history.as_ref().map(|(t, _)| *t))
        };
        let mut rows: Vec<(String, Value)> = vec![];
        for r in remote.iter().flatten() {
            let revertible = semver::lt(&r.version, &cur) && !semver::lt(&r.version, DOWNGRADE_FLOOR) && r.asset.is_some();
            rows.push((r.version.clone(), json!({"version": r.version, "published": r.published, "url": r.url, "notes": r.notes,
                "installed": r.version == cur, "newer": semver::lt(&cur, &r.version), "revertible": revertible, "source": "github"})));
        }
        for (v, notes) in changelog_sections(CHANGELOG) {
            if rows.iter().any(|(x, _)| *x == v) || semver::lt(&v, DOWNGRADE_FLOOR) || semver::lt(&cur, &v) {
                continue;
            }
            rows.push((v.clone(), json!({"version": v, "published": null, "url": null, "notes": notes,
                "installed": v == cur, "newer": false, "revertible": false, "source": "local"})));
        }
        rows.sort_by(|a, b| newest_first(&a.0, &b.0));
        let repo = self.ucfg.lock().unwrap().repo.clone();
        json!({"current": cur, "floor": DOWNGRADE_FLOOR, "installable": ok, "why_not": why, "error": error,
               "fetched_at": fetched_at, "releases_url": format!("https://github.com/{repo}/releases"),
               "versions": rows.into_iter().map(|(_, v)| v).collect::<Vec<_>>()})
    }

    /// Mise a jour vers la derniere version (target None) ou retour a une version precedente publiee (target).
    pub fn start_install(&self, ctl: Arc<Ctl>, target: Option<&str>) -> Result<(), String> {
        let (ok, why) = self.installable();
        if !ok {
            return Err(match why {
                Some("dev") => "Version de développement : installe la nouvelle version depuis la page de la release.",
                Some("os") => "La mise à jour intégrée fonctionne sous Windows seulement.",
                _ => "Dossier d’installation non modifiable : installe depuis la page de la release.",
            }
            .into());
        }
        let busy = |s: &St| matches!(s.phase.as_deref(), Some("downloading" | "verifying" | "extracting" | "applying"));
        if busy(&self.st()) {
            return Err("Une installation est déjà en cours.".into());
        }
        let cur = self.current();
        let target = match target.map(|t| t.trim().trim_start_matches('v').to_string()) {
            None => None,
            Some(t) if semver::parse(&t).is_none() || t.contains(['-', '+']) => return Err("Version invalide.".into()),
            Some(t) if !semver::lt(&t, &cur) => return Err(format!("La {t} n’est pas plus ancienne que la {cur} installée.")),
            Some(t) if semver::lt(&t, DOWNGRADE_FLOOR) => return Err(format!("Retour possible jusqu’à la {DOWNGRADE_FLOOR} seulement.")),
            Some(t) => Some(t),
        };
        let picked = match &target {
            Some(t) => Some(self.fetch_tag(t)?),
            None => None,
        };
        let rel = {
            let mut s = self.st();
            if busy(&s) {
                return Err("Une installation est déjà en cours.".into());
            }
            let rel = match picked {
                Some(r) => r,
                None => s.latest.clone().filter(|r| semver::lt(&cur, &r.version)).ok_or("Aucune version plus récente à installer.")?,
            };
            if rel.asset.is_none() {
                return Err(format!("Cette release ne contient pas {UPDATE_ASSET} : installe depuis sa page."));
            }
            s.phase = Some("downloading".into());
            s.progress = Some(0.0);
            s.error = None;
            s.cancel = false;
            s.target = target;
            rel
        };
        self.bump();
        std::thread::Builder::new().name("update".into()).spawn(move || ctl.updater.install(rel)).ok();
        Ok(())
    }

    pub fn cancel_install(&self) {
        let mut s = self.st();
        if matches!(s.phase.as_deref(), Some("downloading" | "verifying" | "extracting")) {
            s.cancel = true;
        }
    }
    fn cancelled(&self) -> bool {
        self.st().cancel
    }

    fn install(&self, rel: Release) {
        match self.install_inner(&rel) {
            Ok(()) => {}
            Err(e) if e.code == "cancelled" => {
                self.set(|s| {
                    s.phase = None;
                    s.progress = None;
                    s.error = None;
                    s.target = None;
                });
                log("Mise à jour annulée");
            }
            Err(e) => {
                let (code, msg) = (e.code, e.msg.clone());
                self.set(|s| {
                    s.phase = Some("failed".into());
                    s.progress = None;
                    s.error = Some(json!({"code": code, "msg": msg}));
                });
                logf!("Mise à jour : échec ({})", e.code);
            }
        }
    }

    fn install_inner(&self, rel: &Release) -> Result<(), UpdateError> {
        let work = self.data_dir.join("update");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| uerr("internal", e.to_string()))?;
        let asset = rel.asset.clone().ok_or_else(|| uerr("archive", "release sans archive"))?;
        let zpath = work.join(UPDATE_ASSET);
        self.download(&asset.url, &zpath, asset.size)?;
        self.set(|s| {
            s.phase = Some("verifying".into());
            s.progress = None;
        });
        let expected = match asset.sha256.clone() {
            Some(s) => Some(s),
            None => self.fetch_sha(asset.sha_url.as_deref())?,
        }
        .ok_or_else(|| uerr("unverifiable", "La release n’annonce pas d’empreinte SHA-256 : installation refusée."))?;
        let size = std::fs::metadata(&zpath).map(|m| m.len()).unwrap_or(0);
        if asset.size.map(|s| s != size).unwrap_or(false) {
            return Err(uerr("corrupted", "Fichier incomplet : la taille ne correspond pas à celle annoncée par GitHub."));
        }
        if sha256_file(&zpath).map_err(|e| uerr("internal", e.to_string()))? != expected.to_lowercase() {
            return Err(uerr("corrupted", "Fichier modifié ou abîmé : l’empreinte ne correspond pas à celle de la release."));
        }
        if self.cancelled() {
            return Err(uerr("cancelled", ""));
        }
        self.set(|s| s.phase = Some("extracting".into()));
        let new = work.join("new");
        safe_extract(&zpath, &new, MAX_ZIP * 3)?;
        let man = read_json(&new.join("manifest.json"));
        let mver = man.as_ref().and_then(|m| m.get("version")).and_then(|v| v.as_str()).map(String::from);
        if mver.as_deref() != Some(rel.version.as_str()) {
            return Err(uerr("archive", "L’archive ne correspond pas à la release annoncée."));
        }
        let mver = mver.unwrap();
        let target = self.st().target.clone();
        match target.as_deref() {
            Some(t) if mver != t || !semver::lt(&mver, &self.current()) || semver::lt(&mver, DOWNGRADE_FLOOR) => {
                return Err(uerr("not-older", "L’archive ne correspond pas à la version précédente demandée."));
            }
            Some(_) => {}
            None if !semver::lt(&self.current(), &mver) => {
                return Err(uerr("not-newer", "Cette version n’est pas plus récente que celle installée."));
            }
            None => {}
        }
        let missing: Vec<&str> = REQUIRED_PAYLOAD.iter().copied().filter(|p| !new.join(p).exists()).collect();
        if !missing.is_empty() {
            return Err(uerr("archive", format!("Archive incomplète : {}", missing.join(", "))));
        }
        if self.cancelled() {
            return Err(uerr("cancelled", ""));
        }
        self.set(|s| {
            s.phase = Some("applying".into());
            s.progress = None;
        });
        let downgrade = target.is_some();
        if downgrade {
            self.backup_state(&mver);
        }
        let plan = json!({"from": new, "to": self.base_dir, "version": mver, "from_version": self.current(),
                          "data_dir": self.data_dir, "port": self.port, "service": crate::config::SERVICE_NAME, "t": now_epoch(),
                          "downgrade": downgrade});
        std::fs::write(work.join("plan.json"), serde_json::to_string_pretty(&plan).unwrap())
            .map_err(|e| uerr("internal", e.to_string()))?;
        if downgrade {
            logf!("Retour {} → {mver} prêt : remplacement des fichiers", self.current());
        } else {
            logf!("Mise à jour {} → {mver} prête : remplacement des fichiers", self.current());
        }
        let launch = self.launcher.lock().unwrap();
        match launch.as_ref() {
            Some(f) => f(&new, &work).map_err(|e| uerr("launch", e))?,
            None => crate::win::schtasks_run_system(UPDATE_TASK, &new.join("rustudio-engine.exe").to_string_lossy(), "apply-update")
                .map_err(|e| uerr("launch", e))?,
        }
        drop(launch);
        self.watch_apply()
    }

    /// Avant un retour a une version precedente : copie de l'etat des reglages (les 5 dernieres sont gardees).
    fn backup_state(&self, to: &str) {
        let src = self.data_dir.join("state.json");
        if !src.is_file() {
            return;
        }
        let dir = self.data_dir.join("backups");
        let name = format!("state-{}-avant-retour-{to}.json", self.current());
        match std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(&src, dir.join(&name))) {
            Ok(_) => logf!("Réglages copiés dans backups\\{name}"),
            Err(e) => logf!("Copie des réglages impossible avant le retour : {e}"),
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut copies: Vec<(std::time::SystemTime, PathBuf)> = rd
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("state-"))
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect();
            copies.sort();
            let n = copies.len();
            for (_, p) in copies.into_iter().take(n.saturating_sub(STATE_BACKUPS)) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    /// L'installation arrete ce moteur des son debut : s'il tourne encore apres le delai, rien n'a ete touche.
    fn watch_apply(&self) -> Result<(), UpdateError> {
        let deadline = std::time::Instant::now() + *self.apply_timeout.lock().unwrap();
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
            if self.st().phase.as_deref() != Some("applying") {
                return Ok(());
            }
        }
        if cfg!(windows) {
            crate::win::schtasks_delete(UPDATE_TASK);
        }
        Err(uerr("launch", "L’installation n’a pas démarré, rien n’a été modifié. Réessaie ; si ça recommence, installe depuis la page de la release."))
    }

    /// Ouvre une adresse des releases du projet ; chaque redirection doit rester chez GitHub.
    fn open_checked(&self, url: &str, timeout: u64) -> Result<ureq::Response, UpdateError> {
        let c = self.ucfg.lock().unwrap().clone();
        if !url.starts_with(&c.prefix) {
            return Err(uerr("source", "Adresse de téléchargement hors des releases du projet : refusée."));
        }
        let agent = self.agent(timeout, 0);
        let mut cur = url.to_string();
        for _ in 0..6 {
            let host = host_of(&cur);
            if !c.hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
                return Err(uerr("source", format!("Téléchargement redirigé vers {host} : refusé.")));
            }
            let resp = match agent.get(&cur).call() {
                Ok(r) => r,
                Err(ureq::Error::Status(code, _)) => return Err(uerr("download", format!("Téléchargement refusé par GitHub ({code})."))),
                Err(e) => return Err(uerr("download", format!("Téléchargement interrompu ({}).", short(&e.to_string())))),
            };
            if (300..400).contains(&resp.status()) {
                let next = resp.header("Location").unwrap_or("").to_string();
                if next.is_empty() {
                    return Err(uerr("download", "Redirection sans adresse."));
                }
                cur = if next.starts_with("http") { next } else { format!("https://{host}{next}") };
                continue;
            }
            return Ok(resp);
        }
        Err(uerr("download", "Trop de redirections."))
    }

    fn download(&self, url: &str, path: &Path, size: Option<u64>) -> Result<(), UpdateError> {
        let resp = self.open_checked(url, 20)?;
        let total = size.or_else(|| resp.header("Content-Length").and_then(|v| v.parse().ok()));
        let mut r = resp.into_reader();
        let mut f = std::fs::File::create(path).map_err(|e| uerr("internal", e.to_string()))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut got: u64 = 0;
        loop {
            if self.cancelled() {
                return Err(uerr("cancelled", ""));
            }
            let n = r.read(&mut buf).map_err(|e| uerr("download", format!("Téléchargement interrompu ({}).", short(&e.to_string()))))?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > MAX_ZIP {
                return Err(uerr("corrupted", "Fichier trop gros."));
            }
            f.write_all(&buf[..n]).map_err(|e| uerr("internal", e.to_string()))?;
            let p = total.map(|t| ((got as f64 / t as f64) * 1000.0).round() / 1000.0);
            self.set(|s| s.progress = p);
        }
        Ok(())
    }

    fn fetch_sha(&self, url: Option<&str>) -> Result<Option<String>, UpdateError> {
        let Some(url) = url else { return Ok(None) };
        let resp = match self.open_checked(url, 10) {
            Ok(r) => r,
            Err(e) if e.code == "source" => return Err(e),
            Err(_) => return Ok(None),
        };
        let mut r = resp.into_reader();
        let mut buf = vec![0u8; 4096];
        let n = r.read(&mut buf).unwrap_or(0);
        let txt = String::from_utf8_lossy(&buf[..n]);
        Ok(txt.split_whitespace().next().filter(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit())).map(|t| t.to_lowercase()))
    }

    fn load_result(&self) -> bool {
        let p = self.data_dir.join("update").join("result.json");
        if let Some(r) = read_json(&p) {
            if now_epoch() - r.get("t").and_then(|v| v.as_f64()).unwrap_or(0.0) < 3600.0 {
                self.set(|s| s.installed = Some(r));
                return true;
            }
        }
        false
    }

    pub fn ack_installed(&self) {
        self.st().installed = None;
        let _ = std::fs::remove_file(self.data_dir.join("update").join("result.json"));
        self.bump();
    }

    /// Au demarrage : la tache d'installation et la copie preparee ne servent plus.
    pub fn cleanup_after_install(&self) -> bool {
        let work = self.data_dir.join("update");
        if !work.is_dir() {
            return true;
        }
        if self.st().phase.is_some() {
            return false;
        }
        let plan = work.join("plan.json");
        if plan.exists() && !work.join("result.json").exists() {
            let done = read_json(&plan).and_then(|p| p.get("version").and_then(|v| v.as_str()).map(String::from)) == Some(self.current());
            let age = std::fs::metadata(&plan).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs()).unwrap_or(9999);
            // l'installation ecrit son resultat apres avoir relance le moteur et rouvert l'appli : on l'attend
            if age < if done { 120 } else { 300 } {
                return false; // installation peut-etre en cours
            }
        }
        if cfg!(windows) {
            crate::win::schtasks_delete(UPDATE_TASK);
        }
        let _ = std::fs::remove_dir_all(work.join("new"));
        let _ = std::fs::remove_file(work.join(UPDATE_ASSET));
        let _ = std::fs::remove_file(&plan);
        true
    }

    pub fn tick(&self, cleaned: &mut bool) {
        let started = *self.started.lock().unwrap();
        if !*cleaned && now_epoch() > started + 30.0 {
            *cleaned = self.cleanup_after_install();
        }
        let legacy_due = now_epoch() >= *self.legacy_next.lock().unwrap();
        if legacy_due {
            let done = crate::config::legacy_data_dir().map(|d| cleanup_legacy_update(&d.join("update"))).unwrap_or(true);
            *self.legacy_next.lock().unwrap() = if done { f64::INFINITY } else { now_epoch() + 600.0 };
        }
        if self.st().installed.is_none() && now_epoch() - started < 180.0 {
            self.load_result(); // la tache d'installation ecrit son resultat apres le demarrage du moteur
        }
        let due = {
            let s = self.st();
            now_epoch() >= s.next_at && s.phase.is_none()
        };
        if due {
            let _ = self.check(false);
        }
    }

    /// Etat lisible par les tests et le diagnostic.
    pub fn peek(&self) -> UpdPeek {
        let s = self.st();
        UpdPeek {
            status: s.status.clone(),
            result: s.result.clone(),
            phase: s.phase.clone(),
            error: s.error.clone(),
            latest: s.latest.as_ref().map(|l| l.version.clone()),
            next_at: s.next_at,
            installed: s.installed.clone(),
        }
    }
    pub fn set_times(&self, started: Option<f64>, next_at: Option<f64>) {
        if let Some(t) = started {
            *self.started.lock().unwrap() = t;
        }
        if let Some(t) = next_at {
            self.st().next_at = t;
        }
    }

    pub fn run_loop(&self) {
        let mut cleaned = false;
        loop {
            self.wake.wait(Duration::from_secs(5));
            self.tick(&mut cleaned);
        }
    }
}

fn short(s: &str) -> String {
    s.chars().take(80).collect()
}

/// Extrait sans jamais ecrire hors de dest (chemins absolus, « .. », liens) ni depasser limit octets.
pub fn safe_extract(zpath: &Path, dest: &Path, limit: u64) -> Result<(), UpdateError> {
    let f = std::fs::File::open(zpath).map_err(|e| uerr("archive", e.to_string()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|_| uerr("archive", "Archive illisible."))?;
    std::fs::create_dir_all(dest).map_err(|e| uerr("internal", e.to_string()))?;
    let mut total: u64 = 0;
    for i in 0..z.len() {
        let mut file = z.by_index(i).map_err(|_| uerr("archive", "Archive illisible."))?;
        let name = file.name().replace('\\', "/");
        let parts: Vec<&str> = name.split('/').collect();
        if name.starts_with('/') || parts[0].contains(':') || parts.iter().any(|p| *p == "..") {
            return Err(uerr("archive", format!("Chemin refusé dans l'archive : {name}")));
        }
        if file.unix_mode().map(|m| m & 0o170000 == 0o120000).unwrap_or(false) {
            return Err(uerr("archive", format!("Lien refusé dans l'archive : {name}")));
        }
        total += file.size();
        if total > limit {
            return Err(uerr("archive", "Archive trop grosse."));
        }
        let target = parts.iter().filter(|p| !p.is_empty() && **p != ".").fold(dest.to_path_buf(), |acc, p| acc.join(p));
        if name.ends_with('/') {
            std::fs::create_dir_all(&target).map_err(|e| uerr("internal", e.to_string()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| uerr("internal", e.to_string()))?;
        }
        let mut out = std::fs::File::create(&target).map_err(|e| uerr("internal", e.to_string()))?;
        std::io::copy(&mut file, &mut out).map_err(|_| uerr("archive", "Archive illisible."))?;
    }
    Ok(())
}

/// Menage apres le passage depuis GoXLR Mini 4.x : sa tache « GoXLR Mini Update » (lancee « une fois a 23:59 »,
/// elle relancerait le pont le soir meme) et son dossier `update`. Rien tant que le pont peut encore tourner :
/// plan present, pas de resultat, moins de 20 minutes (l'installeur a 15 minutes au plus).
/// Renvoie vrai quand il n'y a plus rien a faire.
pub fn cleanup_legacy_update(work: &Path) -> bool {
    const LEGACY_UPDATE_TASK: &str = "GoXLR Mini Update";
    let plan = work.join("plan.json");
    if plan.exists() && !work.join("result.json").exists() {
        let age = std::fs::metadata(&plan).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
        if age.map(|a| a < 1200).unwrap_or(false) {
            return false;
        }
    }
    if cfg!(windows) {
        crate::win::schtasks_delete(LEGACY_UPDATE_TASK);
    }
    if work.is_dir() {
        let _ = std::fs::remove_dir_all(work);
        log("Passage depuis GoXLR Mini terminé : sa tâche de mise à jour et ses fichiers temporaires sont retirés");
    }
    !work.exists()
}

#[cfg(test)]
mod notes_tests {
    #[test]
    fn notes_des_versions_sautees() {
        let n = super::compose_notes("**Nouveau**\n- b\n", &[("5.1.0".into(), "**Nouveau**\n- a".into()), ("5.0.1".into(), "  ".into())]);
        assert_eq!(n, "**Nouveau**\n- b\n\n#### Aussi dans la 5.1.0\n\n**Nouveau**\n- a");
        assert_eq!(super::compose_notes("x", &[]), "x");
    }
}
