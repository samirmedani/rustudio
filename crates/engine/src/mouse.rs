//! Un fil HID independant de la GoXLR. L'API ne fait qu'envoyer des demandes et lire le cache.
use rustudio_attack_shark::{Mouse, Settings, Transport};
use serde_json::{json, Value};
use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Request { action: String, body: Value, deadline: Instant, answer: mpsc::SyncSender<Result<Value, String>> }
#[derive(Clone)]
/// `present` : récepteur ou câble ouvert ; `asleep` : présent mais souris muette.
struct State { settings: Option<Settings>, connected: bool, present: bool, asleep: bool, error: Option<String> }
struct Shared { state: Mutex<State>, version: AtomicU64, simulated: bool }
pub struct MouseHub { shared: Arc<Shared>, tx: mpsc::SyncSender<Request>, rx: Mutex<Option<mpsc::Receiver<Request>>> }

impl MouseHub {
    pub fn new(simulated: bool) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let settings = if simulated { Mouse::new(rustudio_attack_shark::sim::Sim::default()).read().ok() } else { None };
        Self { shared: Arc::new(Shared { state: Mutex::new(State { connected: settings.is_some(), present: settings.is_some(), asleep: false, settings, error: None }), version: AtomicU64::new(0), simulated }), tx, rx: Mutex::new(Some(rx)) }
    }
    pub fn changes(&self) -> u64 { self.shared.version.load(Ordering::Relaxed) }
    pub fn runtime(&self) -> rustudio_core::modules::ModuleRuntime {
        use rustudio_core::modules::{ModuleRuntime, ModuleStatus};
        let s = self.shared.state.lock().unwrap();
        let status = if s.connected { ModuleStatus::Ready } else if s.asleep { ModuleStatus::Asleep } else if s.present { ModuleStatus::Error } else { ModuleStatus::Disconnected };
        ModuleRuntime { id: "attack-shark-r5-ultra", status, connected: s.connected,
            simulated: self.shared.simulated, firmware: if s.connected { s.settings.as_ref().map(|x| x.firmware.clone()) } else { None }, error: s.error.clone() }
    }
    pub fn state(&self) -> Value {
        let s = self.shared.state.lock().unwrap();
        json!({"state": s.settings, "v": self.changes()})
    }
    pub fn command(&self, action: &str, body: &Value) -> Result<Value, String> {
        if self.rx.lock().unwrap().is_some() { return Err("Le module souris n’est pas démarré".into()); }
        let (answer, rx) = mpsc::sync_channel(1);
        let req = Request { action: action.into(), body: body.clone(), deadline: Instant::now() + Duration::from_secs(20), answer };
        self.tx.try_send(req).map_err(|_| "Une commande souris est déjà en attente".to_string())?;
        rx.recv_timeout(Duration::from_secs(20)).map_err(|_| "La souris ne répond pas : actualise ses réglages avant de réessayer".to_string())?
    }
    pub fn start(&self) {
        let Some(rx) = self.rx.lock().unwrap().take() else { return; };
        let weak = Arc::downgrade(&self.shared);
        std::thread::Builder::new().name("r5-hid".into()).spawn(move || {
            let mut mouse: Option<Mouse<Box<dyn Transport>>> = None;
            loop {
                let Some(shared) = weak.upgrade() else { break; };
                let request = match rx.recv_timeout(if mouse.is_some() { Duration::from_secs(10) } else { Duration::from_millis(100) }) {
                    Ok(req) => Some(req), Err(mpsc::RecvTimeoutError::Timeout) => None, Err(_) => break,
                };
                if request.as_ref().is_some_and(|r| r.deadline <= Instant::now()) { continue; }
                let result = (|| -> Result<Settings, String> {
                    if mouse.is_none() {
                        let opened = open(shared.simulated);
                        shared.state.lock().unwrap().present = opened.is_ok();
                        mouse = Some(Mouse::new(opened?));
                    }
                    let m = mouse.as_mut().unwrap();
                    m.set_deadline(request.as_ref().map(|r| r.deadline));
                    if let Some(r) = &request {
                        if r.action == "settings" {
                            let profile = r.body["profile"].as_u64().and_then(|n| u8::try_from(n).ok()).ok_or("Profil absent")?;
                            let cached = shared.state.lock().unwrap().settings.clone().ok_or("Actualise les réglages avant d’enregistrer")?;
                            return m.apply_changes(&cached, profile, &r.body["changes"]);
                        }
                        if r.action == "setting" {
                            let name = r.body["name"].as_str().ok_or("Nom de réglage absent")?;
                            return m.apply(name, &r.body["value"]);
                        }
                    }
                    m.read()
                })();
                if let Some(m) = mouse.as_mut() { m.set_deadline(None); }
                // Une saisie rejetee ne doit pas faire disparaitre une souris qui repond encore.
                let recovered = if result.is_err() && request.is_some() { mouse.as_mut().and_then(|m| {
                    m.set_deadline(Some(Instant::now() + Duration::from_secs(3)));
                    m.read().ok()
                }) } else { None };
                if let Some(m) = mouse.as_mut() { m.set_deadline(None); }
                {
                    let mut state = shared.state.lock().unwrap();
                    match &result {
                        Ok(settings) => { state.settings = Some(settings.clone()); state.connected = true; state.asleep = false; state.error = None; }
                        Err(e) => {
                            if let Some(settings) = recovered { state.settings = Some(settings); state.connected = true; state.asleep = false; state.error = None; }
                            else {
                                state.connected = false;
                                state.asleep = state.present && is_asleep(e);
                                state.error = Some(if state.asleep { SLEEP_STATUS.into() } else { human(e) });
                                mouse = None;
                            }
                        }
                    }
                    shared.version.fetch_add(1, Ordering::Relaxed);
                }
                if let Some(r) = request {
                    let answer = result.map(|s| json!({"ok":true,"state":s})).map_err(|e| if is_asleep(&e) { SLEEP_ANSWER.to_string() } else { human(&e) });
                    let _ = r.answer.send(answer);
                }
                // Faible consommation hors ligne, sans tenir d'Arc du controleur.
                if mouse.is_none() { drop(shared); std::thread::sleep(Duration::from_secs(2)); }
            }
        }).expect("fil souris");
    }
}
const SLEEP_STATUS: &str = "En veille : bouge ta souris pour la réveiller. Ses réglages se relisent tout seuls.";
const SLEEP_ANSWER: &str = "Ta souris est en veille : bouge-la pour la réveiller, puis réessaie.";

