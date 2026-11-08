//! Coque de l'appli (barre latérale, en-tête, menu ⋯) et pages Mes appareils et Profils.
use crate::{
    backend::{Client, Command, Descriptor, Device},
    goxlr::Section,
    mixer::Mixer,
    profiles::{Profile, Store},
    ui::{self, Icon, State, Tone, alpha, rgb},
};
use gpui::{
    AnimationExt, AnyElement, Context, Div, ElementId, FontWeight, KeyDownEvent, PathPromptOptions, Stateful,
    Window, WindowControlArea, deferred, div, prelude::*, px,
};
use serde_json::json;
use std::path::{Path, PathBuf};

pub(crate) use crate::ui::{hint, panel};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Devices,
    Mouse,
    WindowsAudio,
    Catalogue,
    Versions,
    Help,
    Settings,
    Go(Section),
    Mixer,
    Profiles,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Apply(usize),
    Delete(usize),
}

/// Espace d'un appareil dans la barre latérale.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Space {
    GoXlr,
    Mouse,
}

impl Page {
    /// Nom court d'une page (option --page, captures de la documentation).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "devices" => Page::Devices,
            "profiles" => Page::Profiles,
            "modules" => Page::Catalogue,
            "versions" => Page::Versions,
            "help" => Page::Help,
            "settings" => Page::Settings,
            "audio" => Page::WindowsAudio,
            "mixer" => Page::Mixer,
            "mouse" => Page::Mouse,
            "mic" => Page::Go(Section::Microphone),
            "headphones" => Page::Go(Section::Headphones),
            "routing" => Page::Go(Section::Routing),
            "lighting" => Page::Go(Section::Light),
            "scenes" => Page::Go(Section::Scenes),
            "diagnostic" => Page::Go(Section::Diagnostic),
            _ => return None,
        })
    }
    pub(crate) fn space(self) -> Option<Space> {
        match self {
            Page::Mixer | Page::Go(_) => Some(Space::GoXlr),
            Page::Mouse => Some(Space::Mouse),
            _ => None,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Page::Devices => "Mes appareils",
            Page::Profiles => "Profils",
            Page::Catalogue => "Modules",
            Page::Versions => "Versions",
            Page::Help => "Aide et retours",
            Page::Settings => "Réglages",
            Page::WindowsAudio => "Son Windows",
            Page::Mixer => "Mixeur",
            Page::Mouse => "Ma souris",
            Page::Go(s) => s.label(),
        }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Page::Devices => "Ton matériel et ses réglages, réunis au même endroit.",
            Page::Profiles => "Tes réglages du mixeur pour chaque usage, à partager en JSON.",
            Page::Catalogue => "Les appareils pris en charge et ceux que la communauté peut ajouter.",
            Page::Versions => "Mises à jour vérifiées depuis GitHub, notes et retour à une version précédente.",
            Page::Help => "Signaler un problème, poser une question ou contribuer.",
            Page::Settings => "Le comportement de Rustudio sur ce PC.",
            Page::WindowsAudio => "Entrées et sorties actives, et ce que Windows utilise par défaut.",
            Page::Mixer => "Tes quatre faders et toutes les voies, en direct.",
            Page::Mouse => "Sensibilité, fréquence, batterie et capteur.",
            Page::Go(Section::Microphone) => "Détection, gain et traitement de ta voix.",
            Page::Go(Section::Headphones) => "Égaliseur, correction et styles de ton casque.",
            Page::Go(Section::Routing) => "Choisis quelles sorties reçoivent chaque entrée.",
            Page::Go(Section::Light) => "Couleurs et thèmes des boutons et des faders.",
            Page::Go(Section::Scenes) => "Enregistre tes réglages et rappelle-les, à la main ou selon l’appli.",
            Page::Go(Section::Diagnostic) => "Connexion, son et rapport anonyme à partager.",
        }
    }
}

fn space_pages(space: Space) -> &'static [(Page, Icon, &'static str)] {
    match space {
        Space::GoXlr => &[
            (Page::Mixer, Icon::Sliders, "Mixeur"),
            (Page::Go(Section::Microphone), Icon::Mic, "Micro"),
            (Page::Go(Section::Headphones), Icon::Headphones, "Casque"),
            (Page::Go(Section::Routing), Icon::Route, "Routage"),
            (Page::Go(Section::Light), Icon::Sun, "Éclairage"),
            (Page::Go(Section::Scenes), Icon::Layers, "Scènes"),
            (Page::Go(Section::Diagnostic), Icon::Gauge, "Diagnostic"),
        ],
        Space::Mouse => &[(Page::Mouse, Icon::Mouse, "Ma souris")],
    }
}

