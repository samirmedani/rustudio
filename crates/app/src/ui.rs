//! Kit visuel de Rustudio : clair, verre dépoli, grands arrondis (direction validée sur les maquettes).
//! Toutes les vues passent par ces briques ; aucune couleur ni taille n'est répétée ailleurs.
use gpui::{
    AnyElement, App, AssetSource, BoxShadow, CursorStyle, Div, ElementId, FocusHandle, FontWeight, Hsla, KeyDownEvent,
    MouseButton, ObjectFit, SharedString, Stateful, Svg, div, img, linear_color_stop, linear_gradient, point, prelude::*,
    px, svg,
};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;
use std::sync::{Arc, LazyLock};

static LOGO: LazyLock<Arc<gpui::Image>> = LazyLock::new(|| png(include_bytes!("../assets/logo.png")));
static AMBIENT: LazyLock<Arc<gpui::Image>> = LazyLock::new(|| png(include_bytes!("../assets/ambient.png")));
fn png(bytes: &[u8]) -> Arc<gpui::Image> {
    Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes.to_vec()))
}

pub const FONT: &str = "Inter";
pub const BG: u32 = 0xf5f5f7;
pub const TEXT: u32 = 0x1d1d1f;
pub const TEXT2: u32 = 0x6e6e73;
pub const TEXT3: u32 = 0x8e8e93;
pub const ACCENT: u32 = 0x0071e3;
pub const GREEN: u32 = 0x34c759;
pub const GREEN_TEXT: u32 = 0x24813a;
pub const RED: u32 = 0xff3b30;
pub const RED_TEXT: u32 = 0xd70015;
pub const ORANGE: u32 = 0xff9500;
pub const ORANGE_TEXT: u32 = 0xc66a00;
pub const WHITE: u32 = 0xffffff;

/// Surface claire (cartes, champs, menus) : blanche en clair, gris anthracite en sombre.
/// WHITE reste blanc dans les deux thèmes : texte sur une couleur vive, boutons de curseur.
pub const PAPER: u32 = 0xfffffe;
/// Encre des filets et des survols : noire en clair, blanche en sombre.
/// 0x000000 reste noir dans les deux thèmes : ombres et voile des fenêtres de dialogue.
pub const INK: u32 = 0x000001;

/// Apparence choisie dans Réglages.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Appearance {
    /// Suit le mode clair ou sombre de Windows.
    #[default]
    Auto,
    Light,
    Dark,
}
impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::Auto, Appearance::Light, Appearance::Dark];
    pub fn label(self) -> &'static str {
        match self {
            Appearance::Auto => "Automatique",
            Appearance::Light => "Clair",
            Appearance::Dark => "Sombre",
        }
    }
    /// Nom enregistré dans le fichier de préférence et accepté par « --theme ».
    pub fn name(self) -> &'static str {
        match self {
            Appearance::Auto => "auto",
            Appearance::Light => "clair",
            Appearance::Dark => "sombre",
        }
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name.trim())
    }
    /// Préférence lue sur le disque (absente ou illisible : automatique).
    pub fn load(file: &std::path::Path) -> Self {
        std::fs::read_to_string(file).ok().and_then(|text| Self::from_name(&text)).unwrap_or_default()
    }
    pub fn save(self, file: &std::path::Path) -> Result<(), String> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Dossier de préférences inaccessible : {e}"))?;
        }
        std::fs::write(file, self.name()).map_err(|e| format!("Préférence non enregistrée : {e}"))
    }
}

static APPEARANCE_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
static APPEARANCE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn appearance() -> Appearance {
    Appearance::ALL[APPEARANCE.load(std::sync::atomic::Ordering::Relaxed) as usize % 3]
}
pub fn set_appearance(choice: Appearance) {
    let index = Appearance::ALL.iter().position(|a| *a == choice).unwrap_or(0);
    APPEARANCE.store(index as u8, std::sync::atomic::Ordering::Relaxed);
}
/// Au démarrage : lit la préférence dans ce fichier, où les prochains choix seront enregistrés.
pub fn load_appearance(file: std::path::PathBuf) {
    set_appearance(Appearance::load(&file));
    let _ = APPEARANCE_FILE.set(file);
}
/// Choix fait dans Réglages : appliqué tout de suite, puis enregistré.
pub fn choose_appearance(choice: Appearance) -> Result<(), String> {
    set_appearance(choice);
    match APPEARANCE_FILE.get() {
        Some(file) => choice.save(file),
        None => Ok(()),
    }
}
/// À appeler au début de chaque rendu de fenêtre : applique le choix, ou suit Windows en automatique.
pub fn sync_theme(window: &gpui::Window) {
    let dark = match appearance() {
        Appearance::Auto => matches!(window.appearance(), gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark),
        Appearance::Light => false,
        Appearance::Dark => true,
    };
    DARK.store(dark, std::sync::atomic::Ordering::Relaxed);
}
pub fn is_dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Surface des cartes en sombre (PAPER translucide) ; un élément en relief, rgb(PAPER), est un peu plus clair.
const DARK_SURFACE: u32 = 0x3a3a3c;

