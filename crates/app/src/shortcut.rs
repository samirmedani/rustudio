//! Raccourci clavier du micro : préférence de ce compte Windows, liée au démarrage de l'appli installée.
use crate::backend::Client;
use rustudio_windows_desktop::hotkey;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static FILE: OnceLock<PathBuf> = OnceLock::new();
static CURRENT: Mutex<String> = Mutex::new(String::new());
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn known(id: &str) -> bool {
    hotkey::CHOICES.iter().any(|(choice, _)| *choice == id)
}

/// Coupe le micro s'il est actif, le rétablit sinon (même commande que l'icône près de l'horloge).
fn toggle_mic(client: &Client) {
    if let Ok(live) = client.live_state()
        && live["connected"] == true
    {
        let silent = live["mic"]["muted"] == true || live["mic"]["partial"].is_string();
        let _ = client.post_module("/api/modules/goxlr-mini", "mute", json!({"channel": "Mic", "muted": !silent, "target": "all"}));
    }
}

/// Lit la préférence. Avec « active », écoute le clavier et lie la combinaison enregistrée (ou celle imposée
/// par « --hotkey » pour un essai). Sans « active » (démonstration), le choix reste affiché sans être lié.
pub fn start(file: PathBuf, client: Client, active: bool, forced: Option<String>) {
    let saved = std::fs::read_to_string(&file).map(|t| t.trim().to_owned()).ok().filter(|id| known(id)).unwrap_or_default();
    let _ = FILE.set(file);
    ACTIVE.store(active, Ordering::Relaxed);
    let wanted = forced.unwrap_or(saved);
    if active {
        hotkey::start(move || toggle_mic(&client));
        if !wanted.is_empty() && let Err(error) = hotkey::bind(&wanted) {
            eprintln!("Raccourci du micro : {error}");
            return;
        }
    }
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = wanted;
}

pub fn current() -> String {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Choix fait dans Réglages : lié tout de suite si l'appli pilote le moteur installé, puis enregistré.
pub fn choose(id: &str) -> Result<(), String> {
    if !known(id) {
        return Err("Raccourci non reconnu.".into());
    }
    if active() {
        hotkey::bind(id)?;
    }
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = id.to_owned();
    if let Some(file) = FILE.get() {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Dossier de préférences inaccessible : {e}"))?;
        }
        std::fs::write(file, id).map_err(|e| format!("Préférence non enregistrée : {e}"))?;
    }
    Ok(())
}
