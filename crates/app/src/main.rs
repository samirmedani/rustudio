//! Rustudio - appli Windows (logiciel libre, licence MIT).
//! Fenetre sur l'interface servie par le moteur (http://127.0.0.1:14565/), icone d'etat et menu pres de l'horloge,
//! bulle a l'ecran quand le micro change (jamais de focus, meme en jeu), relance apres une mise a jour,
//! peripheriques son par defaut de Windows (anti-echo). Instance unique.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod badge;
mod soundcheck;

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const ENGINE: &str = "http://127.0.0.1:14565";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
static QUITTING: AtomicBool = AtomicBool::new(false);
static ICON: &[u8] = include_bytes!("../icons/32x32.png");

struct Icons {
    on: Image<'static>,
    muted: Image<'static>,
    part: Image<'static>,
    off: Image<'static>,
}
fn icons() -> &'static Icons {
    static I: OnceLock<Icons> = OnceLock::new();
    I.get_or_init(|| {
        let base = Image::from_bytes(ICON).expect("icone");
        let (w, h) = (base.width(), base.height());
        let mk = |c| Image::new_owned(badge::badge(base.rgba(), w, c), w, h);
        Icons { on: mk(None), muted: mk(Some((255, 69, 58))), part: mk(Some((255, 159, 10))), off: mk(Some((142, 142, 147))) }
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build()
}
fn get(path: &str) -> Option<Value> {
    agent().get(&format!("{ENGINE}{path}")).call().ok()?.into_json().ok()
}
fn post(path: &str, body: Value) {
    let _ = agent().post(&format!("{ENGINE}/api/{path}")).set("X-Rustudio", "1").send_json(body);
}

fn hidden(cmd: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

// ---------- demarrage avec Windows ----------
fn is_startup() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::*;
        winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY).and_then(|k| k.get_value::<String, _>("Rustudio")).is_ok()
    }
    #[cfg(not(windows))]
    false
}
fn set_startup_reg(on: bool) {
    #[cfg(windows)]
    {
        use winreg::enums::*;
        if let Ok((k, _)) = winreg::RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY) {
            if on {
                let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                let _ = k.set_value("Rustudio", &format!("\"{exe}\" --tray"));
            } else {
                let _ = k.delete_value("Rustudio");
            }
        }
    }
    let _ = (on, RUN_KEY);
}

// ---------- commandes appelees par l'interface ----------
#[tauri::command]
fn audio_defaults() -> Value {
    audio::status()
}
#[tauri::command]
fn audio_fix() -> Value {
    let n = audio::fix();
    let mut st = audio::status();
    st["fixed"] = json!(n);
    st
}
/// Test « son qui crépite » : écoute les voies GoXLR sans rien jouer (hors du fil de la fenêtre).
#[tauri::command]
async fn sound_check(seconds: f64) -> Value {
    tauri::async_runtime::spawn_blocking(move || soundcheck::run(seconds)).await.unwrap_or_else(|e| json!({"error": e.to_string()}))
}
/// Test en boucle : son discret joue sur Sample (isole par le moteur), reecoute par le Stream Mix.
#[tauri::command]
async fn sound_loop_test(seconds: f64) -> Value {
    tauri::async_runtime::spawn_blocking(move || soundcheck::loop_test(seconds, false)).await.unwrap_or_else(|e| json!({"status": "error", "error": e.to_string()}))
}
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("adresse refusée".into());
    }
    hidden("explorer.exe").arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
}
#[tauri::command]
fn app_info() -> Value {
    json!({"version": VERSION, "startup": is_startup(), "app": "tauri"})
}
#[tauri::command]
fn set_startup(on: bool) -> bool {
    set_startup_reg(on);
    is_startup()
}
#[tauri::command]
fn osd_done(app: AppHandle) {
    if let Some(w) = app.get_webview_window("osd") {
        let _ = w.hide();
        webview_visible(&w, false);
    }
}

// ---------- fenetre ----------
/// Dit a WebView2 si la page est visible. Cachee ou reduite, la page passe en « hidden » : plus de rendu ni
/// d'animation, et l'interface ralentit ses lectures (WebView2 ne le devine pas seul pour une fenetre cachee).
fn webview_visible(w: &WebviewWindow, on: bool) {
    #[cfg(windows)]
    {
        let _ = w.with_webview(move |wv| unsafe {
            let _ = wv.controller().SetIsVisible(on);
        });
    }
    let _ = (w, on);
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        webview_visible(&w, true);
        let _ = w.set_focus();
    }
}

