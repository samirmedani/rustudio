//! API locale et interface (http://127.0.0.1:14565/) : seulement depuis la machine, en-tete obligatoire en ecriture.
use crate::config::API_HEADER;
use crate::api::ApiError;
use crate::controller::Ctl;
use crate::logf;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response, Server};

pub const ACTIONS: [&str; 38] = [
    "volume", "mute", "fader", "route", "colour", "mic", "raw", "profile", "dsp", "mic-preset", "scene", "scene-save",
    "scene-delete", "theme", "lighting", "detect-mic", "auto-gain", "voice-setup", "mic-listen", "job-cancel", "settings",
    "auto-scenes", "reconnect", "restart-engine", "fx", "hp-eq", "hp-eq-save", "hp-eq-delete", "hp-eq-setup",
    "hp-eq-remove", "update-check", "update-install", "update-cancel", "update-ack", "update-remind", "library-refresh",
    "echo-check", "sound-test",
];

pub const UI_HTML: &str = include_str!("../../../ui/index.html");
pub const OVERLAY_HTML: &str = include_str!("../../../ui/overlay.html");
pub const ICON_PNG: &[u8] = include_bytes!("../../../docs/icon.png");

/// Fichiers de l'interface servis sous /ui/ : liste fermée, intégrée au moteur. Un chemin absent ici répond 404,
/// sans jamais lire le disque à partir de l'URL demandée.
macro_rules! ui_assets {
    ($($path:literal),* $(,)?) => {
        pub const UI_ASSETS: &[(&str, &str)] = &[$(($path, include_str!(concat!("../../../ui/", $path)))),*];
    };
}
ui_assets!(
    "app.css",
    "js/socle.js",
    "js/goxlr/commun.js",
    "js/goxlr/mixeur.js",
    "js/goxlr/micro.js",
    "js/goxlr/casque.js",
    "js/goxlr/routage.js",
    "js/goxlr/eclairage.js",
    "js/goxlr/scenes.js",
    "js/goxlr/son-windows.js",
    "js/attack-shark/r5-ultra.js",
    "js/modules.js",
    "js/menu.js",
    "js/goxlr/voix.js",
    "js/mises-a-jour.js",
    "js/aide.js",
    "js/demarrage.js",
);

fn ui_asset(path: &str) -> Option<(String, &'static str)> {
    let name = path.strip_prefix("/ui/")?;
    let (name, embedded) = UI_ASSETS.iter().find(|(n, _)| *n == name)?;
    let ctype = if name.ends_with(".css") { "text/css; charset=utf-8" } else { "text/javascript; charset=utf-8" };
    Some((ui_file(name, embedded), ctype))
}

fn ui_file(name: &str, embedded: &'static str) -> String {
    if let Ok(dir) = std::env::var("RUSTUDIO_UI_DIR") {
        if let Ok(t) = std::fs::read_to_string(std::path::Path::new(&dir).join(name)) {
            return t;
        }
    }
    embedded.to_string()
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn respond(req: Request, body: impl Into<Vec<u8>>, ctype: &str, code: u16) {
    let resp = Response::from_data(body)
        .with_status_code(code)
        .with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-Content-Type-Options", "nosniff"));
    let _ = req.respond(resp);
}

fn json_resp(req: Request, v: &Value, code: u16) {
    respond(req, v.to_string(), "application/json; charset=utf-8", code);
}

fn hdr<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

pub fn serve(ctl: Arc<Ctl>, server: Arc<Server>, workers: usize) {
    let mut hs = vec![];
    for i in 0..workers {
        let (c, s) = (ctl.clone(), server.clone());
        hs.push(std::thread::Builder::new().name(format!("http{i}")).spawn(move || {
            while let Ok(req) = s.recv() {
                let c2 = c.clone();
                if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || handle(&c2, req))) {
                    logf!("Erreur API : {}", crate::api::panic_text(&p));
                }
            }
        }).expect("fil http"));
    }
    for h in hs {
        let _ = h.join();
    }
}

fn host_ok(req: &Request) -> bool {
    let host = hdr(req, "Host").unwrap_or("");
    let h = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    h == "127.0.0.1" || h == "localhost"
}

