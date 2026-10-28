//! Rustudio-Setup.exe : installeur en un seul fichier (l'appli est embarquee).
//!   Rustudio-Setup.exe                                  fenetre d'installation
//!   Rustudio-Setup.exe --quiet [--no-launch] [--no-desktop] [--no-startup] [--for-user COMPTE]...
//! Code de sortie : 0 si l'installation est faite, 1 sinon.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod install;

#[cfg(windows)]
const HTML: &str = include_str!("setup.html");

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
    ui();
}

#[cfg(windows)]
enum Ev {
    Ipc(String),
    Js(String),
    Close,
}

#[cfg(windows)]
fn ui() {
    use serde_json::{json, Value};
    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;
    let el = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = el.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Installer Rustudio")
        .with_inner_size(LogicalSize::new(540.0, 660.0))
        .with_resizable(false)
        .with_maximizable(false)
        .build(&el)
        .expect("fenetre");
    let data = std::env::temp_dir().join("rustudio-setup-webview");
    let mut ctx = wry::WebContext::new(Some(data));
    let p2 = proxy.clone();
    let webview = wry::WebViewBuilder::new_with_web_context(&mut ctx)
        .with_html(HTML)
        .with_background_color((245, 245, 247, 255))
        .with_ipc_handler(move |req| {
            let _ = p2.send_event(Ev::Ipc(req.body().clone()));
        })
        .build(&window)
        .expect("WebView2 absent : installe Microsoft Edge WebView2 puis relance l'installeur.");
    let mut busy = false;
    el.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Ev::Js(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(Ev::Close) => *flow = ControlFlow::Exit,
            Event::UserEvent(Ev::Ipc(msg)) => {
                let m: Value = serde_json::from_str(&msg).unwrap_or(Value::Null);
                match m["cmd"].as_str() {
                    Some("ready") => {
                        let info = json!({"version": install::payload_version().unwrap_or_else(|| "?".into()), "installed": install::installed_version(), "legacy": install::legacy_present()});
                        let _ = webview.evaluate_script(&format!("rs.info({info})"));
                    }
                    Some("install") if !busy => {
                        busy = true;
                        let o = install::Opts { desktop: m["desktop"].as_bool().unwrap_or(true), startup: m["startup"].as_bool().unwrap_or(true), launch: false, quiet: false, for_users: vec![] };
                        let p = proxy.clone();
                        std::thread::spawn(move || {
                            let p3 = p.clone();
                            let step = move |t: &str, f: f64| {
                                let _ = p3.send_event(Ev::Js(format!("rs.step({}, {f})", json!(t))));
                            };
                            let js = match install::install(&o, &step) {
                                Ok(r) => format!("rs.done({r})"),
                                Err(e) => format!("rs.fail({})", json!(e)),
                            };
                            let _ = p.send_event(Ev::Js(js));
                        });
                    }
                    Some("open") => {
                        let exe = install::install_dir().join("Rustudio.exe");
                        let _ = std::process::Command::new("explorer.exe").arg(exe).spawn();
                        let _ = proxy.send_event(Ev::Close);
                    }
                    Some("close") => *flow = ControlFlow::Exit,
                    _ => {}
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } if !busy => *flow = ControlFlow::Exit,
            _ => {}
        }
    });
}
