#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(any(windows, unix))]
mod backend;
#[cfg(any(windows, unix))]
mod desktop;
#[cfg(any(windows, unix))]
mod goxlr;
#[cfg(any(windows, unix))]
mod library;
#[cfg(any(windows, unix))]
mod mixer;
#[cfg(any(windows, unix))]
mod mouse;
#[cfg(any(windows, unix, test))]
mod profiles;
#[cfg(any(windows, unix))]
mod settings;
#[cfg(any(windows, unix))]
mod shortcut;
#[cfg(any(windows, unix))]
mod tray;
#[cfg(any(windows, unix))]
mod tray_panel;
#[cfg(any(windows, unix))]
mod ui;
#[cfg(any(windows, unix))]
mod update_sheet;
#[cfg(any(windows, unix))]
mod voice;
#[cfg(any(windows, unix))]
mod win_session;

#[cfg(not(any(windows, unix)))]
fn main() {
    eprintln!("Rustudio est une application Windows.");
    std::process::exit(1);
}

#[cfg(any(windows, unix))]
fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

/// Version de l'appli : la même que celle du moteur livré avec elle.
#[cfg(any(windows, unix))]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(any(windows, unix))]
fn run() -> Result<(), String> {
    use gpui::{App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
    use rustudio_windows_desktop::{session, soundcheck};
    use std::time::{Duration, Instant};

    let all: Vec<String> = std::env::args().skip(1).collect();
    // Diagnostic sans fenêtre : Rustudio.exe --sound-check|--loop-test <secondes> [fichier.json] [--exclusive]
    // Rustudio.exe --latency-test [fichier.json] : estimation de Windows et aller-retour mesuré
    // (la voie Sample doit être isolée vers le Stream Mix : action « sound-test » du moteur).
    if let Some(i) = all.iter().position(|a| a == "--latency-test") {
        let out = serde_json::json!({"announced": soundcheck::announced_latency(), "measured": soundcheck::latency_test()}).to_string();
        match all.get(i + 1).filter(|f| !f.starts_with("--")) {
            Some(f) => std::fs::write(f, out).map_err(|e| e.to_string())?,
            None => println!("{out}"),
        }
        return Ok(());
    }
    if let Some(i) = all.iter().position(|a| a == "--sound-check" || a == "--loop-test") {
        let secs = all.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(30.0);
        let out = if all[i] == "--loop-test" {
            soundcheck::loop_test(secs, all.iter().any(|a| a == "--exclusive"))
        } else {
            soundcheck::run(secs)
        }
        .to_string();
        match all.get(i + 2).filter(|f| !f.starts_with("--")) {
            Some(f) => std::fs::write(f, out).map_err(|e| e.to_string())?,
            None => println!("{out}"),
        }
        return Ok(());
    }

    let mut args = all.into_iter();
    let mut port = None;
    let mut profiles_dir = None;
    let mut force_live = false;
    let mut demo = false;
    let mut open_mixer = false;
    let mut inspect = false;
    let mut start_demo = false;
    let mut native_osd = false;
    let mut tray_mode = false;
    let mut duration = None;
    let mut show = true;
    let mut self_test = false;
    let mut relaunch = None;
    let mut start_page = None;
    let mut theme = None;
    let mut hotkey = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = Some(args.next().and_then(|x| x.parse::<u16>().ok()).filter(|p| *p > 0).ok_or("Port moteur invalide.")?);
            }
            "--seconds" => {
                duration = Some(Duration::from_secs(
                    args.next().and_then(|x| x.parse().ok()).filter(|n| (1..=300).contains(n)).ok_or("Durée attendue : 1 à 300 secondes.")?,
                ))
            }
            "--relaunch" => relaunch = args.next().and_then(|x| x.parse::<u32>().ok()),
            "--hidden" => show = false,
            "--live" => force_live = true,
            "--demo" => demo = true,
            "--native-osd" => native_osd = true,
            "--tray" => tray_mode = true,
            "--start-demo" => start_demo = true,
            "--mixer" => open_mixer = true,
            "--page" => start_page = Some(desktop::Page::from_name(&args.next().ok_or("Page manquante.")?).ok_or("Page inconnue.")?),
            // apparence imposée pour cette exécution, sans toucher à la préférence (captures, essais)
            "--theme" => theme = Some(args.next().as_deref().and_then(ui::Appearance::from_name).ok_or("Apparence attendue : auto, clair ou sombre.")?),
            // raccourci du micro imposé pour un essai, y compris en démonstration (jamais enregistré)
            "--hotkey" => hotkey = Some(args.next().ok_or("Raccourci manquant.")?),
            "--inspect" => inspect = true,
            "--profiles-dir" => profiles_dir = Some(std::path::PathBuf::from(args.next().ok_or("Dossier de profils manquant.")?)),
            "--self-test" => self_test = true,
            // ouvre le panneau de l'icône dès le démarrage (captures et bancs de test)
            "--panel" => {}
            _ => return Err(format!("Option inconnue : {arg}")),
        }
    }
    // Sans option, Rustudio pilote le moteur installé. Les essais passent toujours par la démonstration.
    let live = force_live || !(demo || start_demo || self_test || port.is_some_and(|p| p != 14565));
    if live && (demo || start_demo || self_test) {
        return Err("La démonstration ne peut pas utiliser le moteur installé.".into());
    }
    let (port, _demo_engine) = if start_demo {
        let engine = DemoEngine::start(port)?;
        (engine.port, Some(engine))
    } else {
        (port.unwrap_or(if live { 14565 } else { 14566 }), None)
    };
    if !live && port == 14565 {
        return Err("Le port 14565 est celui du moteur installé.".into());
    }
    let dir = profiles_dir.unwrap_or_else(|| {
        if live {
            std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Rustudio").join("profils")
        } else {
            std::env::temp_dir().join("Rustudio Démo").join("profils")
        }
    });
    match theme {
        Some(choice) => ui::set_appearance(choice),
        None => ui::load_appearance(dir.parent().unwrap_or(&dir).join("apparence.txt")),
    }
    let client = backend::Client::new(port, live);
    if inspect {
        let inventory = client.inventory()?;
        let mix = client.snapshot(None)?;
        println!("inventaire={} mixeur={} voies={}", inventory.devices.len(), mix.health, mix.channels.len());
        return Ok(());
    }
    if self_test {
        let state = client.snapshot(None)?;
        backend::verify(&client, &state)?;
        desktop::verify_profiles(&client)?;
        mouse::verify(&client)?;
        return goxlr::verify(&client);
    }
    if let Some(pid) = relaunch {
        session::wait_exit(pid, Duration::from_secs(20));
    }
    // Appli installée : une seule instance par session ; la seconde ouverture montre la fenêtre existante.
    let instance = if live && duration.is_none() {
        match session::Instance::acquire("Rustudio") {
            Some(instance) => Some(instance),
            None => return Ok(()),
        }
    } else {
        None
    };
    // Raccourci du micro : seulement pour l'appli installée (ou un essai explicite avec « --hotkey »).
    shortcut::start(
        dir.parent().unwrap_or(&dir).join("raccourci-micro.txt"),
        client.clone(),
        instance.is_some() || hotkey.is_some(),
        hotkey,
    );
    // Avec le moteur installé, l'icône près de l'horloge garde Rustudio ouvert, fenêtre fermée ou non.
    let resident = live || tray_mode;
    let initial = start_page.unwrap_or(if open_mixer { desktop::Page::Mixer } else { desktop::Page::Devices });
    let start = Instant::now();
    Application::new().with_assets(ui::Assets).run(move |cx: &mut App| {
        ui::load_fonts(cx);
        let window_client = client.clone();
        let open = std::rc::Rc::new(move |cx: &mut App, page: Option<desktop::Page>, visible: bool| {
            if let Some(handle) = cx.windows().iter().find_map(|w| w.downcast::<mixer::Mixer>()) {
                let _ = handle.update(cx, |mixer, window, cx| {
                    if let Some(page) = page {
                        mixer.go_to(page, cx);
                    }
                    window.activate_window();
                });
                return;
            }
            let client = window_client.clone();
            let dir = dir.clone();
            // RUSTUDIO_WINDOW_HEIGHT (développement) : fenêtre plus haute pour voir une page entière
            let height = std::env::var("RUSTUDIO_WINDOW_HEIGHT").ok().and_then(|h| h.parse().ok()).unwrap_or(800.);
            #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
            let mut bounds = Bounds::centered(None, size(px(1240.), px(height)), cx);
            // Aperçu sur Mac (développement, RUSTUDIO_PREVIEW) : fenêtre presque hors de l'écran, voir preview_float.
            #[cfg(target_os = "macos")]
            if std::env::var_os("RUSTUDIO_PREVIEW").is_some()
                && let Some(d) = cx.primary_display()
            {
                let b = d.bounds();
                bounds.origin = gpui::point(b.origin.x + b.size.width - px(24.), b.origin.y + b.size.height - px(24.));
            }
            let result = cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some(if live { "Rustudio" } else { "Rustudio · Démonstration" }.into()),
                        // barre de titre dessinée par Rustudio (boutons de fenêtre dans la page)
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1000.), px(660.))),
                    focus: visible,
                    show: visible,
                    ..Default::default()
                },
                |_window, cx| {
                    #[cfg(target_os = "macos")]
                    preview_float(_window, cx);
                    cx.new(|cx| {
                        // Windows passe du clair au sombre : la fenêtre se redessine aussitôt
                        cx.observe_window_appearance(_window, |_, _, cx| cx.notify()).detach();
                        let mut mixer = mixer::Mixer::new(
                            client,
                            profiles::Store::new(dir),
                            initial,
                            native_osd && !resident,
                            if resident { None } else { duration },
                            start,
                            cx,
                        );
                        mixer.go_to(page.unwrap_or(initial), cx);
                        mixer
                    })
                },
            );
            if let Err(err) = result {
                eprintln!("Ouverture de la fenêtre : {err:#}");
                if !resident {
                    cx.quit();
                }
            }
        });
        if resident {
            let opener = open.clone();
            if let Err(error) = tray::start(cx, client, live || native_osd, duration, move |cx, page| opener(cx, page, true)) {
                eprintln!("Icône Rustudio : {error}");
                cx.quit();
                return;
            }
            if let Some(instance) = instance {
                let (tx, rx) = async_channel::bounded::<()>(4);
                std::thread::spawn(move || {
                    while instance.wait_show() {
                        if tx.send_blocking(()).is_err() {
                            break;
                        }
                    }
                });
                let opener = open.clone();
                cx.spawn(async move |cx| {
                    while rx.recv().await.is_ok() {
                        let _ = cx.update(|cx| opener(cx, None, true));
                    }
                })
                .detach();
            }
            if !tray_mode {
                open(cx, None, show);
            }
        } else {
            open(cx, None, show);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        }
    });
    Ok(())
}
#[cfg(any(windows, unix))]
struct DemoEngine {
    child: std::process::Child,
    _state: tempfile::TempDir,
    port: u16,
}

