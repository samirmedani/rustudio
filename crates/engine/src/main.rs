//! rustudio-engine : moteur de Rustudio.
//!   rustudio-engine run [--simulate] [--port N] [--data-dir D]   superviseur + moteur (developpement)
//!   rustudio-engine run --child                                  moteur seul (lance par le superviseur)
//!   rustudio-engine service                                      point d'entree du service Windows
//!   rustudio-engine service-install | service-uninstall
//!   rustudio-engine apply-update                                 (interne) installe une version preparee
//!   rustudio-engine status | health | version
//!   rustudio-engine volume <voie> <0-255|NN%> | mute <voie> <on|off> [cible] | scene <nom> | reconnect | update-check
use rustudio_engine::config::{Config, API_HEADER, VERSION};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn http(cfg: &Config, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{}{path}", cfg.port);
    let agent = rustudio_engine::net::agent(std::time::Duration::from_secs(10), 5, &rustudio_engine::net::ua());
    let r = if method == "GET" {
        agent.get(&url).call()
    } else {
        agent.post(&url).set(API_HEADER, "1").send_json(body.unwrap_or(json!({})))
    };
    match r {
        Ok(resp) => resp.into_json::<Value>().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(_, resp)) => Err(resp.into_string().unwrap_or_default()),
        Err(e) => Err(format!("moteur injoignable : {e}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::from_env();
    let mut rest = vec![];
    let mut child = false;
    let has_env = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let mut explicit_port = has_env("RUSTUDIO_PORT");
    let mut explicit_data_dir = has_env("RUSTUDIO_DATA_DIR");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--simulate" => cfg.simulate = true,
            "--child" => child = true,
            "--port" if i + 1 < args.len() => {
                cfg.port = args[i + 1].parse().unwrap_or(cfg.port);
                explicit_port = true;
                i += 1;
            }
            "--data-dir" if i + 1 < args.len() => {
                cfg.data_dir = args[i + 1].clone().into();
                explicit_data_dir = true;
                i += 1;
            }
            a => rest.push(a.to_string()),
        }
        i += 1;
    }
    if cfg.simulate {
        if !explicit_port {
            cfg.port = rustudio_engine::config::SIMULATION_PORT;
        }
        if !explicit_data_dir {
            cfg.data_dir = rustudio_engine::config::simulation_data_dir();
        }
    }
    let cmd = rest.first().cloned().unwrap_or_else(|| "run".into());
    let code = match cmd.as_str() {
        "run" | "daemon" => {
            if let Err(message) = cfg.validate_simulation() {
                eprintln!("{message}");
                std::process::exit(2);
            }
            if child {
                rustudio_engine::run_engine(cfg)
            } else {
                rustudio_engine::supervisor::supervise(cfg, Arc::new(AtomicBool::new(false)));
                0
            }
        }
        #[cfg(windows)]
        "service" => match rustudio_engine::service::run_dispatcher() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("service : {e}");
                1
            }
        },
        #[cfg(windows)]
        "service-install" => match rustudio_engine::service::install(&std::env::current_exe().unwrap()) {
            Ok(()) => {
                println!("Service Rustudio installé et démarré");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        #[cfg(windows)]
        "service-uninstall" => match rustudio_engine::service::uninstall() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        "apply-update" => rustudio_engine::apply::run_apply_update(),
        #[cfg(windows)]
        "uninstall" => rustudio_engine::uninstall::run(&args),
        "version" | "--version" => {
            println!("Rustudio {VERSION}");
            0
        }
        _ => {
            let a = |n: usize| rest.get(n).cloned().unwrap_or_default();
            let req: Result<(&str, String, Option<Value>), String> = match cmd.as_str() {
                "status" => Ok(("GET", "/api/state".into(), None)),
                "health" => Ok(("GET", "/api/health".into(), None)),
                "volume" => Ok(("POST", "/api/volume".into(), Some(json!({"channel": a(1), "value": a(2)})))),
                "mute" => Ok(("POST", "/api/mute".into(), Some(json!({"channel": a(1), "muted": a(2), "target": if rest.len() > 3 { a(3) } else { "all".into() }})))),
                "scene" => Ok(("POST", "/api/scene".into(), Some(json!({"name": a(1)})))),
                "reconnect" => Ok(("POST", "/api/reconnect".into(), None)),
                "update-check" => Ok(("POST", "/api/update-check".into(), None)),
                _ => Err(format!("commande inconnue : {cmd}")),
            };
            match req.and_then(|(m, p, b)| http(&cfg, m, &p, b)) {
                Ok(v) => {
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
    };
    std::process::exit(code);
}