fn module_route(path: &str) -> Option<(&str, &str)> {
    let (id, resource) = path.strip_prefix("/api/modules/")?.split_once('/')?;
    (!id.is_empty() && !resource.is_empty() && !resource.contains('/')).then_some((id, resource))
}

pub fn handle(ctl: &Arc<Ctl>, mut req: Request) {
    if !host_ok(&req) {
        return json_resp(req, &json!({"error": "hôte refusé"}), 403);
    }
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.to_string();
    match *req.method() {
        Method::Get => {
            let v = match path.as_str() {
                "/" | "/index.html" => return respond(req, ui_file("index.html", UI_HTML), "text/html; charset=utf-8", 200),
                "/overlay" | "/overlay.html" => return respond(req, ui_file("overlay.html", OVERLAY_HTML), "text/html; charset=utf-8", 200),
                "/icon.png" => return respond(req, ICON_PNG, "image/png", 200),
                p if p.starts_with("/ui/") => {
                    return match ui_asset(p) {
                        Some((body, ctype)) => respond(req, body, ctype, 200),
                        None => json_resp(req, &json!({"error": "introuvable"}), 404),
                    }
                }
                "/api/live" => ctl.live_snapshot(query.split('&').any(|q| q == "level=1")),
                "/api/state" | "/status" => ctl.snapshot(),
                "/api/modules" => ctl.modules_snapshot(),
                "/api/health" => ctl.health(),
                "/api/update" => ctl.update_snapshot(),
                "/api/update/history" => ctl.updater.history(query.split('&').any(|q| q == "refresh=1")),
                "/api/support" => crate::support::support(ctl),
                "/api/usb" => crate::usbmap::query(),
                "/api/glitches" => {
                    let since = query
                        .split('&')
                        .find_map(|q| q.strip_prefix("since="))
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or_else(|| crate::config::now_epoch() - 86400.0);
                    let mut v = crate::glitches::query(since);
                    v["usb"] = ctl.drops.lock().unwrap_or_else(|e| e.into_inner()).json(since);
                    v
                }
                "/api/processes" => ctl.user_apps(),
                "/api/library" => crate::library::snapshot(),
                "/api/default-profile" => {
                    let p: Value = serde_json::from_str(crate::controller::DEFAULT_PROFILE).unwrap_or(Value::Null);
                    rustudio_core::state::normalize_state(&p).to_value()
                }
                "/api/sim/device" if ctl.sim.is_some() => ctl.sim.as_ref().unwrap().describe(),
                _ => match module_route(&path) {
                    Some((id, "state")) => match ctl.module_state_snapshot(id) {
                        Ok(v) => v,
                        Err(e) => return json_resp(req, &json!({"error": e.msg}), e.status),
                    },
                    _ => return json_resp(req, &json!({"error": "introuvable"}), 404),
                },
            };
            json_resp(req, &v, 200)
        }
        Method::Post => {
            if hdr(&req, API_HEADER) != Some("1") {
                return json_resp(req, &json!({"error": "requête refusée"}), 403);
            }
            let mut body = String::new();
            if req.as_reader().take(4_000_000).read_to_string(&mut body).is_err() {
                return json_resp(req, &json!({"error": "corps illisible"}), 400);
            }
            let p: Value = if body.trim().is_empty() {
                json!({})
            } else {
                match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => return json_resp(req, &json!({"error": format!("JSON invalide : {e}")}), 400),
                }
            };
            let name = path.trim_matches('/');
            let name = name.strip_prefix("api/").unwrap_or(name);
            if let Some(sim_action) = name.strip_prefix("sim/") {
                return match &ctl.sim {
                    None => json_resp(req, &json!({"error": "simulateur inactif"}), 404),
                    Some(_) => match sim_act(ctl, sim_action, &p) {
                        Ok(v) => json_resp(req, &v, 200),
                        Err(e) => json_resp(req, &json!({"error": e.msg}), e.status),
                    },
                };
            }
            let r = if let Some((id, action)) = module_route(&path) {
                ctl.module_act(id, action, &p)
            } else {
                let name = if name == "mic-gain" { "mic" } else { name };
                if !ACTIONS.contains(&name) {
                    return json_resp(req, &json!({"error": "action inconnue"}), 404);
                }
                match name {
                    "library-refresh" => Ok(crate::library::refresh()),
                    "echo-check" => Ok(crate::library::echo_check(ctl)),
                    _ => ctl.act(name, &p),
                }
            };
            match r {
                Ok(v) => json_resp(req, &v, 200),
                Err(ApiError { status, msg }) => {
                    if status >= 500 {
                        logf!("Erreur API {path}: {msg}");
                    }
                    json_resp(req, &json!({"error": msg}), status)
                }
            }
        }
        _ => json_resp(req, &json!({"error": "méthode refusée"}), 405),
    }
}