/// Couleur réellement dessinée pour une couleur de la palette claire.
fn themed(c: u32, dark: bool) -> u32 {
    if !dark {
        return match c {
            PAPER => WHITE,
            INK => 0x000000,
            other => other,
        };
    }
    match c {
        PAPER => 0x48484a,
        INK => 0xffffff,
        BG => 0x161618,
        TEXT => 0xf5f5f7,
        0x3a3a3c => 0xd1d1d6,
        TEXT2 => 0xaeaeb2,
        TEXT3 => 0x98989d,
        ACCENT => 0x0a84ff,
        0x2b8af0 => 0x409cff,
        GREEN_TEXT => 0x30d158,
        RED_TEXT => 0xff6961,
        ORANGE_TEXT => 0xffb340,
        // rail des faders, panneau près de l'horloge, barres du graphique de voix, onglet actif de la barre latérale
        0xe3e3e8 => 0x2a2a2c,
        0xeeeef2 => 0x333336,
        0xf7f7f9 => 0x1e1e20,
        0xc7c7cc => 0x636366,
        0xf3f8ff => 0x3d4656,
        other => other,
    }
}
fn solid(c: u32) -> Hsla {
    gpui::rgb(c).into()
}
/// Couleur dessinée telle quelle dans les deux thèmes : celles de l'appareil (éclairage, pastilles de choix).
pub fn raw(c: u32) -> Hsla {
    solid(c)
}

/// Couleur avec transparence (0 = invisible, 1 = opaque).
pub fn alpha(rgb: u32, a: f32) -> Hsla {
    let dark = is_dark();
    let mut c = match (rgb, dark) {
        // une surface translucide reste plus sombre qu'un élément en relief (rgb(PAPER))
        (PAPER, true) => solid(DARK_SURFACE),
        _ => solid(themed(rgb, dark)),
    };
    c.a = match (rgb, dark) {
        // filets et remplissages gris : un peu plus marqués sur fond sombre pour rester visibles
        (INK, true) => (a * 1.5).min(1.),
        (0x767680 | 0x787880, true) => (a * 1.7).min(1.),
        _ => a,
    };
    c
}
pub fn rgb(c: u32) -> Hsla {
    solid(themed(c, is_dark()))
}
/// Fond pastel d'une couleur : mélange vers la surface (t = 0 : couleur pure, t = 1 : surface).
pub fn tint(c: u32, t: f32) -> Hsla {
    if is_dark() { blend(c, 0x2c2c2e, t) } else { blend(c, WHITE, t) }
}
/// Couleur lisible sur son propre fond pastel : plus sombre en clair, plus claire en sombre.
pub fn shade(c: u32, t: f32) -> Hsla {
    if is_dark() { blend(c, WHITE, t * 2.) } else { blend(c, 0x000000, t) }
}
/// Mélange vers le blanc ou le noir, identique dans les deux thèmes (reliefs d'un élément coloré).
pub fn to_white(c: u32, t: f32) -> Hsla {
    blend(c, WHITE, t)
}
pub fn to_black(c: u32, t: f32) -> Hsla {
    blend(c, 0x000000, t)
}
fn blend(a: u32, b: u32, t: f32) -> Hsla {
    let t = t.clamp(0., 1.);
    let ch = |c: u32, s: u32| ((c >> s) & 255) as f32;
    let m = |s| (ch(a, s) + (ch(b, s) - ch(a, s)) * t).round() as u32;
    solid((m(16) << 16) | (m(8) << 8) | m(0))
}

fn shadow(y: f32, blur: f32, a: f32) -> BoxShadow {
    BoxShadow { color: alpha(0x111827, a), offset: point(px(0.), px(y)), blur_radius: px(blur), spread_radius: px(0.) }
}
pub fn soft_shadow() -> Vec<BoxShadow> {
    vec![shadow(1., 2., 0.04), shadow(10., 30., 0.06)]
}

macro_rules! assets {
    ($($name:ident => $file:literal),* $(,)?) => {
        /// Icônes Lucide (licence ISC), embarquées dans l'exécutable.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[allow(dead_code)]
        pub enum Icon { $($name),* }
        impl Icon {
            pub fn path(self) -> &'static str {
                match self { $(Icon::$name => concat!("icons/", $file, ".svg")),* }
            }
        }
        fn embedded(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $file, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $file, ".svg"))),)*
                _ => None,
            }
        }
    };
}
assets!(
    Plug => "plug", Grid => "grid", Bookmark => "bookmark", Settings => "settings", Sliders => "sliders",
    Mic => "mic", MicOff => "micoff", Music => "music", Chat => "chat", Monitor => "monitor", Volume => "vol",
    VolumeOff => "volx", Headphones => "headphones", Route => "route", Sun => "sun", Layers => "layers",
    Mouse => "mouse", Refresh => "refresh", Dots => "dots", Shield => "shield", ChevronDown => "chev",
    ChevronRight => "chevr", ChevronLeft => "chevl", Battery => "battery", Activity => "activity", Radio => "radio",
    Game => "game", Lines => "lines", Ear => "ear", Wave => "wave", Download => "down", Upload => "up",
    Check => "check", Plus => "plus", Power => "power", Close => "x", Leaf => "leaf", Sparkles => "sparkles",
    Tower => "radioTower", Keyboard => "keyboard", Podcast => "pod", Gauge => "gauge", Help => "help",
    History => "history", Trash => "trash", Alert => "alert", Play => "play", Stop => "stop", Palette => "palette",
    External => "link", Minus => "minus", Square => "square",
);

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(embedded(path).map(Cow::Borrowed))
    }
    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}


// ── Survol animé ──────────────────────────────────────────────────────────────
// Chaque élément interactif garde son état de survol et l'instant du changement : couleurs, ombres et élévation
// glissent vers leur nouvel état en 160 ms (courbe douce), dans les deux sens, même si on change d'avis en route.
const HOVER_MS: f32 = 160.;

struct Fade {
    on: bool,
    from: f32,
    since: Instant,
}

