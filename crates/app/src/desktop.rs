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
    Window, WindowControlArea, canvas, deferred, div, prelude::*, px,
};
use serde_json::json;
use std::path::{Path, PathBuf};

pub(crate) use crate::ui::hint;

/// Hauteur de la barre d'état en bas de la fenêtre.
/// Secondes d'historique des mesures (panneau de performance).
pub(crate) const PERF_HISTORY: usize = 60;
const STATUS_BAR_H: f32 = 42.;
/// Bande du haut, réservée au déplacement de la fenêtre et à ses trois boutons.
const TITLE_BAR_H: f32 = 36.;
/// Barre latérale : largeur et écart avec les bords de la fenêtre.
const SIDEBAR_W: f32 = 240.;
const SIDEBAR_MARGIN: f32 = 16.;
const PAGE_LEFT: f32 = 28.;
const PAGE_RIGHT: f32 = 36.;
/// Largeur maximale du contenu : sur un grand écran, la page reste lisible au centre au lieu de s'étirer.
const PAGE_MAX: f32 = 1240.;

pub(crate) fn page_width(window: &Window) -> f32 {
    (f32::from(window.viewport_size().width) - SIDEBAR_W - SIDEBAR_MARGIN - PAGE_LEFT - PAGE_RIGHT).min(PAGE_MAX)
}
/// Listes déroulantes : hauteur bornée, écart avec le bouton et marge gardée avec les bords de la page.
const MENU_MAX_H: f32 = 260.;
const MENU_MIN_H: f32 = 110.;
const BUTTON_GAP: f32 = 4.;
const MENU_MARGIN: f32 = 8.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Devices,
    Mouse,
    MouseSensitivity,
    MouseSensor,
    MousePower,
    Keyboard,
    KeyboardSettings,
    KeyboardPaint,
    KeyboardShortcuts,
    KeyboardInfo,
    KeyboardMacros,
    Webcam,
    WebcamImage,
    WebcamOptics,
    WebcamInfo,
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
    Keyboard,
    Webcam,
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
            "mouse-sensitivity" => Page::MouseSensitivity,
            "mouse-sensor" => Page::MouseSensor,
            "mouse-power" => Page::MousePower,
            "keyboard" => Page::Keyboard,
            "keyboard-settings" => Page::KeyboardSettings,
            "keyboard-paint" => Page::KeyboardPaint,
            "keyboard-shortcuts" => Page::KeyboardShortcuts,
            "keyboard-info" => Page::KeyboardInfo,
            "keyboard-macros" => Page::KeyboardMacros,
            "webcam" => Page::Webcam,
            "webcam-image" => Page::WebcamImage,
            "webcam-optics" => Page::WebcamOptics,
            "webcam-info" => Page::WebcamInfo,
            "mic" => Page::Go(Section::Microphone),
            "headphones" => Page::Go(Section::Headphones),
            "routing" => Page::Go(Section::Routing),
            "lighting" => Page::Go(Section::Light),
            "scenes" => Page::Go(Section::Scenes),
            "diagnostic" => Page::Go(Section::Diagnostic),
            _ => return None,
        })
    }
    /// Nom de la page pour `--page`, l'inverse de `from_name`.
    pub fn name(self) -> Option<&'static str> {
        const NAMES: [&str; 28] = [
            "devices", "profiles", "modules", "versions", "help", "settings", "audio", "mixer", "mouse",
            "mouse-sensitivity", "mouse-sensor", "mouse-power", "keyboard", "keyboard-settings", "keyboard-paint",
            "keyboard-shortcuts", "keyboard-info", "keyboard-macros", "webcam", "webcam-image", "webcam-optics",
            "webcam-info", "mic", "headphones", "routing", "lighting", "scenes", "diagnostic",
        ];
        NAMES.into_iter().find(|n| Self::from_name(n) == Some(self))
    }
    pub(crate) fn space(self) -> Option<Space> {
        match self {
            Page::Mixer | Page::Go(_) => Some(Space::GoXlr),
            Page::Mouse | Page::MouseSensitivity | Page::MouseSensor | Page::MousePower => Some(Space::Mouse),
            Page::Keyboard | Page::KeyboardSettings | Page::KeyboardPaint | Page::KeyboardShortcuts | Page::KeyboardInfo | Page::KeyboardMacros => Some(Space::Keyboard),
            Page::Webcam | Page::WebcamImage | Page::WebcamOptics | Page::WebcamInfo => Some(Space::Webcam),
            _ => None,
        }
    }
    pub(crate) fn is_mouse(self) -> bool { self.space() == Some(Space::Mouse) }
    pub(crate) fn is_keyboard(self) -> bool { self.space() == Some(Space::Keyboard) }
    pub(crate) fn is_webcam(self) -> bool { self.space() == Some(Space::Webcam) }
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
            Page::MouseSensitivity => "Sensibilité",
            Page::MouseSensor => "Capteur",
            Page::MousePower => "Batterie et veille",
            Page::Keyboard => "Mon clavier",
            Page::KeyboardSettings => "Réglages de l’effet",
            Page::KeyboardPaint => "Couleur par touche",
            Page::KeyboardShortcuts => "Raccourcis et mode",
            Page::KeyboardInfo => "Ton MK770",
            Page::KeyboardMacros => "Macros",
            Page::Webcam => "Ma webcam",
            Page::WebcamImage => "Image et couleur",
            Page::WebcamOptics => "Optique et cadrage",
            Page::WebcamInfo => "Ta C920",
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
            Page::WindowsAudio => "Tes sources audio en direct, expliquées simplement.",
            Page::Mixer => "Tes quatre faders et toutes les voies, en direct.",
            Page::Mouse => "Sensibilité, fréquence, batterie et capteur.",
            Page::MouseSensitivity => "Paliers DPI, axes X et Y et sensibilité active.",
            Page::MouseSensor => "Fréquence, hauteur de décrochage et suivi des mouvements.",
            Page::MousePower => "Niveau de batterie, charge et délai de mise en veille.",
            Page::Keyboard => "Effets lumineux, luminosité et couleur de chaque touche.",
            Page::KeyboardSettings => "Luminosité, vitesse, sens et couleur de l’effet affiché.",
            Page::KeyboardPaint => "Choisis ton pinceau et les couleurs de chaque touche.",
            Page::KeyboardShortcuts => "Raccourcis Fn et aide pour le mode Windows ou macOS.",
            Page::KeyboardInfo => "Connexion, firmware et caractéristiques du clavier.",
            Page::KeyboardMacros => "Expérimental · Crée tes séquences de touches et enregistre-les dans ton clavier.",
            Page::Webcam => "Préréglages, maintien anti-réinitialisation et réglages complets de ta C920.",
            Page::WebcamImage => "Luminosité, contraste, saturation, netteté, balance des blancs et 50 / 60 Hz.",
            Page::WebcamOptics => "Exposition manuelle, compensation basse lumière, autofocus, zoom et contrôles XU.",
            Page::WebcamInfo => "Révision USB, formats vidéo, unités XU et réinitialisation d’usine.",
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
        Space::Mouse => &[
            (Page::Mouse, Icon::Grid, "Vue d’ensemble"),
            (Page::MouseSensitivity, Icon::Mouse, "Sensibilité"),
            (Page::MouseSensor, Icon::Gauge, "Capteur"),
            (Page::MousePower, Icon::Battery, "Batterie et veille"),
        ],
        Space::Keyboard => &[
            (Page::Keyboard, Icon::Sparkles, "Effets lumineux"),
            (Page::KeyboardSettings, Icon::Sliders, "Réglages de l’effet"),
            (Page::KeyboardPaint, Icon::Palette, "Couleur par touche"),
            (Page::KeyboardMacros, Icon::Keyboard, "Macros"),
            (Page::KeyboardShortcuts, Icon::Help, "Raccourcis et mode"),
            (Page::KeyboardInfo, Icon::Keyboard, "Informations"),
        ],
        Space::Webcam => &[
            (Page::Webcam, Icon::Camera, "Préréglages et vue"),
            (Page::WebcamImage, Icon::Sun, "Image et couleur"),
            (Page::WebcamOptics, Icon::Sliders, "Optique et cadrage"),
            (Page::WebcamInfo, Icon::Settings, "Fiche C920"),
        ],
    }
}