#[cfg(windows)]
fn style_main(w: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    if let Ok(h) = w.hwnd() {
        let hwnd = HWND(h.0 as _);
        // barre de titre de la couleur du fond de l'interface (#F5F5F7), texte #1D1D1F
        for (attr, val) in [(20u32, 0u32), (35, 0x00F7F5F5), (36, 0x001F1D1D)] {
            unsafe {
                let _ = DwmSetWindowAttribute(hwnd, windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(attr as i32), &val as *const u32 as _, 4);
            }
        }
    }
}

/// « M'écouter » enregistre l'entree Chat Mic : seule la page locale du moteur a le micro, rien d'autre.
#[cfg(windows)]
fn allow_mic(w: &WebviewWindow) {
    let _ = w.with_webview(|wv| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use webview2_com::PermissionRequestedEventHandler;
        let Ok(core) = wv.controller().CoreWebView2() else { return };
        let handler = PermissionRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut uri = windows::core::PWSTR::null();
                args.Uri(&mut uri)?;
                let u = uri.to_string().unwrap_or_default();
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                let local = u.starts_with(ENGINE);
                let ok = local && (kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE);
                args.SetState(if ok { COREWEBVIEW2_PERMISSION_STATE_ALLOW } else { COREWEBVIEW2_PERMISSION_STATE_DENY })?;
            }
            Ok(())
        }));
        let mut token = Default::default();
        let _ = core.add_PermissionRequested(&handler, &mut token);
    });
}

// ---------- bulle a l'ecran ----------
fn osd(app: &AppHandle, title: &str, sub: &str, colour: &str, off: bool) {
    let Some(w) = app.get_webview_window("osd") else { return };
    if let Ok(Some(m)) = w.primary_monitor() {
        let (ms, mp, sc) = (m.size(), m.position(), m.scale_factor());
        let (bw, bh) = ((342.0 * sc) as i32, (86.0 * sc) as i32);
        let x = mp.x + (ms.width as i32 - bw) / 2;
        let y = mp.y + ms.height as i32 - bh - (110.0 * sc) as i32;
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
    webview_visible(&w, true);
    show_no_activate(&w);
    let _ = w.emit("osd", json!({"title": title, "sub": sub, "color": colour, "off": off, "ms": 1400}));
}

fn show_no_activate(w: &WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::*;
        if let Ok(h) = w.hwnd() {
            let hwnd = HWND(h.0 as _);
            unsafe {
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as isize);
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
            return;
        }
    }
    let _ = w.show();
}

fn mic_label(t: &str) -> &'static str {
    match t {
        "stream" => "coupé du stream",
        "chat" => "coupé de Discord",
        "phones" => "coupé du casque",
        "lineout" => "coupé des enceintes",
        "all" => "coupé",
        _ => "actif",
    }
}

// ---------- menu de l'icone ----------
fn build_menu(app: &AppHandle, st: Option<&Value>) -> tauri::Result<Menu<tauri::Wry>> {
    let muted = st.map(|s| {
        let s = &s["state"];
        s["mutes"]["Mic"].as_bool().unwrap_or(false) || s["partial"]["Mic"].is_string()
    });
    let open = MenuItem::with_id(app, "open", "Ouvrir Rustudio", true, None::<&str>)?;
    let mic = CheckMenuItem::with_id(app, "mic", if muted == Some(true) { "Réactiver le micro" } else { "Couper le micro" }, muted.is_some(), muted.unwrap_or(false), None::<&str>)?;
    let scenes = Submenu::with_id(app, "scenes", "Scènes", st.is_some())?;
    if let Some(s) = st {
        let active = s["state"]["scene"].as_str().unwrap_or("");
        if let Some(m) = s["meta"]["scenes"].as_object() {
            for (k, v) in m {
                scenes.append(&CheckMenuItem::with_id(app, format!("scene:{k}"), v["label"].as_str().unwrap_or(k), true, k == active, None::<&str>)?)?;
            }
        }
        if let Some(m) = s["state"]["user_scenes"].as_object().filter(|m| !m.is_empty()) {
            scenes.append(&PredefinedMenuItem::separator(app)?)?;
            for k in m.keys() {
                scenes.append(&CheckMenuItem::with_id(app, format!("scene:{k}"), k, true, k == active, None::<&str>)?)?;
            }
        }
    }
    let reconnect = MenuItem::with_id(app, "reconnect", "Reconnecter la GoXLR", true, None::<&str>)?;
    let diag = MenuItem::with_id(app, "diag", "Diagnostic…", true, None::<&str>)?;
    let startup = CheckMenuItem::with_id(app, "startup", "Démarrer avec Windows", true, is_startup(), None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter l'appli", true, None::<&str>)?;
    Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &mic, &scenes, &PredefinedMenuItem::separator(app)?, &reconnect, &diag, &startup, &quit])
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        "mic" => {
            let st = get("/api/state");
            let muted = st.as_ref().map(|s| s["state"]["mutes"]["Mic"].as_bool().unwrap_or(false) || s["state"]["partial"]["Mic"].is_string()).unwrap_or(false);
            std::thread::spawn(move || post("mute", json!({"channel": "Mic", "muted": !muted, "target": "all"})));
        }
        "reconnect" => {
            std::thread::spawn(|| post("reconnect", json!({})));
        }
        "diag" => {
            show_main(app);
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.eval("typeof openDiag === 'function' && openDiag()");
            }
        }
        "startup" => set_startup_reg(!is_startup()),
        "quit" => {
            QUITTING.store(true, Ordering::SeqCst);
            app.exit(0);
        }
        s if s.starts_with("scene:") => {
            let name = s[6..].to_string();
            std::thread::spawn(move || post("scene", json!({"name": name})));
        }
        _ => {}
    }
}

