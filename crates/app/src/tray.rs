//! Icône près de l'horloge : état du micro, menu rapide, bulle native et surveillance du moteur.
use crate::{backend::Client, desktop::Page, goxlr::Section};
use gpui::{App, AsyncApp};
use rustudio_windows_desktop::{badge, session};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu},
};

const ICON: &[u8] = include_bytes!("../icons/32x32.png");

enum Action {
    Panel,
    Menu(String),
}

struct Icons {
    on: Icon,
    muted: Icon,
    part: Icon,
    off: Icon,
}

fn icons() -> Result<Icons, String> {
    let base = image::load_from_memory(ICON).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = base.dimensions();
    let make = |c| Icon::from_rgba(badge::badge(base.as_raw(), w, c), w, h).map_err(|e| e.to_string());
    Ok(Icons { on: make(None)?, muted: make(Some((255, 69, 58)))?, part: make(Some((255, 159, 10)))?, off: make(Some((142, 142, 147)))? })
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

fn mic_of(live: &Value) -> String {
    if live["mic"]["muted"] == true { "all".into() } else { live["mic"]["partial"].as_str().unwrap_or("on").into() }
}

fn goxlr_connected(live: &Value) -> bool {
    live["modules"]["devices"].as_array().into_iter().flatten().any(|d| d["id"] == "goxlr-mini" && d["connected"] == true)
}

fn connected_devices(live: &Value) -> usize {
    live["modules"]["devices"].as_array().into_iter().flatten().filter(|d| d["connected"] == true).count()
}

/// État de l'icône et infobulle, comme l'appli précédente.
fn status(live: Option<&Value>) -> (&'static str, String) {
    let Some(l) = live else { return ("off", "Rustudio · moteur arrêté".into()) };
    let count = connected_devices(l);
    let connected = goxlr_connected(l);
    let health = l["health"].as_str().unwrap_or("ok");
    let mic = mic_of(l);
    let key = if count == 0 || (connected && health != "ok") { "off" } else if !connected || mic == "on" { "on" } else if mic == "all" { "muted" } else { "part" };
    let text = if count == 0 {
        "Rustudio · aucun appareil connecté".to_string()
    } else if !connected {
        format!("Rustudio · {count} appareil{s} connecté{s}", s = if count > 1 { "s" } else { "" })
    } else if health != "ok" {
        "Rustudio · GoXLR Mini · connexion à vérifier".into()
    } else if mic == "on" {
        "Rustudio · GoXLR Mini · micro actif".into()
    } else {
        format!("Rustudio · GoXLR Mini · micro {}", mic_label(&mic))
    };
    (key, text)
}

fn build_menu(st: Option<&Value>, live_mode: bool) -> Result<Menu, String> {
    let e = |e: tray_icon::menu::Error| e.to_string();
    let connected = st.is_some_and(|s| s["connected"] == true);
    let muted = st.is_some_and(|s| s["state"]["mutes"]["Mic"] == true || s["state"]["partial"]["Mic"].is_string());
    let menu = Menu::new();
    let open = MenuItem::with_id("open", "Ouvrir Rustudio", true, None);
    let devices = MenuItem::with_id("devices", "Mes appareils", true, None);
    let modules = MenuItem::with_id("modules", "Modules", true, None);
    menu.append_items(&[&open, &devices, &modules]).map_err(e)?;
    if connected {
        let goxlr = Submenu::with_id("goxlr", "GoXLR Mini", true);
        let mixer = MenuItem::with_id("goxlr-open", "Ouvrir le mixeur", true, None);
        let mic = CheckMenuItem::with_id("mic", if muted { "Réactiver le micro" } else { "Couper le micro" }, true, muted, None);
        let scenes = Submenu::with_id("scenes", "Scènes", true);
        if let Some(s) = st {
            let active = s["state"]["scene"].as_str().unwrap_or("");
            for (k, v) in s["meta"]["scenes"].as_object().into_iter().flatten() {
                scenes.append(&CheckMenuItem::with_id(MenuId::new(format!("scene:{k}")), v["label"].as_str().unwrap_or(k), true, k == active, None)).map_err(e)?;
            }
            if let Some(user) = s["state"]["user_scenes"].as_object().filter(|m| !m.is_empty()) {
                scenes.append(&PredefinedMenuItem::separator()).map_err(e)?;
                for k in user.keys() {
                    scenes.append(&CheckMenuItem::with_id(MenuId::new(format!("scene:{k}")), k, true, k == active, None)).map_err(e)?;
                }
            }
        }
        let reconnect = MenuItem::with_id("reconnect", "Reconnecter GoXLR Mini", true, None);
        let diag = MenuItem::with_id("diag", "Diagnostic GoXLR Mini…", true, None);
        let items: [&dyn IsMenuItem; 7] = [&mixer, &mic, &scenes, &PredefinedMenuItem::separator(), &reconnect, &diag, &PredefinedMenuItem::separator()];
        goxlr.append_items(&items[..6]).map_err(e)?;
        menu.append_items(&[&PredefinedMenuItem::separator(), &goxlr]).map_err(e)?;
    }
    menu.append(&PredefinedMenuItem::separator()).map_err(e)?;
    if live_mode {
        menu.append(&CheckMenuItem::with_id("startup", "Démarrer avec Windows", true, session::startup_enabled(), None)).map_err(e)?;
    }
    menu.append(&MenuItem::with_id("quit", "Quitter Rustudio", true, None)).map_err(e)?;
    Ok(menu)
}

/// Version écrite sur le disque à côté de l'appli (manifeste de l'installation).
fn installed_version() -> Option<String> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    m["version"].as_str().map(String::from)
}

