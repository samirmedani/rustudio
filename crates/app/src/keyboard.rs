//! Page « Mon clavier » : MK770 en USB. Chaque réglage part tout de suite et n'est affiché qu'après relecture
//! par le moteur ; la couleur par touche se dessine d'abord sur la page, puis s'applique d'un clic.
use crate::{
    backend::Client,
    mixer::Mixer,
    ui::{self, Icon, Tone, alpha, rgb},
};
use gpui::{Context, Div, FontWeight, SharedString, div, prelude::*, px};
use rustudio_core::mk770::{self as data, BRIGHTNESS_STEPS, LAYOUT};
use serde::Deserialize;
use serde_json::{Value, json};

pub const MODULE: &str = "/api/modules/cooler-master-mk770";
/// Côté d'une touche dessinée, et écart entre deux touches.
const KEY: f32 = 36.;
const GAP: f32 = 4.;
const PALETTE: [&str; 12] = ["#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#00C7BE", "#5AC8FA", "#0071E3", "#5856D6", "#8A2BFF", "#FF2D55", "#FFFFFF", "#000000"];

#[derive(Clone, Debug, Deserialize, PartialEq)]
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
    busy: bool,
    notice: Option<(bool, String)>,
    /// couleur du pinceau
    brush: Option<String>,
    /// couleurs des 104 LED en cours de dessin ; None : rien à appliquer
    paint: Option<Vec<String>>,
}

impl KeyboardPage {
    pub fn receive(&mut self, view: Option<KeyboardView>) {
        if !self.busy {
            self.status = view;
        }
    }
    fn live(&self) -> Option<&Settings> {
        self.status.as_ref().filter(|v| v.device.status == "ready")?.state.as_ref()
    }
    fn brush(&self) -> String {
        self.brush.clone().unwrap_or_else(|| "#0071E3".into())
    }
    /// Couleurs à dessiner : le brouillon, sinon ce que le clavier a relu (complété à 104).
    fn colours(&self) -> Vec<String> {
        if let Some(p) = &self.paint {
            return p.clone();
        }
        let mut keys = self.live().map(|s| s.keys.clone()).unwrap_or_default();
        keys.resize(data::LEDS, "#000000".into());
        keys
    }
}

fn hex(c: &str) -> u32 {
    u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0)
}

/// Cran de luminosité le plus proche (0 à 4).
fn brightness_step(b: u8) -> usize {
    BRIGHTNESS_STEPS.iter().enumerate().min_by_key(|(_, s)| (**s as i16 - b as i16).abs()).map(|(i, _)| i).unwrap_or(4)
}