fn module_icon(id: &str) -> Icon {
    match id {
        "goxlr-mini" => Icon::Sliders,
        "attack-shark-r5-ultra" => Icon::Mouse,
        "cooler-master-mk770" => Icon::Keyboard,
        "logitech-c920" => Icon::Camera,
        _ => Icon::Grid,
    }
}
fn module_page(id: &str) -> Option<Page> {
    match id {
        "goxlr-mini" => Some(Page::Mixer),
        "attack-shark-r5-ultra" => Some(Page::Mouse),
        "cooler-master-mk770" => Some(Page::Keyboard),
        "logitech-c920" => Some(Page::Webcam),
        _ => None,
    }
}
/// Espace de la barre latérale propre à un module.
fn module_space(id: &str) -> Option<Space> {
    module_page(id).and_then(Page::space)
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
        "lighting-effects" => "Effets lumineux",
        "per-key" => "Couleur par touche",
        "image" => "Image",
        "optics" => "Optique",
        "presets" => "Préréglages",
        "lock" => "Maintien",
        "led" => "LED / XU",
        other => other,
    }
}
pub(crate) fn device_state(device: Option<&Device>) -> (&'static str, State) {
    match device.map(|d| (d.status.as_str(), d.firmware_kind.as_deref())) {
        // le pilote GoXLR ne voit pas le firmware de laboratoire : sans lui, la Mini paraîtrait débranchée
        Some((_, Some("lab"))) => ("Firmware Rustudio en essai", State::Info),
        Some(("audio", _)) => ("Connectée · Firmware Rustudio", State::Info),
        Some(("ready", Some("audio"))) => ("Connectée · Firmware Rustudio", State::Ok),
        Some(("ready", Some("golden"))) => ("Firmware de secours (Golden)", State::Warn),
        Some(("ready", _)) => ("Connecté", State::Ok),
        Some(("asleep", _)) => ("En veille · bouge-la pour la réveiller", State::Info),
        Some(("starting", _)) => ("Détection en cours", State::Warn),
        Some(("disconnected", _)) => ("Débranché", State::Idle),
        Some(("error" | "blocked", _)) => ("Connexion à vérifier", State::Error),
        _ => ("Indisponible", State::Idle),
    }
}

/// Batterie compacte pour la barre latérale, remplie selon le niveau réel.
fn nav_battery(level: u8, charging: bool) -> Div {
    let level = level.min(100);
    // contour neutre, remplissage coloré selon la charge : vert, orange sous 40 %, rouge sous 15 % (vert en charge)
    let colour = match level {
        _ if charging => ui::GREEN,
        0..=15 => ui::RED,
        16..=40 => ui::ORANGE,
        _ => ui::GREEN,
    };
    let frame = alpha(ui::INK, 0.32);
    let body = div()
        .flex()
        .flex_none()
        .w(px(23.))
        .h(px(12.))
        .p(px(2.))
        .rounded(px(3.5))
        .border_1()
        .border_color(frame)
        .child(div().h_full().w(gpui::relative(f32::from(level.max(8)) / 100.)).rounded(px(1.5)).bg(rgb(colour)));
    let nub = div().flex_none().w(px(2.)).h(px(5.)).rounded_r(px(1.)).bg(frame);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .whitespace_nowrap()
        .child(div().flex().items_center().child(body).child(nub))
        .when(charging, |d| d.child(ui::icon(Icon::Bolt, 11., rgb(ui::GREEN_TEXT))))
        .child(div().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(ui::TEXT2)).child(format!("{level} %")))
}

/// État en direct d'un appareil sur sa carte : micro et scène de la GoXLR, batterie de la souris.
fn glance_row(module: &str, glance: &crate::backend::Glance) -> Option<Div> {
    let item = |glyph: Icon, colour: u32, text: String| {
        // icône et texte colorés, sans pastille autour
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(ui::icon(glyph, 14., rgb(colour)))
            .child(div().text_size(px(12.5)).font_weight(gpui::FontWeight::MEDIUM).text_color(rgb(colour)).child(text))
    };
    let mut row = div().flex().flex_wrap().gap(px(16.));
    match module {
        "goxlr-mini" => {
            let muted = glance.mic_muted?;
            row = row.child(if muted { item(Icon::MicOff, ui::RED_TEXT, "Micro coupé".into()) } else { item(Icon::Mic, ui::GREEN_TEXT, "Micro actif".into()) });
            if let Some(scene) = glance.scene.as_deref() {
                let label = rustudio_core::scenes::scene(scene).map(|s| s.label).unwrap_or(scene);
                row = row.child(item(Icon::Layers, ui::ACCENT, format!("Scène {label}")));
            }
        }
        "attack-shark-r5-ultra" => {
            let (level, charging) = glance.battery?;
            let colour = if level <= 20 && !charging { ui::RED_TEXT } else { ui::GREEN_TEXT };
            row = row.child(item(Icon::Battery, colour, if charging { format!("{level} % · en charge") } else { format!("{level} %") }));
        }
        "cooler-master-mk770" => {
            row = row.child(item(Icon::Sparkles, ui::ACCENT, glance.keyboard_effect.clone()?));
        }
        "logitech-c920" => {
            row = row.child(item(Icon::Camera, ui::ACCENT, glance.webcam_summary.clone()?));
        }
        _ => return None,
    }
    Some(row)
}

