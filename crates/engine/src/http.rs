//! API locale et interface (http://127.0.0.1:14565/) : seulement depuis la machine, en-tete obligatoire en ecriture.
use crate::config::API_HEADER;
use crate::api::ApiError;
use crate::controller::Ctl;
use crate::logf;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response, Server};

pub const ACTIONS: [&str; 42] = [
    "volume", "mute", "fader", "route", "colour", "mic", "raw", "profile", "dsp", "mic-preset", "scene", "scene-save",
    "scene-delete", "theme", "lighting", "detect-mic", "auto-gain", "voice-setup", "mic-listen", "job-cancel", "settings",
    "auto-scenes", "reconnect", "restart-engine", "fx", "hp-eq", "hp-eq-save", "hp-eq-delete", "hp-eq-setup",
    "hp-eq-remove", "update-check", "update-install", "update-cancel", "update-ack", "update-remind", "library-refresh",
    "echo-check", "sound-test", "mic-model", "voice-print", "mic-ab", "voice-forget",
];

/// Page d'accueil de l'API : l'interface est l'appli Rustudio, la bulle OBS reste servie sous /overlay.
pub const HOME_HTML: &str = r#"<!doctype html><html lang="fr"><meta charset="utf-8"><title>Rustudio</title><body style="font-family:Segoe UI,system-ui,sans-serif;margin:3em"><h1>Rustudio</h1><p>Le moteur tourne. Ouvre l’appli Rustudio pour régler tes appareils.</p><p>Source navigateur pour OBS : <code>http://127.0.0.1:14565/overlay</code></p></body></html>"#;
pub const OVERLAY_HTML: &str = include_str!("../assets/overlay.html");
pub const ICON_PNG: &[u8] = include_bytes!("../../../docs/icon.png");

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
                "/" | "/index.html" => return respond(req, HOME_HTML, "text/html; charset=utf-8", 200),
                "/overlay" | "/overlay.html" => return respond(req, OVERLAY_HTML, "text/html; charset=utf-8", 200),
                "/icon.png" => return respond(req, ICON_PNG, "image/png", 200),
                "/api/live" => ctl.live_snapshot(query.split('&').any(|q| q == "level=1")),
                "/api/modules/goxlr-mini/mixer" => {
                    if hdr(&req, API_HEADER) != Some("1") {
                        return json_resp(req, &json!({"error": "requête refusée"}), 403);
                    }
                    let after = query.split('&').find_map(|part| part.strip_prefix("after="));
                    match ctl.mixer_snapshot(after) {
                        Ok(snapshot) => snapshot,
                        Err(error) => return json_resp(req, &json!({"error": error.msg}), error.status),
                    }
                }
                "/api/state" | "/status" => ctl.snapshot(),
                "/api/modules" => ctl.modules_snapshot(),
                "/api/health" => ctl.health(),
                "/api/update" => ctl.update_snapshot(),
                "/api/update/history" => ctl.updater.history(query.split('&').any(|q| q == "refresh=1")),
                "/api/support" => crate::support::support(ctl),
                "/api/usb" if ctl.simulated() => json!({"found": false, "simulated": true}),
                "/api/usb" => crate::usbmap::query(),
                "/api/glitches" => {
                    let since = query
                        .split('&')
                        .find_map(|q| q.strip_prefix("since="))
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or_else(|| crate::config::now_epoch() - 86400.0);
                    let mut v = if ctl.simulated() {
                        crate::glitches::unavailable(since, true)
                    } else {
                        crate::glitches::query(since)
                    };
                    v["usb"] = ctl.drops.lock().unwrap_or_else(|e| e.into_inner()).json(since);
                    v["kernel"] = if ctl.simulated() { crate::glitches::kernel_simulated() } else { crate::glitches::kernel_now() };
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
                    Some(("goxlr-mini", "voice")) => rustudio_core::voicefit::voice_view(&ctl.lock().state),
                    _ => return json_resp(req, &json!({"error": "introuvable"}), 404),
                },
            };
            json_resp(req, &v, 200)
        }
        Method::Post => {
            if hdr(&req, API_HEADER) != Some("1") {
                return json_resp(req, &json!({"error": "requête refusée"}), 403);
            }
            if module_route(&path) == Some(("goxlr-mini", "voice-audio")) {
                // audio de l'empreinte vocale : Int16 mono brut, jamais écrit sur le disque
                let mut body = Vec::new();
                if req.as_reader().take(crate::jobs::VOICE_AUDIO_MAX as u64 + 1).read_to_end(&mut body).is_err() {
                    return json_resp(req, &json!({"error": "corps illisible"}), 400);
                }
                if body.len() > crate::jobs::VOICE_AUDIO_MAX {
                    return json_resp(req, &json!({"error": "enregistrement trop long"}), 413);
                }
                return match crate::jobs::voice_audio(ctl, query, &body) {
                    Ok(v) => json_resp(req, &v, 200),
                    Err(ApiError { status, msg }) => json_resp(req, &json!({"error": msg}), status),
                };
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
            let r = if path == "/api/modules/goxlr-mini/mixer" {
                // Les commandes ciblent explicitement le mode et ce démarrage du moteur.
                let mode = if ctl.simulated() { "demo" } else { "live" };
                let requested = p["mode"].as_str().unwrap_or("demo");
                if requested != mode || p["boot"].as_str() != Some(ctl.boot_id.as_str()) {
                    return json_resp(req, &json!({"error": "mode ou session du moteur incompatible"}), 403);
                }
                match p["action"].as_str() {
                    Some("mixer-profile") => ctl.act("mixer-profile", &p["params"]),
                    Some(action @ ("volume" | "mute" | "fader" | "settings")) => {
                        ctl.module_act("goxlr-mini", action, &p["params"])
                    }
                    _ => Err(ApiError::bad("commande du mixeur inconnue")),
                }
            } else if let Some((id, action)) = module_route(&path) {
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
