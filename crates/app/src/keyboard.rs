//! Navigation, aperçu et réglages des effets lumineux du MK770.
use crate::{
    backend::Client,
    desktop::Page,
    keyboard_paint::{Colours, PaintEditor, draw_preview_keys, read_colours, write_colours},
    mixer::Mixer,
    ui::{self, Icon, Tone, alpha, rgb},
};
use gpui::{AnimationExt, Context, Div, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use rustudio_core::mk770 as data;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Instant};

pub const MODULE: &str = "/api/modules/cooler-master-mk770";
pub(crate) const PALETTE: [&str; 12] = [
    "#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#00C7BE", "#5AC8FA",
    "#0071E3", "#5856D6", "#8A2BFF", "#FF2D55", "#FFFFFF", "#000000",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Effect {
    pub id: u8,
    pub brightness: u8,
    pub speed: u8,
    pub reverse: bool,
    pub colour: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Settings {
    pub wired: bool,
    pub firmware: Option<String>,
    pub active_effect: u8,
    pub effects: Vec<Effect>,
    pub keys: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct KeyboardView {
    pub device: crate::backend::Device,
    pub state: Option<Settings>,
}

#[derive(Default)]
pub struct KeyboardPage {
    pub status: Option<KeyboardView>,
    pub(crate) busy: bool,
    notice: Option<(bool, String)>,
    pub(crate) picker: Option<crate::colour::Picker>,
    pub(crate) paint: PaintEditor,
    os_mode_help: bool,
    selected_effect: Option<u8>,
    drafts: HashMap<u8, Effect>,
    clock: PreviewClock,
}

#[derive(Clone, Copy)]
pub(crate) enum EffectSetting {
    Brightness(u8),
    Speed(u8),
    Reverse(bool),
    Colour(u32),
}

#[derive(Default)]
struct PreviewClock {
    anchor: Option<Instant>,
    seconds: f32,
    paused: bool,
}

impl PreviewClock {
    fn seconds(&self, now: Instant, speed: u8) -> f32 {
        let rate = 0.45 + f32::from(speed.min(data::SPEED_MAX)) * 0.275;
        self.seconds + self.anchor.filter(|_| !self.paused)
            .map_or(0., |anchor| now.duration_since(anchor).as_secs_f32() * rate)
    }
    fn rebase(&mut self, now: Instant, speed: u8) {
        self.seconds = self.seconds(now, speed);
        self.anchor = Some(now);
    }
}

impl KeyboardPage {
    pub fn receive(&mut self, view: Option<KeyboardView>) {
        if !self.busy {
            if let Some(state) = view.as_ref().and_then(|v| v.state.as_ref()) {
                self.paint.set_saved(read_colours(&state.keys), false);
                self.clock.anchor.get_or_insert_with(Instant::now);
            }
            self.status = view;
        }
    }
    pub(crate) fn live(&self) -> Option<&Settings> {
        self.status.as_ref().filter(|v| v.device.status == "ready")?.state.as_ref()
    }
    fn selected(&self, settings: &Settings) -> u8 {
        self.selected_effect.filter(|&id| data::effect(id).is_some())
            .or_else(|| data::effect(settings.active_effect).map(|e| e.id))
            .unwrap_or(data::OFF)
    }
    fn effect<'a>(&'a self, s: &'a Settings, id: u8) -> Option<&'a Effect> {
        self.drafts.get(&id).or_else(|| s.effects.get(id as usize).filter(|e| e.id == id))
    }
    pub(crate) fn shows_previews(&self, page: Page) -> bool {
        matches!(page, Page::Keyboard | Page::KeyboardSettings)
            && !self.clock.paused
            && self.live().is_some_and(|s| {
                let id = self.selected(s);
                data::effect(id).is_some_and(|e| e.speed)
                    && self.effect(s, id).is_some_and(|e| e.brightness > 0)
            })
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Refresh,
    Effect(u8),
    Paint,
}

impl Operation {
    fn success(self) -> &'static str {
        match self {
            Self::Refresh => "Réglages relus",
            Self::Effect(_) => "Effet appliqué",
            Self::Paint => "Couleurs appliquées",
        }
    }
}

impl Effect {
    pub(crate) fn changes(&self) -> Value {
        let Some(info) = data::effect(self.id) else { return Value::Null };
        let mut body = json!({"active_effect": self.id});
        if self.id != data::OFF { body["brightness"] = json!(self.brightness); }
        if info.speed { body["speed"] = json!(self.speed.min(data::SPEED_MAX)); }
        if info.direction { body["reverse"] = json!(self.reverse); }
        if info.colour { body["colour"] = json!(self.colour); }
        body
    }
}

fn hex(c: &str) -> u32 {
    u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0)
}

impl Mixer {
    fn keyboard_send(&mut self, operation: Operation, body: Value, cx: &mut Context<Self>) {
        if self.keyboard.busy { return; }
        self.keyboard.paint.finish();
        self.keyboard.busy = true;
        self.keyboard.notice = None;
        cx.notify();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let action = if matches!(operation, Operation::Refresh) { "refresh" } else { "settings" };
            (client.keyboard_command(action, &body), client.keyboard_view())
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                let k = &mut this.keyboard;
                k.busy = false;
                match result {
                    Ok(settings) => {
                        if let Ok(mut view) = view {
                            if view.device.status == "ready" { view.state = Some(settings.clone()); }
                            k.receive(Some(view));
                        }
                        k.paint.set_saved(read_colours(&settings.keys), matches!(operation, Operation::Paint));
                        match operation {
                            Operation::Effect(id) => { k.drafts.remove(&id); }
                            Operation::Paint => {
                                k.selected_effect = Some(data::CUSTOM);
                                k.picker = None;
                            }
                            Operation::Refresh => {}
                        }
                        k.notice = Some((true, operation.success().into()));
                    }
                    Err(error) => {
                        if let Ok(view) = view { k.receive(Some(view)); }
                        k.notice = Some((false, error));
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    pub(crate) fn keyboard_apply_paint(&mut self, cx: &mut Context<Self>) {
        if self.keyboard.live().is_none() { return; }
        let colours = write_colours(self.keyboard.paint.current());
        self.keyboard_send(Operation::Paint, json!({"keys": colours}), cx);
    }

    fn keyboard_select_effect(&mut self, id: u8, cx: &mut Context<Self>) {
        if self.keyboard.busy || self.keyboard.live().is_some_and(|s| self.keyboard.selected(s) == id) { return; }
        self.keyboard.selected_effect = Some(id);
        self.keyboard.picker = None;
        self.keyboard.clock = PreviewClock { anchor: Some(Instant::now()), ..PreviewClock::default() };
        cx.notify();
    }

    pub(crate) fn keyboard_edit_effect(&mut self, id: u8, setting: EffectSetting, cx: &mut Context<Self>) {
        let k = &mut self.keyboard;
        if k.busy { return; }
        let Some(s) = k.live() else { return };
        let Some(mut next) = k.effect(s, id).cloned() else { return };
        match setting {
            EffectSetting::Brightness(value) => next.brightness = value,
            EffectSetting::Speed(value) => {
                k.clock.rebase(Instant::now(), next.speed);
                next.speed = value;
            }
            EffectSetting::Reverse(value) => next.reverse = value,
            EffectSetting::Colour(value) => next.colour = format!("#{value:06X}"),
        }
        let unchanged = k.live().and_then(|s| s.effects.get(id as usize)) == Some(&next);
        if unchanged { k.drafts.remove(&id); } else { k.drafts.insert(id, next); }
        cx.notify();
    }

    /// En-tête : état du dernier envoi et « Actualiser ».
    pub(crate) fn keyboard_actions(&self, cx: &mut Context<Self>) -> Div {
        let k = &self.keyboard;
        let (colour, text) = if k.busy {
            (ui::TEXT3, Some("Envoi au clavier…".to_owned()))
        } else {
            match &k.notice {
                Some((true, t)) => (ui::GREEN_TEXT, Some(t.clone())),
                Some((false, t)) => (ui::RED_TEXT, Some(t.clone())),
                None => (ui::TEXT3, None),
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().max_w(px(380.)).truncate().text_size(px(13.)).text_color(rgb(colour)).children(text))
            .child(ui::pill("mk770-refresh", Some(Icon::Refresh), "Actualiser", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.keyboard_send(Operation::Refresh, json!({}), cx))))
    }

    pub fn keyboard_view(&self, width: f32, cx: &mut Context<Self>) -> Div {
        if self.page == Page::KeyboardPaint {
            return self.keyboard_painter(width, cx);
        }
        let root = div().w_full().max_w(px(1160.)).mx_auto();
        let Some(s) = self.keyboard.live() else {
            let error = self.keyboard.status.as_ref().and_then(|v| v.device.error.clone());
            return root.child(ui::card().items_center().py(px(36.))
                .child(ui::chip(Icon::Keyboard, ui::ACCENT, 52.))
                .child(ui::title("MK770 non détecté", 18.))
                .child(ui::hint("Branche le clavier en USB pour lire ou appliquer ses réglages."))
                .children(error.map(ui::caption))
                .child(ui::pill("mk770-offline-paint", Some(Icon::Palette), "Ouvrir l’éditeur de couleurs", Tone::Soft)
                    .on_click(cx.listener(|this, _, _, cx| this.navigate(Page::KeyboardPaint, cx)))));
        };
        match self.page {
            Page::KeyboardShortcuts => return root.max_w(px(720.)).child(shortcuts(self.keyboard.os_mode_help, cx)),
            Page::KeyboardInfo => return root.max_w(px(720.)).child(specs(s)),
            _ => {}
        }

        let id = self.keyboard.selected(s);
        let Some(effect) = self.keyboard.effect(s, id) else {
            return root.child(ui::hint("Réglages indisponibles. Clique sur « Actualiser »."));
        };
        let tuning = self.page == Page::KeyboardSettings;
        let hero = self.effect_hero(effect, s.active_effect, cx);
        if tuning {
            let options = data::EFFECTS.iter().map(|e| (e.name.to_owned(), e.id == id)).collect();
            let selector = self.dropdown("keyboard-choose-effect", data::effect_name(id).to_owned(), options, !self.keyboard.busy,
                |this, i, cx| this.keyboard_select_effect(data::EFFECTS[i].id, cx), cx);
            let controls = div().flex().flex_col().gap(px(12.))
                .child(ui::label("Effet à régler")).child(selector)
                .child(self.keyboard_tune(effect, cx));
            return root.child(ui::settings_columns(width, vec![hero.into_any_element()], vec![controls.into_any_element()]));
        }
        let tiles = data::EFFECTS.iter().map(|e| {
            let rec = self.keyboard.effect(s, e.id);
            self.effect_tile(e, rec, id, s.active_effect, cx).into_any_element()
        }).collect();
        root.flex().flex_col().gap(px(20.))
            .child(hero)
            .child(ui::section(Icon::Sparkles, ui::ACCENT, "Tous les effets", None)
                .child(ui::grid(tiles, ui::columns_that_fit(width.min(1160.), 172., 10., 5))))
    }

    fn effect_hero(&self, effect: &Effect, active: u8, cx: &mut Context<Self>) -> Div {
        let id = effect.id;
        let info = data::effect(id);
        let animated = info.is_some_and(|e| e.speed);
        let paused = self.keyboard.clock.paused;
        let ready = !self.keyboard.busy;
        let apply = id != active || self.keyboard.drafts.contains_key(&id);
        let clock = self.keyboard.clock.seconds(Instant::now(), effect.speed);
        let keys = (id == data::CUSTOM).then(|| *self.keyboard.paint.saved());
        let status = if id == active {
            if apply { "Aperçu à appliquer" } else { "Actif sur le clavier" }
        } else { "Aperçu local" };
        let header = div().flex().items_center().justify_between().gap(px(8.)).flex_wrap()
            .child(div().flex().flex_col().gap(px(3.))
                .child(ui::title(data::effect_name(id), 21.))
                .child(ui::caption(status)))
            .when(animated, |d| d.child(ui::pill("mk770-pause", Some(if paused { Icon::Play } else { Icon::Stop }),
                if paused { "Relancer l’aperçu" } else { "Mettre en pause" }, Tone::Plain)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let k = &mut this.keyboard;
                    let speed = k.live().and_then(|s| k.effect(s, id)).map_or(2, |e| e.speed);
                    k.clock.rebase(Instant::now(), speed);
                    k.clock.paused = !k.clock.paused;
                    cx.notify();
                }))));
        let body = effect.changes();
        let actions = div().flex().flex_wrap().gap(px(8.))
            .child(ui::pill("mk770-activate", Some(Icon::Check), "Appliquer au clavier", Tone::Primary)
                .when(!apply || !ready, |d| d.opacity(0.45))
                .when(apply && ready, |d| d.on_click(cx.listener(move |this, _, _, cx| this.keyboard_send(Operation::Effect(id), body.clone(), cx)))))
            .when(self.keyboard.drafts.contains_key(&id), |d| d.child(
                ui::pill("mk770-effect-reset", Some(Icon::History), "Rétablir les réglages", Tone::Plain)
                    .when(ready, |d| d.on_click(cx.listener(move |this, _, _, cx| {
                        this.keyboard.drafts.remove(&id);
                        this.keyboard.picker = None;
                        cx.notify();
                    }))),
            ))
            .when(self.page == Page::Keyboard, |d| d.child(
                ui::pill("mk770-tune-current", Some(Icon::Sliders), "Régler l’effet", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.navigate(Page::KeyboardSettings, cx))),
            ))
            .when(id == data::CUSTOM, |d| d.child(
                ui::pill("mk770-paint-from-effect", Some(Icon::Palette), "Couleur par touche", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.navigate(Page::KeyboardPaint, cx))),
            ));
        div().w_full().min_w(px(0.)).flex().flex_col().gap(px(14.)).p(px(18.))
            .rounded(px(17.)).border_1().border_color(alpha(ui::INK, 0.08)).bg(alpha(ui::PAPER, 0.5))
            .child(header)
            .child(preview(id, effect, clock, keys)
                .with_animation(("mk770-preview-in", u64::from(id)), gpui::Animation::new(std::time::Duration::from_millis(145))
                    .with_easing(gpui::ease_out_quint()), |d, t| d.opacity(0.62 + 0.38 * t)))
            .child(actions)
    }

    fn effect_tile(&self, e: &data::EffectInfo, effect: Option<&Effect>, selected: u8, active: u8, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let id = e.id;
        let key = format!("mk770-effect-{id}");
        let hover = ui::hover_t(&key);
        let colour = if e.colour { effect.map_or(0x6a78ff, |rec| hex(&rec.colour)) }
            else if id == data::OFF { ui::TEXT2 }
            else if e.speed { 0x8175fa } else { ui::ACCENT };
        let icon = if id == data::CUSTOM { Icon::Palette } else if id == data::OFF { Icon::Power }
            else if e.reactive { Icon::Bolt } else if e.speed { Icon::Wave } else { Icon::Sun };
        let tile = div().id(SharedString::from(key.clone())).relative().top(px(-2. * hover))
            .flex().flex_col().gap(px(6.)).min_w(px(0.)).p(px(9.)).rounded(px(12.))
            .bg(if id == selected { alpha(ui::ACCENT, 0.10) } else { alpha(ui::INK, 0.03 + 0.035 * hover) })
            .border_1().border_color(if id == selected { alpha(ui::ACCENT, 0.34) } else { alpha(ui::INK, 0.06) })
            .cursor(gpui::CursorStyle::PointingHand)
            .child(ui::icon(icon, 18., if colour == 0 { rgb(ui::TEXT3) } else if e.colour { ui::raw(colour) } else { rgb(colour) }))
            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(e.name))
            .child(ui::caption(if id == active { "Actif sur le clavier" } else { effect_kind(id) }))
            .on_click(cx.listener(move |this, _, _, cx| this.keyboard_select_effect(id, cx)));
        ui::hoverable(tile, key)
    }

    fn keyboard_tune(&self, e: &Effect, cx: &mut Context<Self>) -> Div {
        let id = e.id;
        let enabled = !self.keyboard.busy;
        let Some(info) = data::effect(id) else { return div() };
        let mut tune = ui::section(Icon::Sliders, 0x8175fa, "Réglages", None);
        if id == data::OFF {
            return tune.child(ui::hint("Applique cet effet pour couper le rétroéclairage."));
        }
        tune = tune.child(self.slider_row_with(
            &format!("mk770-light-{id}"), "Luminosité", f64::from(e.brightness), (0., 255.), 1.,
            |value| format!("{:.0} %", value * 100. / 255.), enabled, true,
            move |this, value, cx| this.keyboard_edit_effect(id, EffectSetting::Brightness(value as u8), cx), cx,
        ));
        if info.speed {
            tune = tune.child(self.slider_row_with(
                &format!("mk770-speed-{id}"), "Vitesse", f64::from(e.speed.min(data::SPEED_MAX)),
                (0., f64::from(data::SPEED_MAX)), 1., |value| format!("{:.0} / {}", value + 1., data::SPEED_MAX + 1),
                enabled, true, move |this, value, cx| this.keyboard_edit_effect(id, EffectSetting::Speed(value as u8), cx), cx,
            ));
        }
        if info.direction {
            let mut directions = ui::segmented();
            for (i, (reverse, label)) in [(false, "Vers la droite"), (true, "Vers la gauche")].into_iter().enumerate() {
                directions = directions.child(ui::seg(("mk770-dir", i), label, e.reverse == reverse).when(enabled && e.reverse != reverse, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| this.keyboard_edit_effect(id, EffectSetting::Reverse(reverse), cx)))
                }));
            }
            tune = tune.child(ui::label("Sens")).child(directions);
        }
        if info.colour {
            let mut palette = div().flex().flex_wrap().gap(px(8.));
            for c in PALETTE {
                let chosen = e.colour.eq_ignore_ascii_case(c);
                palette = palette.child(swatch(SharedString::from(format!("mk770-colour-{c}")), c, chosen).when(enabled && !chosen, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| this.keyboard_edit_effect(id, EffectSetting::Colour(hex(c)), cx)))
                }));
            }
            tune = tune.child(div().flex().justify_between().child(ui::label("Couleur")).child(ui::caption(e.colour.clone())))
                .child(palette).child(self.colour_picker(crate::colour::Target::Effect(id), &e.colour, cx));
        }
        tune
    }
}

