//! Superviseur : lance le moteur dans un processus enfant et le relance s'il meurt ou si la boucle USB se fige
//! (un appel au pilote bloque ne se debloque pas : seul TerminateProcess en sort).
use crate::config::{Config, EXIT_PORT_BUSY, EXIT_RESTART};
use crate::log::log;
use crate::logf;
use serde_json::Value;
use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const WATCHDOG: Duration = Duration::from_secs(15);

/// Le superviseur garde notre entree standard ouverte : a sa disparition, on s'arrete aussi.
pub fn watch_parent() {
    let mut buf = [0u8; 64];
    let mut stdin = std::io::stdin();
    loop {
        match stdin.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    log("Superviseur disparu : arrêt du moteur");
    std::process::exit(0);
}

pub fn health(port: u16) -> Option<Value> {
    let agent = crate::net::agent(Duration::from_secs(3), 5, &crate::net::ua());
    agent.get(&format!("http://127.0.0.1:{port}/api/health")).call().ok()?.into_json::<Value>().ok()
}

fn spawn_child(cfg: &Config, restarts: u32) -> std::io::Result<Child> {
    let exe = std::env::current_exe()?;
    let mut c = crate::win::hidden(&exe.to_string_lossy());
    c.args(["run", "--child"])
        .env("RUSTUDIO_SUPERVISED", "1")
        .env("RUSTUDIO_RESTARTS", restarts.to_string())
        .env("RUSTUDIO_PORT", cfg.port.to_string())
        .env("RUSTUDIO_DATA_DIR", &cfg.data_dir)
        .env("RUSTUDIO_SIMULATE", if cfg.simulate { "1" } else { "0" })
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(&cfg.base_dir);
    c.spawn()
}

pub fn supervise(cfg: Config, stop: Arc<AtomicBool>) {
    crate::log::set_file(cfg.log_path());
    crate::log::set_prefix("[superviseur] ");
    if health(cfg.port).is_some() {
        logf!("Un moteur répond déjà sur le port {} : rien à faire", cfg.port);
        return;
    }
    logf!("Démarrage du superviseur {}{}", crate::config::VERSION, if cfg.simulate { " (appareil simulé)" } else { "" });
    let (mut restarts, mut quick_fails) = (0u32, 0u32);
    while !stop.load(Ordering::SeqCst) {
        let mut child = match spawn_child(&cfg, restarts) {
            Ok(c) => c,
            Err(e) => {
                logf!("Lancement du moteur impossible : {e}");
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        };
        let t0 = Instant::now();
        let mut last_ok = Instant::now();
        let mut reason: Option<String> = None;
        while reason.is_none() {
            for _ in 0..15 {
                std::thread::sleep(Duration::from_millis(100));
                if stop.load(Ordering::SeqCst) {
                    break;
                }
            }
            if stop.load(Ordering::SeqCst) {
                reason = Some("arrêt du service".into());
                break;
            }
            if let Ok(Some(st)) = child.try_wait() {
                reason = Some(format!("moteur arrêté (code {})", st.code().unwrap_or(-1)));
                break;
            }
            match health(cfg.port) {
                Some(h) => {
                    last_ok = Instant::now();
                    let age = h.get("loop_age_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    if age > WATCHDOG.as_millis() as u64 {
                        let stage = h.get("usb_stage").and_then(|v| v.as_str()).map(|s| format!(" pendant : {s}")).unwrap_or_default();
                        reason = Some(format!("boucle USB figée depuis {} s (pilote bloqué{stage})", age / 1000));
                    }
                }
                None if last_ok.elapsed() > Duration::from_secs(25) => reason = Some("moteur muet depuis 25 s".into()),
                None => {}
            }
        }
        if child.try_wait().ok().flatten().is_none() {
            drop(child.stdin.take()); // l'enfant s'arrete proprement s'il le peut
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(if stop.load(Ordering::SeqCst) { 3000 } else { 300 }) {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill(); // TerminateProcess : fonctionne meme si un appel au pilote est bloque
            let _ = child.wait();
        }
        if stop.load(Ordering::SeqCst) {
            log("Arrêt du moteur (service arrêté)");
            return;
        }
        let rc = child.try_wait().ok().flatten().and_then(|s| s.code()).unwrap_or(-1);
        if rc == EXIT_PORT_BUSY && health(cfg.port).is_some() {
            log("Un autre moteur occupe le port : arrêt du superviseur");
            return;
        }
        restarts += 1;
        let delay = if rc == EXIT_RESTART {
            quick_fails = 0;
            0.3
        } else {
            quick_fails = if t0.elapsed() < Duration::from_secs(60) { quick_fails + 1 } else { 0 };
            if quick_fails <= 1 { 1.0 } else { 30f64.min(2f64.powi(quick_fails as i32 - 1)) }
        };
        logf!("Relance du moteur : {}", reason.unwrap_or_default());
        std::thread::sleep(Duration::from_secs_f64(delay));
    }
}