/// Ligne « Officiel Rustudio · Créé par … » d'un module.
pub(crate) fn credit(module: &Descriptor) -> Div {
    let official = module.origin != "community";
    div()
        .flex()
        .flex_wrap()
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
                d.bg(gpui::linear_gradient(180., gpui::linear_color_stop(rgb(ui::PAPER), 0.), gpui::linear_color_stop(rgb(0xf3f8ff), 1.)))
                    .border_1()
                    .border_color(alpha(ui::ACCENT, 0.14))
                    .shadow(ui::soft_shadow())
            })
            .when(!on, |d| d.bg(ui::lerp(alpha(ui::PAPER, 0.), alpha(ui::PAPER, 0.75), t)).border_1().border_color(ui::lerp(alpha(ui::PAPER, 0.), alpha(ui::INK, 0.04), t)))
            .child(ui::icon(glyph, 19., if on { rgb(ui::ACCENT) } else { ui::lerp(rgb(ui::TEXT2), rgb(ui::TEXT), t) }))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx)));
        ui::hoverable(el, key)
    }

    /// Un appareil dans la barre latérale : icône, nom, pastille d'état et flèche qui replie ses pages.
    /// Un clic sur le nom ouvre l'appareil ; sur l'appareil déjà ouvert, il replie ou déplie ses pages.
    fn device_nav(&self, module: &Descriptor, open: bool, collapsed: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let target = module_page(&module.id);
        let foldable = module_space(&module.id).is_some_and(|s| space_pages(s).len() > 1);
        let on = open && !foldable;
        let key = format!("devnav-{}", module.id);
        let t = if on { 0. } else { ui::hover_t(&key) };
        let (_, state) = device_state(self.device(&module.id));
        let battery = self.inventory.as_ref().filter(|_| module.id == "attack-shark-r5-ultra").and_then(|i| i.glance.battery);
        let dot = match state {
            State::Ok => ui::GREEN,
            State::Info => ui::ACCENT,
            State::Warn => ui::ORANGE,
            State::Error => ui::RED,
            State::Idle => 0xb0b0b6,
        };
        let id = module.id.clone();
        let title = div()
            .id(ElementId::Name(format!("devnav-open-{}", module.id).into()))
            .flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .items_center()
            .gap(px(11.))
            .when(target.is_some() || foldable, |d| d.cursor(gpui::CursorStyle::PointingHand))
            .child(ui::icon(module_icon(&module.id), 18., if on || open { rgb(ui::ACCENT) } else { ui::lerp(rgb(ui::TEXT2), rgb(ui::TEXT), t) }))
            .child(div().flex_1().min_w(px(0.)).truncate().child(module.name.clone()))
            // la batterie prend la place de la pastille : sa couleur dit déjà que la souris répond
            .map(|d| match battery {
                Some((level, charging)) => d.child(nav_battery(level, charging)),
                None => d.child(div().flex_none().size(px(7.)).rounded_full().bg(rgb(dot))),
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if open && foldable {
                    this.toggle_device(&id, cx);
                } else if let Some(page) = target {
                    this.collapsed.remove(&id);
                    this.go_to(page, cx);
                }
            }));
        let fold_key = format!("devfold-{}", module.id);
        let fold_t = ui::hover_t(&fold_key);
        let fold_id = module.id.clone();
        let chevron = foldable.then(|| {
            ui::hoverable(
                div()
                    .id(ElementId::Name(fold_key.clone().into()))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(24.))
                    .rounded(px(8.))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .bg(ui::lerp(alpha(ui::INK, 0.), alpha(ui::INK, 0.06), fold_t))
                    .child(ui::icon(if collapsed { Icon::ChevronRight } else { Icon::ChevronDown }, 14., ui::lerp(rgb(ui::TEXT3), rgb(ui::TEXT), fold_t)))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_device(&fold_id, cx))),
                fold_key,
            )
        });
        let el = div()
            .id(ElementId::Name(key.clone().into()))
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(38.))
            .pl(px(12.))
            .pr(px(if foldable { 7. } else { 12. }))
            .rounded(px(12.))
            .text_size(px(14.))
            .font_weight(if open { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
            .text_color(rgb(if on { ui::ACCENT } else { ui::TEXT }))
            .when(on, |d| d.bg(rgb(ui::PAPER)).border_1().border_color(alpha(ui::ACCENT, 0.14)).shadow(ui::soft_shadow()))
            .when(!on, |d| d.bg(ui::lerp(alpha(ui::PAPER, 0.), alpha(ui::PAPER, 0.75), t)).border_1().border_color(ui::lerp(alpha(ui::PAPER, 0.), alpha(ui::INK, 0.04), t)))
            .child(title)
            .children(chevron);
        ui::hoverable(el, key)
    }

    /// Page d'un appareil ouvert, en retrait sous son nom.
    fn sub_nav(&self, page: Page, glyph: Icon, label: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let on = self.page == page;
        let key = format!("subnav-{label}");
        let t = if on { 0. } else { ui::hover_t(&key) };
        let el = div()
            .id(ElementId::Name(key.clone().into()))
            .flex()
            .items_center()
            .gap(px(10.))
            .flex_none()
            .h(px(32.))
            .pl(px(12.))
            .pr(px(10.))
            .rounded(px(10.))
            .text_size(px(13.5))
            .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
            .text_color(rgb(if on { ui::TEXT } else { ui::TEXT2 }))
            .cursor(gpui::CursorStyle::PointingHand)
            .bg(if on { alpha(ui::ACCENT, 0.16) } else { ui::lerp(alpha(ui::PAPER, 0.), alpha(ui::PAPER, 0.7), t) })
            .child(ui::icon(glyph, 16., if on { rgb(ui::ACCENT) } else { ui::lerp(rgb(ui::TEXT3), rgb(ui::TEXT), t) }))
            .child(div().flex_1().min_w(px(0.)).truncate().child(label))
            .when(on, |d| d.child(ui::icon(Icon::Check, 14., rgb(ui::ACCENT))))
            .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx)));
        ui::hoverable(el, key)
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let mut side = div()
            .flex()
            .flex_col()
            // position absolue, comme la page : la fenêtre se met en page sans mesurer le contenu de la barre latérale
            .absolute()
            .top(px(SIDEBAR_MARGIN))
            .left(px(SIDEBAR_MARGIN))
            .bottom(px(SIDEBAR_MARGIN))
            .w(px(SIDEBAR_W))
            .p(px(12.))
            .pt(px(18.))
            // sans boîte : un simple filet sépare la navigation de la page
            .border_r_1()
            .border_color(alpha(ui::INK, 0.07))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .px(px(8.))
                    .child(ui::sidebar_logo(38.))
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
        // tous les appareils, toujours : on passe de l'un à l'autre sans revenir à l'accueil
        if let Some(inventory) = self.inventory.as_ref().filter(|i| !i.installed.is_empty()) {
            let current = self.page.space();
            let mut list = div().flex().flex_col().gap(px(2.)).mt(px(6.));
            for module in &inventory.installed {
                let space = module_space(&module.id);
                let open = space.is_some() && space == current;
                // Seules les pages de l'appareil ouvert sont dépliées : la barre reste courte et on voit où on est.
                // La flèche inverse ce choix (replier l'appareil ouvert, déplier un autre pour y aller vite).
                let shown = open != self.collapsed.contains(&module.id);
                list = list.child(self.device_nav(module, open, !shown, cx));
                if let Some(space) = space.filter(|_| shown) {
                    let pages = space_pages(space);
                    if pages.len() > 1 {
                        let mut subpages = div().flex().flex_col().gap(px(2.)).ml(px(28.));
                        for (page, glyph, label) in pages {
                            subpages = subpages.child(self.sub_nav(*page, *glyph, label, cx));
                        }
                        list = list.child(subpages);
                    }
                }
            }
            side = side.child(
                div()
                    .id("side-devices")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .mt(px(14.))
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(alpha(ui::INK, 0.07))
                    .child(div().px(px(12.)).child(ui::caption("MES APPAREILS")))
                    .child(list),
            );
        }
        side.child(
            div()
                .flex_none()
                .mt_auto()
                .pt(px(12.))
                .px(px(6.))
                .border_t_1()
                .border_color(alpha(ui::INK, 0.07))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(ui::menu_item("sidebar-help", Some(Icon::Help), "Aide et retours", self.page == Page::Help)
                    .on_click(cx.listener(|this, _, _, cx| this.go_to(Page::Help, cx))))
                .child(ui::menu_item("sidebar-github-star", Some(Icon::Star), "Étoile sur GitHub", false)
                    .child(div().flex_1())
                    .child(ui::icon(Icon::External, 13., rgb(ui::TEXT3)))
                    .on_click(cx.listener(|_, _, _, cx| cx.open_url("https://github.com/samirmedani/rustudio")))),
        )
    }

    /// Barre d'état en bas de la fenêtre : appareils, mesures et version.
    fn status_bar(&self, cx: &mut Context<Self>) -> Div {
        let detected = self
            .inventory
            .as_ref()
            .map(|i| i.devices.iter().filter(|d| matches!(d.status.as_str(), "ready" | "asleep" | "audio")).count())
            .unwrap_or(0);
        let sub = if !self.client.is_live() {
            "Démonstration".to_owned()
        } else if detected == 0 {
            "Aucun appareil détecté".to_owned()
        } else {
            format!("{detected} appareil{s} détecté{s}", s = if detected > 1 { "s" } else { "" })
        };
        let divider = || div().flex_none().w(px(1.)).h(px(16.)).bg(alpha(ui::INK, 0.08));
        div()
            .flex_none()
            .h(px(STATUS_BAR_H))
            .px(px(22.))
            .flex()
            .items_center()
            .gap(px(16.))
            .border_t_1()
            .border_color(alpha(ui::INK, 0.06))
            .bg(alpha(ui::PAPER, 0.55))
            .text_size(px(12.))
            .when(crate::live::enabled(), |d| d.child(ui::badge("DEV · en direct", ui::ORANGE).flex_none()))
            .child(ui::caption(sub))
            .when(self.monitor_on(), |d| {
                d.child(divider()).child(
                    ui::pill("status-monitor-stop", Some(ui::Icon::VolumeOff), "Tu t’entends dans le casque · couper", ui::Tone::Danger)
                        .on_click(cx.listener(|this, _, _, cx| this.cut_monitor(cx))),
                )
            })
            .child(divider())
            .child(self.performance(cx))
            .child(div().flex_1())
            .child(self.update_widget(cx))
    }

    /// Mesures et détails du moteur, regroupés dans le panneau de performance.
    fn performance(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        // moyennes des 5 dernières secondes : des chiffres qui se lisent au lieu de sauter chaque seconde
        let recent = |i: usize, n: usize| -> Option<f64> {
            let v: Vec<f64> = self.perf.iter().rev().take(n).map(|s| s[i]).filter(|x| x.is_finite()).collect();
            (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
        };
        let (cpu, ram, ms) = (recent(0, 5), recent(1, 5), recent(2, 5));
        let total_gb = self.usage.total_bytes.map(|b| b as f64 / 1e9);
        let grades = [
            cpu.map(|v| Grade::of(v, [1., 5., 15.])),
            ram.map(|v| Grade::of(v, [150., 300., 600.])),
            ms.map(|v| Grade::of(v, [2., 10., 50.])),
        ];
        let overall = grades.iter().flatten().max().copied();
        let (summary, colour) = match overall {
            None => ("Mesure en cours", ui::TEXT3),
            Some(Grade::Excellent) => ("Ultra léger", ui::GREEN),
            Some(Grade::Good) => ("Léger", ui::ACCENT),
            Some(Grade::Fair) => ("Un peu chargé", ui::ORANGE),
            Some(Grade::High) => ("Consommation élevée", ui::RED),
        };
        let fr = |v: f64, decimals: usize| format!("{v:.decimals$}").replace('.', ",");
        let cpu_text = cpu.map(|v| format!("{} %", fr(v, 1)));
        let ram_text = ram.map(|v| format!("{} Mo", fr(v, 0)));
        let ms_text = ms.map(|v| if v < 0.1 { "< 0,1 ms".into() } else { format!("{} ms", fr(v, 1)) });
        let key = "perf-summary";
        let t = ui::hover_t(key);
        let open = self.performance_open;
        let mut strip = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .h(px(28.))
            .pl(px(10.))
            .pr(px(12.))
            .rounded_full()
            .bg(ui::lerp(alpha(ui::INK, 0.), alpha(ui::INK, 0.05), if open { 1. } else { t }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_none().size(px(7.)).rounded_full().bg(rgb(colour)))
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(rgb(grade_text(overall))).child(summary)),
            );
        for (short, value, grade) in [("CPU", &cpu_text, grades[0]), ("RAM", &ram_text, grades[1]), ("Réponse", &ms_text, grades[2])] {
            strip = strip.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(ui::caption(short))
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(rgb(grade_text(grade))).child(value.clone().unwrap_or_else(|| "—".into()))),
            );
        }
        let panel = open.then(|| {
            // une carte par mesure : le chiffre, une jauge, une phrase, et les 60 dernières secondes
            let card = |title: &'static str, value: Option<String>, grade: Option<Grade>, fill: Option<f64>, line: Option<String>, series: usize, floor: f64| {
                let tone = grade.map(Grade::colour).unwrap_or(ui::TEXT3);
                let history: Vec<f64> = self.perf.iter().map(|s| s[series]).collect();
                let top = history.iter().copied().filter(|x| x.is_finite()).fold(floor, f64::max);
                let mut spark = div().flex().items_end().gap(px(1.)).h(px(26.));
                for i in 0..PERF_HISTORY {
                    let v = history.len().checked_sub(PERF_HISTORY - i).and_then(|j| history.get(j)).copied().filter(|x| x.is_finite());
                    let h = v.map(|v| (v / top).clamp(0., 1.) * 24. + 2.).unwrap_or(0.);
                    spark = spark.child(div().flex_1().h(px(h as f32)).rounded_t(px(1.)).bg(alpha(tone, if i + 1 == PERF_HISTORY { 0.9 } else { 0.35 })));
                }
                div()
                    .p(px(12.))
                    .rounded(px(12.))
                    .bg(alpha(ui::INK, 0.035))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .justify_between()
                            .child(ui::label(title))
                            .child(div().text_size(px(22.)).line_height(px(24.)).font_weight(FontWeight::BOLD).text_color(rgb(grade_text(grade))).child(value.unwrap_or_else(|| "—".into()))),
                    )
                    .child(
                        div().h(px(6.)).rounded_full().bg(alpha(ui::INK, 0.08)).child(
                            div().h_full().rounded_full().bg(rgb(tone)).min_w(px(4.)).w(gpui::relative(fill.unwrap_or(0.).clamp(0., 1.) as f32)),
                        ),
                    )
                    .children(line.map(|l| div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(l)))
                    .child(spark)
            };
            let cpu_card = card(
                "Processeur",
                cpu_text.clone(),
                grades[0],
                cpu.map(|v| v / 100.),
                cpu.map(|v| format!("{} % de ton processeur reste libre", fr(100. - v, 1))),
                0,
                2.,
            );
            let ram_card = card(
                "Mémoire",
                ram_text.clone(),
                grades[1],
                ram.zip(total_gb).map(|(v, t)| v / 1000. / t),
                ram.zip(total_gb).map(|(v, t)| format!("{} % de tes {} Go", fr(v / 10. / t, 1), fr(t, 0))),
                1,
                50.,
            );
            let ms_card = card(
                "Réponse du moteur",
                ms_text.clone(),
                grades[2],
                ms.map(|v| v / 16.7),
                ms.map(|v| format!("{}× plus rapide qu’une image à 60 Hz", fr((16.7 / v.max(0.05)).min(999.), 0))),
                2,
                2.,
            );
            let (engine, state, detail) = match &self.inventory {
                Some(data) => {
                    let mode = match (self.client.is_live(), data.engine_supervised) {
                        (false, _) => "Démonstration",
                        (true, Some(true)) => "Moteur supervisé",
                        (true, Some(false)) => "Moteur autonome",
                        (true, None) => "Moteur local",
                    };
                    let detail = data.engine_version.as_ref().map_or_else(|| mode.to_owned(), |v| format!("{mode} · {v}"));
                    ("Moteur connecté", State::Ok, detail)
                }
                None => ("Moteur indisponible", State::Error, String::new()),
            };
            let firmware = match self.device("goxlr-mini").and_then(|d| d.firmware_kind.as_deref()) {
                Some("official") => Some("firmware officiel"),
                Some("golden") => Some("firmware de secours"),
                Some("lab") | Some("audio") => Some("firmware Rustudio"),
                _ => None,
            };
            let chip = |text: &'static str| div().px(px(8.)).py(px(3.)).rounded_full().bg(alpha(ui::GREEN, 0.12)).text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(ui::GREEN_TEXT)).child(text);
            deferred(
                div()
                    .id("performance-panel")
                    .absolute()
                    .bottom(px(38.))
                    .left_0()
                    .w(px(420.))
                    .p(px(16.))
                    .rounded(px(16.))
                    .bg(rgb(ui::PAPER))
                    .border_1()
                    .border_color(alpha(ui::INK, 0.08))
                    .shadow(ui::soft_shadow())
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                        // Le bouton dans la barre du bas ferme lui-même le panneau au clic.
                        if event.position.y < window.viewport_size().height - px(STATUS_BAR_H) {
                            this.performance_open = false;
                            cx.notify();
                        }
                    }))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).child("Rustudio sur ton PC"))
                            .child(ui::badge(summary, colour)),
                    )
                    .child(cpu_card)
                    .child(ram_card)
                    .child(ms_card)
                    .child(div().flex().flex_wrap().gap(px(6.)).child(chip("100 % Rust natif")).child(chip("0 navigateur embarqué")).child(chip("Mesuré par Windows")))
                    .child(
                        div()
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(alpha(ui::INK, 0.07))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(ui::status(engine, state))
                            .child(ui::caption(match firmware {
                                Some(f) => format!("{detail} · {f}"),
                                None => detail,
                            })),
                    ),
            )
        });
        let button = ui::hoverable(
            strip.id(key).cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.performance_open = !open;
                    cx.notify();
                }))
                .child(ui::icon(Icon::ChevronDown, 12., rgb(ui::TEXT3))),
            key.to_owned(),
        );
        div().id("performance-popover").relative().flex_none().child(button).children(panel)
    }

    /// Droite de la barre d'état : version, état et bouton de mise à jour (cherche, puis installe d'un clic).
    fn update_widget(&self, cx: &mut Context<Self>) -> Div {
        // RUSTUDIO_PREVIEW_UPDATE (développement) : force un état pour vérifier le bloc à l'écran
        let preview = std::env::var("RUSTUDIO_PREVIEW_UPDATE").ok();
        let u = match preview.as_deref() {
            Some("ready") => json!({"status":"available","installable":true,"latest":{"version":"6.2.0"},"release_build":true}),
            Some("downloading") => json!({"phase":"downloading","progress":0.42,"latest":{"version":"6.2.0"},"release_build":true}),
            Some("checking") => json!({"status":"checking","release_build":true}),
            Some("failed") => json!({"phase":"failed","release_build":true}),
            Some(_) => json!({"status":"up-to-date","release_build":true}),
            None => self.update.clone().unwrap_or(serde_json::Value::Null),
        };
        let phase = u["phase"].as_str().unwrap_or("");
        let working = matches!(phase, "downloading" | "verifying" | "extracting" | "applying");
        let checking = u["status"] == "checking" || (self.update_then_open && !working);
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
        let root = div().flex().flex_none().items_center().gap(px(8.));
        if preview.is_none() && (!self.client.is_live() || u["release_build"] == false) {
            return root.child(version).child(ui::caption("Version de développement"));
        }
        let fraction = if phase == "downloading" { u["progress"].as_f64().unwrap_or(0.) as f32 } else { 1. };
        let (state, colour): (String, u32) = if working {
            match phase {
                "downloading" => (format!("{latest} · {} %", (fraction * 100.).round()), ui::ACCENT),
                "verifying" => ("Vérification…".into(), ui::ACCENT),
                "extracting" => ("Préparation…".into(), ui::ACCENT),
                _ => ("Installation…".into(), ui::ACCENT),
            }
        } else if ready {
            (format!("{latest} disponible"), ui::ACCENT)
        } else if checking {
            ("Recherche…".into(), ui::TEXT3)
        } else if phase == "failed" {
            ("Mise à jour interrompue".into(), ui::ORANGE)
        } else if u["status"] == "up-to-date" {
            ("À jour".into(), ui::GREEN)
        } else {
            ("Rechercher".into(), ui::TEXT3)
        };
        let t = ui::hover_t("update-check");
        let (bg, fg) = if ready {
            (ui::lerp(rgb(ui::ACCENT), rgb(0x2b8af0), t), rgb(ui::WHITE))
        } else {
            (ui::lerp(alpha(ui::ACCENT, 0.), alpha(ui::ACCENT, 0.12), t), ui::lerp(rgb(ui::TEXT2), rgb(ui::ACCENT), t.max(if checking { 1. } else { 0. })))
        };
        let button = ui::hoverable(
            div()
                .id("update-check")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded_full()
                .bg(bg)
                .when(!checking, |d| d.cursor(gpui::CursorStyle::PointingHand))
                .child(ui::icon(if checking || working { Icon::Refresh } else { Icon::Download }, 14., fg))
                .when(!checking, |d| d.on_click(cx.listener(|this, _, _, cx| this.update_click(cx)))),
            "update-check".to_owned(),
        );
        root.child(version)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(div().flex_none().size(px(6.)).rounded_full().bg(rgb(colour)))
                    .child(ui::caption(state).whitespace_nowrap().text_color(rgb(if colour == ui::GREEN { ui::GREEN_TEXT } else if colour == ui::ORANGE { ui::ORANGE_TEXT } else if colour == ui::ACCENT { ui::ACCENT } else { ui::TEXT3 }))),
            )
            .when(working, |d| d.child(div().w(px(72.)).child(ui::progress(fraction, ui::ACCENT))))
            .child(button)
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
            Some(Space::Keyboard) => format!("MK770 · {}", suffix("cooler-master-mk770").to_uppercase()),
            Some(Space::Webcam) => format!("LOGITECH C920 · {}", suffix("logitech-c920").to_uppercase()),
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
            Page::Mouse | Page::MouseSensitivity | Page::MouseSensor | Page::MousePower => acts = acts.child(self.mouse_actions(cx)),
            Page::Keyboard | Page::KeyboardSettings | Page::KeyboardPaint | Page::KeyboardShortcuts | Page::KeyboardInfo => acts = acts.child(self.keyboard_actions(cx)),
            Page::Webcam | Page::WebcamImage | Page::WebcamOptics | Page::WebcamInfo => acts = acts.child(self.webcam_actions(cx)),
            _ => {}
        }
        if self.page.space() == Some(Space::GoXlr)
            && let Some((muted, partial)) = self.mic_state() {
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
        acts = acts.child(
            ui::round("more", Icon::Dots, 40., alpha(ui::PAPER, 0.92), rgb(ui::TEXT))
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
            Some(Space::Keyboard) => self.module("cooler-master-mk770"),
            Some(Space::Webcam) => self.module("logitech-c920"),
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
            .when_some(self.firmware_notice(), |d, notice| d.child(notice.w_full().mb(px(18.))))
    }

    /// Mini pilotée sous firmware Rustudio : une seule sortie Windows, ni traitements ni effets.
    pub(crate) fn rustudio_firmware(&self) -> bool {
        self.device("goxlr-mini").is_some_and(|d| d.status == "ready" && d.firmware_kind.as_deref() == Some("audio"))
    }

    /// Explique les limitations des firmwares alternatifs sur les pages GoXLR.
    fn firmware_notice(&self) -> Option<Div> {
        if self.page.space() != Some(Space::GoXlr) {
            return None;
        }
        let device = self.device("goxlr-mini")?;
        let message = match (device.status.as_str(), device.firmware_kind.as_deref()) {
            (_, Some("golden")) => "La GoXLR Mini a démarré sur son firmware de secours (Golden) : un retour arrière a eu lieu et son firmware officiel ne s’est pas lancé. Réinstalle le firmware officiel.",
            ("audio", _) => "Le firmware Rustudio fournit le micro et le casque dans Son Windows. Mets à jour Rustudio pour piloter le mixeur et les commandes avec ce firmware.",
            // firmware Rustudio piloté : chaque réglage sans effet porte sa propre étiquette, pas de bandeau
            _ => return None,
        };
        Some(ui::notice(false, message))
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
                    .flex_1()
                    .min_w(px(0.))
                    .child(ui::eyebrow(self.eyebrow()))
                    .child(div().mt(px(4.)).text_size(px(40.)).line_height(px(44.)).font_weight(FontWeight::BOLD).child(self.page.title()))
                    .child(div().mt(px(6.)).text_size(px(15.)).text_color(rgb(ui::TEXT2)).child(self.page.subtitle()))
                    // l'origine n'est rappelée que pour un module communautaire : sur les modules officiels, c'est du bruit
                    .when_some(module.filter(|m| m.origin == "community"), |d, m| d.child(credit(m).mt(px(8.)))),
            )
            .child(self.header_actions(cx).flex_none().pb(px(4.)))
    }

    fn caption_buttons() -> Div {
        let button = |id: &'static str, glyph: Icon, area: Option<WindowControlArea>, close: bool| {
            let t = ui::hover_t(id);
            let hover = if close { rgb(0xe81123) } else { alpha(ui::INK, 0.06) };
            let el = div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h(px(TITLE_BAR_H - 1.))
                .when_some(area, |d, a| d.window_control_area(a))
                .occlude()
                .bg(ui::lerp(alpha(if close { 0xe81123 } else { ui::INK }, 0.), hover, t))
                .child(gpui::svg().path(glyph.path()).size(px(if close { 15. } else { 13. })).text_color(ui::lerp(alpha(ui::INK, 0.62), if close { rgb(ui::WHITE) } else { alpha(ui::INK, 0.85) }, t)));
            ui::hoverable(el, id.to_owned())
        };
        div()
            .absolute()
            .top_0()
            .right_0()
            .flex()
            .child(button("win-min", Icon::Minus, Some(WindowControlArea::Min), false))
            .child(button("win-max", Icon::Square, Some(WindowControlArea::Max), false))
            // la croix range Rustudio dans la barre des tâches au lieu de le fermer : il continue de piloter tes
            // appareils (pour quitter vraiment : l'icône près de l'horloge)
            .child(button("win-close", Icon::Close, None, true).cursor(gpui::CursorStyle::Arrow).on_click(|_, window, _| window.minimize_window()))
    }

    /// Fenêtre complète : fond, barre latérale, en-tête et page.
    pub fn shell(&self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let page: AnyElement = match self.page {
            Page::Devices => self.devices_view(window, cx).into_any_element(),
            Page::Profiles => self.profiles_view(window, cx).into_any_element(),
            Page::Mixer => self.mixer_view(cx).into_any_element(),
            Page::Mouse | Page::MouseSensitivity | Page::MouseSensor | Page::MousePower => self.mouse_view(window, cx).into_any_element(),
            Page::Keyboard | Page::KeyboardSettings | Page::KeyboardPaint | Page::KeyboardShortcuts | Page::KeyboardInfo => self.keyboard_view(page_width(window), cx).into_any_element(),
            Page::KeyboardMacros => self.macros_view(window, cx).into_any_element(),
            Page::Webcam | Page::WebcamImage | Page::WebcamOptics | Page::WebcamInfo => self.webcam_view(window, cx).into_any_element(),
            Page::WindowsAudio => self.win_audio(window,cx).into_any_element(),
            Page::Catalogue | Page::Versions | Page::Help | Page::Settings => self.library_view(self.page, window, cx).into_any_element(),
            Page::Go(section) => self.go_controls(section, page_width(window), cx).into_any_element(),
        };
        // Changement de page : le contenu glisse et apparaît en douceur.
        let content = div()
            .relative()
            .child(self.header(cx))
            .child(page)
            .with_animation(
                ElementId::NamedInteger("page-in".into(), self.nav_count),
                gpui::Animation::new(std::time::Duration::from_millis(260)).with_easing(gpui::ease_out_quint()),
                |d, t| d.opacity(0.2 + 0.8 * t).top(px(14. * (1. - t))),
            );
        div()
            .id("rustudio")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.performance_open && event.keystroke.key == "escape" {
                    this.performance_open = false;
                    cx.notify();
                }
            }))
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
                    .right_0()
                    .bottom(px(STATUS_BAR_H))
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(SIDEBAR_MARGIN + SIDEBAR_W))
                            .right_0()
                            // zone de déplacement : elle s'arrête avant les trois boutons, sinon Windows y voit la barre de titre
                            .child(div().absolute().top_0().left_0().right(px(3. * 46.)).h(px(TITLE_BAR_H)).window_control_area(WindowControlArea::Drag))
                            // barre de titre à part : les boutons de fenêtre ne se mêlent plus au contenu qui défile
                            .child(div().absolute().top(px(TITLE_BAR_H - 1.)).left_0().right_0().h(px(1.)).bg(alpha(ui::INK, 0.07)))
                            .child(
                                // en position absolue : la page qui défile se met en page une seule fois, à la taille de la zone,
                                // au lieu d'être mesurée par chaque niveau de la fenêtre avant d'être placée
                                div()
                                    .id(ElementId::NamedInteger("page".into(), self.nav_count))
                                    .absolute()
                                    .top(px(TITLE_BAR_H))
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.page_scroll)
                                    .child(
                                        // la molette passe ici avant le défilement de la page : elle glisse au lieu de sauter
                                        div()
                                            .pt(px(14.))
                                            .pb(px(28.))
                                            .pl(px(PAGE_LEFT))
                                            .pr(px(PAGE_RIGHT))
                                            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| this.wheel(event, cx)))
                                            .child(div().mt(px(-preview_scroll())).w_full().max_w(px(PAGE_MAX)).mx_auto().child(content)),
                                    ),
                            ),
                    ),
            )
            .child(
                div().absolute().left_0().right_0().bottom_0().child(self.status_bar(cx)),
            )
            .child(Self::caption_buttons())
    }

    /// Liste déroulante : bouton, puis choix affichés au-dessus du reste de la page.
    /// Elle s'ouvre vers le haut quand la place manque en bas, et défile si elle reste trop longue.
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
        // relève la place libre autour du bouton (sans la barre d'état) pour placer la liste à la prochaine ouverture
        let room = self.select_room.clone();
        let measure = canvas(
            move |bounds, window, _| {
                let above = f32::from(bounds.top()) - MENU_MARGIN;
                let below = f32::from(window.viewport_size().height) - STATUS_BAR_H - f32::from(bounds.bottom()) - MENU_MARGIN;
                room.borrow_mut().insert(key, (above, below));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let mut root = div().relative().child(measure).child(ui::select_button(ElementId::Name(format!("select-{key}").into()), current, open, enabled).on_click(cx.listener(move |this, _, _, cx| {
            if enabled {
                this.select_open = if open { None } else { Some(key.into()) };
                cx.notify();
            }
        })));
        if open {
            // 34 px par choix, plus les marges de la liste
            let wanted = (options.len() as f32 * 34. + 14.).min(MENU_MAX_H);
            let (above, below) = self.select_room.borrow().get(key).copied().unwrap_or((0., f32::MAX));
            let upward = below < wanted && above > below;
            let height = (if upward { above } else { below } - BUTTON_GAP).clamp(MENU_MIN_H, MENU_MAX_H);
            // occlude : la molette fait défiler la liste, pas la page derrière
            let mut list = ui::menu_panel().id(ElementId::Name(format!("list-{key}").into())).max_h(px(height)).overflow_y_scroll().occlude();
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
                    .map(|d| if upward { d.bottom(px(38. + BUTTON_GAP)) } else { d.top(px(38. + BUTTON_GAP)) })
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

    /// Confirmation d'un profil (appliquer ou supprimer), en fenêtre par-dessus la page.
    pub(crate) fn profile_confirm_box(&self, cx: &mut Context<Self>) -> Option<Div> {
        let confirm = self.confirm?;
        let (index, apply) = match confirm {
            Confirm::Apply(i) => (i, true),
            Confirm::Delete(i) => (i, false),
        };
        let name = self.profiles.iter().find(|(i, _)| *i == index).map(|(_, p)| p.name.clone())?;
        let (title, text) = if apply {
            (format!("Appliquer « {name} » ?"), vec!["Les volumes et les coupures du mixeur changent, y compris celle du micro.".into()])
        } else {
            (format!("Supprimer le profil « {name} » ?"), vec![])
        };
        let cancel = ui::pill("profile-cancel", None, "Annuler", Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
            this.confirm = None;
            cx.notify();
        }));
        let confirm = ui::pill("profile-confirm", None, if apply { "Appliquer" } else { "Supprimer" }, if apply { Tone::Primary } else { Tone::Danger })
            .on_click(cx.listener(|this, _, _, cx| this.confirm_profile(cx)));
        let glyph = if apply { (Icon::Play, ui::ACCENT) } else { (Icon::Trash, ui::RED) };
        Some(ui::dialog(glyph.0, glyph.1, title, text, vec![cancel.into_any_element(), confirm.into_any_element()]))
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
            .flex_auto()
            .min_w(px(200.))
            .max_w(px(280.))
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
                        .flex_wrap()
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
                        .flex_wrap()
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
                );
            cards.push(card.into_any_element());
        }
        if !cards.is_empty() {
            page = page.child(ui::grid(cards, ui::columns_that_fit(page_width(window), 340., 10., 2)));
        }
        page
    }

    fn device_card(&self, module: &Descriptor, cx: &mut Context<Self>) -> AnyElement {
        let device = self.device(&module.id);
        let (status, state) = device_state(device);
        let status = if device.is_some_and(|d| d.simulated) { format!("{status} · Appareil simulé") } else { status.to_owned() };
        let audio_only = device.is_some_and(|d| d.status == "audio");
        let target = if audio_only { Some(Page::WindowsAudio) } else { module_page(&module.id) };
        let summary = if audio_only {
            "Micro et casque via USB Audio. Les commandes du mixeur sont indisponibles.".to_owned()
        } else {
            module.summary.clone()
        };
        let detail = device.and_then(|d| d.firmware.clone()).map(|fw| format!("Firmware {fw}"));
        // ce que le module règle, en une ligne de texte (pas d'étiquettes)
        let caps: Vec<String> = if audio_only {
            vec!["Micro".into(), "Casque".into()]
        } else {
            module.capabilities.iter().map(|c| capability(c).to_owned()).collect()
        };
        let glance = self.inventory.as_ref().and_then(|i| glance_row(&module.id, &i.glance));
        let card = ui::card()
            .relative()
            .min_w(px(0.))
            .p(px(20.))
            .pb(px(78.))
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(ui::chip(module_icon(&module.id), ui::ACCENT, 40.))
                    .child(div().flex().flex_col().min_w(px(0.)).child(ui::title(module.name.clone(), 17.)).child(hint(module.vendor.clone()))),
            )
            .when(module.origin == "community", |d| d.child(credit(module)))
            // état, puis ce qui se passe en direct, sur une même ligne qui garde sa place : les cartes restent alignées
            .child(div().min_h(px(22.)).flex().flex_wrap().items_center().gap(px(16.)).child(ui::status(status, state)).children(glance))
            .child(hint(summary).text_size(px(12.5)).line_height(px(19.)))
            .child(ui::caption(caps.join(" · ")).line_height(px(17.)))
            .child(
                div()
                    .absolute()
                    .bottom(px(20.))
                    .left(px(20.))
                    .right(px(20.))
                    .flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .items_center()
                    .justify_between()
                    .child(ui::caption(detail.unwrap_or_default()))
                    .when_some(target, |d, page| {
                        d.child(
                            ui::pill(ElementId::Name(format!("open-{}", module.id).into()), None, if audio_only { "Son Windows" } else { "Ouvrir" }, Tone::Primary)
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
                .on_click(cx.listener(move |this, _, _, cx| this.go_to(page, cx)))
                .into_any_element(),
            None => card.into_any_element(),
        }
    }

    fn devices_view(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let mut page = div().flex().flex_col().gap(px(18.));
        match &self.inventory {
            Some(inventory) if !inventory.installed.is_empty() => {
                let cards = inventory.installed.iter().map(|module| self.device_card(module, cx)).collect();
                page = page.child(ui::device_grid(cards, window));
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
                .child(div().flex().flex_col().flex_1().min_w(px(0.)).child(ui::title("Son Windows", 15.)).child(hint("Micros, casques et sorties actives, avec ceux que Windows utilise par défaut.")))
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



/// Aperçu sur Mac (développement, RUSTUDIO_PREVIEW_SCROLL) : décale la page pour capturer le bas des écrans.
/// Toujours 0 sous Windows.
fn preview_scroll() -> f32 {
    if !cfg!(target_os = "macos") {
        return 0.;
    }
    static OFFSET: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| std::env::var("RUSTUDIO_PREVIEW_SCROLL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.))
}

/// Note d'une mesure, de la meilleure à la moins bonne.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Grade {
    Excellent,
    Good,
    Fair,
    High,
}

impl Grade {
    /// Note d'une valeur d'après trois seuils croissants.
    fn of(value: f64, [excellent, good, fair]: [f64; 3]) -> Self {
        if value < excellent {
            Grade::Excellent
        } else if value < good {
            Grade::Good
        } else if value < fair {
            Grade::Fair
        } else {
            Grade::High
        }
    }

    fn colour(self) -> u32 {
        match self {
            Grade::Excellent => ui::GREEN,
            Grade::Good => ui::ACCENT,
            Grade::Fair => ui::ORANGE,
            Grade::High => ui::RED,
        }
    }
}

/// Couleur de texte lisible pour une note (gris tant que la mesure manque).
fn grade_text(grade: Option<Grade>) -> u32 {
    match grade {
        None => ui::TEXT2,
        Some(Grade::Excellent) => ui::GREEN_TEXT,
        Some(Grade::Good) => ui::ACCENT,
        Some(Grade::Fair) => ui::ORANGE_TEXT,
        Some(Grade::High) => ui::RED_TEXT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goxlr(device: serde_json::Value) -> Device {
        serde_json::from_value(device).unwrap()
    }

    #[test]
    fn firmware_officiel_golden_ou_labo_affiche() {
        let official = goxlr(json!({"id": "goxlr-mini", "status": "ready", "simulated": false, "firmware": "1.2.1.49", "firmware_kind": "official"}));
        assert!(matches!(device_state(Some(&official)), ("Connecté", State::Ok)));
        let golden = goxlr(json!({"id": "goxlr-mini", "status": "ready", "simulated": false, "firmware": "1.0.0.21", "firmware_kind": "golden"}));
        assert!(matches!(device_state(Some(&golden)), ("Firmware de secours (Golden)", State::Warn)));
        let lab = goxlr(json!({"id": "goxlr-mini", "status": "disconnected", "simulated": false, "firmware": null, "firmware_kind": "lab"}));
        assert!(matches!(device_state(Some(&lab)), ("Firmware Rustudio en essai", State::Info)));
        let audio = goxlr(json!({"id": "goxlr-mini", "status": "audio", "simulated": false, "firmware": null, "firmware_kind": "audio"}));
        assert!(matches!(device_state(Some(&audio)), ("Connectée · Firmware Rustudio", State::Info)));
        // moteur d'une version précédente : pas de sorte de firmware, état habituel
        let older = goxlr(json!({"id": "goxlr-mini", "status": "ready", "simulated": false, "firmware": "1.2.1.49"}));
        assert!(matches!(device_state(Some(&older)), ("Connecté", State::Ok)));
    }
}