/// Type d'un effet, en un mot.
fn effect_kind(id: u8) -> &'static str {
    match data::effect(id) {
        Some(e) if e.reactive => "Réagit à la frappe",
        Some(_) if id == data::OFF => "Rétroéclairage coupé",
        Some(_) if id == data::CUSTOM => "Une couleur par touche",
        Some(e) if !e.speed => "Fixe",
        Some(_) => "Animé",
        None => "Rien à l’écran",
    }
}

/// Fiche technique du clavier.
fn specs(s: &Settings) -> Div {
    let row = |k: &'static str, v: String| {
        div()
            .flex()
            .justify_between()
            .gap(px(12.))
            .py(px(7.))
            .border_b_1()
            .border_color(alpha(ui::INK, 0.06))
            .child(div().text_size(px(12.5)).text_color(rgb(ui::TEXT2)).child(k))
            .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_right().child(v))
    };
    ui::section(Icon::Keyboard, 0x34c759, "Ton MK770", None)
        .gap(px(4.))
        .child(row("Fabricant", "Cooler Master".into()))
        .child(row("Disposition", "AZERTY, pavé numérique".into()))
        .child(row("Éclairage", "RGB, 104 LED".into()))
        .child(row("Effets intégrés", format!("{}", data::EFFECTS.len())))
        .child(row("Liaisons", "USB-C · 2,4 GHz · Bluetooth".into()))
        .child(row("Dans Rustudio", if s.wired { "USB".into() } else { "—".into() }))
        .child(row("Batterie", "4 000 mAh".into()))
        .child(row("Firmware", s.firmware.clone().unwrap_or_else(|| "inconnu".into())))
        .child(ui::hint("Les réglages restent dans le clavier : ils suivent ton MK770 sur un autre PC.").text_size(px(12.)).pt(px(8.)))
}