fn is_asleep(e: &str) -> bool {
    e.starts_with(rustudio_attack_shark::NO_RESPONSE)
}

/// Message pour l'utilisateur : sans la cible ni le numéro de commande HID, réservés au diagnostic.
fn human(e: &str) -> String {
    let mut out = e.to_string();
    while let Some(a) = out.find(" (cible ") {
        let Some(len) = out[a..].find(')') else { break };
        out.replace_range(a..=a + len, "");
    }
    out
}

fn open(simulated: bool) -> Result<Box<dyn Transport>, String> {
    if simulated { return Ok(Box::new(rustudio_attack_shark::sim::Sim::default())); }
    #[cfg(windows)] { return rustudio_attack_shark::Hid::open().map(|h| Box::new(h) as Box<dyn Transport>); }
    #[cfg(not(windows))] { Err("Le module R5 Ultra réel nécessite Windows".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_sans_detail_hid() {
        let e = format!("{} (cible 0, commande 81)", rustudio_attack_shark::NO_RESPONSE);
        assert!(is_asleep(&e));
        assert_eq!(human("Enregistrement interrompu : Valeur HID absente (cible 1, commande 88). Actualise."), "Enregistrement interrompu : Valeur HID absente. Actualise.");
        assert!(!is_asleep("R5 Ultra absente : branche son récepteur 2,4 GHz ou son câble USB"));
    }

    #[test]
    fn veille_puis_reveil_sans_action() {
        let flag = std::env::temp_dir().join(format!("rustudio-r5-veille-{}", std::process::id()));
        std::fs::write(&flag, b"").unwrap();
        std::env::set_var("RUSTUDIO_SIM_MOUSE_SLEEP_FILE", &flag);
        let hub = MouseHub::new(true);
        hub.start();
        let wait = |want: rustudio_core::modules::ModuleStatus| {
            let end = Instant::now() + Duration::from_secs(10);
            while hub.runtime().status != want && Instant::now() < end { std::thread::sleep(Duration::from_millis(50)); }
            hub.runtime()
        };
        let asleep = wait(rustudio_core::modules::ModuleStatus::Asleep);
        assert_eq!(asleep.status, rustudio_core::modules::ModuleStatus::Asleep, "{asleep:?}");
        assert!(!asleep.connected);
        assert_eq!(asleep.error.as_deref(), Some(SLEEP_STATUS));
        let answer = hub.command("refresh", &json!({})).unwrap_err();
        assert_eq!(answer, SLEEP_ANSWER, "une action pendant la veille donne une consigne claire");
        std::fs::remove_file(&flag).unwrap();
        let awake = wait(rustudio_core::modules::ModuleStatus::Ready);
        assert!(awake.connected && awake.error.is_none(), "{awake:?}");
        assert_eq!(hub.state()["state"]["polling_hz"], 1000);
        std::env::remove_var("RUSTUDIO_SIM_MOUSE_SLEEP_FILE");
    }
}