fn sim_act(ctl: &Arc<Ctl>, name: &str, p: &Value) -> Result<Value, ApiError> {
    use rustudio_core::value::{get, num, parse_bool};
    let sim = ctl.sim.as_ref().unwrap();
    let s = |k: &str| get(p, k).as_str().map(String::from).ok_or_else(|| ApiError::bad(format!("paramètre manquant : {k}")));
    match name {
        "device" => return Ok(sim.describe()),
        "press" => sim.press(&s("button")?, num(get(p, "ms")).unwrap_or(150.0)),
        "down" => sim.button_down(&s("button")?),
        "up" => sim.button_up(&s("button")?),
        "fader" => sim.move_fader(num(get(p, "index")).unwrap_or(0.0) as usize % 4, num(get(p, "value")).unwrap_or(0.0) as i32),
        "unplug" => sim.unplug(),
        "usb-drop" => sim.stream_drop(num(get(p, "ms")).unwrap_or(1.0).max(1.0) as u32),
        "plug" => sim.plug(),
        "hang" => sim.hang(num(get(p, "seconds")).unwrap_or(20.0)),
        "talk" => {
            let mut g = sim.lock();
            g.dev.talking = p.get("on").map(parse_bool).unwrap_or(true);
            if let Some(db) = num(get(p, "db")) {
                g.dev.voice_db = db;
            }
        }
        "mic-input" => sim.lock().dev.mic_input = get(p, "input").as_str().filter(|s| !s.is_empty()).map(String::from),
        "apps" => {
            let mut g = sim.lock();
            g.apps = get(p, "apps").as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        }
        other => return Err(ApiError::bad(format!("action de simulation inconnue: {other}"))),
    }
    Ok(json!({"ok": true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn referenced() -> Vec<&'static str> {
        UI_HTML.split("\"/ui/").skip(1).filter_map(|s| s.split_once('"').map(|(p, _)| p)).collect()
    }

    #[test]
    fn page_et_fichiers_servis_correspondent() {
        let refs = referenced();
        let served: Vec<&str> = UI_ASSETS.iter().map(|(n, _)| *n).collect();
        for r in &refs {
            assert!(served.contains(r), "référencé par index.html mais non servi : {r}");
        }
        for s in &served {
            assert!(refs.contains(s), "servi mais jamais chargé par index.html : {s}");
        }
        let scripts: Vec<&str> = refs.iter().copied().filter(|r| r.ends_with(".js")).collect();
        assert_eq!(scripts[0], "js/socle.js", "le socle commun se charge en premier");
        assert_eq!(scripts[scripts.len() - 1], "js/demarrage.js", "le démarrage se charge en dernier");
        for (name, body) in UI_ASSETS {
            if name.ends_with(".js") {
                assert!(body.contains("\"use strict\";"), "{name} garde le mode strict");
            }
        }
    }

    #[test]
    fn chemins_hors_liste_refuses() {
        assert!(ui_asset("/ui/js/socle.js").is_some());
        assert_eq!(ui_asset("/ui/app.css").unwrap().1, "text/css; charset=utf-8");
        for p in ["/ui/../Cargo.toml", "/ui/js/../../Cargo.toml", "/ui/", "/ui/js/inconnu.js", "/ui//etc/hosts"] {
            assert!(ui_asset(p).is_none(), "{p}");
        }
    }
}