#[cfg(any(windows, unix))]
impl DemoEngine {
    fn start(port: Option<u16>) -> Result<Self, String> {
        #[cfg(windows)]
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        if port == Some(14565) {
            return Err(
                "Le port de l’installation n’est pas disponible pour la démonstration.".into(),
            );
        }
        let guard = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port.unwrap_or(0)))
            .map_err(|_| "Le port de démonstration est indisponible.")?;
        let port = guard.local_addr().map_err(|e| e.to_string())?.port();
        let state = tempfile::Builder::new()
            .prefix("rustudio-gpui-demo-")
            .tempdir()
            .map_err(|e| format!("Dossier de démonstration inaccessible : {e}"))?;
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name(if cfg!(windows) { "rustudio-engine.exe" } else { "rustudio-engine" });
        drop(guard);
        let mut command = Command::new(exe);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let child = command
            .args([
                "run",
                "--child",
                "--simulate",
                "--port",
                &port.to_string(),
                "--data-dir",
            ])
            .arg(state.path())
            .env_remove("RUSTUDIO_SUPERVISED")
            .env_remove("RUSTUDIO_SIM_FW")
            .env_remove("RUSTUDIO_DEMO_UPDATE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Impossible de démarrer le moteur de démonstration : {e}"))?;
        Ok(Self {
            child,
            _state: state,
            port,
        })
    }
}