/// Mise à jour ou retour de version appliqué : le moteur annonce la version du disque, différente de la nôtre.
pub fn should_relaunch(engine: &str, running: &str, disk: Option<&str>) -> bool {
    engine != running && disk == Some(engine)
}

fn relaunch(cx: &mut App) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut c = std::process::Command::new(exe);
    c.args(["--relaunch", &std::process::id().to_string()]);
    if cx.windows().is_empty() {
        c.arg("--tray");
    }
    if c.spawn().is_ok() {
        cx.quit();
        return true;
    }
    false
}

fn on_menu(id: &str, client: &Client, live_mode: bool, cx: &mut App, open: &dyn Fn(&mut App, Option<Page>)) {
    let post = |action: &'static str, body: Value| {
        let client = client.clone();
        cx.background_executor().spawn(async move {
            let _ = client.post_module("/api/modules/goxlr-mini", action, body);
        }).detach();
    };
    match id {
        "open" => open(cx, None),
        "devices" => open(cx, Some(Page::Devices)),
        "modules" => open(cx, Some(Page::Catalogue)),
        "goxlr-open" => open(cx, Some(Page::Mixer)),
        "diag" => open(cx, Some(Page::Go(Section::Diagnostic))),
        "reconnect" => post("reconnect", json!({})),
        "mic" => {
            let client = client.clone();
            cx.background_executor().spawn(async move {
                if let Ok(l) = client.live_state()
                    && goxlr_connected(&l) {
                        let muted = mic_of(&l) != "on";
                        let _ = client.post_module("/api/modules/goxlr-mini", "mute", json!({"channel": "Mic", "muted": !muted, "target": "all"}));
                    }
            }).detach();
        }
        "startup" if live_mode => {
            let _ = session::set_startup(!session::startup_enabled());
        }
        "quit" => cx.quit(),
        s if s.starts_with("scene:") => post("scene", json!({"name": &s[6..]})),
        _ => {}
    }
}