fn module_icon(id: &str) -> Icon {
    match id {
        "goxlr-mini" => Icon::Sliders,
        "attack-shark-r5-ultra" => Icon::Mouse,
        _ => Icon::Grid,
    }
}
fn module_page(id: &str) -> Option<Page> {
    match id {
        "goxlr-mini" => Some(Page::Mixer),
        "attack-shark-r5-ultra" => Some(Page::Mouse),
        _ => None,
    }
}
fn capability(id: &str) -> &str {
    match id {
        "mixer" => "Mixeur",
        "microphone" => "Micro",
        "headphone-eq" => "Égaliseur casque",
        "routing" => "Routage",
        "lighting" => "Éclairage",
        "scenes" => "Scènes",
        "dpi" => "DPI",
        "polling" => "Fréquence",
        "battery" => "Batterie",
        "sensor" => "Capteur",
        other => other,
    }
}
pub(crate) fn device_state(device: Option<&Device>) -> (&'static str, State) {
    match device.map(|d| d.status.as_str()) {
        Some("ready") => ("Connecté", State::Ok),
        Some("asleep") => ("En veille · bouge-la pour la réveiller", State::Info),
        Some("starting") => ("Détection en cours", State::Warn),
        Some("disconnected") => ("Débranché", State::Idle),
        Some("error" | "blocked") => ("Connexion à vérifier", State::Error),
        _ => ("Indisponible", State::Idle),
    }
}

/// Ligne « Officiel Rustudio · Créé par … » d'un module.
pub(crate) fn credit(module: &Descriptor) -> Div {
    let official = module.origin != "community";
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .text_size(px(11.5))
        .text_color(rgb(ui::ACCENT))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(ui::icon(Icon::Shield, 14., rgb(ui::ACCENT)))
                .child(if official { "Officiel Rustudio" } else { "Module communautaire" }),
        )
        .when(!module.author.is_empty(), |d| d.child(format!("Créé par {}", module.author)))
}

impl Mixer {
    pub(crate) fn device(&self, id: &str) -> Option<&Device> {
        self.inventory.as_ref()?.devices.iter().find(|d| d.id == id)
    }
    fn module(&self, id: &str) -> Option<&Descriptor> {
        self.inventory.as_ref()?.installed.iter().find(|m| m.id == id)
    }