impl Mixer {
    fn keyboard_send(&mut self, body: Value, done: &'static str, cx: &mut Context<Self>) {
        if self.keyboard.busy {
            return;
        }
        self.keyboard.busy = true;
        self.keyboard.notice = None;
        cx.notify();
        let client = self.client.clone();
        let refresh = body.is_null();
        let work = cx.background_executor().spawn(async move {
            let result = if refresh { client.keyboard_command("refresh", &json!({})) } else { client.keyboard_command("settings", &body) };
            (result, client.keyboard_view())
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.keyboard.busy = false;
                this.keyboard.receive(view.ok());
                match result {
                    Ok(settings) => {
                        if let Some(v) = this.keyboard.status.as_mut() {
                            v.state = Some(settings);
                        }
                        this.keyboard.notice = Some((true, done.into()));
                    }
                    Err(error) => this.keyboard.notice = Some((false, error)),
                }
                cx.notify();
            });
        })
        .detach();
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
            .child(ui::pill("mk770-refresh", Some(Icon::Refresh), "Actualiser", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.keyboard_send(Value::Null, "Réglages relus", cx))))
    }

    pub fn keyboard_view(&self, cx: &mut Context<Self>) -> Div {
        let root = div().flex().flex_col().gap(px(16.));
        let Some(s) = self.keyboard.live().cloned() else {
            let error = self.keyboard.status.as_ref().and_then(|v| v.device.error.clone());
            return root.child(
                ui::card()
                    .items_center()
                    .py(px(36.))
                    .child(ui::chip(Icon::Keyboard, ui::ACCENT, 52.))
                    .child(ui::title("MK770 non détecté", 18.))
                    .child(ui::hint("Branche le câble USB du clavier. Le récepteur 2,4 GHz et le Bluetooth ne sont pas encore pris en charge."))
                    .children(error.map(|e| ui::caption(e))),
            );
        };
        let active = s.active_effect;
        let enabled = !self.keyboard.busy;
        let current = s.effects.get(active as usize).cloned();

        // en tête : connexion, effet, luminosité
        let metric = |glyph: Icon, title: &'static str, value: String, sub: String| {
            ui::card()
                .flex_1()
                .min_w(px(0.))
                .gap(px(4.))
                .px(px(16.))
                .py(px(12.))
                .rounded(px(18.))
                .child(div().flex().items_center().gap(px(8.)).text_size(px(12.)).text_color(rgb(ui::TEXT2)).child(ui::icon(glyph, 18., rgb(ui::ACCENT))).child(title))
                .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child(value))
                .child(ui::hint(sub).text_size(px(12.)))
        };
        let light = current.as_ref().map(|e| format!("{} %", brightness_step(e.brightness) * 25)).unwrap_or_else(|| "—".into());
        let metrics = div()
            .flex()
            .gap(px(12.))
            .child(metric(Icon::Plug, "Connexion", "Câble USB".into(), s.firmware.as_ref().map(|f| format!("Firmware {f}")).unwrap_or_else(|| "Firmware inconnu".into())))
            .child(metric(Icon::Sparkles, "Effet", data::effect_name(active).into(), if data::effect(active).is_some_and(|e| e.reactive) { "S’allume quand tu tapes".into() } else { "Affiché en ce moment".into() }))
            .child(metric(Icon::Sun, "Luminosité", light, "Fn + F8 sur le clavier".into()));

        // effets
        let mut tiles = vec![];
        for e in data::EFFECTS.iter() {
            let id = e.id;
            let desc: Option<SharedString> = e.reactive.then(|| "Réagit quand tu tapes".into());
            let tile = ui::tile(SharedString::from(format!("mk770-effect-{id}")), None, e.name, desc, id == active, None)
                .when(enabled && id != active, |t| t.on_click(cx.listener(move |this, _, _, cx| this.keyboard_send(json!({"active_effect": id}), "Effet appliqué", cx))));
            tiles.push(tile.into_any_element());
        }
        let effects = ui::section(Icon::Sparkles, ui::ACCENT, "Effet", None).child(ui::grid(tiles, 4));

        // réglages de l'effet affiché
        let info = data::effect(active);
        let mut tune = ui::section(Icon::Sliders, 0x5856d6, format!("Réglages · {}", data::effect_name(active)), None);
        if let (Some(e), Some(info)) = (current.clone(), info) {
            if active != data::OFF {
                let mut seg = ui::segmented();
                let step = brightness_step(e.brightness);
                for (i, label) in ["Éteinte", "25 %", "50 %", "75 %", "100 %"].into_iter().enumerate() {
                    let value = BRIGHTNESS_STEPS[i];
                    let el = ui::seg(SharedString::from(format!("mk770-light-{i}")), label, i == step);
                    seg = seg.child(el.when(enabled && i != step, |d| d.on_click(cx.listener(move |this, _, _, cx| this.keyboard_send(json!({"brightness": value}), "Luminosité appliquée", cx)))));
                }
                tune = tune.child(ui::label("Luminosité")).child(seg);
            }
            if info.speed {
                let mut seg = ui::segmented();
                for (i, label) in ["Très lente", "Lente", "Moyenne", "Rapide", "Très rapide"].into_iter().enumerate() {
                    let v = i as u8;
                    let el = ui::seg(SharedString::from(format!("mk770-speed-{i}")), label, e.speed == v);
                    seg = seg.child(el.when(enabled && e.speed != v, |d| d.on_click(cx.listener(move |this, _, _, cx| this.keyboard_send(json!({"speed": v}), "Vitesse appliquée", cx)))));
                }
                tune = tune.child(ui::label("Vitesse")).child(seg);
            }
            if info.direction {
                let mut seg = ui::segmented();
                for (reverse, label) in [(false, "Normal"), (true, "Inversé")] {
                    let el = ui::seg(SharedString::from(format!("mk770-dir-{reverse}")), label, e.reverse == reverse);
                    seg = seg.child(el.when(enabled && e.reverse != reverse, |d| d.on_click(cx.listener(move |this, _, _, cx| this.keyboard_send(json!({"reverse": reverse}), "Sens appliqué", cx)))));
                }
                tune = tune.child(ui::label("Sens")).child(seg);
            }
            if info.colour {
                let mut palette = div().flex().flex_wrap().gap(px(8.));
                for c in PALETTE {
                    if c == "#000000" {
                        continue;
                    }
                    let chosen = e.colour.eq_ignore_ascii_case(c);
                    palette = palette.child(swatch(SharedString::from(format!("mk770-colour-{c}")), c, chosen).when(enabled && !chosen, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| this.keyboard_send(json!({"colour": c}), "Couleur appliquée", cx)))
                    }));
                }
                tune = tune.child(div().flex().items_center().justify_between().child(ui::label("Couleur")).child(ui::caption(e.colour.clone()))).child(palette);
            }
            if active == data::CUSTOM {
                tune = tune.child(ui::hint("Les couleurs viennent de « Couleur par touche », ci-dessous."));
            } else if active == data::OFF {
                tune = tune.child(ui::hint("Le rétroéclairage est éteint. Choisis un autre effet pour le rallumer."));
            } else if !info.speed && !info.colour {
                tune = tune.child(ui::hint("Cet effet n’a pas d’autre réglage."));
            }
        } else {
            tune = tune.child(ui::hint("Cet effet n’affiche rien sur le MK770. Choisis un effet ci-dessus."));
        }

        div().flex().flex_col().gap(px(16.)).child(metrics).child(effects).child(tune).child(self.keyboard_painter(cx))
    }

    /// Couleur par touche : un pinceau, le clavier dessiné, puis « Appliquer au clavier ».
    fn keyboard_painter(&self, cx: &mut Context<Self>) -> Div {
        let k = &self.keyboard;
        let colours = k.colours();
        let brush = k.brush();
        let enabled = !k.busy;
        let mut palette = div().flex().flex_wrap().items_center().gap(px(8.));
        for c in PALETTE {
            let chosen = brush.eq_ignore_ascii_case(c);
            palette = palette.child(swatch(SharedString::from(format!("mk770-brush-{c}")), c, chosen).on_click(cx.listener(move |this, _, _, cx| {
                this.keyboard.brush = Some(c.to_owned());
                cx.notify();
            })));
        }
        let mut board = div().flex().flex_col().gap(px(GAP)).p(px(14.)).rounded(px(16.)).bg(alpha(ui::INK, 0.05));
        for (r, row) in LAYOUT.iter().enumerate() {
            let mut line = div().flex().gap(px(GAP));
            for (i, key) in row.iter().enumerate() {
                let width = key.width * KEY + (key.width - 1.).max(0.) * GAP;
                if key.leds.is_empty() && key.label.is_empty() {
                    line = line.child(div().flex_none().w(px(width - GAP)).h(px(KEY)));
                    continue;
                }
                let colour = key.leds.first().map(|&l| colours[l as usize].clone()).unwrap_or_else(|| "#000000".into());
                let lit = colour != "#000000";
                let c = hex(&colour);
                let bright = ((c >> 16) & 255) * 3 + ((c >> 8) & 255) * 6 + (c & 255) > 1400;
                let id = SharedString::from(format!("mk770-key-{r}-{i}"));
                let leds: Vec<usize> = key.leds.iter().map(|l| *l as usize).collect();
                let cell = div()
                    .id(id)
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(width))
                    .h(px(KEY))
                    .rounded(px(7.))
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor(gpui::CursorStyle::PointingHand)
                    .bg(if lit { ui::raw(c) } else { alpha(ui::INK, 0.07) })
                    .border_1()
                    .border_color(alpha(ui::INK, 0.1))
                    // texte lisible sur la couleur de la touche, quel que soit le thème
                    .text_color(if lit { ui::raw(if bright { 0x1d1d1f } else { 0xffffff }) } else { rgb(ui::TEXT3) })
                    .child(key.label)
                    .when(enabled, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            let mut paint = this.keyboard.colours();
                            let brush = this.keyboard.brush();
                            for &l in &leds {
                                paint[l] = brush.clone();
                            }
                            this.keyboard.paint = Some(paint);
                            cx.notify();
                        }))
                    });
                line = line.child(cell);
            }
            board = board.child(line);
        }
        let dirty = k.paint.is_some();
        let actions = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::pill("mk770-fill", Some(Icon::Palette), "Tout remplir", Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                let brush = this.keyboard.brush();
                this.keyboard.paint = Some(vec![brush; data::LEDS]);
                cx.notify();
            })))
            .child(ui::pill("mk770-clear", Some(Icon::Close), "Tout éteindre", Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                this.keyboard.paint = Some(vec!["#000000".into(); data::LEDS]);
                cx.notify();
            })))
            .child(div().flex_1())
            .when(dirty, |d| {
                d.child(ui::pill("mk770-cancel", None, "Annuler", Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                    this.keyboard.paint = None;
                    cx.notify();
                })))
            })
            .child(
                ui::pill("mk770-apply", Some(Icon::Check), "Appliquer au clavier", if dirty { Tone::Primary } else { Tone::Plain })
                    .when(dirty && enabled, |d| d.on_click(cx.listener(|this, _, _, cx| this.keyboard_apply_paint(cx)))),
            );
        ui::section(Icon::Keyboard, 0x34c759, "Couleur par touche", None)
            .child(ui::hint("Choisis une couleur, puis clique sur les touches à peindre. « Appliquer au clavier » passe le clavier sur l’effet Personnalisé.").text_size(px(12.5)))
            .child(palette)
            .child(div().flex().justify_center().child(board))
            .child(actions)
    }

    fn keyboard_apply_paint(&mut self, cx: &mut Context<Self>) {
        let Some(paint) = self.keyboard.paint.clone() else { return };
        if self.keyboard.busy {
            return;
        }
        self.keyboard.busy = true;
        self.keyboard.notice = None;
        cx.notify();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let result = client.keyboard_command("settings", &json!({"keys": paint}));
            (result, client.keyboard_view())
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.keyboard.busy = false;
                this.keyboard.receive(view.ok());
                match result {
                    Ok(settings) => {
                        if let Some(v) = this.keyboard.status.as_mut() {
                            v.state = Some(settings);
                        }
                        // dessin appliqué et relu : la page affiche maintenant ce que le clavier montre
                        this.keyboard.paint = None;
                        this.keyboard.notice = Some((true, "Couleurs appliquées".into()));
                    }
                    Err(error) => this.keyboard.notice = Some((false, error)),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn swatch(id: SharedString, colour: &str, chosen: bool) -> gpui::Stateful<Div> {
    let key = id.to_string();
    let t = ui::hover_t(&key);
    let c = hex(colour);
    ui::hoverable(
        div()
            .id(id)
            .size(px(28.))
            .rounded_full()
            .cursor(gpui::CursorStyle::PointingHand)
            .bg(ui::raw(c))
            .border_2()
            .border_color(if chosen { rgb(ui::ACCENT) } else { ui::lerp(alpha(ui::INK, 0.14), alpha(ui::ACCENT, 0.6), t) }),
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
    let mut page = KeyboardPage::default();
    page.receive(Some(client.keyboard_view()?));
    let mut paint = page.colours();
    paint[0] = "#FF3B30".into();
    let s = client.keyboard_command("settings", &json!({"keys": paint}))?;
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
