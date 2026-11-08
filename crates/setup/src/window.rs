//! Fenêtre de l'installeur : options, progression, résultat.
use crate::install;
use gpui::{
    App, Application, Bounds, Context, CursorStyle, Div, FontWeight, Image, ImageFormat, IntoElement, ParentElement, Render,
    SharedString, Stateful, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, div, img, prelude::*, px, relative,
    rgb, size,
};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const LOGO: &[u8] = include_bytes!("../../../docs/icon.png");
const BACKGROUND: u32 = 0xf5f5f7;
const TEXT: u32 = 0x1d1d1f;
const MUTED: u32 = 0x6e6e73;
const BLUE: u32 = 0x0071e3;
const GREEN: u32 = 0x34c759;
const RED: u32 = 0xff3b30;

enum View {
    Welcome,
    Work,
    Done(String),
    Fail(String),
}

enum Msg {
    Step(String, f64),
    Done(Value),
    Fail(String),
}

struct Setup {
    view: View,
    version: String,
    installed: Option<String>,
    legacy: bool,
    startup: bool,
    desktop: bool,
    step: String,
    progress: f64,
    busy: Arc<AtomicBool>,
    logo: Arc<Image>,
}

pub fn run() {
    Application::new().run(|cx: &mut App| {
        let _ = cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../../app/assets/fonts/Inter-Regular.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../../app/assets/fonts/Inter-Medium.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../../app/assets/fonts/Inter-SemiBold.ttf").as_slice()),
            std::borrow::Cow::Borrowed(include_bytes!("../../app/assets/fonts/Inter-Bold.ttf").as_slice()),
        ]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let busy = Arc::new(AtomicBool::new(false));
        let bounds = Bounds::centered(None, size(px(520.), px(620.)), cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions { title: Some("Installer Rustudio".into()), ..Default::default() }),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            is_resizable: false,
            ..Default::default()
        };
        let guard = busy.clone();
        let opened = cx.open_window(options, move |window, cx| {
            // pas de fermeture pendant l'installation : elle doit aller au bout (ou revenir en arrière)
            window.on_window_should_close(cx, move |_, _| !guard.load(Ordering::SeqCst));
            cx.new(|_| Setup {
                view: View::Welcome,
                version: install::payload_version().unwrap_or_else(|| "?".into()),
                installed: install::installed_version(),
                legacy: install::legacy_present(),
                startup: true,
                desktop: true,
                step: "Préparation…".into(),
                progress: 0.0,
                busy,
                logo: Arc::new(Image::from_bytes(ImageFormat::Png, LOGO.to_vec())),
            })
        });
        if opened.is_err() {
            cx.quit();
        }
    });
}

impl Setup {
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.view = View::Work;
        let o = install::Opts { desktop: self.desktop, startup: self.startup, launch: false, quiet: false, for_users: vec![] };
        let (tx, rx) = async_channel::unbounded::<Msg>();
        std::thread::spawn(move || {
            let steps = tx.clone();
            let step = move |t: &str, p: f64| {
                let _ = steps.send_blocking(Msg::Step(t.to_string(), p));
            };
            let _ = tx.send_blocking(match install::install(&o, &step) {
                Ok(r) => Msg::Done(r),
                Err(e) => Msg::Fail(e),
            });
        });
        cx.spawn(async move |this, cx| {
            while let Ok(msg) = rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    match msg {
                        Msg::Step(t, p) => {
                            this.step = t;
                            this.progress = p.clamp(0.0, 1.0);
                        }
                        Msg::Done(r) => {
                            this.busy.store(false, Ordering::SeqCst);
                            let mut sub = String::new();
                            if r["migrated"].as_bool() == Some(true) {
                                sub.push_str("GoXLR Mini a été remplacé, tes réglages sont repris. ");
                            }
                            sub.push_str(if r["connected"].as_bool() == Some(true) {
                                "Ta GoXLR Mini est connectée. Retrouve-la dans Mes appareils."
                            } else {
                                "Ouvre Mes appareils pour retrouver tes appareils et leur état."
                            });
                            this.view = View::Done(sub);
                        }
                        Msg::Fail(e) => {
                            this.busy.store(false, Ordering::SeqCst);
                            this.view = View::Fail(e);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn welcome(&self, cx: &mut Context<Self>) -> Div {
        let sub = match &self.installed {
            Some(v) => format!("Version {} · installée : {v}", self.version),
            None => format!("Version {}", self.version),
        };
        let action = match &self.installed {
            Some(v) if *v == self.version => "Réinstaller",
            Some(_) => "Mettre à jour",
            None => "Installer",
        };
        let card = div()
            .w_full()
            .flex()
            .flex_col()
            .px(px(18.))
            .py(px(4.))
            .rounded(px(18.))
            .bg(rgb(0xffffff))
            .shadow_sm()
            .child(self.option("startup", "Démarrer avec Windows", "L’icône attend près de l’horloge.", self.startup, cx))
            .child(div().h(px(1.)).bg(rgb(0xe5e5ea)))
            .child(self.option("desktop", "Raccourci sur le Bureau", "En plus du menu Démarrer.", self.desktop, cx));
        let mut view = div()
            .flex()
            .flex_col()
            .items_center()
            .size_full()
            .child(title("Rustudio"))
            .child(subtitle(format!("Ton matériel, dans un même espace.\n{sub}")))
            .child(card);
        if self.legacy {
            view = view.child(
                div()
                    .mt(px(14.))
                    .w_full()
                    .p(px(12.))
                    .rounded(px(14.))
                    .bg(rgb(0xe6f0fc))
                    .text_color(rgb(0x0b4f9c))
                    .text_size(px(12.8))
                    .child("GoXLR Mini est installé : Rustudio le remplace et reprend tes réglages (voix, scènes, couleurs, égaliseur)."),
            );
        }
        view.child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(pill("cancel", "Annuler", false).on_click(cx.listener(|_, _, _, cx| cx.quit())))
                    .child(pill("install", action, true).on_click(cx.listener(|this, _, _, cx| this.start(cx)))),
            )
            .child(
                div()
                    .mt(px(14.))
                    .text_size(px(11.))
                    .text_color(rgb(0x8e8e93))
                    .child("Logiciel libre (licence MIT). Projet indépendant, non affilié à TC-Helicon ni à Music Tribe."),
            )
    }

    fn option(&self, id: &'static str, label: &'static str, hint: &'static str, on: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .py(px(13.))
            .cursor(CursorStyle::PointingHand)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(label))
                    .child(div().text_size(px(12.5)).text_color(rgb(MUTED)).child(hint)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(46.))
                    .h(px(28.))
                    .p(px(2.))
                    .rounded_full()
                    .flex()
                    .when(on, |d| d.justify_end())
                    .bg(rgb(if on { GREEN } else { 0xd1d1d6 }))
                    .child(div().size(px(24.)).rounded_full().bg(rgb(0xffffff)).shadow_sm()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if id == "startup" {
                    this.startup = !this.startup;
                } else {
                    this.desktop = !this.desktop;
                }
                cx.notify();
            }))
    }