// ---------- surveillance du moteur ----------
fn installed_version() -> Option<String> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    m["version"].as_str().map(String::from)
}

fn newer(a: &str, b: &str) -> bool {
    let p = |s: &str| -> Vec<u64> { s.split(['.', '-', '+']).take(3).map(|x| x.parse().unwrap_or(0)).collect() };
    p(a) > p(b)
}

/// Apres une mise a jour, le moteur annonce la nouvelle version et l'exe a deja ete remplace : on relance le nouveau.
fn relaunch_if_updated(app: &AppHandle, engine_version: &str) -> bool {
    if !newer(engine_version, VERSION) {
        return false;
    }
    let Some(disk) = installed_version() else { return false };
    if !newer(&disk, VERSION) {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let visible = app.get_webview_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
    let mut c = std::process::Command::new(exe);
    c.args(["--relaunch", &std::process::id().to_string()]);
    if !visible {
        c.arg("--tray");
    }
    if c.spawn().is_ok() {
        QUITTING.store(true, Ordering::SeqCst);
        app.exit(0);
        return true;
    }
    false
}

fn monitor(app: AppHandle, tray: TrayIcon) {
    let mut last_mic: Option<String> = None;
    let (mut icon_key, mut tip) = (String::new(), String::new());
    let (mut last_ok, mut last_rescue) = (Instant::now(), Instant::now() - Duration::from_secs(120));
    let (mut tick, mut audio_key, mut menu_key, mut loaded) = (0u64, String::new(), String::new(), false);
    loop {
        std::thread::sleep(Duration::from_secs(1));
        tick += 1;
        let live = get("/api/live");
        let (key, text) = match &live {
            Some(l) => {
                last_ok = Instant::now();
                if !loaded {
                    loaded = true;
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.navigate(format!("{ENGINE}/").parse().unwrap());
                    }
                }
                let connected = l["connected"].as_bool().unwrap_or(false);
                let health = l["health"].as_str().unwrap_or("ok");
                let mic = if l["mic"]["muted"].as_bool().unwrap_or(false) { "all".to_string() } else { l["mic"]["partial"].as_str().unwrap_or("on").to_string() };
                if let Some(v) = l["app_version"].as_str() {
                    if relaunch_if_updated(&app, v) {
                        return;
                    }
                }
                if connected && l["osd"].as_bool().unwrap_or(true) && last_mic.as_deref().map(|m| m != mic).unwrap_or(false) {
                    match mic.as_str() {
                        "on" => osd(&app, "Micro actif", "Tout le monde t'entend", "#30D158", false),
                        "all" => osd(&app, "Micro coupé", "Personne ne t'entend", "#FF453A", true),
                        m => osd(&app, &format!("Micro {}", mic_label(m)), "Les autres t'entendent encore", "#FF9F0A", true),
                    }
                }
                if connected {
                    last_mic = Some(mic.clone());
                }
                let key = if !connected || health != "ok" { "off" } else if mic == "on" { "on" } else if mic == "all" { "muted" } else { "part" };
                let text = if !connected {
                    if health == "absent" { "Rustudio · GoXLR débranchée".to_string() } else { "Rustudio · reconnexion…".to_string() }
                } else if mic == "on" {
                    "Rustudio · micro actif".into()
                } else if mic == "all" {
                    "Rustudio · micro coupé".into()
                } else {
                    format!("Rustudio · micro {}", mic_label(&mic))
                };
                (key.to_string(), text)
            }
            None => {
                // moteur muet : le service est relance (l'installeur autorise ce compte a le demarrer)
                if last_ok.elapsed() > Duration::from_secs(20) && last_rescue.elapsed() > Duration::from_secs(60) {
                    last_rescue = Instant::now();
                    let _ = hidden("sc").args(["start", "Rustudio"]).spawn();
                }
                ("off".to_string(), "Rustudio · moteur arrêté".to_string())
            }
        };
        if key != icon_key {
            let i = icons();
            let img = match key.as_str() { "on" => &i.on, "muted" => &i.muted, "part" => &i.part, _ => &i.off };
            let _ = tray.set_icon(Some(img.clone()));
            icon_key = key;
        }
        if text != tip {
            let _ = tray.set_tooltip(Some(&text));
            tip = text;
        }
        // peripheriques Windows par defaut : relus toutes les 4 s, envoyes a l'interface s'ils changent
        if tick % 4 == 0 && live.is_some() {
            let st = audio::status();
            let k = st.to_string();
            if k != audio_key {
                audio_key = k;
                let _ = app.emit_to("main", "audio-defaults", st);
            }
        }
        // menu : micro et scenes a jour
        if tick % 3 == 1 {
            let st = get("/api/state");
            let mk = st.as_ref().map(|s| format!("{}|{}|{}|{}", s["state"]["mutes"]["Mic"], s["state"]["partial"]["Mic"], s["state"]["scene"], s["state"]["user_scenes"].as_object().map(|m| m.keys().cloned().collect::<Vec<_>>().join(",")).unwrap_or_default())).unwrap_or_default();
            let mk = format!("{mk}|{}", is_startup());
            if mk != menu_key {
                menu_key = mk;
                if let Ok(m) = build_menu(&app, st.as_ref()) {
                    let _ = tray.set_menu(Some(m));
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tray_only = args.iter().any(|a| a == "--tray");
    // diagnostic sans fenetre : Rustudio.exe --sound-check|--loop-test <secondes> <fichier.json>
    if let Some(i) = args.iter().position(|a| a == "--sound-check" || a == "--loop-test") {
        let secs = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(30.0);
        let exclusive = args.iter().any(|a| a == "--exclusive");
        let out = if args[i] == "--loop-test" { soundcheck::loop_test(secs, exclusive) } else { soundcheck::run(secs) }.to_string();
        match args.get(i + 2) {
            Some(f) => {
                let _ = std::fs::write(f, out);
            }
            None => println!("{out}"),
        }
        return;
    }
    // relance apres mise a jour : on attend que l'ancienne instance soit partie
    if let Some(i) = args.iter().position(|a| a == "--relaunch") {
        if let Some(pid) = args.get(i + 1).and_then(|p| p.parse::<u32>().ok()) {
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(20) && pid_alive(pid) {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    let ui_url = format!("{ENGINE}/");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| show_main(app)))
        .invoke_handler(tauri::generate_handler![audio_defaults, audio_fix, sound_check, sound_loop_test, open_external, app_info, set_startup, osd_done])
        .setup(move |app| {
            let handle = app.handle().clone();
            let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Rustudio")
                .inner_size(1280.0, 800.0)
                .min_inner_size(900.0, 620.0)
                .center()
                .theme(Some(tauri::Theme::Light))
                .visible(!tray_only)
                .build()?;
            #[cfg(windows)]
            {
                style_main(&main);
                allow_mic(&main);
            }
            let _ = &ui_url;
            let _osd = WebviewWindowBuilder::new(app, "osd", WebviewUrl::App("osd.html".into()))
                .title("Rustudio")
                .inner_size(342.0, 86.0)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .resizable(false)
                .shadow(false)
                .visible(false)
                .build()?;
            let _ = _osd.set_ignore_cursor_events(true);
            let menu = build_menu(&handle, None)?;
            let tray = TrayIconBuilder::with_id("main")
                .icon(icons().off.clone())
                .tooltip("Rustudio")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            static TOLD: Mutex<bool> = Mutex::new(false);
            let h2 = handle.clone();
            if tray_only {
                webview_visible(&main, false);
            }
            webview_visible(&_osd, false);
            main.on_window_event(move |ev| {
                if let tauri::WindowEvent::Resized(_) = ev {
                    if let Some(w) = h2.get_webview_window("main") {
                        let shown = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
                        webview_visible(&w, shown);
                    }
                }
                if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                    if !QUITTING.load(Ordering::SeqCst) {
                        api.prevent_close();
                        if let Some(w) = h2.get_webview_window("main") {
                            let _ = w.hide();
                            webview_visible(&w, false);
                        }
                        let mut t = TOLD.lock().unwrap();
                        if !*t {
                            *t = true;
                            osd(&h2, "Rustudio reste ouvert", "Retrouve-le près de l'horloge", "#0A84FF", false);
                        }
                    }
                }
            });
            std::thread::spawn(move || monitor(handle, tray));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Rustudio");
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == 259;
        let _ = CloseHandle(h);
        ok
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}
