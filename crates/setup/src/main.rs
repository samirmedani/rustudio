//! Rustudio-Setup.exe : installeur en un seul fichier (l'appli est embarquée), fenêtre native en GPUI.
//!   Rustudio-Setup.exe                                  fenêtre d'installation
//!   Rustudio-Setup.exe --quiet [--no-launch] [--no-desktop] [--no-startup] [--for-user COMPTE]...
//! Code de sortie : 0 si l'installation est faite, 1 sinon.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod install;
#[cfg(windows)]
mod window;

#[cfg(not(windows))]
fn main() {
    eprintln!("Rustudio-Setup ne fonctionne que sous Windows.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |a: &str| args.iter().any(|x| x == a);
    let mut users = vec![];
    for (i, a) in args.iter().enumerate() {
        if a == "--for-user" {
            if let Some(u) = args.get(i + 1) {
                users.push(u.clone());
            }
        }
    }
    if has("--quiet") {
        let o = install::Opts { desktop: !has("--no-desktop"), startup: !has("--no-startup"), launch: !has("--no-launch"), quiet: true, for_users: users };
        let log = std::env::var("RUSTUDIO_SETUP_STATUS").ok();
        let step = |t: &str, p: f64| {
            if let Some(l) = &log {
                let _ = std::fs::OpenOptions::new().create(true).append(true).open(l).and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, "{p:.2}|{t}")
                });
            }
        };
        let code = match install::install(&o, &step) {
            Ok(_) => 0,
            Err(e) => {
                step(&format!("error|{e}"), 1.0);
                1
            }
        };
        std::process::exit(code);
    }
    window::run();
}