thread_local! {
    static HOVERS: RefCell<HashMap<String, Fade>> = RefCell::new(HashMap::new());
    static MOVING: Cell<bool> = const { Cell::new(false) };
}

fn fade_value(f: &Fade) -> (f32, bool) {
    let p = (f.since.elapsed().as_secs_f32() * 1000. / HOVER_MS).min(1.);
    let eased = 1. - (1. - p).powi(3);
    let target = if f.on { 1. } else { 0. };
    (f.from + (target - f.from) * eased, p < 1.)
}

/// Avancement du survol : 0 au repos, 1 survolé.
pub fn hover_t(key: &str) -> f32 {
    HOVERS.with_borrow_mut(|map| {
        let Some(f) = map.get(key) else { return 0. };
        let (t, moving) = fade_value(f);
        if moving {
            MOVING.set(true);
        } else if !f.on {
            map.remove(key);
        }
        t
    })
}

/// Branche le suivi du survol sur un élément ; la vue redessine tant qu'une transition est en cours.
pub fn hoverable(el: Stateful<Div>, key: String) -> Stateful<Div> {
    el.on_hover(move |hovered, window, _| {
        HOVERS.with_borrow_mut(|map| {
            let from = map.get(&key).map(|f| fade_value(f).0).unwrap_or(0.);
            map.insert(key.clone(), Fade { on: *hovered, from, since: Instant::now() });
        });
        window.refresh();
    })
}

/// À appeler à la fin du rendu d'une fenêtre : demande l'image suivante si une transition n'est pas finie.
pub fn keep_animating(window: &gpui::Window) {
    if MOVING.replace(false) {
        window.request_animation_frame();
    }
}

/// Oublie les survols (changement de page ou menu refermé : l'élément survolé a disparu).
pub fn reset_hover() {
    HOVERS.with_borrow_mut(|map| map.clear());
}

/// Mélange de deux couleurs (t = 0 : a, t = 1 : b).
pub fn lerp(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b): (gpui::Rgba, gpui::Rgba) = (a.into(), b.into());
    let m = |x: f32, y: f32| x + (y - x) * t;
    gpui::Rgba { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b), a: m(a.a, b.a) }.into()
}

fn key_of(id: &ElementId) -> String {
    id.to_string()
}

/// Ombre qui grandit au survol (élévation des cartes et des boutons).
fn lift_shadow(t: f32, tint: u32, strength: f32) -> Vec<BoxShadow> {
    vec![
        shadow(1. + t, 2. + 2. * t, 0.04 + 0.02 * t),
        BoxShadow { color: alpha(tint, (0.06 + 0.08 * t) * strength), offset: point(px(0.), px(10. + 8. * t)), blur_radius: px(30. + 14. * t), spread_radius: px(0.) },
    ]
}

/// Carte cliquable : elle se soulève, son ombre s'étire et son contour prend la couleur d'accent.
pub fn lift(card: Div, id: impl Into<ElementId>) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = hover_t(&key);
    hoverable(
        card.id(id)
            .relative()
            .top(px(-3. * t))
            .cursor(CursorStyle::PointingHand)
            .border_color(lerp(alpha(INK, 0.05), alpha(ACCENT, 0.32), t))
            .shadow(lift_shadow(t, if t > 0. { 0x0b4fae } else { 0x111827 }, 1.)),
        key,
    )
}

/// Fond qui apparaît au survol (lignes de menu, en-têtes cliquables…).
pub fn hover_fill(el: Stateful<Div>, key: impl Into<String>, rest: Hsla, hover: Hsla) -> Stateful<Div> {
    let key = key.into();
    let t = hover_t(&key);
    hoverable(el.bg(lerp(rest, hover, t)), key)
}

/// Inter (SIL OFL 1.1), sous-ensemble latin : cinq graisses, 240 Ko au total.
pub fn load_fonts(cx: &App) {
    let _ = cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Light.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Bold.ttf").as_slice()),
    ]);
}

pub fn icon(name: Icon, size: f32, colour: impl Into<Hsla>) -> Svg {
    svg().path(name.path()).flex_none().size(px(size)).text_color(colour.into())
}

/// Fond de l'appli : lumière douce et halos pastel, sous toutes les cartes.
pub fn ambient() -> impl IntoElement {
    // en sombre, l'image claire ne laisse qu'un halo coloré discret sur le fond
    img(AMBIENT.clone()).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Cover).opacity(if is_dark() { 0.1 } else { 1. })
}
pub fn logo(size: f32) -> impl IntoElement {
    img(LOGO.clone()).flex_none().size(px(size))
}

/// Carte en verre : fond blanc translucide, filet, ombre douce.
pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(20.))
        .rounded(px(22.))
        .bg(alpha(PAPER, 0.74))
        .border_1()
        .border_color(alpha(INK, 0.05))
        .shadow(soft_shadow())
}
/// Ancien nom, gardé pour les vues qui empilent des réglages.
pub fn panel() -> Div {
    card()
}

pub fn card_title(glyph: Option<(Icon, u32)>, title: impl Into<SharedString>, aside: Option<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(9.))
        .when_some(glyph, |d, (i, c)| d.child(icon(i, 18., rgb(c))))
        .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .when_some(aside, |d, a| d.child(div().flex_1().flex().justify_end().text_size(px(12.5)).text_color(rgb(TEXT3)).child(a)))
}

