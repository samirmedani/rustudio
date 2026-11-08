use crate::{
    backend::Client,
    mixer::Mixer,
    ui::{self, Icon, Tone, alpha, rgb},
};
use gpui::{Context, Div, FocusHandle, FontWeight, KeyDownEvent, SharedString, Stateful, div, prelude::*, px};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const MODULE: &str = "/api/modules/attack-shark-r5-ultra";
const RATES: [u32; 7] = [125, 250, 500, 1000, 2000, 4000, 8000];

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Dpi {
    x: u16,
    y: u16,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Settings {
    firmware: String,
    wired: bool,
    profile: u8,
    battery: u8,
    charging: bool,
    stages: Vec<Dpi>,
    active_stage: usize,
    dpi_max: u16,
    polling_hz: u32,
    lod_mm: f64,
    debounce_ms: u8,
    sleep_seconds: u16,
    angle_snap: bool,
    motion_sync: bool,
    ripple_control: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MouseView {
    pub device: crate::backend::Device,
    pub state: Option<Settings>,
}

#[derive(Default)]
pub struct MousePage {
    pub status: Option<MouseView>,
    last: Option<Settings>,
    draft: BTreeMap<&'static str, Value>,
    dpi: BTreeMap<(usize, bool), String>,
    basis: Option<Settings>,
    busy: bool,
    notice: Option<(bool, String)>,
    /// numéro de la dernière modification : seul le dernier délai d'enregistrement envoie
    seq: u64,
    /// brouillon en cours d'envoi, pour garder ce qui a changé pendant l'échange
    sent: Option<(BTreeMap<&'static str, Value>, BTreeMap<(usize, bool), String>)>,
}

impl MousePage {
    pub fn receive(&mut self, view: Option<MouseView>) {
        if self.busy {
            return;
        }
        if let Some(settings) = view
            .as_ref()
            .filter(|v| v.device.status == "ready")
            .and_then(|v| v.state.as_ref())
        {
            self.last = Some(settings.clone());
        }
        self.status = view;
    }

    fn live(&self) -> Option<&Settings> {
        self.status
            .as_ref()
            .filter(|v| v.device.status == "ready")?
            .state
            .as_ref()
    }

    fn dirty(&self) -> bool {
        !self.draft.is_empty() || !self.dpi.is_empty()
    }
    fn conflict(&self, settings: &Settings) -> bool {
        self.basis.as_ref().is_some_and(|before| {
            before.profile != settings.profile
                || (!self.dpi.is_empty()
                    && (before.stages.len() != settings.stages.len()
                        || self
                            .dpi
                            .keys()
                            .any(|&(i, y)| dpi_at(before, i, y) != dpi_at(settings, i, y))))
                || self
                    .draft
                    .keys()
                    .any(|&key| field(before, key) != field(settings, key))
        })
    }
    fn editable(&self) -> bool {
        self.live().is_some_and(|s| !self.conflict(s))
    }
    fn begin(&mut self) -> Option<Settings> {
        if !self.editable() {
            return None;
        }
        let s = self.live()?.clone();
        self.basis.get_or_insert_with(|| s.clone());
        self.notice = None;
        self.seq += 1;
        Some(s)
    }
    fn value(&self, s: &Settings, key: &'static str) -> Value {
        self.draft
            .get(key)
            .cloned()
            .unwrap_or_else(|| field(s, key))
    }
    fn edit(&mut self, key: &'static str, value: Value) {
        let Some(s) = self.begin() else {
            return;
        };
        if field(&s, key) == value {
            self.draft.remove(key);
        } else {
            self.draft.insert(key, value);
        }
        if !self.dirty() {
            self.basis = None;
        }
    }
    fn edit_dpi(&mut self, i: usize, y: bool, value: String) {
        let Some(s) = self.begin() else {
            return;
        };
        if value
            .parse::<u16>()
            .ok()
            .is_some_and(|v| Some(v) == dpi_at(&s, i, y))
        {
            self.dpi.remove(&(i, y));
        } else {
            self.dpi.insert((i, y), value);
        }
        if !self.dirty() {
            self.basis = None;
        }
    }
    fn changes(&self) -> Result<Value, String> {
        let s = self
            .live()
            .ok_or("Réveille ou reconnecte la souris avant d’enregistrer.")?;
        if self.conflict(s) {
            return Err("La souris a changé depuis le début des modifications. Annule le brouillon puis réessaie.".into());
        }
        let mut changes = self
            .draft
            .iter()
            .map(|(&key, val)| (key.into(), val.clone()))
            .collect::<Map<_, _>>();
        if !self.dpi.is_empty() {
            let mut stages: Vec<_> = s.stages.iter().map(|d| (d.x, d.y)).collect();
            for (&(i, y), text) in &self.dpi {
                let value = text
                    .parse::<u16>()
                    .ok()
                    .filter(|&n| (50..=s.dpi_max).contains(&n) && n % 50 == 0)
                    .ok_or_else(|| {
                        format!("Choisis des DPI entre 50 et {}, par pas de 50.", s.dpi_max)
                    })?;
                let row = stages.get_mut(i).ok_or("Ce palier n’existe plus.")?;
                if y {
                    row.1 = value;
                } else {
                    row.0 = value;
                }
            }
            changes.insert(
                "stages".into(),
                json!(
                    stages
                        .iter()
                        .map(|&(x, y)| json!({"x":x,"y":y}))
                        .collect::<Vec<_>>()
                ),
            );
        }
        if s.wired
            && changes
                .get("polling_hz")
                .and_then(Value::as_u64)
                .is_some_and(|v| v > 1000)
        {
            return Err("La connexion USB est limitée à 1 000 Hz.".into());
        }
        Ok(json!({"profile":s.profile,"changes":changes}))
    }
}

fn dpi_at(s: &Settings, i: usize, y: bool) -> Option<u16> {
    s.stages.get(i).map(|d| if y { d.y } else { d.x })
}
fn field(s: &Settings, key: &str) -> Value {
    match key {
        "active_stage" => json!(s.active_stage),
        "polling_hz" => json!(s.polling_hz),
        "lod_mm" => json!(s.lod_mm),
        "debounce_ms" => json!(s.debounce_ms),
        "sleep_seconds" => json!(s.sleep_seconds),
        "angle_snap" => json!(s.angle_snap),
        "motion_sync" => json!(s.motion_sync),
        "ripple_control" => json!(s.ripple_control),
        _ => Value::Null,
    }
}

fn metric(glyph: Icon, title: &'static str, value: String, sub: String, battery: Option<(u8, bool)>, stale: bool) -> Div {
    ui::card()
        .flex_1()
        .min_w(px(0.))
        .gap(px(4.))
        .px(px(16.))
        .py(px(12.))
        .rounded(px(18.))
        .child(div().flex().items_center().gap(px(8.)).text_size(px(12.)).text_color(rgb(ui::TEXT2)).child(ui::icon(glyph, 18., rgb(ui::ACCENT))).child(title))
        .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).when(stale, |d| d.opacity(0.45)).child(value))
        .child(ui::hint(sub).text_size(px(12.)))
        .when_some(battery, |d, (level, low)| {
            d.child(
                div()
                    .mt(px(4.))
                    .h(px(4.))
                    .w_full()
                    .rounded_full()
                    .bg(alpha(0x767680, 0.12))
                    .child(div().h_full().w(gpui::relative(f32::from(level.min(100)) / 100.)).rounded_full().bg(rgb(if low { ui::ORANGE } else { ui::GREEN }))),
            )
        })
}

impl Mixer {
    fn mouse_action(&mut self, save: bool, cx: &mut Context<Self>) {
        if self.mouse.busy {
            return;
        }
        let payload = if save { self.mouse.changes() } else { Ok(json!({})) };
        let payload = match payload {
            Ok(payload) => payload,
            Err(error) => {
                self.mouse.notice = Some((false, error));
                cx.notify();
                return;
            }
        };
        self.mouse.busy = true;
        self.mouse.notice = None;
        if save {
            self.mouse.sent = Some((self.mouse.draft.clone(), self.mouse.dpi.clone()));
        }
        cx.notify();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let action = if save { "settings" } else { "refresh" };
            let state = client.mouse_command(action, &payload);
            (state, client.mouse_view())
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.mouse.busy = false;
                this.mouse.receive(view.ok());
                match result {
                    Ok(settings) => {
                        this.mouse.last = Some(settings.clone());
                        if let Some(view) = this.mouse.status.as_mut()
                            && view.device.status == "ready" {
                                view.state = Some(settings.clone());
                            }
                        if let Some((draft, dpi)) = this.mouse.sent.take() {
                            // ce qui a été envoyé est enregistré ; ce qui a changé entre-temps part au prochain envoi
                            this.mouse.draft.retain(|k, v| draft.get(k) != Some(v));
                            this.mouse.dpi.retain(|k, v| dpi.get(k) != Some(v));
                            this.mouse.basis = this.mouse.dirty().then(|| settings.clone());
                        }
                        this.mouse.notice = Some((true, if save { "Enregistré".into() } else { "Réglages relus".into() }));
                        this.mouse_fade_notice(cx);
                        if save && this.mouse.dirty() {
                            this.mouse_autosave(0, cx);
                        }
                    }
                    Err(error) => {
                        this.mouse.sent = None;
                        this.mouse.notice = Some((false, error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// En-tête de la page souris : chaque réglage s'enregistre seul, l'état s'affiche ici sans décaler la page.
    pub(crate) fn mouse_actions(&self, cx: &mut Context<Self>) -> Div {
        let m = &self.mouse;
        let (colour, text): (u32, Option<String>) = if m.busy || m.dirty() {
            (ui::TEXT3, Some("Enregistrement…".into()))
        } else {
            match &m.notice {
                Some((true, t)) => (ui::GREEN_TEXT, Some(t.clone())),
                Some((false, t)) => (ui::RED_TEXT, Some(t.clone())),
                None => (ui::TEXT3, None),
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().max_w(px(360.)).truncate().text_size(px(13.)).text_color(rgb(colour)).children(text))
            .child(ui::pill("r5-refresh", Some(Icon::Refresh), "Actualiser", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.mouse_action(false, cx))))
    }

    /// Enregistre après une courte pause : plusieurs clics rapides partent en un seul envoi.
    fn mouse_autosave(&mut self, delay_ms: u64, cx: &mut Context<Self>) {
        let seq = self.mouse.seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(delay_ms)).await;
            let _ = this.update(cx, |this, cx| {
                if this.mouse.seq == seq && this.mouse.dirty() && !this.mouse.busy && this.mouse.editable() {
                    this.mouse_action(true, cx);
                }
            });
        })
        .detach();
    }

    /// « Enregistré » s'efface seul après deux secondes.
    fn mouse_fade_notice(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.mouse.notice.as_ref().is_some_and(|n| n.0) {
                    this.mouse.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn dpi_input(&self, s: &Settings, i: usize, y: bool, editing: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = self.mouse.editable();
        let id = i * 2 + usize::from(y);
        let focus: FocusHandle = self.mouse_focus[id].clone();
        let current = self.mouse.dpi.get(&(i, y)).cloned().unwrap_or_else(|| dpi_at(s, i, y).unwrap_or(0).to_string());
        let draft = self.mouse.dpi.contains_key(&(i, y));
        ui::field(("dpi-input", id), &focus, &current, "—", editing, enabled)
            .flex_1()
            .h(px(34.))
            .when(draft, |d| d.text_color(rgb(ui::ORANGE_TEXT)).border_color(alpha(ui::ORANGE, 0.55)))
            .on_key_down(cx.listener(move |this, k: &KeyDownEvent, _, cx| {
                if !this.mouse.editable() {
                    return;
                }
                let key = &k.keystroke;
                if key.modifiers.control && key.key == "a" {
                    this.mouse.edit_dpi(i, y, String::new());
                } else if key.key == "backspace" || key.key == "delete" {
                    // Sans brouillon, Retour arrière part de la valeur affichée (1600 → 160).
                    let mut text = this.mouse.dpi.get(&(i, y)).cloned().unwrap_or_else(|| {
                        this.mouse.live().and_then(|s| dpi_at(s, i, y)).map(|v| v.to_string()).unwrap_or_default()
                    });
                    if key.key == "delete" {
                        text.clear();
                    } else {
                        text.pop();
                    }
                    this.mouse.edit_dpi(i, y, text);
                } else if key.key == "up" || key.key == "down" {
                    let Some(s) = this.mouse.live() else { return };
                    let base = this.mouse.dpi.get(&(i, y)).and_then(|v| v.parse::<u16>().ok()).or_else(|| dpi_at(s, i, y)).unwrap_or(50);
                    let delta = if key.key == "up" { 50 } else { -50 };
                    let value = (i32::from(base) + delta).clamp(50, i32::from(s.dpi_max));
                    this.mouse.edit_dpi(i, y, value.to_string());
                } else if let Some(c) = key.key_char.as_deref().filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_digit()) {
                    let mut text = this.mouse.dpi.get(&(i, y)).cloned().unwrap_or_default();
                    if text.len() < 5 {
                        text.push_str(c);
                        this.mouse.edit_dpi(i, y, text);
                    }
                } else {
                    return;
                }
                // DPI tapés au clavier : envoi après une pause de frappe
                this.mouse_autosave(1200, cx);
                cx.stop_propagation();
                cx.notify();
            }))
    }

    /// Liste déroulante d'un réglage de la souris (enregistré aussitôt choisi).
    fn mouse_select(&self, label: &'static str, key: &'static str, options: Vec<(Value, String)>, s: &Settings, cx: &mut Context<Self>) -> Div {
        let current = self.mouse.value(s, key);
        let text = options.iter().find(|(v, _)| *v == current).map(|(_, l)| l.clone()).unwrap_or_else(|| current.to_string());
        let values: Vec<Value> = options.iter().map(|(v, _)| v.clone()).collect();
        let items = options.into_iter().map(|(v, l)| (l, v == current)).collect();
        div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .min_w(px(0.))
            .child(ui::label(label))
            .child(self.dropdown(key, text, items, self.mouse.editable(), move |this, i, cx| {
                this.mouse.edit(key, values[i].clone());
                this.mouse_autosave(250, cx);
            }, cx))
    }

    pub fn mouse_view(&self, window: &gpui::Window, cx: &mut Context<Self>) -> Div {
        let m = &self.mouse;
        let mut content = div().flex().flex_col().gap(px(14.));
        let state = m.status.as_ref().map(|v| v.device.status.as_str());
        if m.live().is_some_and(|s| m.conflict(s)) {
            content = content.child(ui::notice(false, "Réglages modifiés ailleurs : annule le brouillon pour repartir de la lecture."));
        }
        let waiting = match state {
            Some("asleep") => Some(("La souris dort", "Bouge-la ou clique pour la réveiller ; ses réglages s’affichent aussitôt.")),
            Some("disconnected") => Some(("Souris introuvable", "Branche le récepteur 2,4 GHz ou le câble USB.")),
            Some("error" | "blocked") => Some(("Lecture impossible", "Rustudio réessaie tout seul.")),
            Some("ready") => None,
            _ => Some(("En attente de la souris…", "Les réglages apparaissent après la première lecture.")),
        };
        if let Some((title, text)) = waiting {
            content = content.child(ui::card().items_center().py(px(28.)).child(ui::chip(Icon::Mouse, ui::ACCENT, 52.)).child(ui::title(title, 17.)).child(ui::hint(text)));
        }
        let Some(s) = m.live().or(m.last.as_ref()) else {
            return content;
        };
        let stale = m.live().is_none();
        let sub = |text: String| if stale { "Dernière valeur lue".to_owned() } else { text };
        let stage = s.stages.get(s.active_stage.saturating_sub(1));
        let sens = stage.map(|d| if d.x == d.y { d.x.to_string() } else { format!("{} / {}", d.x, d.y) }).unwrap_or_else(|| "—".into());
        let battery = if s.charging { "En charge" } else if s.battery <= 20 { "Batterie faible" } else if s.wired { "Alimentée en USB" } else { "Sur batterie" };
        content = content.child(
            div()
                .flex()
                .gap(px(14.))
                .child(metric(Icon::Battery, "Batterie", format!("{} %", s.battery), sub(battery.into()), Some((s.battery, s.battery <= 20)), stale))
                .child(metric(Icon::Mouse, "Sensibilité active", sens, sub(format!("DPI · palier {} / {}", s.active_stage, s.stages.len())), None, stale))
                .child(metric(Icon::Activity, "Fréquence", format!("{} Hz", group(s.polling_hz)), sub("Enregistrée sur la souris".into()), None, stale))
                .child(metric(Icon::Radio, "Connexion", if s.wired { "USB".into() } else { "2,4 GHz".into() }, sub(format!("Profil {} · FW {}", s.profile, s.firmware)), None, stale)),
        );
        let active = m.value(s, "active_stage").as_u64().unwrap_or(s.active_stage as u64) as usize;
        let mut dpi = ui::card()
            .flex_1()
            .min_w(px(0.))
            .child(ui::card_title(Some((Icon::Mouse, ui::ACCENT)), "Sensibilité", Some("Paliers X / Y · pas de 50".into())))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(13.5)).child("Palier actif"))
                    .child(div().w(px(160.)).child(self.dropdown(
                        "active_stage",
                        format!("Palier {active}"),
                        (1..=s.stages.len()).map(|i| (format!("Palier {i}"), i == active)).collect(),
                        m.editable(),
                        |this, i, cx| {
                            this.mouse.edit("active_stage", json!(i + 1));
                            this.mouse_autosave(250, cx);
                        },
                        cx,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .px(px(6.))
                    .text_size(px(11.))
                    .text_color(rgb(ui::TEXT2))
                    .child(div().w(px(26.)).child("N°"))
                    .child(div().flex_1().child("Axe X · DPI"))
                    .child(div().flex_1().child("Axe Y · DPI")),
            );
        let mut rows = div().flex().flex_col().gap(px(4.));
        for i in 0..s.stages.len() {
            let on = i + 1 == active;
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(if on { alpha(ui::ACCENT, 0.2) } else { alpha(ui::ACCENT, 0.) })
                    .when(on, |d| d.bg(alpha(ui::ACCENT, 0.065)))
                    .child(div().w(px(26.)).flex().justify_center().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(if on { ui::ACCENT } else { ui::TEXT2 })).child((i + 1).to_string()))
                    .child(self.dpi_input(s, i, false, self.mouse_focus[i * 2].is_focused(window), cx))
                    .child(self.dpi_input(s, i, true, self.mouse_focus[i * 2 + 1].is_focused(window), cx)),
            );
        }
        dpi = dpi.child(rows).child(ui::hint("Tape une valeur ou utilise ↑ ↓ (pas de 50) : c’est enregistré tout seul.").text_size(px(12.)));
        let rates: Vec<(Value, String)> = RATES.into_iter().filter(|&n| !s.wired || n <= 1000).map(|n| (json!(n), format!("{} Hz", group(n)))).collect();
        let mut sensor = ui::card()
            .flex_1()
            .min_w(px(0.))
            .child(ui::card_title(Some((Icon::Settings, ui::ACCENT)), "Capteur et autonomie", None))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(div().flex_1().min_w(px(0.)).child(self.mouse_select("Fréquence", "polling_hz", rates, s, cx)))
                    .child(div().flex_1().min_w(px(0.)).child(self.mouse_select(
                        "Hauteur de décrochage",
                        "lod_mm",
                        vec![(json!(0.7), "0,7 mm".into()), (json!(1.0), "1 mm".into()), (json!(2.0), "2 mm".into())],
                        s,
                        cx,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(div().flex_1().min_w(px(0.)).child(self.mouse_select("Anti-double clic", "debounce_ms", (0..=30).map(|i| (json!(i), format!("{i} ms"))).collect(), s, cx)))
                    .child(div().flex_1().min_w(px(0.)).child(self.mouse_select(
                        "Mise en veille",
                        "sleep_seconds",
                        [(30, "30 secondes"), (60, "1 minute"), (120, "2 minutes"), (300, "5 minutes"), (600, "10 minutes"), (65535, "Jamais")]
                            .into_iter()
                            .map(|(n, l)| (json!(n), l.into()))
                            .collect(),
                        s,
                        cx,
                    ))),
            );
        let mut toggles = div().flex().flex_col().mt(px(4.));
        for (key, label, glyph) in [("angle_snap", "Correction des lignes", Icon::Route), ("motion_sync", "Synchronisation du capteur", Icon::Refresh), ("ripple_control", "Lissage des mouvements", Icon::Activity)] {
            let on = m.value(s, key) == true;
            let editable = m.editable();
            toggles = toggles.child(ui::toggle_row(Some(glyph), label, ui::toggle(SharedString::from(format!("r5-{key}")), on, editable).on_click(cx.listener(move |this, _, _, cx| {
                if editable {
                    this.mouse.edit(key, json!(!on));
                    this.mouse_autosave(250, cx);
                    cx.notify();
                }
            }))));
        }
        sensor = sensor.child(toggles);
        content
            .child(div().flex().items_start().gap(px(16.)).child(dpi).child(sensor))
            .child(ui::caption("USB jusqu’à 1 000 Hz · récepteur 2,4 GHz jusqu’à 8 000 Hz. Bluetooth, macros, remappage et firmware ne sont pas pris en charge."))
    }
}

/// 1000 → « 1 000 » (espace fine des milliers).
fn group(n: u32) -> String {
    if n >= 1000 { format!("{} {:03}", n / 1000, n % 1000) } else { n.to_string() }
}

pub fn verify(client: &Client) -> Result<(), String> {
    let original = client
        .mouse_view()?
        .state
        .ok_or("Réglages de démo absents")?;
    if original.battery != 84 || original.stages.len() != 3 || original.wired {
        return Err("Le test exige les données de démonstration de la R5 Ultra.".into());
    }
    let mut page = MousePage::default();
    page.receive(Some(client.mouse_view()?));
    page.edit_dpi(0, false, "850".into());
    page.edit_dpi(0, true, "950".into());
    page.edit("polling_hz", json!(2000));
    page.edit("active_stage", json!(1));
    page.edit("lod_mm", json!(0.7));
    page.edit("debounce_ms", json!(7));
    page.edit("sleep_seconds", json!(60));
    page.edit("angle_snap", json!(true));
    page.edit("motion_sync", json!(false));
    page.edit("ripple_control", json!(true));
    if client.mouse_view()?.state != Some(original.clone()) {
        return Err("Le brouillon a modifié la souris.".into());
    }
    let result = client.mouse_command("settings", &page.changes()?)?;
    if result.stages[0] != (Dpi { x: 850, y: 950 })
        || result.polling_hz != 2000
        || result.active_stage != 1
        || result.lod_mm != 0.7
        || result.debounce_ms != 7
        || result.sleep_seconds != 60
        || !result.angle_snap
        || result.motion_sync
        || !result.ripple_control
        || result.battery != 84
        || client.mouse_view()?.state != Some(result)
    {
        return Err("La relecture des réglages ne correspond pas.".into());
    }
    println!("OK : batterie, DPI X/Y, palier, fréquence, capteur et relecture R5 Ultra.");
    Ok(())
}
