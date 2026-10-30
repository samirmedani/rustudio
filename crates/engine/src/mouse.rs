//! Un fil HID independant de la GoXLR. L'API ne fait qu'envoyer des demandes et lire le cache.
use rustudio_attack_shark::{Mouse, Settings, Transport};
use serde_json::{json, Value};
use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Request { action: String, body: Value, deadline: Instant, answer: mpsc::SyncSender<Result<Value, String>> }
#[derive(Clone)]
struct State { settings: Option<Settings>, connected: bool, error: Option<String> }
struct Shared { state: Mutex<State>, version: AtomicU64, simulated: bool }
pub struct MouseHub { shared: Arc<Shared>, tx: mpsc::SyncSender<Request>, rx: Mutex<Option<mpsc::Receiver<Request>>> }

impl MouseHub {
    pub fn new(simulated: bool) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let settings = if simulated { Mouse::new(rustudio_attack_shark::sim::Sim::default()).read().ok() } else { None };
        Self { shared: Arc::new(Shared { state: Mutex::new(State { connected: settings.is_some(), settings, error: None }), version: AtomicU64::new(0), simulated }), tx, rx: Mutex::new(Some(rx)) }
    }
    pub fn changes(&self) -> u64 { self.shared.version.load(Ordering::Relaxed) }
    pub fn runtime(&self) -> rustudio_core::modules::ModuleRuntime {
        use rustudio_core::modules::{ModuleRuntime, ModuleStatus};
        let s = self.shared.state.lock().unwrap();
        ModuleRuntime { id: "attack-shark-r5-ultra", status: if s.connected { ModuleStatus::Ready } else { ModuleStatus::Disconnected }, connected: s.connected,
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
                    if mouse.is_none() { mouse = Some(Mouse::new(open(shared.simulated)?)); }
                    let m = mouse.as_mut().unwrap();
                    m.set_deadline(request.as_ref().map(|r| r.deadline));
                    if let Some(r) = &request {
                        if r.action == "setting" {
                            let name = r.body["name"].as_str().ok_or("Nom de réglage absent")?;
                            return m.apply(name, &r.body["value"]);
                        }
                    }
                    m.read()
                })();
                if let Some(m) = mouse.as_mut() { m.set_deadline(None); }
                // Une saisie rejetee ne doit pas faire disparaitre une souris qui repond encore.
                let recovered = if result.is_err() && request.is_some() { mouse.as_mut().and_then(|m| m.read().ok()) } else { None };
                {
                    let mut state = shared.state.lock().unwrap();
                    match &result {
                        Ok(settings) => { state.settings = Some(settings.clone()); state.connected = true; state.error = None; }
                        Err(e) => {
                            if let Some(settings) = recovered { state.settings = Some(settings); state.connected = true; state.error = None; }
                            else { state.connected = false; state.error = Some(e.clone()); mouse = None; }
                        }
                    }
                    shared.version.fetch_add(1, Ordering::Relaxed);
                }
                if let Some(r) = request { let _ = r.answer.send(result.map(|s| json!({"ok":true,"state":s}))); }
                // Faible consommation hors ligne, sans tenir d'Arc du controleur.
                if mouse.is_none() { drop(shared); std::thread::sleep(Duration::from_secs(2)); }
            }
        }).expect("fil souris");
    }
}
fn open(simulated: bool) -> Result<Box<dyn Transport>, String> {
    if simulated { return Ok(Box::new(rustudio_attack_shark::sim::Sim::default())); }
    #[cfg(windows)] { return rustudio_attack_shark::Hid::open().map(|h| Box::new(h) as Box<dyn Transport>); }
    #[cfg(not(windows))] { Err("Le module R5 Ultra réel nécessite Windows".into()) }
}