pub fn hint(text: impl Into<SharedString>) -> Div {
    div().text_size(px(13.)).line_height(px(19.)).text_color(rgb(TEXT2)).child(text.into())
}
pub fn caption(text: impl Into<SharedString>) -> Div {
    div().text_size(px(12.)).text_color(rgb(TEXT3)).child(text.into())
}
pub fn label(text: impl Into<SharedString>) -> Div {
    div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(TEXT2)).child(text.into())
}
pub fn eyebrow(text: impl Into<SharedString>) -> Div {
    div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(TEXT3)).child(text.into())
}
pub fn title(text: impl Into<SharedString>, size: f32) -> Div {
    div().text_size(px(size)).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Primary,
    Soft,
    Danger,
    Warn,
}

/// Bouton en pilule. Le style suit l'usage : action principale, secondaire, état du micro…
pub fn pill(id: impl Into<ElementId>, glyph: Option<Icon>, text: impl Into<SharedString>, tone: Tone) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = hover_t(&key);
    let (bg, fg, hover) = match tone {
        Tone::Primary => (rgb(ACCENT), rgb(WHITE), rgb(0x2b8af0)),
        Tone::Soft => (alpha(ACCENT, 0.1), rgb(ACCENT), alpha(ACCENT, 0.17)),
        Tone::Danger => (alpha(RED, 0.12), rgb(RED_TEXT), alpha(RED, 0.2)),
        Tone::Warn => (alpha(ORANGE, 0.14), rgb(ORANGE_TEXT), alpha(ORANGE, 0.22)),
        Tone::Plain => (alpha(PAPER, 0.86), rgb(TEXT), rgb(PAPER)),
    };
    let mut base = div();
    // dans une colonne, le bouton garde sa largeur au lieu de s'étirer
    base.style().align_self = Some(gpui::AlignSelf::FlexStart);
    let el = base
        .id(id)
        .relative()
        .top(px(-t))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .h(px(34.))
        .px(px(14.))
        .rounded_full()
        .bg(lerp(bg, hover, t))
        .text_color(fg)
        .text_size(px(13.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor(CursorStyle::PointingHand)
        .when(tone == Tone::Plain, |d| d.border_1().border_color(lerp(alpha(INK, 0.08), alpha(INK, 0.12), t)).shadow(vec![shadow(1. + 2. * t, 2. + 8. * t, 0.04 + 0.05 * t)]))
        .when(tone == Tone::Primary, |d| d.shadow(vec![BoxShadow { color: alpha(ACCENT, 0.24 + 0.14 * t), offset: point(px(0.), px(5. + 3. * t)), blur_radius: px(14. + 8. * t), spread_radius: px(0.) }]))
        .when_some(glyph, move |d, i| d.child(icon(i, 16., fg)))
        .child(text.into());
    hoverable(el, key)
}

/// Choix parmi plusieurs valeurs : le choix actif est plein, les autres en verre.
pub fn button(id: String, text: String, selected: bool) -> Stateful<Div> {
    pill(ElementId::Name(id.into()), None, text, if selected { Tone::Primary } else { Tone::Plain })
}

/// Bouton rond à icône (menu ⋯, couper une voie…).
pub fn round(id: impl Into<ElementId>, glyph: Icon, size: f32, bg: Hsla, fg: Hsla) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = hover_t(&key);
    // au survol : un fond discret apparaît, un fond plein fonce légèrement
    let hover = if bg.a < 0.3 { alpha(0x767680, 0.2) } else { lerp(bg, alpha(INK, bg.a), 0.08) };
    let el = div()
        .id(id)
        .relative()
        .top(px(-t))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .bg(lerp(bg, hover, t))
        .cursor(CursorStyle::PointingHand)
        .child(icon(glyph, (size * 0.42).max(15.), fg));
    hoverable(el, key)
}

/// Interrupteur iOS.
pub fn toggle(id: impl Into<ElementId>, on: bool, enabled: bool) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = if enabled { hover_t(&key) } else { 0. };
    let track = if on { lerp(rgb(GREEN), rgb(0x2fb850), t) } else { lerp(alpha(0x787880, 0.2), alpha(0x787880, 0.3), t) };
    let el = div()
        .id(id)
        .flex()
        .flex_none()
        .w(px(46.))
        .h(px(28.))
        .p(px(2.))
        .rounded_full()
        .when(on, |d| d.justify_end())
        .bg(track)
        .when(enabled, |d| d.cursor(CursorStyle::PointingHand))
        .when(!enabled, |d| d.opacity(0.5))
        .child(div().size(px(24.)).rounded_full().bg(rgb(WHITE)).shadow(vec![shadow(2. + t, 6. + 3. * t, 0.2 + 0.06 * t)]));
    hoverable(el, key)
}

/// Ligne « libellé … interrupteur » des listes de réglages.
pub fn toggle_row(glyph: Option<Icon>, text: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .py(px(10.))
        .border_t_1()
        .border_color(alpha(INK, 0.07))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(13.))
                .when_some(glyph, |d, i| d.child(icon(i, 17., rgb(TEXT2))))
                .child(text.into()),
        )
        .child(control)
}

pub fn tag(text: impl Into<SharedString>) -> Div {
    div().flex_none().h(px(24.)).px(px(10.)).flex().items_center().rounded_full().bg(alpha(0x767680, 0.12)).text_size(px(12.)).text_color(rgb(0x3a3a3c)).child(text.into())
}

