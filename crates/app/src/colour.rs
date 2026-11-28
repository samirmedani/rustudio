//! Sélecteur de couleur du clavier : brouillon local, appliqué seulement au clic.
use crate::{mixer::Mixer, ui::{self, Icon, Tone}};
use gpui::{Bounds, ClipboardItem, Context, Div, FocusHandle, KeyDownEvent, MouseButton, Pixels, Point, SharedString, Stateful, canvas, div, linear_color_stop, linear_gradient, prelude::*, px};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target { Effect(u8), Brush, Gradient }

pub(crate) struct Picker {
    target: Target,
    h: f32,
    s: f32,
    v: f32,
    text: String,
    focus: FocusHandle,
    editing: bool,
    selected: bool,
    drag: Option<(Bounds<Pixels>, bool)>,
}

fn parse_hex(text: &str) -> Option<u32> {
    let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if !text.is_ascii() || !text.bytes().all(|c| c.is_ascii_hexdigit()) { return None; }
    match text.len() {
        6 => u32::from_str_radix(text, 16).ok(),
        3 => u32::from_str_radix(&text.chars().flat_map(|c| [c, c]).collect::<String>(), 16).ok(),
        _ => None,
    }
}

fn hsv(colour: u32) -> (f32, f32, f32) {
    let [r, g, b] = [16, 8, 0].map(|shift| ((colour >> shift) & 255) as f32 / 255.);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta == 0. { 0. } else if max == r { ((g - b) / delta).rem_euclid(6.) * 60. } else if max == g { ((b - r) / delta + 2.) * 60. } else { ((r - g) / delta + 4.) * 60. };
    (h, if max == 0. { 0. } else { delta / max }, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> u32 {
    let h = h.rem_euclid(360.) / 60.;
    let c = v * s;
    let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
    let m = v - c;
    let (r, g, b) = match h as u8 { 0 => (c, x, 0.), 1 => (x, c, 0.), 2 => (0., c, x), 3 => (0., x, c), 4 => (x, 0., c), _ => (c, 0., x) };
    let byte = |v: f32| ((v + m).clamp(0., 1.) * 255.).round() as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

impl Picker {
    fn colour(&self) -> u32 { from_hsv(self.h, self.s, self.v) }
    fn set_rgb(&mut self, colour: u32) {
        let (h, s, v) = hsv(colour);
        // Conserver la teinte choisie quand on passe par le blanc ou le noir.
        if s > 0. { self.h = h; }
        self.s = s;
        self.v = v;
        self.text = format!("#{colour:06X}");
    }
    fn at(&mut self, at: Point<Pixels>) {
        let Some((bounds, hue)) = self.drag else { return };
        let x = (f32::from(at.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.)).clamp(0., 1.);
        let y = (f32::from(at.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.)).clamp(0., 1.);
        if hue { self.h = x * 360.; } else { self.s = x; self.v = 1. - y; }
        self.text = format!("#{:06X}", self.colour());
        self.editing = false;
    }
}

impl Mixer {
    pub(crate) fn finish_colour_picker(&mut self) {
        if let Some(p) = self.keyboard.picker.as_mut() { p.drag = None; }
    }

    pub(crate) fn move_colour_picker(&mut self, at: Point<Pixels>, button: Option<MouseButton>, cx: &mut Context<Self>) -> bool {
        let Some(p) = self.keyboard.picker.as_mut().filter(|p| p.drag.is_some()) else { return false };
        if !self.page.is_keyboard() || button != Some(MouseButton::Left) { p.drag = None; return false; }
        p.at(at);
        cx.notify();
        true
    }

    fn colour_area(&self, target: Target, hue: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let layout = bounds.clone();
        div().id(if hue { "keyboard-colour-hue" } else { "keyboard-colour-sv" }).relative().w_full().flex_none()
            .cursor(gpui::CursorStyle::Crosshair)
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                if let Some(p) = this.keyboard.picker.as_mut().filter(|p| p.target == target)
                    && let Some(bounds) = bounds.get() {
                        p.drag = Some((bounds, hue));
                        p.at(event.position);
                        cx.stop_propagation();
                        cx.notify();
                    }
            }))
            .child(canvas(move |bounds, _, _| layout.set(Some(bounds)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
    }

    fn apply_colour_picker(&mut self, cx: &mut Context<Self>) {
        if self.keyboard.busy { return; }
        let Some(p) = self.keyboard.picker.as_ref() else { return };
        let Some(colour) = parse_hex(&p.text) else { return };
        match p.target {
            Target::Effect(effect) => self.keyboard_edit_effect(effect, crate::keyboard::EffectSetting::Colour(colour), cx),
            Target::Brush => self.keyboard.paint.set_brush(colour),
            Target::Gradient => self.keyboard.paint.set_second(colour),
        }
        self.keyboard.picker = None;
        cx.notify();
    }

    pub(crate) fn colour_picker(&self, target: Target, current: &str, cx: &mut Context<Self>) -> Div {
        let enabled = !self.keyboard.busy;
        let opened = self.keyboard.picker.as_ref().filter(|p| p.target == target);
        let is_open = opened.is_some();
        let current = current.to_owned();
        let (id, label, apply) = match target {
            Target::Effect(_) => ("keyboard-custom-effect", "Couleur personnalisée", "Prévisualiser la couleur"),
            Target::Brush => ("keyboard-custom-brush", "Couleur personnalisée", "Utiliser pour le pinceau"),
            Target::Gradient => ("keyboard-custom-gradient", "Seconde couleur", "Utiliser pour les motifs"),
        };
        let mut root = div().flex().flex_col().max_w(px(360.)).gap(px(10.)).child(
            ui::pill(SharedString::from(id), Some(Icon::Palette), if is_open { "Masquer le sélecteur" } else { label }, Tone::Plain)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.keyboard.picker = if is_open { None } else {
                        let (h, s, v) = hsv(parse_hex(&current).unwrap_or(0x0071e3));
                        Some(Picker { target, h, s, v, text: current.clone(), focus: cx.focus_handle(), editing: false, selected: false, drag: None })
                    };
                    cx.notify();
                })));
        let Some(p) = opened else { return root };
        let colour = p.colour();
        let raw = ui::raw;
        let sv = self.colour_area(target, false, cx).h(px(132.)).bg(raw(from_hsv(p.h, 1., 1.)))
            .child(div().absolute().size_full().bg(linear_gradient(90., linear_color_stop(raw(0xffffff), 0.), linear_color_stop(raw(0xffffff).opacity(0.), 1.))))
            .child(div().absolute().size_full().bg(linear_gradient(180., linear_color_stop(raw(0).opacity(0.), 0.), linear_color_stop(raw(0), 1.))))
            .child(div().absolute().left(gpui::relative(p.s)).top(gpui::relative(1. - p.v)).ml(px(-6.)).mt(px(-6.)).size(px(12.)).rounded_full().border_2().border_color(raw(0xffffff)).bg(raw(colour)));
        let mut hue = self.colour_area(target, true, cx).h(px(18.)).flex();
        for n in 0..6 {
            hue = hue.child(div().flex_1().h_full().bg(linear_gradient(90., linear_color_stop(raw(from_hsv(n as f32 * 60., 1., 1.)), 0.), linear_color_stop(raw(from_hsv((n + 1) as f32 * 60., 1., 1.)), 1.))));
        }
        hue = hue.child(div().absolute().left(gpui::relative(p.h / 360.)).top(px(-2.)).ml(px(-3.)).w(px(6.)).h(px(22.)).rounded(px(3.)).border_2().border_color(raw(0xffffff)).bg(raw(from_hsv(p.h, 1., 1.))));
        root = root.child(ui::caption("Teinte, saturation et intensité")).child(sv).child(hue);
        for (n, label) in ["Rouge", "Vert", "Bleu"].into_iter().enumerate() {
            let shift = (2 - n) * 8;
            let value = ((colour >> shift) & 255) as f64;
            let scope = match target { Target::Effect(id) => format!("effect-{id}"), Target::Brush => "brush".into(), Target::Gradient => "gradient".into() };
            root = root.child(self.slider_row(&format!("keyboard-rgb-{scope}-{n}"), label, value, (0., 255.), 1., |v| format!("{v:.0}"), true, move |this, value, cx| {
                if let Some(p) = this.keyboard.picker.as_mut().filter(|p| p.target == target) {
                    let colour = (p.colour() & !(255 << shift)) | ((value as u32) << shift);
                    p.set_rgb(colour);
                    cx.notify();
                }
            }, cx));
        }
        let valid = parse_hex(&p.text).is_some();
        let field = ui::field("keyboard-colour-hex", &p.focus, &p.text, "#RRGGBB", p.editing, true).flex_1().min_w(px(0.))
            .when(p.editing && p.selected, |d| d.bg(ui::alpha(ui::ACCENT, 0.16)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if let Some(p) = this.keyboard.picker.as_mut() { p.editing = true; p.selected = true; }
                cx.notify();
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if let Some(p) = this.keyboard.picker.as_mut().filter(|p| p.editing) { p.editing = false; cx.notify(); }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let Some(p) = this.keyboard.picker.as_mut() else { return };
                if event.keystroke.modifiers.control && event.keystroke.key == "a" { p.selected = true; cx.stop_propagation(); cx.notify(); return; }
                if event.keystroke.modifiers.control && event.keystroke.key == "c" { cx.write_to_clipboard(ClipboardItem::new_string(p.text.clone())); cx.stop_propagation(); return; }
                if event.keystroke.key == "escape" { p.editing = false; p.text = format!("#{:06X}", p.colour()); cx.stop_propagation(); cx.notify(); return; }
                if p.selected && (event.keystroke.key_char.is_some() || event.keystroke.key == "backspace" || event.keystroke.modifiers.control && event.keystroke.key == "v") { p.text.clear(); p.selected = false; }
                match ui::edit_text(&mut p.text, event, 32, |_| true, cx) {
                    ui::Edit::Ignored => return,
                    ui::Edit::Submit => { this.apply_colour_picker(cx); }
                    ui::Edit::Changed => {
                        if let Some(colour) = parse_hex(&p.text) { let text = p.text.clone(); p.set_rgb(colour); p.text = text; }
                    }
                }
                cx.stop_propagation();
                cx.notify();
            }));
        root.child(ui::label("Code hexadécimal"))
            .child(div().flex().items_center().gap(px(8.)).child(div().flex_none().size(px(36.)).rounded(px(9.)).border_1().border_color(ui::alpha(ui::INK, 0.15)).bg(raw(colour))).child(field))
            .when(!valid, |d| d.child(ui::caption("Saisis un code de 3 ou 6 caractères, par exemple #64D2FF.").text_color(ui::rgb(ui::RED_TEXT))))
            .child(ui::pill("keyboard-colour-apply", Some(Icon::Check), apply, Tone::Primary)
                .when(!valid || !enabled, |d| d.opacity(0.5))
                .when(valid && enabled, |d| d.on_click(cx.listener(|this, _, _, cx| this.apply_colour_picker(cx)))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes_hex_valides_et_invalides() {
        assert_eq!(parse_hex(" #64d2ff "), Some(0x64d2ff));
        assert_eq!(parse_hex("abc"), Some(0xaabbcc));
        assert_eq!(parse_hex("000000"), Some(0));
        for bad in ["", "#", "#12345", "#12345678", "#gg00ff", "#１２３４５６"] { assert_eq!(parse_hex(bad), None); }
    }
    #[test]
    fn conversion_sans_perte_et_extremes() {
        for r in (0..=255).step_by(17) { for g in (0..=255).step_by(17) { for b in (0..=255).step_by(17) {
            let colour = (r << 16) | (g << 8) | b;
            let (h, s, v) = hsv(colour);
            assert_eq!(from_hsv(h, s, v), colour);
        }}}
        assert_eq!(from_hsv(360., 1., 1.), 0xff0000);
        assert_eq!(from_hsv(120., 0., 1.), 0xffffff);
        assert_eq!(from_hsv(240., 1., 0.), 0);
    }
}
