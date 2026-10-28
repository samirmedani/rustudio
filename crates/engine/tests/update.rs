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
    status: Arc<Mutex<(u16, Vec<(String, String)>)>>,
    blobs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl FakeGitHub {
    fn new() -> FakeGitHub {
        let srv = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", srv.server_addr().to_ip().unwrap().port());
        let gh = FakeGitHub { base, release: Arc::new(Mutex::new(Value::Null)), status: Arc::new(Mutex::new((200, vec![]))), blobs: Arc::new(Mutex::new(HashMap::new())) };
        let (rel, st, blobs) = (gh.release.clone(), gh.status.clone(), gh.blobs.clone());
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
        *self.release.lock().unwrap() = json!({"tag_name": format!("v{version}"), "draft": false, "prerelease": false,
            "body": "## Nouveau\n- truc", "html_url": format!("https://github.com/x/y/releases/tag/v{version}"), "assets": [asset]});
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
        self.ctl.updater.start_install(self.ctl.clone())
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
fn a_jour_et_une_recherche_par_minute() {
    let u = U::new();
    u.gh.publish("4.9.0", make_zip("4.9.0", None, vec![]), true);
    u.ctl.updater.check(true).unwrap();
    assert_eq!(u.ctl.updater.peek().status, "up-to-date");
    assert!(u.install().is_err());
    assert!(u.ctl.updater.check(true).is_err(), "une recherche manuelle par minute");
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