/// Raccourcis Fn du clavier : ce qu'on peut aussi faire sans Rustudio.
fn shortcuts(show_mode_help: bool, cx: &mut Context<Mixer>) -> Div {
    let row = |keys: &'static str, what: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .py(px(5.))
            .child(div().flex_none().px(px(8.)).h(px(24.)).flex().items_center().rounded(px(7.)).bg(alpha(ui::INK, 0.07)).text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).child(keys))
            .child(div().flex_1().min_w(px(0.)).text_size(px(12.5)).text_color(rgb(ui::TEXT2)).child(what))
    };
    ui::section(Icon::Sparkles, ui::ORANGE, "Raccourcis du clavier", None)
        .gap(px(2.))
        .child(row("Fn + F8", "Luminosité, 5 crans"))
        .child(row("Fn + F10 / F11", "Plus vite, moins vite"))
        .child(row("Fn + F12", "Sens de l’effet"))
        .child(row("Fn + F5 à F7", "Changer d’effet"))
        .child(row("Fn + 1, 2, 3", "Profils du clavier"))
        .child(row("Fn + Maj droite", "Windows ↔ macOS"))
        .child(div().mt(px(10.)).child(ui::hint("Mode actuel : à vérifier sur le clavier.").text_size(px(12.))))
        .child(ui::pill("mk770-os-mode-help", Some(Icon::Help), if show_mode_help { "Masquer les instructions" } else { "Vérifier / changer le mode" }, Tone::Plain)
            .mt(px(8.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.keyboard.os_mode_help = !this.keyboard.os_mode_help;
                cx.notify();
            })))
        .when(show_mode_help, |d| {
            let indicator = |colour, label: &'static str| div().flex().items_center().gap(px(8.))
                .child(div().flex_none().size(px(12.)).rounded_full().bg(ui::raw(colour)).border_1().border_color(alpha(ui::INK, 0.2)))
                .child(ui::hint(label).text_size(px(12.5)));
            d.child(div().flex().flex_col().gap(px(8.)).mt(px(10.)).p(px(12.)).rounded(px(12.)).bg(alpha(ui::INK, 0.04))
                .child(ui::hint("1. Maintiens Fn et regarde la touche Maj de droite.").text_size(px(12.5)))
                .child(indicator(0x0071e3, "Bleue : mode Windows"))
                .child(indicator(0xffffff, "Blanche : mode macOS"))
                .child(ui::hint("2. Pour changer de mode, appuie sur Fn + Maj droite, puis vérifie à nouveau la couleur.").text_size(px(12.5)))
                .child(ui::caption("Le changement se fait sur le clavier. Rustudio ne détecte pas encore ce mode automatiquement.")))
        })
}