    fn work(&self) -> Div {
        div()
            .flex()
            .flex_col()
            .items_center()
            .size_full()
            .child(title("Installation…"))
            .child(subtitle("Quelques secondes. Le son peut couper un instant."))
            .child(
                div()
                    .mt(px(20.))
                    .w_full()
                    .h(px(8.))
                    .rounded(px(4.))
                    .bg(rgb(0xe5e5ea))
                    .child(div().h_full().w(relative(self.progress as f32)).rounded(px(4.)).bg(rgb(BLUE))),
            )
            .child(div().mt(px(12.)).text_size(px(13.5)).text_color(rgb(MUTED)).child(self.step.clone()))
    }

    fn result(&self, ok: bool, text: &str, cx: &mut Context<Self>) -> Div {
        let badge = div()
            .size(px(64.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(if ok { GREEN } else { RED }))
            .text_color(rgb(0xffffff))
            .text_size(px(30.))
            .font_weight(FontWeight::BOLD)
            .child(if ok { "✓" } else { "✕" });
        let mut actions = div().flex().gap(px(10.));
        if ok {
            actions = actions
                .child(pill("close", "Fermer", false).on_click(cx.listener(|_, _, _, cx| cx.quit())))
                .child(pill("open", "Ouvrir Rustudio", true).on_click(cx.listener(|_, _, _, cx| {
                    // explorer.exe lance l'appli avec les droits normaux de la session, pas ceux de l'installeur
                    let exe = install::install_dir().join("Rustudio.exe");
                    let _ = std::process::Command::new("explorer.exe").arg(exe).spawn();
                    cx.quit();
                })));
        } else {
            actions = actions.child(pill("close", "Fermer", true).on_click(cx.listener(|_, _, _, cx| cx.quit())));
        }
        div()
            .flex()
            .flex_col()
            .items_center()
            .size_full()
            .child(div().mb(px(18.)).child(badge))
            .child(title(if ok { "Rustudio est installé" } else { "Installation impossible" }))
            .child(subtitle(text.to_string()))
            .child(div().flex_1())
            .child(actions)
    }
}

impl Render for Setup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.view {
            View::Welcome => self.welcome(cx),
            View::Work => self.work(),
            View::Done(text) => {
                let text = text.clone();
                self.result(true, &text, cx)
            }
            View::Fail(text) => {
                let text = text.clone();
                self.result(false, &text, cx)
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(rgb(BACKGROUND))
            .font_family("Inter")
            .text_color(rgb(TEXT))
            .px(px(40.))
            .pt(px(40.))
            .pb(px(26.))
            .child(img(self.logo.clone()).size(px(96.)).mb(px(20.)))
            .child(body)
    }
}

fn title(text: &'static str) -> Div {
    div().text_size(px(30.)).font_weight(FontWeight::BOLD).mb(px(6.)).child(text)
}

fn subtitle(text: impl Into<SharedString>) -> Div {
    div().text_size(px(14.5)).text_color(rgb(MUTED)).text_center().mb(px(24.)).child(text.into())
}

fn pill(id: &'static str, text: &'static str, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(44.))
        .px(px(26.))
        .flex()
        .items_center()
        .rounded_full()
        .cursor(CursorStyle::PointingHand)
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .when(primary, |d| d.bg(rgb(BLUE)).text_color(rgb(0xffffff)).hover(|s| s.bg(rgb(0x0077ed))))
        .when(!primary, |d| d.bg(rgb(0xe8e8ed)).hover(|s| s.bg(rgb(0xdcdce1))))
        .child(text)
}