#[derive(Clone, Copy)]
pub enum State {
    Ok,
    Info,
    Warn,
    Error,
    Idle,
}
impl State {
    fn colours(self) -> (u32, u32) {
        match self {
            State::Ok => (GREEN, GREEN_TEXT),
            State::Info => (ACCENT, ACCENT),
            State::Warn => (ORANGE, ORANGE_TEXT),
            State::Error => (RED, RED_TEXT),
            State::Idle => (TEXT3, TEXT2),
        }
    }
}
pub fn status(text: impl Into<SharedString>, state: State) -> Div {
    let (dot, fg) = state.colours();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(fg))
        .child(div().flex_none().size(px(7.)).rounded_full().bg(rgb(dot)))
        .child(text.into())
}
/// Message de résultat : vert si tout va bien, orange sinon.
pub fn notice(ok: bool, text: impl Into<SharedString>) -> Div {
    let (bg, fg, glyph) = if ok { (GREEN, GREEN_TEXT, Icon::Check) } else { (ORANGE, ORANGE_TEXT, Icon::Alert) };
    div()
        .flex()
        .items_center()
        .gap(px(9.))
        .px(px(14.))
        .py(px(9.))
        .rounded(px(12.))
        .bg(alpha(bg, 0.12))
        .text_color(rgb(fg))
        .text_size(px(13.))
        .child(icon(glyph, 16., rgb(fg)).flex_none())
        // un message long passe à la ligne au lieu de sortir de la carte
        .child(div().flex_1().min_w(px(0.)).line_height(px(19.)).child(text.into()))
}

/// Fenêtre de dialogue centrée, au-dessus d'un voile léger : la page ne bouge pas.
pub fn modal(content: Div) -> impl IntoElement {
    use gpui::AnimationExt as _;
    div()
        .id("modal")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(alpha(0x000000, if is_dark() { 0.45 } else { 0.18 }))
        .occlude()
        .child(
            content
                .w(px(560.))
                .bg(alpha(PAPER, 1.))
                .shadow(vec![gpui::BoxShadow { color: alpha(0x000000, 0.2), offset: gpui::point(px(0.), px(18.)), blur_radius: px(48.), spread_radius: px(0.) }]),
        )
        .with_animation("modal-in", gpui::Animation::new(std::time::Duration::from_millis(180)).with_easing(gpui::ease_out_quint()), |d, t| d.opacity(t))
}

/// Notification flottante en bas de la fenêtre (au-dessus de la page, sans la décaler).
pub fn toast(ok: bool, text: impl Into<SharedString>) -> impl IntoElement {
    use gpui::AnimationExt as _;
    let (fg, glyph) = if ok { (GREEN_TEXT, Icon::Check) } else { (ORANGE_TEXT, Icon::Alert) };
    let text: SharedString = text.into();
    div()
        .absolute()
        .bottom(px(28.))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .id(SharedString::from(format!("toast-{text}")))
                .flex()
                .items_center()
                .gap(px(10.))
                .max_w(px(620.))
                .px(px(18.))
                .py(px(12.))
                .rounded(px(16.))
                .bg(alpha(PAPER, 0.96))
                .border_1()
                .border_color(alpha(INK, 0.06))
                .shadow(vec![gpui::BoxShadow { color: alpha(0x000000, 0.14), offset: gpui::point(px(0.), px(10.)), blur_radius: px(30.), spread_radius: px(0.) }])
                .text_size(px(13.5))
                .text_color(rgb(TEXT))
                .child(icon(glyph, 17., rgb(fg)))
                .child(text)
                .with_animation(
                    "toast-in",
                    gpui::Animation::new(std::time::Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
                    |d, t| d.opacity(t).top(px(10. * (1. - t))),
                ),
        )
}

/// Pastille d'icône colorée des cartes.
pub fn chip(glyph: Icon, colour: u32, size: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(size * 0.32))
        .bg(linear_gradient(145., linear_color_stop(tint(colour, 0.88), 0.), linear_color_stop(tint(colour, 0.8), 1.)))
        .child(icon(glyph, size * 0.53, shade(colour, 0.12)))
}

/// Curseur horizontal : piste fine, remplissage coloré, bouton blanc.
pub fn hslider(fraction: f32, colour: Hsla, hover: f32) -> Div {
    let f = fraction.clamp(0., 1.);
    let knob = 22. + 4. * hover;
    div()
        .relative()
        .h(px(22.))
        .w_full()
        .child(div().absolute().left_0().right_0().top(px(8.)).h(px(6.)).rounded_full().bg(alpha(0x767680, 0.16)))
        .child(div().absolute().left_0().top(px(8.)).h(px(6.)).w(gpui::relative(f)).rounded_full().bg(colour))
        .child(
            div()
                .absolute()
                .top(px(11. - knob / 2.))
                .left(gpui::relative(f))
                .ml(px(-knob / 2.))
                .size(px(knob))
                .rounded_full()
                .bg(rgb(WHITE))
                .border_1()
                .border_color(alpha(INK, 0.06))
                .shadow(vec![shadow(1., 3., 0.18), shadow(4. + 2. * hover, 10. + 6. * hover, 0.08 + 0.06 * hover)]),
        )
}

/// Champ de saisie d'une ligne (le texte est tenu par la vue).
pub fn field(id: impl Into<ElementId>, focus: &FocusHandle, text: &str, placeholder: &str, editing: bool, enabled: bool) -> Stateful<Div> {
    let handle = focus.clone();
    let empty = text.is_empty();
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = if enabled { hover_t(&key) } else { 0. };
    let el = div()
        .id(id)
        .track_focus(focus)
        .tab_stop(enabled)
        .flex()
        .items_center()
        .h(px(36.))
        .px(px(12.))
        .rounded(px(11.))
        .bg(alpha(PAPER, 0.95))
        .border_1()
        .border_color(if editing { alpha(ACCENT, 0.7) } else { lerp(alpha(INK, 0.1), alpha(ACCENT, 0.38), t) })
        .text_size(px(13.5))
        .text_color(rgb(if empty { TEXT3 } else { TEXT }))
        .when(enabled, |d| d.cursor(CursorStyle::IBeam))
        .when(!enabled, |d| d.opacity(0.55))
        .on_mouse_down(MouseButton::Left, move |_, window, _| {
            if enabled {
                handle.focus(window)
            }
        })
        .child(if editing {
            format!("{text}│")
        } else if empty {
            placeholder.to_owned()
        } else {
            text.to_owned()
        });
    hoverable(el, key)
}