pub fn start(
    cx: &mut App,
    client: Client,
    native_osd: bool,
    duration: Option<Duration>,
    open: impl Fn(&mut App, Option<Page>) + 'static,
) -> Result<(), String> {
    let icons = icons()?;
    let live_mode = client.is_live();
    let tray = TrayIconBuilder::new()
        .with_icon(icons.off.clone())
        .with_tooltip("Rustudio")
        .with_menu(Box::new(build_menu(None, live_mode)?))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())?;
    let (tx, rx) = async_channel::bounded(32);
    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = menu_tx.try_send(Action::Menu(event.id.0.clone()));
    }));
    let tray_id = tray.id().clone();
    if std::env::args().any(|a| a == "--panel") {
        let _ = tx.try_send(Action::Panel);
    }
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click { id, button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event
            && id == tray_id {
                let _ = tx.try_send(Action::Panel);
            }
    }));
    let actions = client.clone();
    let open: crate::tray_panel::Opener = std::rc::Rc::new(open);
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(action) = rx.recv().await {
            let quit = matches!(&action, Action::Menu(id) if id == "quit");
            let _ = cx.update(|cx| match action {
                Action::Panel => crate::tray_panel::toggle(cx, actions.clone(), live_mode, open.clone()),
                Action::Menu(id) => on_menu(&id, &actions, live_mode, cx, &*open),
            });
            if quit {
                break;
            }
        }
    })
    .detach();
    let watch = client.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let tray: TrayIcon = tray;
        let mut previous_mic: Option<String> = None;
        let (mut icon_key, mut tip, mut menu_key) = (String::new(), String::new(), String::new());
        let (mut last_ok, mut last_rescue) = (Instant::now(), Instant::now() - Duration::from_secs(120));
        let mut tick = 0u64;
        loop {
            let reader = watch.clone();
            let with_state = tick.is_multiple_of(3);
            let (live, state) = cx
                .background_executor()
                .spawn(async move {
                    let live = reader.live_state().ok();
                    let state = if with_state && live.is_some() { reader.get("/api/state").ok() } else { None };
                    (live, state)
                })
                .await;
            match &live {
                Some(l) => {
                    last_ok = Instant::now();
                    if live_mode
                        && let Some(v) = l["app_version"].as_str()
                            && should_relaunch(v, crate::VERSION, installed_version().as_deref()) && cx.update(relaunch).unwrap_or(false) {
                                return;
                            }
                    if goxlr_connected(l) {
                        let mic = mic_of(l);
                        if native_osd && l["osd"] != false && previous_mic.as_deref().is_some_and(|m| m != mic) {
                            let shown = if mic == "on" { String::new() } else { mic.clone() };
                            let _ = cx.update(|_| crate::win_session::show_mic(&shown));
                        }
                        previous_mic = Some(mic);
                    } else {
                        previous_mic = None;
                    }
                }
                None => {
                    previous_mic = None;
                    if live_mode && last_ok.elapsed() > Duration::from_secs(20) && last_rescue.elapsed() > Duration::from_secs(60) {
                        last_rescue = Instant::now();
                        session::start_service();
                    }
                }
            }
            let (key, text) = status(live.as_ref());
            if key != icon_key {
                let icon = match key { "on" => &icons.on, "muted" => &icons.muted, "part" => &icons.part, _ => &icons.off };
                let _ = tray.set_icon(Some(icon.clone()));
                icon_key = key.into();
            }
            if text != tip {
                let _ = tray.set_tooltip(Some(&text));
                tip = text;
            }
            if with_state {
                let s = state.as_ref();
                let mk = format!(
                    "{}|{}|{}|{}|{}|{}",
                    s.map(|s| s["connected"].to_string()).unwrap_or_default(),
                    s.map(|s| s["state"]["mutes"]["Mic"].to_string()).unwrap_or_default(),
                    s.map(|s| s["state"]["partial"]["Mic"].to_string()).unwrap_or_default(),
                    s.map(|s| s["state"]["scene"].to_string()).unwrap_or_default(),
                    s.and_then(|s| s["state"]["user_scenes"].as_object()).map(|m| m.keys().cloned().collect::<Vec<_>>().join(",")).unwrap_or_default(),
                    session::startup_enabled(),
                );
                if mk != menu_key
                    && let Ok(menu) = build_menu(s, live_mode) {
                        tray.set_menu(Some(Box::new(menu)));
                        menu_key = mk;
                    }
            }
            tick += 1;
            cx.background_executor().timer(Duration::from_millis(if native_osd { 300 } else { 1000 })).await;
        }
    })
    .detach();
    if let Some(duration) = duration {
        cx.spawn(async move |cx: &mut AsyncApp| {
            cx.background_executor().timer(duration).await;
            let _ = cx.update(|cx| cx.quit());
        })
        .detach();
    }
    println!("tray_ready");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn relance_apres_mise_a_jour_ou_retour() {
        assert!(super::should_relaunch("5.7.1", "5.7.0", Some("5.7.1")));
        assert!(super::should_relaunch("5.6.14", "5.7.0", Some("5.6.14")));
        assert!(!super::should_relaunch("5.7.0", "5.7.0", Some("5.7.0")));
        assert!(!super::should_relaunch("5.6.14", "5.7.0", Some("5.7.0")));
        assert!(!super::should_relaunch("5.6.14", "5.7.0", None));
    }
}
