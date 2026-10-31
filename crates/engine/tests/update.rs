//! Mises a jour : faux GitHub local, verification de l'archive, echange des fichiers et retour arriere.
use rustudio_engine::apply::{apply_update, Hooks, Plan};
use rustudio_engine::config::Config;
use rustudio_engine::controller::Ctl;
use rustudio_engine::updater::{sha256_hex, UpdCfg};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);
fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rustudio-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn make_zip(version: &str, files: Option<Vec<(&str, &str)>>, extra: Vec<(&str, &str)>) -> Vec<u8> {
    let files = files.unwrap_or_else(|| vec![("rustudio-engine.exe", "moteur"), ("Rustudio.exe", "appli")]);
    let mut man = json!({"version": version, "release": true, "files": {}});
    for (k, _) in &files {
        man["files"][*k] = json!("x");
    }
    man["files"]["manifest.json"] = json!("");
    let mut buf = std::io::Cursor::new(vec![]);
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("manifest.json", o).unwrap();
        z.write_all(man.to_string().as_bytes()).unwrap();
        for (k, v) in files.iter().chain(extra.iter()) {
            z.start_file(*k, o).unwrap();
            z.write_all(v.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

struct FakeGitHub {
    base: String,
    release: Arc<Mutex<Value>>,
    /// toutes les releases publiees (liste et lecture par tag), la plus recente d'abord
    list: Arc<Mutex<Vec<Value>>>,
    status: Arc<Mutex<(u16, Vec<(String, String)>)>>,
    blobs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl FakeGitHub {
    fn new() -> FakeGitHub {
        let srv = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", srv.server_addr().to_ip().unwrap().port());
        let gh = FakeGitHub { base, release: Arc::new(Mutex::new(Value::Null)), list: Arc::new(Mutex::new(vec![])), status: Arc::new(Mutex::new((200, vec![]))), blobs: Arc::new(Mutex::new(HashMap::new())) };
        let (rel, list, st, blobs) = (gh.release.clone(), gh.list.clone(), gh.status.clone(), gh.blobs.clone());
        std::thread::spawn(move || {
            for req in srv.incoming_requests() {
                let url = req.url().to_string();
                if url.ends_with("/releases/latest") {
                    let (code, headers) = st.lock().unwrap().clone();
                    let mut resp = tiny_http::Response::from_string(if code == 200 { rel.lock().unwrap().to_string() } else { "{}".into() }).with_status_code(code);
                    for (k, v) in headers {
                        resp = resp.with_header(tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap());
                    }
                    let _ = req.respond(resp);
                } else if let Some(tag) = url.split("/releases/tags/").nth(1) {
                    let found = list.lock().unwrap().iter().find(|r| r["tag_name"] == tag).cloned();
                    let _ = req.respond(match found {
                        Some(r) => tiny_http::Response::from_string(r.to_string()),
                        None => tiny_http::Response::from_string("{}").with_status_code(404),
                    });
                } else if url.split('?').next().unwrap_or("").ends_with("/releases") {
                    let all = Value::Array(list.lock().unwrap().clone());
                    let _ = req.respond(tiny_http::Response::from_string(all.to_string()));
                } else if let Some(b) = blobs.lock().unwrap().get(&url).cloned() {
                    let _ = req.respond(tiny_http::Response::from_data(b));
                } else {
                    let _ = req.respond(tiny_http::Response::empty(404));
                }
            }
        });
        gh
    }
    fn publish(&self, version: &str, blob: Vec<u8>, digest: bool) {
        let path = format!("/dl/v{version}/Rustudio.zip");
        let mut asset = json!({"name": "Rustudio.zip", "size": blob.len(), "browser_download_url": format!("{}{path}", self.base)});
        if digest {
            asset["digest"] = json!(format!("sha256:{}", sha256_hex(&blob)));
        }
        self.blobs.lock().unwrap().insert(path, blob);
        let rel = json!({"tag_name": format!("v{version}"), "draft": false, "prerelease": false, "published_at": "2026-10-31T10:00:00Z",
            "body": format!("## Nouveau\n- dans la {version}"), "html_url": format!("https://github.com/x/y/releases/tag/v{version}"), "assets": [asset]});
        self.add(rel.clone());
        *self.release.lock().unwrap() = rel;
    }
    /// Release plus ancienne : listee et lisible par son tag, sans devenir la derniere.
    fn publish_old(&self, version: &str, blob: Vec<u8>) {
        let latest = self.release.lock().unwrap().clone();
        self.publish(version, blob, true);
        *self.release.lock().unwrap() = latest;
    }
    fn add(&self, rel: Value) {
        let mut l = self.list.lock().unwrap();
        l.retain(|r| r["tag_name"] != rel["tag_name"]);
        l.push(rel);
    }
}

struct U {
    ctl: Arc<Ctl>,
    gh: FakeGitHub,
    applied: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
    tmp: PathBuf,
}

impl U {
    fn new() -> U {
        rustudio_engine::log::set_echo(false);
        let tmp = tmpdir("upd");
        let data = tmp.join("data");
        let cfg = Config { simulate: true, port: 0, data_dir: data.clone(), base_dir: tmp.join("install"), supervised: false, engine_restarts: 0 };
        let (loader, sim) = rustudio_engine::make_loader(&cfg);
        let ctl = Ctl::new(cfg, loader, sim, false);
        let gh = FakeGitHub::new();
        let up = &ctl.updater;
        *up.ucfg.lock().unwrap() = UpdCfg { repo: "x/y".into(), api: gh.base.clone(), prefix: format!("{}/dl/", gh.base), hosts: vec!["127.0.0.1".into()] };
        *up.current.lock().unwrap() = "4.9.0".into();
        up.force_installable.store(true, Ordering::Relaxed);
        let applied = Arc::new(Mutex::new(vec![]));
        let a2 = applied.clone();
        *up.launcher.lock().unwrap() = Some(Box::new(move |new: &Path, work: &Path| {
            a2.lock().unwrap().push((new.to_path_buf(), work.to_path_buf()));
            Ok(())
        }));
        U { ctl, gh, applied, tmp }
    }
    fn phase(&self, want: &[&str], timeout: f64) -> String {
        let t0 = Instant::now();
        while t0.elapsed().as_secs_f64() < timeout {
            let p = self.ctl.updater.peek();
            if let Some(ph) = p.phase.as_deref() {
                if want.contains(&ph) {
                    return ph.into();
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("phase {:?}, attendu {want:?} ; erreur {:?}", self.ctl.updater.peek().phase, self.ctl.updater.peek().error);
    }
    fn install(&self) -> Result<(), String> {
        self.ctl.updater.start_install(self.ctl.clone(), None)
    }
    fn revert(&self, version: &str) -> Result<Value, String> {
        self.ctl.act("update-install", &json!({"version": version})).map_err(|e| e.msg)
    }
}
impl Drop for U {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

#[test]
fn trouve_et_installe() {
    let u = U::new();
    u.gh.publish("5.0.0", make_zip("5.0.0", None, vec![]), true);
    u.ctl.updater.check(false).unwrap();
    let p = u.ctl.updater.peek();
    assert_eq!((p.status.as_str(), p.latest.as_deref()), ("available", Some("5.0.0")));
    assert_eq!(u.ctl.live_snapshot(false)["update"]["status"], "available", "proposee, rien ne s'installe seul");
    assert!(u.applied.lock().unwrap().is_empty());
    u.install().unwrap();
    u.phase(&["applying"], 8.0);
    std::thread::sleep(Duration::from_millis(100));
    let a = u.applied.lock().unwrap().clone();
    assert_eq!(a.len(), 1);
    let plan: Value = serde_json::from_str(&std::fs::read_to_string(a[0].1.join("plan.json")).unwrap()).unwrap();
    assert_eq!((plan["version"].as_str().unwrap(), plan["from_version"].as_str().unwrap()), ("5.0.0", "4.9.0"));
    assert!(a[0].0.join("Rustudio.exe").is_file());
}

#[test]
fn installation_qui_ne_demarre_pas() {
    let u = U::new();
    *u.ctl.updater.apply_timeout.lock().unwrap() = Duration::from_millis(300);
    u.gh.publish("5.0.0", make_zip("5.0.0", None, vec![]), true);
    u.ctl.updater.check(false).unwrap();
    u.install().unwrap();
    u.phase(&["failed"], 8.0);
    assert_eq!(u.ctl.updater.peek().error.unwrap()["code"], "launch");
    u.install().unwrap();
    u.phase(&["applying"], 8.0);
    assert_eq!(u.applied.lock().unwrap().len(), 2);
}

#[test]
fn recherche_manuelle_reessaie_apres_cinq_secondes_sans_installer() {
    let u = U::new();
    u.gh.publish("4.9.0", make_zip("4.9.0", None, vec![]), true);
    u.ctl.updater.check(true).unwrap();
    assert_eq!(u.ctl.updater.peek().status, "up-to-date");
    assert!(u.install().is_err());
    assert!(u.ctl.updater.check(true).is_err(), "les doubles clics restent limités");
    let wait = u.ctl.updater.snapshot(None)["check_wait"].as_f64().unwrap();
    assert!(wait > 0.0 && wait <= 5.0, "attente annoncée de cinq secondes au maximum : {wait}");
    std::thread::sleep(Duration::from_millis(5100));
    assert_eq!(u.ctl.updater.snapshot(None)["check_wait"], 0.0);
    u.ctl.updater.check(true).unwrap();
    assert!(u.applied.lock().unwrap().is_empty(), "rechercher ne lance aucune installation");
}

#[test]
fn limite_github_et_hors_ligne() {
    let u = U::new();
    *u.gh.status.lock().unwrap() = (429, vec![("Retry-After".into(), "900".into())]);
    u.ctl.updater.check(false).unwrap();
    let p = u.ctl.updater.peek();
    assert_eq!(p.result.as_deref(), Some("rate-limited"));
    assert!((p.next_at - rustudio_engine::config::now_epoch() - 900.0).abs() < 5.0);
    *u.gh.status.lock().unwrap() = (503, vec![]);
    u.ctl.updater.check(false).unwrap();
    assert_eq!(u.ctl.updater.peek().result.as_deref(), Some("offline"));
    *u.gh.status.lock().unwrap() = (404, vec![]);
    u.ctl.updater.check(false).unwrap();
    assert_eq!(u.ctl.updater.peek().result.as_deref(), Some("up-to-date"));
}

#[test]
fn empreinte_fausse_ou_absente_refusee() {
    let u = U::new();
    u.gh.publish("5.0.0", make_zip("5.0.0", None, vec![]), true);
    u.gh.release.lock().unwrap()["assets"][0]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    u.ctl.updater.check(false).unwrap();
    u.install().unwrap();
    u.phase(&["failed"], 8.0);
    assert_eq!(u.ctl.updater.peek().error.unwrap()["code"], "corrupted");
    let u = U::new();
    u.gh.publish("5.0.0", make_zip("5.0.0", None, vec![]), false);
    u.ctl.updater.check(false).unwrap();
    u.install().unwrap();
    u.phase(&["failed"], 8.0);
    assert_eq!(u.ctl.updater.peek().error.unwrap()["code"], "unverifiable");
    assert!(u.applied.lock().unwrap().is_empty());
}

#[test]
fn empreinte_en_fichier_acceptee() {
    let u = U::new();
    let blob = make_zip("5.0.0", None, vec![]);
    let sha = sha256_hex(&blob);
    u.gh.publish("5.0.0", blob, false);
    let path = "/dl/v5.0.0/Rustudio.sha256";
    u.gh.blobs.lock().unwrap().insert(path.into(), format!("{sha}  Rustudio.zip\n").into_bytes());
    u.gh.release.lock().unwrap()["assets"].as_array_mut().unwrap().push(json!({"name": "Rustudio.sha256", "browser_download_url": format!("{}{path}", u.gh.base)}));
    u.ctl.updater.check(false).unwrap();
    u.install().unwrap();
    u.phase(&["applying"], 8.0);
}

#[test]
fn archives_piegees_ou_fausses_refusees() {
    for (zipb, code) in [
        (make_zip("5.0.0", None, vec![("../../evil.txt", "x")]), "archive"),
        (make_zip("4.0.9", None, vec![]), "archive"),
        (make_zip("5.0.0", Some(vec![("rustudio-engine.exe", "x")]), vec![]), "archive"),
    ] {
        let u = U::new();
        u.gh.publish("5.0.0", zipb, true);
        u.ctl.updater.check(false).unwrap();
        u.install().unwrap();
        u.phase(&["failed"], 8.0);
        assert_eq!(u.ctl.updater.peek().error.unwrap()["code"], code);
        assert!(!u.tmp.join("evil.txt").exists() && !u.tmp.join("data/evil.txt").exists());
    }
}

#[test]
fn version_de_developpement_ne_s_installe_pas() {
    let u = U::new();
    u.ctl.updater.force_installable.store(false, Ordering::Relaxed);
    assert_eq!(u.ctl.updater.installable(), (false, Some("dev")));
}

#[test]
fn nettoyage_apres_installation() {
    let u = U::new();
    let work = u.tmp.join("data/update");
    std::fs::create_dir_all(work.join("new")).unwrap();
    std::fs::write(work.join("plan.json"), json!({"version": "4.9.0"}).to_string()).unwrap();
    assert!(!u.ctl.updater.cleanup_after_install(), "installation pas encore terminee (pas de resultat)");
    std::fs::write(work.join("result.json"), json!({"version": "4.9.0", "ok": true}).to_string()).unwrap();
    assert!(u.ctl.updater.cleanup_after_install());
    assert!(!work.join("new").exists() && !work.join("plan.json").exists());
    assert!(work.join("result.json").exists(), "le resultat reste pour l'avis « installee »");
    std::fs::remove_file(work.join("result.json")).unwrap();
    std::fs::create_dir_all(work.join("new")).unwrap();
    std::fs::write(work.join("plan.json"), json!({"version": "9.9.9"}).to_string()).unwrap();
    assert!(!u.ctl.updater.cleanup_after_install(), "installation d'une autre version peut-etre en cours");
}

// ---------- echange des fichiers ----------
#[test]
fn historique_des_versions_publiees() {
    let u = U::new();
    *u.ctl.updater.current.lock().unwrap() = "5.6.12".into();
    u.gh.publish_old("5.6.11", make_zip("5.6.11", None, vec![]));
    u.gh.publish_old("4.9.0", make_zip("4.9.0", None, vec![]));
    u.gh.publish("5.6.12", make_zip("5.6.12", None, vec![]), true);
    u.gh.add(json!({"tag_name": "v5.6.13-beta.1", "prerelease": true, "body": "essai", "assets": []}));
    let h = u.ctl.updater.history(true);
    assert_eq!(h["current"], "5.6.12");
    assert!(h["error"].is_null());
    let gh: Vec<&Value> = h["versions"].as_array().unwrap().iter().filter(|v| v["source"] == "github").collect();
    let names: Vec<&str> = gh.iter().map(|v| v["version"].as_str().unwrap()).collect();
    assert_eq!(names, ["5.6.12", "5.6.11", "4.9.0"], "pre-version ecartee, de la plus recente a la plus ancienne");
    assert_eq!((gh[0]["installed"].as_bool(), gh[0]["revertible"].as_bool()), (Some(true), Some(false)));
    assert_eq!(gh[1]["revertible"], true, "version precedente publiee avec son archive");
    assert_eq!(gh[1]["notes"], "## Nouveau\n- dans la 5.6.11");
    assert_eq!(gh[2]["revertible"], false, "avant la 5.0.0 : pas de retour");
    let all: Vec<&str> = h["versions"].as_array().unwrap().iter().map(|v| v["version"].as_str().unwrap()).collect();
    let mut sorted = all.clone();
    sorted.sort_by(|a, b| if rustudio_core::semver::lt(a, b) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Less });
    assert_eq!(all, sorted, "notes embarquees fusionnees dans l'ordre");
    assert!(u.applied.lock().unwrap().is_empty(), "lire l'historique n'installe rien");
}

#[test]
fn historique_hors_ligne_avec_les_notes_embarquees() {
    let u = U::new();
    *u.ctl.updater.current.lock().unwrap() = "5.6.11".into();
    u.ctl.updater.ucfg.lock().unwrap().api = "http://127.0.0.1:9".into();
    let h = u.ctl.updater.history(true);
    assert_eq!(h["error"], "offline");
    let v = h["versions"].as_array().unwrap();
    assert!(v.iter().any(|x| x["version"] == "5.6.11" && x["installed"] == true && x["source"] == "local"));
    assert!(v.iter().all(|x| x["revertible"] == false), "hors ligne, aucun retour propose");
    assert!(v.iter().all(|x| !rustudio_core::semver::lt(x["version"].as_str().unwrap(), "5.0.0")));
    assert!(v.iter().all(|x| !rustudio_core::semver::lt("5.6.11", x["version"].as_str().unwrap())), "rien de plus recent invente");
}

#[test]
fn retour_a_une_version_precedente() {
    let u = U::new();
    *u.ctl.updater.current.lock().unwrap() = "5.6.12".into();
    u.gh.publish("5.6.12", make_zip("5.6.12", None, vec![]), true);
    u.gh.publish_old("5.6.11", make_zip("5.6.11", None, vec![]));
    let data = u.tmp.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("state.json"), "{\"prefs\": {}}").unwrap();
    u.revert("5.6.11").unwrap();
    u.phase(&["applying"], 8.0);
    std::thread::sleep(Duration::from_millis(100));
    let a = u.applied.lock().unwrap().clone();
    assert_eq!(a.len(), 1);
    let plan: Value = serde_json::from_str(&std::fs::read_to_string(a[0].1.join("plan.json")).unwrap()).unwrap();
    assert_eq!((plan["version"].as_str(), plan["from_version"].as_str(), plan["downgrade"].as_bool()), (Some("5.6.11"), Some("5.6.12"), Some(true)));
    assert!(data.join("backups").join("state-5.6.12-avant-retour-5.6.11.json").is_file(), "reglages copies avant le retour");
    assert_eq!(u.ctl.lock().state.prefs.update_reminded, Some(json!("5.6.12")), "la version quittee n'est plus rappelee");
    assert_eq!(u.ctl.update_snapshot()["target"], "5.6.11");
}

#[test]
fn retours_refuses() {
    let u = U::new();
    *u.ctl.updater.current.lock().unwrap() = "5.6.12".into();
    u.gh.publish("5.6.12", make_zip("5.6.12", None, vec![]), true);
    u.gh.publish_old("4.9.0", make_zip("4.9.0", None, vec![]));
    u.gh.publish_old("5.6.10", make_zip("5.6.9", None, vec![]));
    assert!(u.revert("5.6.12").unwrap_err().contains("pas plus ancienne"));
    assert!(u.revert("5.7.0").unwrap_err().contains("pas plus ancienne"));
    assert!(u.revert("4.9.0").unwrap_err().contains("5.0.0"));
    assert!(u.revert("5.6.5").unwrap_err().contains("introuvable"));
    assert!(u.revert("abc").unwrap_err().contains("invalide"));
    assert!(u.revert("5.6.11-beta.1").unwrap_err().contains("invalide"));
    assert_eq!(u.ctl.lock().state.prefs.update_reminded, None, "un retour refuse ne change rien");
    u.revert("5.6.10").unwrap();
    u.phase(&["failed"], 8.0);
    assert_eq!(u.ctl.updater.peek().error.unwrap()["code"], "archive", "archive d'une autre version refusee");
    assert!(u.applied.lock().unwrap().is_empty(), "rien d'installe");
}

#[test]
fn mise_a_jour_sans_version_refuse_toujours_une_archive_plus_ancienne() {
    let u = U::new();
    *u.ctl.updater.current.lock().unwrap() = "5.6.12".into();
    u.gh.publish("5.6.13", make_zip("5.6.11", None, vec![]), true);
    u.ctl.updater.check(false).unwrap();
    u.install().unwrap();
    u.phase(&["failed"], 8.0);
    assert_eq!(u.ctl.updater.peek().error.unwrap()["code"], "archive");
    assert!(u.applied.lock().unwrap().is_empty());
}

struct FakeHooks {
    ok: bool,
    log: Mutex<Vec<String>>,
}
impl Hooks for FakeHooks {
    fn stop(&self, _: &Plan) {
        self.log.lock().unwrap().push("stop".into());
    }
    fn start(&self, _: &Plan) {
        self.log.lock().unwrap().push("start".into());
    }
    fn wait(&self, _: &Plan) -> bool {
        self.ok
    }
    fn list_apps(&self, _: &Plan) -> Vec<(u32, String)> {
        vec![]
    }
    fn restart_apps(&self, _: &Plan, _: Vec<(u32, String)>) {}
}

fn apply_bench(ok: bool) -> (PathBuf, Value, Vec<String>) {
    let tmp = tmpdir("apply");
    let (src, dst, data) = (tmp.join("new"), tmp.join("install"), tmp.join("data"));
    for (root, ver, txt) in [(&dst, "4.9.0", "ancien"), (&src, "5.0.0", "nouveau")] {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("Rustudio.exe"), txt).unwrap();
        std::fs::write(root.join("rustudio-engine.exe"), txt).unwrap();
        std::fs::write(root.join("manifest.json"), json!({"version": ver, "files": {"Rustudio.exe": "x", "rustudio-engine.exe": "x", "manifest.json": ""}}).to_string()).unwrap();
    }
    std::fs::write(dst.join("ancien-seulement.txt"), "a").unwrap();
    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(dst.join("manifest.json")).unwrap()).unwrap();
    m["files"]["ancien-seulement.txt"] = json!("x");
    std::fs::write(dst.join("manifest.json"), m.to_string()).unwrap();
    let plan = Plan { from: src, to: dst, version: "5.0.0".into(), from_version: "4.9.0".into(), data_dir: data, port: 1, service: "Rustudio".into() };
    let hooks = FakeHooks { ok, log: Mutex::new(vec![]) };
    let r = apply_update(&plan, &hooks).unwrap();
    let log = hooks.log.lock().unwrap().clone();
    (tmp, r, log)
}

#[test]
fn echange_reussi() {
    let (tmp, r, log) = apply_bench(true);
    assert_eq!(r["ok"], json!(true));
    assert_eq!(std::fs::read_to_string(tmp.join("install/Rustudio.exe")).unwrap(), "nouveau");
    assert!(!tmp.join("install/ancien-seulement.txt").exists(), "fichier retire de la nouvelle version");
    assert_eq!(log, vec!["stop", "start"]);
    let res: Value = serde_json::from_str(&std::fs::read_to_string(tmp.join("data/update/result.json")).unwrap()).unwrap();
    assert_eq!(res["version"], "5.0.0");
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn retour_arriere_si_la_nouvelle_version_ne_repond_pas() {
    let (tmp, r, log) = apply_bench(false);
    assert_eq!(r["ok"], json!(false));
    assert_eq!(std::fs::read_to_string(tmp.join("install/Rustudio.exe")).unwrap(), "ancien");
    assert!(tmp.join("install/ancien-seulement.txt").exists());
    assert!(tmp.join("install/backup/echec-5.0.0/Rustudio.exe").exists());
    assert_eq!(log, vec!["stop", "start", "stop", "start"]);
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn menage_apres_passage_depuis_goxlr_mini() {
    use rustudio_engine::updater::cleanup_legacy_update;
    let d = tmpdir("legacy");
    let work = d.join("update");
    std::fs::create_dir_all(work.join("new")).unwrap();
    std::fs::write(work.join("plan.json"), "{}").unwrap();
    // le pont tourne encore (plan recent, pas de resultat) : on ne touche a rien
    assert!(!cleanup_legacy_update(&work));
    assert!(work.join("new").is_dir());
    // le pont a fini : dossier retire
    std::fs::write(work.join("result.json"), "{\"ok\": true}").unwrap();
    assert!(cleanup_legacy_update(&work));
    assert!(!work.exists());
    // plus rien a faire ensuite
    assert!(cleanup_legacy_update(&work));
}