pub enum Edit {
    Changed,
    Submit,
    Ignored,
}
/// Saisie clavier commune aux champs : Retour arrière, Entrée, Ctrl+V, espace et caractères.
pub fn edit_text(text: &mut String, event: &KeyDownEvent, max: usize, accept: impl Fn(char) -> bool, cx: &App) -> Edit {
    let k = &event.keystroke;
    if k.key == "backspace" {
        text.pop();
        return Edit::Changed;
    }
    if k.key == "enter" {
        return Edit::Submit;
    }
    let pasted;
    let input = if k.modifiers.control && k.key == "v" {
        pasted = cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default();
        pasted.as_str()
    } else if k.key == "space" {
        " "
    } else if let Some(c) = k.key_char.as_deref() {
        c
    } else {
        return Edit::Ignored;
    };
    for c in input.chars().filter(|c| !c.is_control() && accept(*c)) {
        if text.chars().count() >= max {
            break;
        }
        text.push(c);
    }
    Edit::Changed
}

/// Liste déroulante : le bouton, puis la liste ouverte sous lui (au-dessus du reste).
pub fn select_button(id: impl Into<ElementId>, text: impl Into<SharedString>, open: bool, enabled: bool) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = if enabled { hover_t(&key) } else { 0. };
    let el = div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .h(px(38.))
        .px(px(12.))
        .rounded(px(11.))
        .bg(lerp(alpha(PAPER, 0.92), rgb(PAPER), t))
        .border_1()
        .border_color(if open { alpha(ACCENT, 0.6) } else { lerp(alpha(INK, 0.1), alpha(ACCENT, 0.38), t) })
        .shadow(vec![shadow(1. + t, 2. + 6. * t, 0.03 + 0.05 * t)])
        .text_size(px(13.5))
        .when(enabled, |d| d.cursor(CursorStyle::PointingHand))
        .when(!enabled, |d| d.opacity(0.55))
        .child(text.into())
        .child(icon(Icon::ChevronDown, 15., rgb(TEXT2)));
    hoverable(el, key)
}
pub fn menu_panel() -> Div {
    div()
        .flex()
        .flex_col()
        .p(px(6.))
        .rounded(px(14.))
        .bg(alpha(PAPER, 0.98))
        .border_1()
        .border_color(alpha(INK, 0.08))
        .shadow(vec![shadow(2., 6., 0.06), shadow(18., 44., 0.14)])
}
pub fn menu_item(id: impl Into<ElementId>, glyph: Option<Icon>, text: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = hover_t(&key);
    let rest = if selected { alpha(ACCENT, 0.1) } else { alpha(ACCENT, 0.) };
    let el = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(34.))
        .px(px(10.))
        .rounded(px(9.))
        .text_size(px(13.5))
        .cursor(CursorStyle::PointingHand)
        .bg(lerp(rest, alpha(ACCENT, if selected { 0.15 } else { 0.08 }), t))
        .when(selected, |d| d.text_color(rgb(ACCENT)))
        .when_some(glyph, |d, i| d.child(icon(i, 17., rgb(if selected { ACCENT } else { TEXT2 }))))
        .child(text.into())
        .when(selected, |d| d.child(div().flex_1().flex().justify_end().child(icon(Icon::Check, 15., rgb(ACCENT)))));
    hoverable(el, key)
}


// ── Briques de mise en page ───────────────────────────────────────────────────

/// Carte de réglages avec son en-tête (pastille, titre, mention à droite).
pub fn section(glyph: Icon, colour: u32, title_text: impl Into<SharedString>, aside: Option<SharedString>) -> Div {
    card().child(
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(chip(glyph, colour, 28.))
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title_text.into()))
            .when_some(aside, |d, a| d.child(div().flex_1().flex().justify_end().text_size(px(12.5)).text_color(rgb(TEXT3)).child(a))),
    )
}

/// Ligne d'une liste groupée : icône, titre, sous-titre, contrôle à droite. Filet au-dessus sauf pour la première.
pub fn list_row(glyph: Option<(Icon, u32)>, title_text: impl Into<SharedString>, sub: Option<SharedString>, trailing: impl IntoElement, first: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(48.))
        .py(px(8.))
        .when(!first, |d| d.border_t_1().border_color(alpha(INK, 0.07)))
        .when_some(glyph, |d, (i, c)| d.child(icon(i, 18., rgb(c))))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(title_text.into()))
                .when_some(sub, |d, s| d.child(div().text_size(px(12.)).line_height(px(17.)).text_color(rgb(TEXT2)).child(s))),
        )
        .child(div().flex_none().flex().items_center().gap(px(8.)).child(trailing))
}

