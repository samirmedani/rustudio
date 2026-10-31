//! Les vues des modules relisent l'etat deja connu : ces tests interdisent tout chargement du pilote.
use rustudio_engine::config::Config;
use rustudio_engine::controller::Ctl;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn controller(simulated: bool) -> Arc<Ctl> {
    let dir = std::env::temp_dir().join(format!("rustudio-modules-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let cfg = Config { simulate: simulated, port: 0, data_dir: dir.clone(), base_dir: dir, supervised: false, engine_restarts: 0 };
    Ctl::new(cfg, Box::new(|| panic!("une vue de module ne doit jamais charger le pilote")), None, false)
}

fn request(ctl: &Arc<Ctl>, path: &str, body: Option<Value>, authorized: bool, host: Option<&str>) -> (u16, Value) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let c = ctl.clone();
    let worker = std::thread::spawn(move || rustudio_engine::http::handle(&c, server.recv().unwrap()));
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
    let url = format!("http://127.0.0.1:{port}{path}");
    let mut req = if body.is_some() { agent.post(&url) } else { agent.get(&url) };
    if authorized { req = req.set("X-Rustudio", "1"); }
    if let Some(host) = host { req = req.set("Host", host); }
    let result = match body { Some(body) => req.send_json(body), None => req.call() };
    let response = match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{e}"),
    };
    let code = response.status();
    let json = response.into_json::<Value>().unwrap();
    worker.join().unwrap();
    (code, json)
}

#[test]
fn adaptateur_suit_connexion_erreur_et_simulation() {
    let ctl = controller(true);
    let installed = ctl.modules_snapshot()["installed"].clone();
    assert_eq!(installed.as_array().unwrap().len(), 2);
    assert_eq!(installed[0]["id"], "goxlr-mini");
    let devices = || ctl.modules_snapshot()["devices"].clone();
    assert_eq!(devices()[0]["status"], "starting");
    assert_eq!(devices()[0]["simulated"], true);
    {
        let mut g = ctl.lock();
        g.present = Some(false);
        g.last_error = Some("GoXLR débranchée".into());
    }
    assert_eq!(devices()[0]["status"], "disconnected");
    assert_eq!(devices()[0]["connected"], false);
    assert_eq!(devices()[0]["error"], "GoXLR débranchée");
    {
        let mut g = ctl.lock();
        g.present = Some(true);
        g.last_error = Some("Pilote indisponible".into());
    }
    assert_eq!(devices()[0]["status"], "error");
    ctl.dev_info.lock().unwrap().firmware = Some("1.2.1.49".into());
    ctl.lock().connected = true;
    ctl.beat();
    assert_eq!(devices()[0], json!({"id": "goxlr-mini", "status": "ready", "connected": true,
        "simulated": true, "firmware": "1.2.1.49", "error": null}));
    ctl.lock().connected = false;
    ctl.lock().present = Some(false);
    assert!(devices()[0]["firmware"].is_null(), "un firmware ancien ne prouve pas qu'un appareil est connecté");
    assert_eq!(ctl.modules_snapshot()["installed"], installed, "débrancher le matériel ne désinstalle pas son module");
    assert_eq!(controller(false).modules_snapshot()["devices"][0]["simulated"], false);
}

#[test]
fn blocage_visible_en_direct_sans_changement_de_version() {
    let ctl = controller(false);
    ctl.lock().connected = true;
    ctl.beat();
    let version = ctl.version();
    assert_eq!(ctl.live_snapshot(false)["modules"]["devices"][0]["status"], "ready");
    std::thread::sleep(Duration::from_millis(5100));
    assert_eq!(ctl.version(), version);
    assert_eq!(ctl.live_snapshot(false)["modules"]["devices"][0]["status"], "blocked");
    assert_eq!(ctl.modules_snapshot()["devices"][0]["status"], "blocked");
}

#[test]
fn api_modules_et_vues_existantes_partagent_le_contrat() {
    let ctl = controller(true);
    let before = ctl.lock().state.to_value();
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let port = server.server_addr().to_ip().unwrap().port();
    let worker = {
        let (ctl, server) = (ctl.clone(), server.clone());
        std::thread::spawn(move || {
            for _ in 0..3 {
                rustudio_engine::http::handle(&ctl, server.recv().unwrap());
            }
        })
    };
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
    let get = |path: &str| agent.get(&format!("http://127.0.0.1:{port}{path}")).call().unwrap().into_json::<Value>().unwrap();
    let modules = get("/api/modules");
    let state = get("/api/state");
    let live = get("/api/live");
    worker.join().unwrap();
    assert_eq!(modules, state["modules"]);
    assert_eq!(modules["devices"], live["modules"]["devices"]);
    assert!(live["modules"].get("installed").is_none(), "la vue rapide évite de renvoyer le registre");
    assert_eq!(state["state"], before);
    assert_eq!(state["meta"]["device"]["id"], "goxlr-mini", "les anciennes API restent compatibles");
    assert_eq!(ctl.lock().state.to_value(), before, "consulter les modules ne modifie aucun réglage");
}

#[test]
fn actions_declarees_correspondent_aux_actions_materielles_existantes() {
    let actions = rustudio_core::modules::GOXLR_MINI.actions;
    let global = ["update-check", "update-install", "update-cancel", "update-ack", "update-remind", "library-refresh", "restart-engine"];
    for action in actions {
        assert!(rustudio_engine::http::ACTIONS.contains(action), "action sans implementation : {action}");
    }
    for action in rustudio_engine::http::ACTIONS {
        assert_eq!(actions.contains(&action), !global.contains(&action), "classement de l'action : {action}");
    }
}