const PREVIEW_COLS: usize = 12;
const PREVIEW_ROWS: usize = 4;
/// Rythme des aperçus : 30 images/s suffisent à ces petites animations (l'animation intégrée suivait l'écran, 360 Hz ici).
pub(crate) const PREVIEW_FRAME: std::time::Duration = std::time::Duration::from_micros(33_300);

/// Aperçu local des effets intégrés, sur la disposition du MK770.
fn preview(id: u8, effect: &Effect, clock: f32, keys: Option<Colours>) -> Div {
    let colour = hex(&effect.colour);
    let reverse = effect.reverse;
    let brightness = f32::from(effect.brightness) / 255.;
    let seconds = match id {
        8 => 3.2,
        11 => 1.6,
        7 | 13 | 14 => 4.0,
        _ => 2.4,
    };
    let phase = (clock % seconds) / seconds;
    div().p(px(8.)).rounded(px(10.)).bg(ui::raw(0x14151a)).child(
        div().w_full().h(px(172.)).child(gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                draw_preview_keys(bounds, window, |x, y, led| {
                    let off = lit(0, 0.);
                    let colour = match id {
                        data::CUSTOM => keys.map_or(off, |keys| {
                            if keys[led] == 0 { off } else { keys[led] }
                        }),
                        data::OFF => off,
                        _ => {
                            let x = (x * PREVIEW_COLS as f32) as usize;
                            let y = (y * PREVIEW_ROWS as f32) as usize;
                            cell_colour(id, colour, reverse, x.min(PREVIEW_COLS - 1), y.min(PREVIEW_ROWS - 1), phase)
                        }
                    };
                    lit(colour, brightness)
                });
            },
        ).size_full()),
    )
}