/// Contrôle segmenté (choix exclusif court) : un rail gris, le choix actif en blanc.
pub fn segmented() -> Div {
    div().flex().w_full().p(px(3.)).gap(px(3.)).rounded(px(11.)).bg(alpha(0x767680, 0.12))
}
pub fn seg(id: impl Into<ElementId>, text: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = if selected { 0. } else { hover_t(&key) };
    let el = div()
        .id(id)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .h(px(30.))
        .px(px(10.))
        .rounded(px(8.))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor(CursorStyle::PointingHand)
        // en sombre, l'onglet choisi doit trancher sur le rail : un gris plus clair que les surfaces
        .when(selected, |d| d.bg(if is_dark() { raw(0x636366) } else { rgb(PAPER) }).shadow(vec![shadow(1., 3., 0.1)]))
        .when(!selected, |d| d.bg(lerp(alpha(PAPER, 0.), alpha(PAPER, 0.55), t)).text_color(lerp(rgb(0x3a3a3c), rgb(TEXT), t)))
        .child(text.into());
    hoverable(el, key)
}

/// Tuile de choix (préréglage, style, scène, thème) : se soulève au survol, cadre bleu quand elle est choisie.
pub fn tile(id: impl Into<ElementId>, glyph: Option<(Icon, u32)>, title_text: impl Into<SharedString>, desc: Option<SharedString>, selected: bool, badge_text: Option<(SharedString, u32)>) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = hover_t(&key);
    let el = div()
        .id(id)
        .relative()
        .top(px(-2. * t))
        .flex()
        .flex_col()
        .gap(px(6.))
        .flex_1()
        .min_w(px(0.))
        .p(px(14.))
        .rounded(px(16.))
        .cursor(CursorStyle::PointingHand)
        .bg(if selected { alpha(ACCENT, 0.06) } else { lerp(alpha(PAPER, 0.7), rgb(PAPER), t) })
        .border_1()
        .border_color(if selected { alpha(ACCENT, 0.9) } else { lerp(alpha(INK, 0.08), alpha(ACCENT, 0.3), t) })
        .shadow(if selected { vec![BoxShadow { color: alpha(ACCENT, 0.14), offset: point(px(0.), px(6.)), blur_radius: px(18.), spread_radius: px(0.) }] } else { lift_shadow(t, 0x111827, 0.6) })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .when_some(glyph, |d, (i, c)| d.child(chip(i, c, 30.)))
                .child(div().flex_1())
                .when_some(badge_text, |d, (b, c)| d.child(badge(b, c)))
                .when(selected, |d| d.child(div().flex().items_center().justify_center().size(px(20.)).rounded_full().bg(rgb(ACCENT)).child(icon(Icon::Check, 12., rgb(WHITE))))),
        )
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(title_text.into()))
        .when_some(desc, |d, s| d.child(div().text_size(px(12.)).line_height(px(17.)).text_color(rgb(TEXT2)).child(s)));
    hoverable(el, key)
}

/// Grille régulière : les éléments sont rangés par lignes de 'cols', toutes de même largeur.
pub fn grid(items: Vec<AnyElement>, cols: usize) -> Div {
    let mut root = div().flex().flex_col().gap(px(10.));
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        let mut row = div().flex().gap(px(10.));
        let mut n = 0;
        for item in items.by_ref().take(cols) {
            row = row.child(div().flex().flex_1().min_w(px(0.)).child(item));
            n += 1;
        }
        for _ in n..cols {
            row = row.child(div().flex_1());
        }
        root = root.child(row);
    }
    root
}

/// Étiquette d'état colorée (Installée, Nouvelle, Conseillé…).
pub fn badge(text: impl Into<SharedString>, colour: u32) -> Div {
    let fg = if colour == GREEN { GREEN_TEXT } else if colour == ORANGE { ORANGE_TEXT } else if colour == RED { RED_TEXT } else { colour };
    div()
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .bg(alpha(colour, 0.13))
        .text_color(rgb(fg))
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// Valeur réglable par pas : bouton −, valeur centrée de largeur fixe, bouton +.
pub fn stepper(less: Stateful<Div>, value: impl Into<SharedString>, more: Stateful<Div>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .p(px(3.))
        .rounded_full()
        .bg(alpha(0x767680, 0.1))
        .child(less)
        .child(div().w(px(70.)).flex().justify_center().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(value.into()))
        .child(more)
}
pub fn step_button(id: impl Into<ElementId>, glyph: Icon) -> Stateful<Div> {
    round(id, glyph, 28., rgb(PAPER), rgb(TEXT)).shadow(vec![shadow(1., 2., 0.08)])
}

/// Case d'une matrice (routage) : cercle plein coché, ou anneau vide.
pub fn check_cell(id: impl Into<ElementId>, on: bool, enabled: bool) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = key_of(&id);
    let t = if enabled { hover_t(&key) } else { 0. };
    let el = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded_full()
        .cursor(CursorStyle::PointingHand)
        .when(on, |d| d.bg(lerp(rgb(ACCENT), rgb(0x2b8af0), t)).shadow(vec![BoxShadow { color: alpha(ACCENT, 0.25 + 0.15 * t), offset: point(px(0.), px(3.)), blur_radius: px(8.), spread_radius: px(0.) }]).child(icon(Icon::Check, 15., rgb(WHITE))))
        .when(!on, |d| d.border_2().border_color(lerp(alpha(INK, 0.14), alpha(ACCENT, 0.55), t)).bg(lerp(alpha(PAPER, 0.6), alpha(ACCENT, 0.06), t)))
        .when(!enabled, |d| d.opacity(0.45));
    hoverable(el, key)
}

pub fn swatch(colour: u32, size: f32) -> Div {
    div().flex_none().size(px(size)).rounded(px(size * 0.3)).bg(raw(colour)).border_1().border_color(alpha(INK, 0.12))
}