#[cfg(any(windows, unix))]
impl Drop for DemoEngine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Aperçu sur Mac (développement, RUSTUDIO_PREVIEW) : la fenêtre passe au-dessus des autres (Dock compris), sans
/// jamais devenir active (l'appli n'est pas activée) et en laissant passer tous les clics. macOS la voit presque hors
/// de l'écran et GPUI suspend alors son affichage : on le relance une fois la fenêtre posée.
#[cfg(target_os = "macos")]
fn preview_float(window: &gpui::Window, cx: &mut gpui::App) {
    use objc::{msg_send, runtime::Object, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if std::env::var_os("RUSTUDIO_PREVIEW").is_none() {
        return;
    }
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return };
    let view = appkit.ns_view.as_ptr() as usize;
    unsafe {
        let view = view as *mut Object;
        let win: *mut Object = msg_send![view, window];
        let _: () = msg_send![win, setLevel: 101isize];
        let _: () = msg_send![win, setIgnoresMouseEvents: objc::runtime::YES];
        let _: () = msg_send![win, orderFrontRegardless];
    }
    // une image toutes les 100 ms : la couche est redessinée sans attendre l'horloge d'affichage suspendue
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
            let alive = cx.update(|_| unsafe {
                let view = view as *mut Object;
                let layer: *mut Object = msg_send![view, layer];
                if !layer.is_null() {
                    let _: () = msg_send![layer, setNeedsDisplay];
                }
            });
            if alive.is_err() {
                break;
            }
        }
    })
    .detach();
}