/// Teinte (0 à 1) en couleur vive.
fn hue(h: f32) -> u32 {
    let h = h.rem_euclid(1.) * 6.;
    let x = 1. - ((h % 2.) - 1.).abs();
    let (r, g, b) = match h as u32 {
        0 => (1., x, 0.),
        1 => (x, 1., 0.),
        2 => (0., 1., x),
        3 => (0., x, 1.),
        4 => (x, 0., 1.),
        _ => (1., 0., x),
    };
    ((r * 255.) as u32) << 16 | ((g * 255.) as u32) << 8 | (b * 255.) as u32
}

/// Mélange de la touche éteinte vers la couleur, selon l'intensité (0 à 1).
fn lit(colour: u32, intensity: f32) -> u32 {
    let k = intensity.clamp(0., 1.);
    let off = [0x27, 0x28, 0x30];
    let ch = |s: u32, i: usize| ((off[i] as f32) + ((((colour >> s) & 255) as f32) - off[i] as f32) * k) as u32;
    ch(16, 0) << 16 | ch(8, 1) << 8 | ch(0, 2)
}

/// Pseudo-hasard stable pour une touche.
fn noise(i: usize) -> f32 {
    let x = (i as u32).wrapping_mul(2_654_435_761).rotate_left(13) ^ 0x9e37_79b9;
    (x % 1000) as f32 / 1000.
}

