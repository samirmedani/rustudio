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

/// Couleur avec transparence (0 = invisible, 1 = opaque).
pub fn alpha(rgb: u32, a: f32) -> Hsla {
    let mut c: Hsla = gpui::rgb(rgb).into();
    c.a = a;
    c
}
pub fn rgb(c: u32) -> Hsla {
    gpui::rgb(c).into()
}
/// Mélange vers le blanc (t = 0 : couleur pure, t = 1 : blanc).
pub fn tint(c: u32, t: f32) -> Hsla {
    mix(c, WHITE, t)
}
pub fn shade(c: u32, t: f32) -> Hsla {
    mix(c, 0x000000, t)
}
fn mix(a: u32, b: u32, t: f32) -> Hsla {
    let ch = |c: u32, s: u32| ((c >> s) & 255) as f32;
    let m = |s| (ch(a, s) + (ch(b, s) - ch(a, s)) * t).round() as u32;
    rgb((m(16) << 16) | (m(8) << 8) | m(0))
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
            .border_color(lerp(alpha(0x000000, 0.05), alpha(ACCENT, 0.32), t))
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
    img(AMBIENT.clone()).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Cover)
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
        .bg(alpha(WHITE, 0.74))
        .border_1()
        .border_color(alpha(0x000000, 0.05))
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
        Tone::Plain => (alpha(WHITE, 0.86), rgb(TEXT), rgb(WHITE)),
    };
    let mut base = div();
    // dans une colonne, le bouton garde sa largeur au lieu de s'étirer
    base.style().align_self = Some(gpui::AlignSelf::FlexStart);
    let el = base
        .id(id)
        .relative()
        .top(px(-1. * t))
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
        .when(tone == Tone::Plain, |d| d.border_1().border_color(lerp(alpha(0x000000, 0.08), alpha(0x000000, 0.12), t)).shadow(vec![shadow(1. + 2. * t, 2. + 8. * t, 0.04 + 0.05 * t)]))
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
    let hover = if bg.a < 0.3 { alpha(0x767680, 0.2) } else { lerp(bg, alpha(0x000000, bg.a), 0.08) };
    let el = div()
        .id(id)
        .relative()
        .top(px(-1. * t))
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
        .border_color(alpha(0x000000, 0.07))
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
        .child(icon(glyph, 16., rgb(fg)))
        .child(text.into())
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
                .border_color(alpha(0x000000, 0.06))
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
        .bg(alpha(WHITE, 0.95))
        .border_1()
        .border_color(if editing { alpha(ACCENT, 0.7) } else { lerp(alpha(0x000000, 0.1), alpha(ACCENT, 0.38), t) })
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
        .bg(lerp(alpha(WHITE, 0.92), rgb(WHITE), t))
        .border_1()
        .border_color(if open { alpha(ACCENT, 0.6) } else { lerp(alpha(0x000000, 0.1), alpha(ACCENT, 0.38), t) })
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
        .bg(alpha(WHITE, 0.98))
        .border_1()
        .border_color(alpha(0x000000, 0.08))
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
        .when(!first, |d| d.border_t_1().border_color(alpha(0x000000, 0.07)))
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
        .when(selected, |d| d.bg(rgb(WHITE)).shadow(vec![shadow(1., 3., 0.1)]))
        .when(!selected, |d| d.bg(lerp(alpha(WHITE, 0.), alpha(WHITE, 0.55), t)).text_color(lerp(rgb(0x3a3a3c), rgb(TEXT), t)))
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
        .bg(if selected { alpha(ACCENT, 0.06) } else { lerp(alpha(WHITE, 0.7), rgb(WHITE), t) })
        .border_1()
        .border_color(if selected { alpha(ACCENT, 0.9) } else { lerp(alpha(0x000000, 0.08), alpha(ACCENT, 0.3), t) })
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
    round(id, glyph, 28., rgb(WHITE), rgb(TEXT)).shadow(vec![shadow(1., 2., 0.08)])
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
        .when(!on, |d| d.border_2().border_color(lerp(alpha(0x000000, 0.14), alpha(ACCENT, 0.55), t)).bg(lerp(alpha(WHITE, 0.6), alpha(ACCENT, 0.06), t)))
        .when(!enabled, |d| d.opacity(0.45));
    hoverable(el, key)
}

pub fn swatch(colour: u32, size: f32) -> Div {
    div().flex_none().size(px(size)).rounded(px(size * 0.3)).bg(rgb(colour)).border_1().border_color(alpha(0x000000, 0.12))
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
        .child(div().absolute().left(px(7.)).w(px(14.)).h(px(2.)).top(px(height * (1. - zero) - 1.)).rounded_full().bg(alpha(0x000000, 0.18)))
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
                .border_color(alpha(0x000000, 0.06))
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