pub fn progress(fraction: f32, colour: u32) -> Div {
    div().h(px(6.)).w_full().rounded_full().bg(alpha(0x767680, 0.14)).child(div().h_full().w(gpui::relative(fraction.clamp(0., 1.))).rounded_full().bg(rgb(colour)))
}

/// Curseur vertical (égaliseurs) : rail fin, remplissage depuis le zéro, bouton blanc.
pub fn vslider(fraction: f32, zero: f32, colour: Hsla, hover: f32, height: f32) -> Div {
    let f = fraction.clamp(0., 1.);
    let knob = 20. + 4. * hover;
    let (lo, hi) = if f < zero { (f, zero) } else { (zero, f) };
    div()
        .relative()
        .w(px(28.))
        .h(px(height))
        .child(div().absolute().top_0().bottom_0().left(px(11.)).w(px(6.)).rounded_full().bg(alpha(0x767680, 0.16)))
        .child(div().absolute().left(px(7.)).w(px(14.)).h(px(2.)).top(px(height * (1. - zero) - 1.)).rounded_full().bg(alpha(INK, 0.18)))
        .child(div().absolute().left(px(11.)).w(px(6.)).rounded_full().top(px(height * (1. - hi))).h(px(height * (hi - lo))).bg(colour))
        .child(
            div()
                .absolute()
                .left(px(14. - knob / 2.))
                .top(px(height * (1. - f) - knob / 2.))
                .size(px(knob))
                .rounded_full()
                .bg(rgb(WHITE))
                .border_1()
                .border_color(alpha(INK, 0.06))
                .shadow(vec![shadow(1., 3., 0.18), shadow(4. + 2. * hover, 10. + 6. * hover, 0.08 + 0.06 * hover)]),
        )
}

/// Texte de notes (CHANGELOG) : listes à puces, intertitres et **gras**, sans afficher la syntaxe.
pub fn rich_text(markdown: &str) -> Div {
    fn styled(line: &str) -> gpui::StyledText {
        let mut text = String::new();
        let mut bold = vec![];
        for (i, part) in line.split("**").enumerate() {
            let start = text.len();
            text.push_str(part);
            if i % 2 == 1 {
                bold.push((start..text.len(), gpui::HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), color: Some(rgb(TEXT)), ..Default::default() }));
            }
        }
        gpui::StyledText::new(text).with_highlights(bold)
    }
    let mut root = div().flex().flex_col().gap(px(7.)).text_size(px(13.)).line_height(px(19.)).text_color(rgb(0x3a3a3c));
    for line in markdown.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(h) = line.strip_prefix("#### ").or_else(|| line.strip_prefix("### ")) {
            root = root.child(div().mt(px(6.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(TEXT3)).child(h.to_uppercase()));
        } else if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            root = root.child(div().flex().gap(px(9.)).child(div().flex_none().mt(px(7.)).size(px(5.)).rounded_full().bg(rgb(ACCENT))).child(div().flex_1().min_w(px(0.)).child(styled(item))));
        } else {
            root = root.child(div().child(styled(line)));
        }
    }
    root
}

/// Répartit des cartes sur deux colonnes équilibrées.
pub fn columns(items: Vec<AnyElement>) -> Div {
    let (mut left, mut right) = (div().flex().flex_col().flex_1().min_w(px(0.)).gap(px(16.)), div().flex().flex_col().flex_1().min_w(px(0.)).gap(px(16.)));
    for (i, item) in items.into_iter().enumerate() {
        if i % 2 == 0 {
            left = left.child(item);
        } else {
            right = right.child(item);
        }
    }
    div().flex().items_start().gap(px(16.)).child(left).child(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: u32) -> f32 {
        let ch = |s: u32| ((c >> s) & 255) as f32 / 255.;
        0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
    }

    #[test]
    fn theme_clair_inchange_et_theme_sombre_lisible() {
        // le thème clair dessine exactement la palette d'origine
        for c in [BG, TEXT, TEXT2, TEXT3, ACCENT, GREEN, GREEN_TEXT, RED, RED_TEXT, ORANGE, ORANGE_TEXT, WHITE] {
            assert_eq!(themed(c, false), c);
        }
        assert_eq!(themed(PAPER, false), WHITE);
        assert_eq!(themed(INK, false), 0x000000);
        // en sombre : fond foncé, surfaces plus claires que le fond, textes clairs, blanc et noir conservés
        assert!(luminance(themed(BG, true)) < 0.12);
        assert!(luminance(DARK_SURFACE) > luminance(themed(BG, true)));
        assert!(luminance(themed(PAPER, true)) > luminance(DARK_SURFACE));
        for text in [TEXT, TEXT2, TEXT3, GREEN_TEXT, RED_TEXT, ORANGE_TEXT, ACCENT] {
            assert!(luminance(themed(text, true)) - luminance(DARK_SURFACE) > 0.2, "{text:06x} illisible sur une carte sombre");
        }
        assert_eq!(themed(WHITE, true), WHITE);
        assert_eq!(themed(0x000000, true), 0x000000);
    }

    #[test]
    fn apparence_enregistree_puis_relue() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prefs").join("apparence.txt");
        assert_eq!(Appearance::load(&file), Appearance::Auto, "sans fichier : automatique");
        for choice in Appearance::ALL {
            choice.save(&file).unwrap();
            assert_eq!(Appearance::load(&file), choice);
        }
        std::fs::write(&file, "fluo").unwrap();
        assert_eq!(Appearance::load(&file), Appearance::Auto, "valeur inconnue : automatique");
    }
}