    fn nav(&self, page: Page, glyph: Icon, label: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let on = self.page == page;
        let fg = if on { ui::ACCENT } else { ui::TEXT };
        let key = format!("nav-{label}");
        let t = if on { 0. } else { ui::hover_t(&key) };
        let el = div()
            .id(ElementId::Name(key.clone().into()))
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(40.))
            .px(px(12.))
            .rounded(px(13.))
            .text_size(px(14.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(fg))
            .cursor(gpui::CursorStyle::PointingHand)
            .when(on, |d| {
                d.bg(gpui::linear_gradient(180., gpui::linear_color_stop(rgb(ui::WHITE), 0.), gpui::linear_color_stop(rgb(0xf3f8ff), 1.)))
                    .border_1()
                    .border_color(alpha(ui::ACCENT, 0.14))
                    .shadow(ui::soft_shadow())
            })
            .when(!on, |d| d.bg(ui::lerp(alpha(ui::WHITE, 0.), alpha(ui::WHITE, 0.75), t)).border_1().border_color(ui::lerp(alpha(ui::WHITE, 0.), alpha(0x000000, 0.04), t)))
            .child(ui::icon(glyph, 19., if on { rgb(ui::ACCENT) } else { ui::lerp(rgb(ui::TEXT2), rgb(ui::TEXT), t) }))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx)));
        ui::hoverable(el, key)
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let mut side = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(240.))
            .m(px(16.))
            .mr(px(0.))
            .p(px(12.))
            .pt(px(18.))
            .rounded(px(24.))
            .bg(alpha(ui::WHITE, 0.62))
            .border_1()
            .border_color(alpha(0x000000, 0.05))
            .shadow(ui::soft_shadow())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .px(px(8.))
                    .child(ui::logo(38.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(19.)).font_weight(FontWeight::BOLD).child("Rustudio"))
                            .child(ui::caption("Ton matériel, ton espace")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .mt(px(22.))
                    .child(self.nav(Page::Devices, Icon::Plug, "Mes appareils", cx))
                    .child(self.nav(Page::Profiles, Icon::Bookmark, "Profils", cx))
                    .child(self.nav(Page::Catalogue, Icon::Grid, "Modules", cx)),
            );
        if let Some(space) = self.current_space() {
            let name = match space {
                Space::GoXlr => "GoXLR Mini",
                Space::Mouse => "R5 Ultra",
            };
            let mut list = div().flex().flex_col().gap(px(3.)).mt(px(8.));
            for (page, glyph, label) in space_pages(space) {
                list = list.child(self.nav(*page, *glyph, label, cx));
            }
            side = side.child(
                div()
                    .mt(px(16.))
                    .pt(px(14.))
                    .border_t_1()
                    .border_color(alpha(0x000000, 0.07))
                    .child(div().px(px(12.)).child(ui::caption("ESPACE APPAREIL")))
                    .child(div().px(px(12.)).pt(px(2.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(name))
                    .child(list),
            );
        }
        let engine = self.inventory.is_some();
        let detected = self
            .inventory
            .as_ref()
            .map(|i| i.devices.iter().filter(|d| matches!(d.status.as_str(), "ready" | "asleep")).count())
            .unwrap_or(0);
        let sub = if !self.client.is_live() {
            "Démonstration".to_owned()
        } else if detected == 0 {
            "Aucun appareil détecté".to_owned()
        } else {
            format!("{detected} appareil{s} détecté{s}", s = if detected > 1 { "s" } else { "" })
        };
        side.child(
            div()
                .mt_auto()
                .pt(px(12.))
                .px(px(6.))
                .border_t_1()
                .border_color(alpha(0x000000, 0.07))
                .child(ui::status(if engine { "Moteur connecté" } else { "Moteur indisponible" }, if engine { State::Ok } else { State::Error }).text_color(rgb(ui::TEXT)))
                .child(div().pl(px(15.)).child(ui::caption(sub)))
                .child(self.update_widget(cx)),
        )
    }

    /// Bas de la barre latérale : version, état et bouton de mise à jour (cherche, puis installe d'un clic).
    fn update_widget(&self, cx: &mut Context<Self>) -> Div {
        let u = self.update.clone().unwrap_or(serde_json::Value::Null);
        let phase = u["phase"].as_str().unwrap_or("");
        let working = matches!(phase, "downloading" | "verifying" | "extracting" | "applying");
        let checking = u["status"] == "checking" || (self.update_then_install && !working);
        let ready = u["status"] == "available" && u["installable"] == true;
        let latest = u["latest"]["version"].as_str().unwrap_or("").to_owned();
        let version = div()
            .id("version-label")
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor(gpui::CursorStyle::PointingHand)
            .text_color(ui::lerp(rgb(ui::TEXT), rgb(ui::ACCENT), ui::hover_t("version-label")))
            .child(format!("v{}", crate::VERSION))
            .on_click(cx.listener(|this, _, _, cx| this.go_to(Page::Versions, cx)));
        let version = ui::hoverable(version, "version-label".to_owned());
        let mut root = div().mt(px(12.)).pt(px(10.)).border_t_1().border_color(alpha(0x000000, 0.07)).flex().flex_col().gap(px(8.));
        if !self.client.is_live() || u["release_build"] == false {
            return root.child(div().flex().items_center().gap(px(8.)).child(version).child(ui::caption("Version de développement")));
        }
        if working {
            let fraction = if phase == "downloading" { u["progress"].as_f64().unwrap_or(0.) as f32 } else { 1. };
            let text = match phase {
                "downloading" => format!("Téléchargement de la {latest}… {} %", (fraction * 100.).round()),
                "verifying" => "Vérification de l’empreinte…".into(),
                "extracting" => "Préparation…".into(),
                _ => "Installation : Rustudio se rouvre tout seul.".into(),
            };
            return root.child(version).child(ui::progress(fraction, ui::ACCENT)).child(ui::caption(text));
        }
        if ready {
            return root.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .p(px(10.))
                    .rounded(px(14.))
                    .bg(alpha(ui::ACCENT, 0.08))
                    .border_1()
                    .border_color(alpha(ui::ACCENT, 0.18))
                    .child(div().flex().items_center().gap(px(8.)).child(div().size(px(7.)).rounded_full().bg(rgb(ui::ACCENT))).child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(format!("Rustudio {latest} est prête"))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(ui::pill("update-now", Some(Icon::Download), "Mettre à jour", Tone::Primary).h(px(30.)).px(px(12.)).text_size(px(12.5)).on_click(cx.listener(|this, _, _, cx| this.update_click(cx))))
                            .child(ui::pill("update-notes", None, "Nouveautés", Tone::Plain).h(px(30.)).px(px(10.)).text_size(px(12.5)).on_click(cx.listener(|this, _, _, cx| this.go_to(Page::Versions, cx)))),
                    )
                    .child(div().flex().items_center().gap(px(6.)).child(version).child(ui::caption("installée"))),
            );
        }
        let (state, colour) = if checking {
            ("Recherche…", ui::TEXT3)
        } else if phase == "failed" {
            ("Mise à jour interrompue", ui::ORANGE)
        } else if u["status"] == "up-to-date" {
            ("À jour", ui::GREEN)
        } else {
            ("Rechercher", ui::TEXT3)
        };
        let t = ui::hover_t("update-check");
        root = root.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(version)
                .child(div().flex().items_center().gap(px(5.)).child(div().size(px(6.)).rounded_full().bg(rgb(colour))).child(ui::caption(state)))
                .child(div().flex_1())
                .child(
                    ui::hoverable(
                        div()
                            .id("update-check")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(30.))
                            .rounded_full()
                            .cursor(gpui::CursorStyle::PointingHand)
                            .bg(ui::lerp(alpha(ui::ACCENT, 0.), alpha(ui::ACCENT, 0.12), t))
                            .child(ui::icon(if checking { Icon::Refresh } else { Icon::Download }, 16., ui::lerp(rgb(ui::TEXT2), rgb(ui::ACCENT), t.max(if checking { 1. } else { 0. }))))
                            .when(!checking, |d| d.on_click(cx.listener(|this, _, _, cx| this.update_click(cx)))),
                        "update-check".to_owned(),
                    ),
                ),
        );
        if checking {
            root = root.child(ui::caption("Si une version est trouvée, elle s’installe aussitôt."));
        }
        root
    }

    /// Espace d'appareil affiché : celui de la page, ou le dernier ouvert tant qu'on ne revient pas à l'accueil.
    fn current_space(&self) -> Option<Space> {
        self.page.space().or(if self.page == Page::Devices { None } else { self.last_space })
    }

    fn eyebrow(&self) -> String {
        let suffix = |id: &str| {
            let (text, _) = device_state(self.device(id));
            let text = text.split(" · ").next().unwrap_or(text);
            if self.device(id).is_some_and(|d| d.simulated) { format!("{text} · Simulation") } else { text.to_owned() }
        };
        match self.page.space() {
            None if self.page == Page::Profiles && self.last_space == Some(Space::GoXlr) => "GOXLR MINI · PROFILS DU MIXEUR".into(),
            Some(Space::GoXlr) => format!("GOXLR MINI · {}", suffix("goxlr-mini").to_uppercase()),
            Some(Space::Mouse) => format!("R5 ULTRA · {}", suffix("attack-shark-r5-ultra").to_uppercase()),
            None if matches!(self.page, Page::Devices | Page::Profiles) => "TON ESPACE PERSONNEL".into(),
            None if self.page == Page::WindowsAudio => "CE PC".into(),
            None => "RUSTUDIO".into(),
        }
    }

    /// État du micro pour le bouton de l'en-tête : (coupé, coupure partielle).
    fn mic_state(&self) -> Option<(bool, bool)> {
        let mic = self.snapshot.as_ref().filter(|s| s.connected)?.channels.iter().find(|c| c.id == "Mic")?;
        Some((mic.mute.is_some(), mic.mute.as_deref().is_some_and(|m| m != "all")))
    }

    fn header_actions(&self, cx: &mut Context<Self>) -> Div {
        let mut acts = div().relative().flex().items_center().gap(px(10.));
        match self.page {
            Page::Devices => {
                acts = acts.child(ui::pill("add-device", Some(Icon::Plus), "Ajouter un appareil", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.go_to(Page::Catalogue, cx))));
            }
            Page::Mixer => {
                acts = acts.child(ui::pill("open-profiles", Some(Icon::Bookmark), "Profils", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.go_to(Page::Profiles, cx))));
            }
            Page::Mouse => acts = acts.child(self.mouse_actions(cx)),
            _ => {}
        }
        if self.page.space() == Some(Space::GoXlr) {
            if let Some((muted, partial)) = self.mic_state() {
                let (text, glyph, tone) = if !muted {
                    ("Micro actif", Icon::Mic, Tone::Soft)
                } else if partial {
                    ("Micro coupé en partie", Icon::MicOff, Tone::Warn)
                } else {
                    ("Micro coupé", Icon::MicOff, Tone::Danger)
                };
                acts = acts.child(ui::pill("header-mic", Some(glyph), text, tone).h(px(40.)).px(px(18.)).on_click(cx.listener(move |this, _, _, cx| {
                    this.enqueue("mute", json!({"channel": "Mic", "muted": !muted, "target": "all"}), cx);
                })));
            }
        }
        acts = acts.child(
            ui::round("more", Icon::Dots, 40., alpha(ui::WHITE, 0.92), rgb(ui::TEXT))
                .shadow(ui::soft_shadow())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu_open = !this.menu_open;
                    cx.notify();
                })),
        );
        if self.menu_open {
            let item = |id: &'static str, glyph: Icon, text: &'static str, page: Page, cx: &mut Context<Self>| {
                ui::menu_item(id, Some(glyph), text, self.page == page).on_click(cx.listener(move |this, _, _, cx| {
                    this.menu_open = false;
                    ui::reset_hover();
                    this.go_to(page, cx);
                }))
            };
            acts = acts.child(deferred(
                div().absolute().top(px(48.)).right_0().w(px(240.)).child(
                    ui::menu_panel()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu_open = false;
                            ui::reset_hover();
                            cx.notify();
                        }))
                        .child(item("menu-audio", Icon::Volume, "Son Windows", Page::WindowsAudio, cx))
                        .child(item("menu-settings", Icon::Settings, "Réglages", Page::Settings, cx))
                        .child(item("menu-versions", Icon::History, "Versions", Page::Versions, cx))
                        .child(item("menu-help", Icon::Help, "Aide et retours", Page::Help, cx)),
                ),
            ));
        }
        acts
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let module = match self.page.space() {
            Some(Space::GoXlr) => self.module("goxlr-mini"),
            Some(Space::Mouse) => self.module("attack-shark-r5-ultra"),
            None => None,
        };
        let back = self.history.last().copied().map(|previous| {
            let t = ui::hover_t("back");
            ui::hoverable(
                div()
                    .id("back")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .mb(px(8.))
                    .pl(px(4.))
                    .pr(px(10.))
                    .h(px(26.))
                    .rounded_full()
                    .cursor(gpui::CursorStyle::PointingHand)
                    .bg(ui::lerp(alpha(ui::ACCENT, 0.), alpha(ui::ACCENT, 0.1), t))
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(ui::ACCENT))
                    .child(div().relative().left(px(-2. * t)).child(ui::icon(Icon::ChevronLeft, 16., rgb(ui::ACCENT))))
                    .child(previous.title())
                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                "back".to_owned(),
            )
        });
        div()
            .flex()
            .flex_col()
            .items_start()
            .when_some(back, |d, b| d.child(b))
            .child(self.header_main(module, cx))
    }

    fn header_main(&self, module: Option<&Descriptor>, cx: &mut Context<Self>) -> Div {
        div()
            .w_full()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(20.))
            .mb(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(ui::eyebrow(self.eyebrow()))
                    .child(div().mt(px(4.)).text_size(px(40.)).line_height(px(44.)).font_weight(FontWeight::BOLD).child(self.page.title()))
                    .child(div().mt(px(6.)).text_size(px(15.)).text_color(rgb(ui::TEXT2)).child(self.page.subtitle()))
                    .when_some(module, |d, m| d.child(credit(m).mt(px(8.)))),
            )
            .child(self.header_actions(cx).pb(px(4.)))
    }

    fn caption_buttons() -> Div {
        let button = |id: &'static str, glyph: Icon, area: WindowControlArea, close: bool| {
            let t = ui::hover_t(id);
            let hover = if close { rgb(0xe81123) } else { alpha(0x000000, 0.06) };
            let el = div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h(px(34.))
                .window_control_area(area)
                .occlude()
                .bg(ui::lerp(alpha(if close { 0xe81123 } else { 0x000000 }, 0.), hover, t))
                .child(gpui::svg().path(glyph.path()).size(px(if close { 15. } else { 13. })).text_color(ui::lerp(alpha(0x000000, 0.62), if close { rgb(ui::WHITE) } else { alpha(0x000000, 0.85) }, t)));
            ui::hoverable(el, id.to_owned())
        };
        div()
            .absolute()
            .top_0()
            .right_0()
            .flex()
            .child(button("win-min", Icon::Minus, WindowControlArea::Min, false))
            .child(button("win-max", Icon::Square, WindowControlArea::Max, false))
            .child(button("win-close", Icon::Close, WindowControlArea::Close, true))
    }

    /// Fenêtre complète : fond, barre latérale, en-tête et page.
    pub fn shell(&self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let page: AnyElement = match self.page {
            Page::Devices => self.devices_view(cx).into_any_element(),
            Page::Profiles => self.profiles_view(window, cx).into_any_element(),
            Page::Mixer => self.mixer_view(cx).into_any_element(),
            Page::Mouse => self.mouse_view(window, cx).into_any_element(),
            Page::WindowsAudio => self.win_audio(cx).into_any_element(),
            Page::Catalogue | Page::Versions | Page::Help | Page::Settings => self.library_view(self.page, cx).into_any_element(),
            Page::Go(section) => self.go_controls(section, cx).into_any_element(),
        };
        let mut body = div().flex().flex_col().gap(px(16.));
        body = body.child(page);
        // Changement de page : le contenu glisse et apparaît en douceur.
        let content = div()
            .relative()
            .child(self.header(cx))
            .child(body)
            .with_animation(
                ElementId::NamedInteger("page-in".into(), self.nav_count),
                gpui::Animation::new(std::time::Duration::from_millis(260)).with_easing(gpui::ease_out_quint()),
                |d, t| d.opacity(0.2 + 0.8 * t).top(px(14. * (1. - t))),
            );
        div()
            .id("rustudio")
            .relative()
            .size_full()
            .font_family(ui::FONT)
            .text_color(rgb(ui::TEXT))
            .text_size(px(14.))
            .bg(rgb(ui::BG))
            .child(ui::ambient())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            // zone de déplacement : elle s'arrête avant les trois boutons, sinon Windows y voit la barre de titre
                            .child(div().absolute().top_0().left_0().right(px(3. * 46.)).h(px(40.)).window_control_area(WindowControlArea::Drag))
                            .child(
                                div()
                                    .id(ElementId::NamedInteger("page".into(), self.nav_count))
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .pt(px(38.))
                                    .pb(px(28.))
                                    .pl(px(28.))
                                    .pr(px(36.))
                                    .child(content),
                            ),
                    ),
            )
            .child(Self::caption_buttons())
    }

    /// Liste déroulante : bouton, puis choix affichés au-dessus du reste de la page.
    pub(crate) fn dropdown(
        &self,
        key: &'static str,
        current: String,
        options: Vec<(String, bool)>,
        enabled: bool,
        pick: impl Fn(&mut Self, usize, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.select_open.as_ref().is_some_and(|k| k.as_ref() == key);
        let pick = std::rc::Rc::new(pick);
        let mut root = div().relative().child(ui::select_button(ElementId::Name(format!("select-{key}").into()), current, open, enabled).on_click(cx.listener(move |this, _, _, cx| {
            if enabled {
                this.select_open = if open { None } else { Some(key.into()) };
                cx.notify();
            }
        })));
        if open {
            let mut list = ui::menu_panel().id(ElementId::Name(format!("list-{key}").into())).max_h(px(260.)).overflow_y_scroll();
            for (i, (label, selected)) in options.into_iter().enumerate() {
                let pick = pick.clone();
                list = list.child(ui::menu_item((key, i), None, label, selected).on_click(cx.listener(move |this, _, _, cx| {
                    this.select_open = None;
                    ui::reset_hover();
                    pick(this, i, cx);
                    cx.notify();
                })));
            }
            root = root.child(deferred(
                div()
                    .absolute()
                    .top(px(42.))
                    .left_0()
                    .right_0()
                    .min_w(px(140.))
                    .child(list.on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.select_open = None;
                        cx.notify();
                    }))),
            ).with_priority(2));
        }
        root
    }

    fn refresh_profiles(&mut self) {
        self.profiles = self.store.list().0;
    }

    fn profile_result(&mut self, result: Result<(), String>, success: &'static str, cx: &mut Context<Self>) {
        self.profile_notice = Some(match result {
            Ok(()) => (true, success.into()),
            Err(err) => (false, err),
        });
        cx.notify();
    }

    fn save_current(&mut self, cx: &mut Context<Self>) {
        if !self.enabled() {
            return;
        }
        let client = self.client.clone();
        let name = if self.profile_name.trim().is_empty() {
            (1..=65)
                .map(|i| format!("Profil {i}"))
                .find(|candidate| !self.profiles.iter().any(|(_, p)| p.name.eq_ignore_ascii_case(candidate)))
                .unwrap_or_else(|| "Profil".into())
        } else {
            self.profile_name.clone()
        };
        let store = self.store.clone();
        let task = cx.background_executor().spawn(async move {
            let state = client.snapshot(None)?;
            if !state.connected || state.health != "ok" {
                return Err("Connecte la GoXLR Mini pour enregistrer son mixeur.".into());
            }
            store.save(&Profile::new(&name, state.settings())?)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.profile_name.clear();
                    this.refresh_profiles();
                }
                this.profile_result(result, "Profil enregistré.", cx);
            });
        })
        .detach();
    }

    fn pick_export(&mut self, profile: Profile, cx: &mut Context<Self>) {
        let initial = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .map(|p| p.join("Documents"))
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let picker = cx.prompt_for_new_path(&initial, Some("profil-rustudio.json"));
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(path))) => {
                    let path = if path.extension().is_none() { path.with_extension("json") } else { path };
                    cx.background_executor().spawn(async move { profile.export(&path) }).await
                }
                Ok(Ok(None)) => return,
                _ => Err("Impossible d’ouvrir le dossier d’export.".into()),
            };
            let _ = this.update(cx, |this, cx| this.profile_result(result, "Profil exporté en JSON.", cx));
        })
        .detach();
    }

    fn pick_import(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let store = self.store.clone();
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(paths))) if !paths.is_empty() => {
                    let path = paths[0].clone();
                    cx.background_executor().spawn(async move { store.save(&Profile::read(&path)?) }).await
                }
                Ok(Ok(_)) => return,
                _ => Err("Impossible d’ouvrir le sélecteur de profils.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.refresh_profiles();
                }
                this.profile_result(result, "Profil importé dans la liste.", cx);
            });
        })
        .detach();
    }

    fn confirm_profile(&mut self, cx: &mut Context<Self>) {
        match self.confirm.take() {
            Some(Confirm::Apply(id)) => {
                if self.profile_request.is_some() {
                    return;
                }
                if let Some((_, profile)) = self.profiles.iter().find(|(i, _)| *i == id) {
                    let payload = json!(profile.mix);
                    self.profile_request = self.enqueue("mixer-profile", payload, cx);
                    if self.profile_request.is_none() {
                        self.profile_result(Err("Le mixeur n’a pas accepté la commande. Vérifie sa connexion.".into()), "", cx);
                    }
                }
            }
            Some(Confirm::Delete(id)) => {
                let result = self.store.remove(id);
                if result.is_ok() {
                    self.refresh_profiles();
                }
                self.profile_result(result, "Profil supprimé.", cx);
            }
            None => {}
        }
        cx.notify();
    }

    fn profiles_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let enabled = self.enabled();
        let editing = self.profile_focus.is_focused(window);
        let input = ui::field("profile-name", &self.profile_focus, &self.profile_name, "Nom du profil (facultatif)", editing, true)
            .w(px(280.))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                match ui::edit_text(&mut this.profile_name, event, 40, |_| true, cx) {
                    ui::Edit::Submit => this.save_current(cx),
                    ui::Edit::Changed => {}
                    ui::Edit::Ignored => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let mut page = div().flex().flex_col().gap(px(16.)).child(
            ui::card()
                .child(ui::card_title(Some((Icon::Bookmark, ui::ACCENT)), "Enregistrer le mixeur", Some("Volumes, faders et coupures".into())))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(input)
                        .when(enabled, |el| el.child(ui::pill("profile-save", Some(Icon::Check), "Enregistrer", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.save_current(cx)))))
                        .child(ui::pill("profile-import", Some(Icon::Upload), "Importer un JSON", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.pick_import(cx)))),
                )
                .when(!enabled, |el| el.child(hint("Connecte la GoXLR Mini pour enregistrer ou appliquer un profil."))),
        );
        if self.profile_request.is_some() {
            page = page.child(hint("Application du profil…"));
        }
        if self.profiles.is_empty() {
            page = page.child(ui::card().items_center().py(px(36.)).child(ui::chip(Icon::Bookmark, ui::ACCENT, 48.)).child(ui::title("Aucun profil pour le moment", 17.)).child(hint("Règle ton mixeur, puis enregistre-le ici pour le retrouver en un clic.")));
        }
        let mut cards: Vec<AnyElement> = vec![];
        for (index, profile) in &self.profiles {
            let id = *index;
            let export = profile.clone();
            let confirming = self.confirm.is_some_and(|c| matches!(c, Confirm::Apply(i) | Confirm::Delete(i) if i == id));
            let card = ui::card()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(ui::chip(Icon::Sliders, ui::ACCENT, 34.))
                        .child(div().flex().flex_col().min_w(px(0.)).child(ui::title(profile.name.clone(), 16.)).child(ui::caption(format!("{} voies · 4 faders", profile.mix.volumes.len())))),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(enabled && self.profile_request.is_none(), |el| {
                            el.child(ui::pill(("apply", id), Some(Icon::Play), "Appliquer", Tone::Primary).on_click(cx.listener(move |this, _, _, cx| {
                                this.confirm = Some(Confirm::Apply(id));
                                cx.notify();
                            })))
                        })
                        .child(ui::pill(("export", id), Some(Icon::Download), "Exporter…", Tone::Plain).on_click(cx.listener(move |this, _, _, cx| this.pick_export(export.clone(), cx))))
                        .child(div().flex_1().flex().justify_end().child(ui::round(("delete", id), Icon::Trash, 34., alpha(0x767680, 0.12), rgb(ui::TEXT2)).on_click(cx.listener(move |this, _, _, cx| {
                            this.confirm = Some(Confirm::Delete(id));
                            cx.notify();
                        })))),
                )
                .when(confirming, |el| {
                    let apply = self.confirm == Some(Confirm::Apply(id));
                    el.child(hint(if apply {
                        "Appliquer ce profil modifie les volumes et les coupures du mixeur, y compris celle du micro."
                    } else {
                        "Supprimer ce profil de la liste ?"
                    }))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(ui::pill(("confirm", id), None, "Confirmer", if apply { Tone::Primary } else { Tone::Danger }).on_click(cx.listener(|this, _, _, cx| this.confirm_profile(cx))))
                            .child(ui::pill(("cancel", id), None, "Annuler", Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                                this.confirm = None;
                                cx.notify();
                            }))),
                    )
                });
            cards.push(card.into_any_element());
        }
        if !cards.is_empty() {
            page = page.child(ui::columns(cards));
        }
        page
    }

    fn device_card(&self, module: &Descriptor, cx: &mut Context<Self>) -> AnyElement {
        let device = self.device(&module.id);
        let (status, state) = device_state(device);
        let status = if device.is_some_and(|d| d.simulated) { format!("{status} · Appareil simulé") } else { status.to_owned() };
        let target = module_page(&module.id);
        let detail = device.and_then(|d| d.firmware.clone()).map(|fw| format!("Firmware {fw}"));
        let mut tags = div().flex().flex_wrap().gap(px(8.));
        for c in &module.capabilities {
            tags = tags.child(ui::tag(capability(c).to_owned()));
        }
        let card = ui::card()
            .flex_1()
            .min_w(px(0.))
            .p(px(24.))
            .gap(px(13.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(ui::chip(module_icon(&module.id), ui::ACCENT, 38.))
                    .child(div().flex().flex_col().child(ui::title(module.name.clone(), 17.)).child(hint(module.vendor.clone()))),
            )
            .child(credit(module))
            .child(ui::status(status, state))
            .child(hint(module.summary.clone()))
            .child(tags)
            .child(div().flex_1())
            .child(
                div()
                    .pt(px(6.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(ui::caption(detail.unwrap_or_default()))
                    .when_some(target, |d, page| {
                        d.child(
                            ui::pill(ElementId::Name(format!("open-{}", module.id).into()), None, "Ouvrir", Tone::Primary)
                                .h(px(38.))
                                .px(px(18.))
                                .child(ui::icon(Icon::ChevronRight, 15., rgb(ui::WHITE)))
                                .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx))),
                        )
                    })
                    .when(target.is_none(), |d| d.child(ui::caption("Réglages à venir"))),
            );
        match target {
            Some(page) => ui::lift(card, ElementId::Name(format!("card-{}", module.id).into()))
                .flex_1()
                .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx)))
                .into_any_element(),
            None => card.into_any_element(),
        }
    }

    fn devices_view(&self, cx: &mut Context<Self>) -> Div {
        let mut page = div().flex().flex_col().gap(px(18.));
        match &self.inventory {
            Some(inventory) if !inventory.installed.is_empty() => {
                for pair in inventory.installed.chunks(2) {
                    let mut row = div().flex().gap(px(18.));
                    for module in pair {
                        row = row.child(self.device_card(module, cx));
                    }
                    if pair.len() == 1 {
                        row = row.child(div().flex_1());
                    }
                    page = page.child(row);
                }
            }
            Some(_) => page = page.child(ui::card().child(hint("Aucun module disponible."))),
            None => {
                page = page.child(
                    ui::card()
                        .items_center()
                        .py(px(40.))
                        .child(ui::chip(Icon::Refresh, ui::ACCENT, 52.))
                        .child(ui::title("En attente du moteur", 20.))
                        .child(hint("La détection reprend toute seule dès que le moteur Rustudio répond.")),
                )
            }
        }
        page.child(
            ui::lift(ui::card()
                .flex_row()
                .items_center()
                .gap(px(16.)), "card-audio")
                .on_click(cx.listener(|this, _, _, cx| this.go_to(Page::WindowsAudio, cx)))
                .child(ui::chip(Icon::Volume, ui::ACCENT, 34.))
                .child(div().flex().flex_col().flex_1().child(ui::title("Son Windows", 15.)).child(hint("Micros, casques et sorties actives, avec ceux que Windows utilise par défaut.")))
                .child(
                    ui::pill("open-audio", None, "Afficher", Tone::Plain)
                        .child(ui::icon(Icon::ChevronRight, 15., rgb(ui::TEXT)))
                        .on_click(cx.listener(|this, _, _, cx| this.go_to(Page::WindowsAudio, cx))),
                ),
        )
    }
}