fn cell_colour(effect: u8, colour: u32, reverse: bool, x: usize, y: usize, t: f32) -> u32 {
    let (fx, fy) = (x as f32, y as f32);
    let (cx, cy) = ((PREVIEW_COLS - 1) as f32 / 2., (PREVIEW_ROWS - 1) as f32 / 2.);
    let dist = ((fx - cx).powi(2) + ((fy - cy) * 1.4).powi(2)).sqrt();
    let i = y * PREVIEW_COLS + x;
    match effect {
        0 => colour,
        1 => hue(fx / PREVIEW_COLS as f32 + if reverse { t } else { -t }),
        // deux frappes par cycle : la ligne et la colonne s'allument puis s'éteignent
        2 | 3 | 10 => {
            let presses = [(3usize, 1usize), (8, 2)];
            let (px_, py_) = presses[((t * 2.) as usize).min(1)];
            let fade = 1. - (t * 2.).fract();
            let hit = match effect {
                2 => x == px_ || y == py_,
                3 => x == px_ && y == py_,
                _ => (fx - px_ as f32).abs() <= 1.5 && (fy - py_ as f32).abs() <= 1.,
            };
            if hit { lit(colour, fade) } else { lit(colour, 0.) }
        }
        5 => {
            let phase = (t + noise(i)).fract();
            lit(colour, if noise(i * 7) > 0.6 { (phase * std::f32::consts::PI).sin() } else { 0. })
        }
        6 => {
            let pos = (t + noise(x)).fract() * (PREVIEW_ROWS as f32 + 1.) - 1.;
            lit(colour, (1. - (fy - pos).abs()).max(0.))
        }
        7 => hue(t),
        8 => lit(colour, 0.5 - 0.5 * (t * std::f32::consts::TAU).cos()),
        9 | 15 => {
            let r = t * 8.;
            lit(colour, (1. - (dist - r).abs()).max(0.) * (1. - t))
        }
        11 => {
            let beat = (-((t - 0.15) / 0.06).powi(2)).exp() + 0.7 * (-((t - 0.38) / 0.06).powi(2)).exp();
            lit(colour, beat)
        }
        12 => {
            let head = t * (PREVIEW_COLS as f32 + 5.) - 2.;
            let k = if y == 1 || y == 2 { (1. - (head - fx) / 4.).clamp(0., 1.) * if fx <= head { 1. } else { 0. } } else { 0. };
            lit(colour, k)
        }
        13 => hue(((fy - cy) * 1.4).atan2(fx - cx) / std::f32::consts::TAU + t),
        14 => hue(((fy - cy) * 1.4).atan2(fx - cx) / std::f32::consts::TAU + dist / 7. - t),
        _ => lit(colour, 0.),
    }
}

