//! Version de développement en direct (`--dev-live`, lancée par `private/live-pc.mjs`) : l'appli note la page
//! ouverte, pour que la relance après une modification revienne au même endroit, et affiche « DEV » en bas.
use std::sync::atomic::{AtomicBool, Ordering};

static LIVE: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    LIVE.store(true, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    LIVE.load(Ordering::Relaxed)
}

/// Page ouverte, relue par le lanceur à la relance suivante.
pub fn note_page(name: &str) {
    if !enabled() {
        return;
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("Rustudio")) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("dev-live-page.txt"), name);
    }
}
