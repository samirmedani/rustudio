//! Raccourci clavier global de la session : une combinaison, une action (couper ou rétablir le micro).
//! Rien n'est enregistré auprès de Windows tant que l'utilisateur n'a pas choisi une combinaison.

/// Combinaisons proposées dans Réglages : identifiant enregistré, libellé affiché. La première désactive.
pub const CHOICES: &[(&str, &str)] = &[
    ("", "Aucun"),
    ("ctrl+alt+m", "Ctrl + Alt + M"),
    ("ctrl+shift+m", "Ctrl + Maj + M"),
    ("ctrl+alt+f12", "Ctrl + Alt + F12"),
    ("f13", "F13"),
    ("pause", "Pause"),
    ("scrolllock", "Arrêt défil"),
];

const MOD_ALT: u32 = 1;
const MOD_CTRL: u32 = 2;
const MOD_SHIFT: u32 = 4;
const MOD_WIN: u32 = 8;

/// Modificateurs et touche virtuelle Windows d'une combinaison comme « ctrl+alt+m ».
/// Une lettre ou un chiffre sans Ctrl, Alt ni Windows est refusé : il capterait la frappe normale.
pub fn parse(combo: &str) -> Option<(u32, u32)> {
    let (mut mods, mut key) = (0u32, None);
    for part in combo.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "ctrl" => mods |= MOD_CTRL,
            "alt" => mods |= MOD_ALT,
            "shift" | "maj" => mods |= MOD_SHIFT,
            "win" => mods |= MOD_WIN,
            name => {
                if key.is_some() {
                    return None;
                }
                key = Some(virtual_key(name)?);
            }
        }
    }
    let key = key?;
    if mods & (MOD_ALT | MOD_CTRL | MOD_WIN) == 0 && (0x30..=0x5a).contains(&key) {
        return None;
    }
    Some((mods, key))
}

fn virtual_key(name: &str) -> Option<u32> {
    match name {
        "pause" => Some(0x13),
        "scrolllock" => Some(0x91),
        k if k.len() == 1 => {
            let c = k.as_bytes()[0];
            (c.is_ascii_lowercase() || c.is_ascii_digit()).then(|| c.to_ascii_uppercase() as u32)
        }
        k => {
            let n: u32 = k.strip_prefix('f')?.parse().ok()?;
            (1..=24).contains(&n).then(|| 0x70 + n - 1)
        }
    }
}

/// Démarre l'écoute (une fois par processus) : « action » s'exécute à chaque appui sur la combinaison liée.
pub fn start(action: impl Fn() + Send + 'static) {
    #[cfg(windows)]
    win::start(Box::new(action));
    #[cfg(not(windows))]
    drop(action);
}

/// Lie la combinaison (vide : aucune). Échoue si elle est invalide ou déjà prise par un autre logiciel.
pub fn bind(combo: &str) -> Result<(), String> {
    let keys = if combo.trim().is_empty() { None } else { Some(parse(combo).ok_or("Raccourci non reconnu.")?) };
    #[cfg(windows)]
    {
        win::bind(keys)
    }
    #[cfg(not(windows))]
    {
        if keys.is_some() { Err("Disponible sous Windows seulement.".into()) } else { Ok(()) }
    }
}

#[cfg(windows)]
mod win {
    use std::sync::{mpsc, Mutex};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT};
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY};

    type Keys = Option<(u32, u32)>;
    struct Worker {
        thread: u32,
        requests: mpsc::Sender<Keys>,
        replies: mpsc::Receiver<Result<(), String>>,
    }
    static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
    const ID: i32 = 1;

    pub fn start(action: Box<dyn Fn() + Send>) {
        let mut worker = WORKER.lock().unwrap_or_else(|e| e.into_inner());
        if worker.is_some() {
            return;
        }
        let (requests, wanted) = mpsc::channel::<Keys>();
        let (answer, replies) = mpsc::channel();
        let (ready, thread) = mpsc::channel();
        // Un raccourci sans fenêtre arrive dans la file de messages du fil qui l'a enregistré : ce fil fait tout.
        std::thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE); // crée la file de messages du fil
            let _ = ready.send(GetCurrentThreadId());
            let mut bound = false;
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                match msg.message {
                    WM_HOTKEY => action(),
                    WM_APP => {
                        while let Ok(keys) = wanted.try_recv() {
                            if bound {
                                let _ = UnregisterHotKey(None, ID);
                                bound = false;
                            }
                            let result = match keys {
                                None => Ok(()),
                                Some((mods, key)) => RegisterHotKey(None, ID, HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT, key)
                                    .map(|()| bound = true)
                                    .map_err(|_| "Ce raccourci est déjà utilisé par un autre logiciel.".to_owned()),
                            };
                            let _ = answer.send(result);
                        }
                    }
                    _ => {}
                }
            }
        });
        if let Ok(thread) = thread.recv_timeout(std::time::Duration::from_secs(2)) {
            *worker = Some(Worker { thread, requests, replies });
        }
    }

    pub fn bind(keys: Keys) -> Result<(), String> {
        let worker = WORKER.lock().unwrap_or_else(|e| e.into_inner());
        let worker = worker.as_ref().ok_or("Raccourcis indisponibles.")?;
        worker.requests.send(keys).map_err(|_| "Raccourcis indisponibles.")?;
        unsafe { PostThreadMessageW(worker.thread, WM_APP, WPARAM(0), LPARAM(0)) }.map_err(|_| "Raccourcis indisponibles.")?;
        worker.replies.recv_timeout(std::time::Duration::from_secs(2)).map_err(|_| "Raccourcis indisponibles.".to_owned())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinaisons_proposees_valides_et_frappe_normale_refusee() {
        for (id, _) in CHOICES.iter().skip(1) {
            assert!(parse(id).is_some(), "{id}");
        }
        assert_eq!(parse("ctrl+alt+m"), Some((MOD_CTRL | MOD_ALT, 0x4d)));
        assert_eq!(parse("Ctrl + Maj + F12"), Some((MOD_CTRL | MOD_SHIFT, 0x7b)));
        assert_eq!(parse("f13"), Some((0, 0x7c)));
        assert_eq!(parse("pause"), Some((0, 0x13)));
        for refused in ["m", "shift+m", "7", "ctrl+alt", "ctrl+m+n", "f25", "ctrl+é", ""] {
            assert!(parse(refused).is_none(), "{refused}");
        }
        assert!(bind("touche inconnue").is_err());
    }
}