pub(crate) fn swatch(id: SharedString, colour: &str, chosen: bool) -> gpui::Stateful<Div> {
    let key = id.to_string();
    let t = ui::hover_t(&key);
    let c = hex(colour);
    ui::hoverable(
        div()
            .id(id)
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor(gpui::CursorStyle::PointingHand)
            .bg(ui::raw(c))
            .border_2()
            .border_color(if chosen { rgb(ui::ACCENT) } else { ui::lerp(alpha(ui::INK, 0.14), alpha(ui::ACCENT, 0.6), t) })
            .when(chosen, |d| d.child(ui::icon(Icon::Check, 14., ui::raw(crate::keyboard_paint::label_colour(c))))),
        key,
    )
}

/// Auto-test (démonstration) : effet, vitesse, sens, couleur et couleurs des touches, chaque fois relus.
pub fn verify(client: &Client) -> Result<(), String> {
    let view = client.keyboard_view()?;
    let original = view.state.ok_or("Réglages de démo du clavier absents")?;
    if original.active_effect != 0 || original.keys.len() != data::LEDS_READ {
        return Err("Le test exige les données de démonstration du MK770.".into());
    }
    let s = client.keyboard_command("settings", &json!({"active_effect": 1, "speed": 4, "reverse": true}))?;
    if s.active_effect != 1 || s.effects[1].speed != 4 || !s.effects[1].reverse {
        return Err("Effet, vitesse ou sens non relus.".into());
    }
    let s = client.keyboard_command("settings", &json!({"effect": 0, "colour": "#34C759", "brightness": 0x7F}))?;
    if s.effects[0].colour != "#34C759" || s.effects[0].brightness != 0x7F || s.active_effect != 1 {
        return Err("Couleur ou luminosité non relues.".into());
    }
    let mut paint = read_colours(&original.keys);
    paint[0] = 0xff3b30;
    let s = client.keyboard_command("settings", &json!({"keys": write_colours(&paint)}))?;
    if s.active_effect != data::CUSTOM || s.keys[0] != "#FF3B30" {
        return Err("Couleurs des touches non relues.".into());
    }
    if client.keyboard_command("settings", &json!({"speed": 9})).is_ok() {
        return Err("Une vitesse hors limites a été acceptée.".into());
    }
    client.keyboard_command("settings", &json!({"active_effect": 0}))?;
    println!("OK : effet, vitesse, sens, couleur, luminosité et couleurs des touches du MK770.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn pause_et_changement_de_vitesse_conservent_la_phase() {
        let now = Instant::now();
        let tick = Duration::from_secs(1);
        let mut clock = PreviewClock { anchor: Some(now), ..PreviewClock::default() };
        let start = clock.seconds(now + tick, 2);
        clock.rebase(now + tick, 2);
        assert_eq!(clock.seconds(now + tick, 4), start);
        assert!(clock.seconds(now + tick * 2, 4) > start + 1.);
        clock.rebase(now + tick * 2, 4);
        clock.paused = true;
        let paused = clock.seconds(now + tick * 2, 4);
        assert_eq!(clock.seconds(now + tick * 5, 4), paused);
        clock.rebase(now + tick * 5, 4);
        clock.paused = false;
        assert_eq!(clock.seconds(now + tick * 5, 4), paused);
    }

    #[test]
    fn application_de_l_effet_ne_transmet_que_ses_reglages() {
        let mut effect = Effect { id: 1, brightness: 0x7f, speed: 3, reverse: true, colour: "#8455FF".into() };
        assert_eq!(effect.changes(), json!({"active_effect": 1, "brightness": 127, "speed": 3, "reverse": true}));
        effect.id = 0;
        assert_eq!(effect.changes(), json!({"active_effect": 0, "brightness": 127, "colour": "#8455FF"}));
        effect.id = data::CUSTOM;
        assert_eq!(effect.changes(), json!({"active_effect": 4, "brightness": 127}));
        effect.id = data::OFF;
        assert_eq!(effect.changes(), json!({"active_effect": 16}));
    }
}