#[test]
fn commandes_par_module_et_routes_historiques_restent_compatibles() {
    let ctl = controller(true);
    let (code, result) = request(&ctl, "/api/modules/goxlr-mini/volume", Some(json!({"channel": "Game", "value": 211})), true, None);
    assert_eq!(code, 200);
    assert_eq!(result["ok"], true);
    assert_eq!(ctl.lock().state.volume("Game"), 211);
    assert!(ctl.lock().dirty.contains(&rustudio_engine::controller::Key::Volume(rustudio_engine::controller::ch("Game"))));
    let (code, _) = request(&ctl, "/api/volume", Some(json!({"channel": "Chat", "value": 96})), true, None);
    assert_eq!(code, 200);
    assert_eq!(ctl.lock().state.volume("Chat"), 96);
    let version = ctl.version();
    let (code, echo) = request(&ctl, "/api/modules/goxlr-mini/echo-check", Some(json!({})), true, None);
    assert_eq!(code, 200);
    assert_eq!(echo["checks"].as_array().unwrap().len(), 4);
    assert_eq!(ctl.version(), version, "la vérification d'écho ne modifie aucun réglage");
}

#[test]
fn module_inconnu_actions_globales_et_requetes_non_autorisees_sont_refusees() {
    let ctl = controller(true);
    let before = ctl.lock().state.to_value();
    let version = ctl.version();
    for id in ["inconnu", "souris-non-supportee"] {
        let path = format!("/api/modules/{id}/volume");
        assert_eq!(request(&ctl, &path, Some(json!({"channel": "Game", "value": 1})), true, None).0, 404);
    }
    for action in ["update-check", "update-install", "update-cancel", "update-ack", "update-remind", "library-refresh", "restart-engine", "inconnue"] {
        let path = format!("/api/modules/goxlr-mini/{action}");
        assert_eq!(request(&ctl, &path, Some(json!({})), true, None).0, 404, "{action}");
    }
    let path = "/api/modules/goxlr-mini/volume";
    let body = json!({"channel": "Game", "value": 1});
    assert_eq!(request(&ctl, path, Some(body.clone()), false, None).0, 403);
    assert_eq!(request(&ctl, path, Some(body), true, Some("evil.example")).0, 403);
    assert_eq!(request(&ctl, "/api/modules/goxlr-mini/state", None, false, Some("evil.example")).0, 403);
    assert_eq!(ctl.lock().state.to_value(), before);
    assert_eq!(ctl.version(), version);
}

#[test]
fn etat_par_module_contient_seulement_les_donnees_de_son_adaptateur() {
    let ctl = controller(true);
    let before = ctl.lock().state.to_value();
    let (code, state) = request(&ctl, "/api/modules/goxlr-mini/state", None, false, None);
    assert_eq!(code, 200);
    let keys: std::collections::HashSet<_> = state.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["module", "device", "state", "meta", "live", "v"].into_iter().collect());
    assert_eq!(state["module"], ctl.modules_snapshot()["installed"][0]);
    assert_eq!(state["device"], ctl.modules_snapshot()["devices"][0]);
    assert_eq!(state["state"], before);
    assert_eq!(state["meta"]["device"]["id"], "goxlr-mini");
    assert_eq!(state["live"], ctl.live_snapshot(false)["live"]);
    assert_eq!(request(&ctl, "/api/modules/inconnu/state", None, false, None).0, 404);
    assert_eq!(ctl.lock().state.to_value(), before);
}

#[test]
fn souris_independante_du_moteur_audio_et_commandes_confirmees() {
    let ctl = controller(true);
    ctl.mouse.start();
    let original_audio = ctl.snapshot()["state"].clone();
    let (code, initial) = request(&ctl, "/api/modules/attack-shark-r5-ultra/state", None, false, None);
    assert_eq!(code, 200);
    assert_eq!(initial["device"]["connected"], true);
    assert!(initial["state"].get("volumes").is_none());
    let (code, changed) = request(&ctl, "/api/modules/attack-shark-r5-ultra/setting", Some(json!({"name":"debounce_ms","value":7})), true, None);
    assert_eq!(code, 200);
    assert_eq!(changed["state"]["debounce_ms"], 7);
    let (code, grouped) = request(&ctl, "/api/modules/attack-shark-r5-ultra/settings", Some(json!({"profile":1,"changes":{"polling_hz":2000,"angle_snap":true,"sleep_seconds":60}})), true, None);
    assert_eq!(code, 200);
    assert_eq!(grouped["state"]["polling_hz"], 2000);
    assert_eq!(grouped["state"]["angle_snap"], true);
    assert_eq!(grouped["state"]["sleep_seconds"], 60);
    let (code, _) = request(&ctl, "/api/modules/attack-shark-r5-ultra/settings", Some(json!({"profile":2,"changes":{"polling_hz":1000}})), true, None);
    assert_eq!(code, 400);
    assert_eq!(ctl.module_state_snapshot("attack-shark-r5-ultra").unwrap()["state"]["polling_hz"], 2000);
    assert_eq!(ctl.snapshot()["state"], original_audio);
    let (code, _) = request(&ctl, "/api/modules/attack-shark-r5-ultra/volume", Some(json!({"channel":"Music","value":10})), true, None);
    assert_eq!(code, 404);
    let (code, _) = request(&ctl, "/api/modules/attack-shark-r5-ultra/setting", Some(json!({"name":"firmware","value":1})), true, None);
    assert_eq!(code, 400);
    assert_eq!(ctl.modules_snapshot()["devices"][1]["connected"], true);
    assert_eq!(ctl.module_state_snapshot("attack-shark-r5-ultra").unwrap()["state"]["debounce_ms"], 7);
    let (code, _) = request(&ctl, "/api/modules/attack-shark-r5-ultra/setting", Some(json!({"name":"debounce_ms","value":4})), false, None);
    assert_eq!(code, 403);
}