pub fn verify_profiles(client: &Client) -> Result<(), String> {
    let inventory = client.inventory()?;
    if !["goxlr-mini", "attack-shark-r5-ultra"]
        .iter()
        .all(|id| inventory.devices.iter().any(|d| d.id == *id && d.simulated && d.status == "ready"))
    {
        return Err("Les deux appareils de démonstration ne sont pas détectés.".into());
    }
    let start = client.snapshot(None)?;
    let folder = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = Store::new(folder.path().join("profils"));
    let profile = Profile::new("Démo Windows", start.settings())?;
    store.save(&profile)?;
    let output = folder.path().join(Path::new("partage.json"));
    let read_back = store.list().0.into_iter().next().ok_or("Profil enregistré introuvable.")?.1;
    read_back.export(&output)?;
    let imported = Profile::read(&output)?;
    if imported != profile {
        return Err("L’export modifie le profil.".into());
    }
    let channel = &start.channels[0];
    client.send(&Command {
        id: 7,
        boot: start.boot.clone(),
        action: "volume",
        params: json!({"channel":channel.id, "value":if channel.volume == 0 { 1 } else { 0 }}),
    })?;
    let changed = client.snapshot(Some(&start.cursor))?;
    if changed.settings() == start.settings() {
        return Err("Le volume de test n’a pas changé.".into());
    }
    client.send(&Command { id: 8, boot: start.boot.clone(), action: "mixer-profile", params: json!(imported.mix) })?;
    if client.snapshot(Some(&changed.cursor))?.settings() != start.settings() {
        return Err("Le profil importé n’a pas restauré le mixeur.".into());
    }
    println!("OK : inventaire de démonstration, enregistrement, export, import et restauration du mixeur.");
    Ok(())
}


