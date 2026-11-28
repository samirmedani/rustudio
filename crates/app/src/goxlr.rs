//! Les écrans du module GoXLR utilisent le même contrat local que l'interface distribuée.
use crate::{
    backend::Client,
    desktop::{Page, hint},
    mic_filters::{MicTab, mic_signal},
    mic_wizard::Step,
    mixer::Mixer,
};
use crate::mixer::fmt_num;
use crate::ui::{self, Icon, alpha};
use crate::ui::rgb;
use gpui::{AnyElement, ClipboardItem, Context, Div, FontWeight, IntoElement, KeyDownEvent, MouseButton, SharedString, Stateful, div, prelude::*, px};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
use std::time::{Duration, Instant};

const MODULE: &str = "/api/modules/goxlr-mini";
const MONITOR_START: u32 = 12;

fn monitor_level(percent: f64) -> u32 {
    (percent.clamp(1., 100.) * 255. / 100.).round() as u32
}

fn monitor_percent(level: u32) -> f64 {
    (f64::from(level) * 100. / 255.).round()
}

enum FeedbackEvent {
    Suspected,
    Unavailable,
}

pub const ACTIONS: &[&str] = &[
    "route",
    "settings",
    "scene",
    "scene-save",
    "scene-delete",
    "auto-scenes",
    "theme",
    "lighting",
    "colour",
    "mic",
    "mic-model",
    "mic-preset",
    "mic-profile",
    "volume",
    "mute",
    "dsp",
    "fx",
    "detect-mic",
    "auto-gain",
    "voice-setup",
    "voice-forget",
    "job-cancel",
    "mic-listen",
    "mic-ab",
    "hp-eq",
    "hp-eq-save",
    "hp-eq-delete",
    "hp-eq-setup",
    "hp-eq-remove",
    "hp-eq-uninstall",
    "mic-ns",
    "mic-ns-setup",
    "mic-ns-remove",
    "echo-check",
    "reconnect",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Routing,
    Scenes,
    Light,
    Microphone,
    Headphones,
    Diagnostic,
}
impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Routing => "Routage",
            Self::Scenes => "Scènes",
            Self::Light => "Éclairage",
            Self::Microphone => "Micro",
            Self::Headphones => "Casque",
            Self::Diagnostic => "Diagnostic",
        }
    }
}

/// Question posée avant une action GoXLR : titre, explication, bouton de confirmation.
#[derive(Clone)]
pub(crate) struct Ask {
    title: String,
    text: Vec<&'static str>,
    label: &'static str,
    danger: bool,
}

impl Ask {
    fn new(title: impl Into<String>, label: &'static str) -> Ask {
        Ask { title: title.into(), text: vec![], label, danger: false }
    }
    fn text(mut self, paragraph: &'static str) -> Ask {
        self.text.push(paragraph);
        self
    }
    fn danger(mut self) -> Ask {
        self.danger = true;
        self
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GoView {
    pub device: crate::backend::Device,
    pub state: Value,
    pub meta: Value,
    #[serde(default)]
    job: Value,
    /// État de l'égaliseur du casque côté Windows (sortie par défaut suivie…)
    #[serde(default)]
    hp_eq: Value,
    /// Voix brute en cours d'écoute (comparaison avant / après traitement)
    #[serde(default)]
    pub(crate) ab: bool,
    /// écoute temporaire de ta voix dans le casque en cours
    #[serde(default)]
    pub(crate) listening: bool,
}
#[derive(Default)]
pub struct GoPage {
    pub view: Option<GoView>,
    pub(crate) busy: bool,
    pub(crate) notice: Option<(bool, String)>,
    pub(crate) voice_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) job: Value,
    preset_in_flight: Option<String>,
    preset_pending: Option<Value>,
    /// intensité du style : valeur en attente, envoi en cours
    pub(crate) intensity_pending: Option<i64>,
    pub(crate) intensity_sending: bool,
    monitor: Option<Arc<AtomicU32>>,
    monitor_stopping: bool,
    monitor_notice: Option<&'static str>,
    mic_connection_open: bool,
    /// Niveau d'entrée du micro en dB, lu seulement page Micro ouverte.
    pub(crate) level: Option<f64>,
    /// Niveau affiché par le vumètre : il suit `level` en douceur.
    pub(crate) level_shown: Option<f64>,
    /// Réduction en direct des 4 traitements du firmware Rustudio (dB) : gate, compresseur, de-esser, limiteur.
    pub(crate) voice_meters: Option<[f64; 4]>,
    /// Annulation proposée tant que le compresseur relu correspond au réglage doux envoyé.
    pub(crate) softer_compressor_undo: Option<(rustudio_core::dsp::Comp, rustudio_core::dsp::Comp)>,
    level_tick: Option<std::time::Instant>,
    confirm: Option<(&'static str, Value, Ask)>,
    editing: Option<usize>,
    name: String,
    app_name: String,
    auto_scene: Option<String>,
    colour: String,
    diagnostics: Option<Value>,
    diagnostic_loading: bool,
    diagnostic_error: Option<String>,
    diagnostic_details: ui::Disclosure,
    sound_test: Option<Value>,
    /// Latence en direct (page Diagnostic ouverte) : réponse USB et estimation de Windows.
    pub(crate) latency: Option<Value>,
    latency_test: Option<Value>,
    latency_running: bool,
}
impl Drop for GoPage {
    fn drop(&mut self) {
        self.end_monitor();
        if let Some(cancel) = self.voice_cancel.as_ref() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl GoPage {
    pub(crate) fn end_monitor(&mut self) {
        if let Some(level) = self.monitor.take() {
            level.store(0, Ordering::SeqCst);
        }
    }

    pub(crate) fn owns_monitor(&self) -> bool { self.monitor.is_some() }

    pub fn receive(&mut self, view: Option<GoView>) {
        if let Some(v) = view.as_ref() {
            self.job = v.job.clone();
        }
        if !self.busy {
            if let (Some(v), Some((before, after))) = (&view, &self.softer_compressor_undo)
                && v.state["dsp"]["comp"] != json!(before)
                && v.state["dsp"]["comp"] != json!(after)
            {
                self.softer_compressor_undo = None;
            }
            self.view = view;
        }
    }
    pub fn clear_confirm(&mut self) {
        self.confirm = None;
    }
    /// La page reste vive pendant l'envoi d'un réglage (pas de grisé à chaque clic) : go_send ignore
    /// simplement un second envoi tant que le premier n'a pas répondu.
    pub(crate) fn enabled(&self) -> bool {
        self
                .view
                .as_ref()
                .is_some_and(|v| v.device.status == "ready")
    }

    /// Le vumètre est en train de rejoindre la dernière mesure.
    pub(crate) fn level_moving(&self) -> bool {
        self.level_tick.is_some()
    }

    pub(crate) fn clear_level_display(&mut self) {
        self.level_shown = None;
        self.level_tick = None;
    }

    /// Une image du vumètre : l'affichage monte vite vers le niveau mesuré et redescend doucement, comme le
    /// vumètre d'une console. Renvoie vrai tant qu'il bouge encore.
    pub(crate) fn step_level(&mut self) -> bool {
        let Some(target) = self.level else {
            self.clear_level_display();
            return false;
        };
        let now = std::time::Instant::now();
        let elapsed = self.level_tick.map_or(0., |at| now.duration_since(at).as_secs_f64().min(0.1));
        let shown = self.level_shown.unwrap_or(LEVEL_FLOOR);
        let ease = if target > shown { LEVEL_RISE_S } else { LEVEL_FALL_S };
        let next = shown + (target - shown) * (1. - (-elapsed / ease).exp());
        let settled = (target - next).abs() < 0.05;
        self.level_shown = Some(if settled { target } else { next });
        self.level_tick = (!settled).then_some(now);
        !settled
    }
}

impl Client {
    pub fn go_view(&self) -> Result<GoView, String> {
        let mut view: GoView = serde_json::from_value(self.get(&format!("{MODULE}/state"))?)
            .map_err(|_| "État GoXLR illisible")?;
        if view.device.simulated == self.is_live() {
            return Err("Mode du moteur incompatible.".into());
        }
        view.job = self
            .live_state()
            .map(|v| v["job"].clone())
            .unwrap_or_default();
        Ok(view)
    }
    pub(crate) fn go_command(&self, action: &str, params: Value) -> Result<Value, String> {
        if !ACTIONS.contains(&action) {
            return Err("Commande GoXLR non autorisée.".into());
        }
        self.live_state()?;
        self.post_module(MODULE, action, params)
    }
    fn diagnostics(&self) -> Result<Value, String> {
        self.live_state()?;
        Ok(
            json!({"support":self.get("/api/support")?,"usb":self.get("/api/usb")?,
            "glitches":self.get("/api/glitches")?,"live":self.get("/api/live")?["job"],
            "echo":self.go_command("echo-check",json!({}))?,
            // seulement sous firmware Rustudio ; ailleurs le moteur répond par une erreur, ignorée
            "firmware":self.get("/api/firmware").ok()}),
        )
    }
}
pub(crate) fn entries(v: &Value) -> impl Iterator<Item = (&String, &Value)> {
    v.as_object().into_iter().flat_map(|v| v.iter())
}
fn labels(v: &Value) -> impl Iterator<Item = &str> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str)
}
fn light_label(v: &str) -> &str {
    match v {
        "two-colour" => "Deux couleurs",
        "gradient" => "Dégradé",
        "meter" => "Vumètre",
        "gradient-meter" => "Dégradé et vumètre",
        "none" => "Aucune",
        "retro-rainbow" => "Arc-en-ciel rétro",
        "rainbow-dark" => "Arc-en-ciel sombre",
        "rainbow-bright" => "Arc-en-ciel vif",
        "simple" => "Simple",
        "ripple" => "Vague",
        _ => v,
    }
}
/// Emplacement d'une couleur : « Fader1Mute » → « Fader 1 · coupé ».
fn colour_target(id: &str) -> String {
    if let Some(n) = id.strip_prefix("Fader").and_then(|r| r.strip_suffix("Mute")) {
        return format!("Fader {n} · bouton couper");
    }
    if let Some(n) = id.strip_prefix("FadeMeter") {
        return format!("Fader {n} · vumètre");
    }
    match id {
        "Global" => "Ensemble de l’appareil".into(),
        "Accent" => "Accent".into(),
        "Bleep" => "Bouton Bleep".into(),
        "MicrophoneMute" | "Cough" => "Bouton du micro".into(),
        other => other.into(),
    }
}

impl Mixer {
    /// Intensité du style : la dernière valeur part dès que la précédente est confirmée, sans jamais en perdre
    /// (glissement fluide au pour cent près, et la valeur finale arrive toujours).
    pub(crate) fn send_intensity(&mut self, value: i64, cx: &mut Context<Self>) {
        self.go.intensity_pending = Some(value);
        if !self.go.intensity_sending {
            self.go.intensity_sending = true;
            self.pump_intensity(cx);
        }
    }

    fn pump_intensity(&mut self, cx: &mut Context<Self>) {
        let Some(value) = self.go.intensity_pending.take() else {
            self.go.intensity_sending = false;
            return;
        };
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let result = client.go_command("hp-eq", json!({"intensity": value}));
            (result, client.go_view().ok())
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.go.intensity_pending.is_none() {
                    this.go.receive(view);
                }
                if let Err(e) = result {
                    this.go.notice = Some((false, e));
                }
                this.pump_intensity(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn go_send(&mut self, action: &'static str, data: Value, cx: &mut Context<Self>) {
        // arrêter une mesure passe toujours, même pendant un autre envoi
        if action == "job-cancel" {
            let client = self.client.clone();
            cx.background_executor()
                .spawn(async move {
                    let _ = client.go_command("job-cancel", json!({}));
                })
                .detach();
            return;
        }
        if self.go.busy {
            if action == "mic-preset" && self.go.enabled() {
                self.go.preset_pending = Some(data);
                cx.notify();
            }
            return;
        }
        if !self.go.enabled() && !matches!(action, "reconnect" | "job-cancel") {
            return;
        }
        self.go.busy = true;
        if action == "mic-preset" {
            self.go.preset_in_flight = data["name"].as_str().map(str::to_owned);
        }
        self.go.notice = None;
        let client = self.client.clone();
        let diagnostic = self.page == Page::Go(Section::Diagnostic);
        let work = cx.background_executor().spawn(async move {
            let result = client.go_command(action, data);
            let view = client.go_view().ok();
            let details = if diagnostic && result.is_ok() {
                client.diagnostics().ok()
            } else {
                None
            };
            (result, view, details)
        });
        cx.spawn(async move |this, cx| {
            let (result, view, details) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.busy = false;
                this.go.receive(view);
                if let Some(details) = details {
                    this.go.diagnostics = Some(details);
                }
                // un réglage qui passe se voit sur la page : seul un échec mérite un message
                this.go.notice = result.err().map(|error| (false, error));
                this.go.preset_in_flight = None;
                this.resume_mic_preset(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn resume_mic_preset(&mut self, cx: &mut Context<Self>) {
        if let Some(data) = self.go.preset_pending.take() {
            if self.go.enabled() {
                self.go_send("mic-preset", data, cx);
            } else if self.go.notice.is_none() {
                self.go.notice = Some((false, "Impossible d’appliquer le préréglage. La GoXLR ne répond pas.".into()));
            }
        }
    }

    /// Coupe le bail temporaire et, s’il alimente le casque, le retour réglé à la main.
    pub(crate) fn cut_monitor(&mut self, cx: &mut Context<Self>) {
        self.go.end_monitor();
        if self.go.monitor_stopping { return; }
        self.go.monitor_stopping = true;
        let manual = self.manual_monitor_on();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let mut result = client.stop_mic_listen().map(|_| ());
            if manual {
                let cut = client.go_command("volume", json!({"channel":"MicMonitor","value":0})).map(|_| ());
                result = result.and(cut);
            }
            (result, client.go_view().ok())
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.monitor_stopping = false;
                this.go.receive(view);
                this.go.notice = result.err().map(|e| (false, e));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn start_monitor(&mut self, cx: &mut Context<Self>) {
        if !self.go.enabled() || self.go.monitor_stopping || self.monitor_on() || self.voice.preview_busy() { return; }
        let level = Arc::new(AtomicU32::new(MONITOR_START));
        self.go.monitor = Some(level.clone());
        self.go.monitor_notice = None;
        self.go.notice = None;
        let client = self.client.clone();
        let (feedback, events) = async_channel::bounded(1);
        if client.is_live() {
            let requested = level.clone();
            std::thread::spawn(move || {
                if rustudio_windows_desktop::soundcheck::monitor_feedback(requested, {
                    let feedback = feedback.clone();
                    move || { let _ = feedback.try_send(FeedbackEvent::Suspected); }
                }).is_err() {
                    let _ = feedback.try_send(FeedbackEvent::Unavailable);
                }
            });
        }
        cx.spawn(async move |this, cx| {
            let mut sent = 0;
            let mut refreshed = Instant::now();
            loop {
                if let Ok(event) = events.try_recv() {
                    let _ = this.update(cx, |this, cx| {
                        if !this.go.monitor.as_ref().is_some_and(|current| Arc::ptr_eq(current, &level)) {
                            return;
                        }
                        match event {
                            FeedbackEvent::Suspected if level.load(Ordering::SeqCst) > MONITOR_START => {
                                // Un curseur encore tenu ne doit pas rétablir le niveau juste après la réduction.
                                this.param_drag = None;
                                this.param_held.remove("param-mic-quick-monitor");
                                this.param_held.remove("param-wizard-live-monitor");
                                level.store(MONITOR_START, Ordering::SeqCst);
                                this.go.monitor_notice = Some("Larsen possible. Retour ramené à 5 %.");
                            }
                            FeedbackEvent::Suspected => {
                                this.go.monitor_notice = Some("Larsen possible. Retour coupé.");
                                this.cut_monitor(cx);
                            }
                            FeedbackEvent::Unavailable => {
                                this.go.monitor_notice = Some("Protection anti-Larsen indisponible sur cette entrée.");
                            }
                        }
                        cx.notify();
                    });
                }
                let desired = level.load(Ordering::SeqCst);
                if desired == 0 { break; }
                if desired != sent || refreshed.elapsed() >= Duration::from_secs(2) {
                    let client = client.clone();
                    let requested = level.clone();
                    let starting = sent == 0;
                    let result = cx.background_executor().spawn(async move { client.renew_mic_listen(&requested, starting) }).await;
                    match result {
                        Ok(Some(actual)) => {
                            sent = actual;
                            refreshed = Instant::now();
                        }
                        stopped => {
                            let _ = this.update(cx, |this, cx| {
                                if this.go.monitor.as_ref().is_some_and(|current| Arc::ptr_eq(current, &level)) {
                                    this.go.end_monitor();
                                    this.go.notice = stopped.err().map(|error| (false, error));
                                    cx.notify();
                                }
                            });
                            break;
                        }
                    }
                }
                cx.background_executor().timer(Duration::from_millis(120)).await;
            }
        }).detach();
        cx.notify();
    }

    pub(crate) fn monitor_controls(&self, id: &'static str, enabled: bool, cx: &mut Context<Self>) -> Div {
        let on = self.monitor_on();
        let can_start = enabled && !self.go.monitor_stopping && !self.voice.preview_busy();
        let toggle = ui::pill((id, 0usize), Some(if on { Icon::VolumeOff } else { Icon::Headphones }),
            if on { "Couper le retour" } else { "M’écouter en direct" },
            if on { ui::Tone::Warn } else { ui::Tone::Plain })
            .when(!on && !can_start, |d| d.opacity(0.45))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.monitor_on() { this.cut_monitor(cx); }
                else if can_start { this.start_monitor(cx); }
            }));
        let mut controls = div().flex().items_center().gap(px(8.)).child(toggle);
        if let Some(level) = &self.go.monitor {
            let current = monitor_percent(level.load(Ordering::SeqCst));
            let requested = level.clone();
            controls = controls.child(div().w(px(170.)).child(self.slider_row(
                id, "Niveau du retour", current, (1., 100.), 1.,
                |value| format!("{value:.0} %"),
                true,
                move |this, value, cx| {
                    if this.go.monitor.as_ref().is_some_and(|active| Arc::ptr_eq(active, &requested)) {
                        requested.store(monitor_level(value), Ordering::SeqCst);
                        cx.notify();
                    }
                },
                cx,
            )));
        }
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(controls)
            .when_some(self.go.monitor_notice, |d, notice| {
                d.child(ui::status(notice, ui::State::Warn).max_w(px(350.)))
            })
    }

    /// Le casque peut aussi recevoir le micro par son routage et son volume manuels.
    pub(crate) fn monitor_on(&self) -> bool {
        self.go.owns_monitor() || self.go.view.as_ref().is_some_and(|v| v.listening) || self.manual_monitor_on()
    }

    fn manual_monitor_on(&self) -> bool {
        self.go.view.as_ref().is_some_and(|v| {
            v.state["volumes"]["MicMonitor"].as_i64().unwrap_or(0) > 0
                && v.state["routing"]["Microphone"]["Headphones"] == true
        })
    }

    pub(crate) fn refresh_diag(&mut self, cx: &mut Context<Self>) {
        if self.go.diagnostic_loading {
            return;
        }
        self.go.diagnostic_loading = true;
        self.go.diagnostic_error = None;
        let client = self.client.clone();
        let job = cx
            .background_executor()
            .spawn(async move { client.diagnostics() });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.go.diagnostic_loading = false;
                match result {
                    Ok(data) => this.go.diagnostics = Some(data),
                    Err(err) => this.go.diagnostic_error = Some(err),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn go_text(&self, key: &'static str, placeholder: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let index = match key {
            "app" => 1,
            "colour" => 2,
            _ => 0,
        };
        let current = match key {
            "app" => &self.go.app_name,
            "colour" => &self.go.colour,
            _ => &self.go.name,
        };
        ui::field(("go-text", index), &self.go_focus[index], current, placeholder, self.go.editing == Some(index), true)
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.go.editing = Some(index);
                cx.notify();
            }))
            .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                if this.go.editing == Some(index) {
                    this.go.editing = None;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let field = match key {
                    "app" => &mut this.go.app_name,
                    "colour" => &mut this.go.colour,
                    _ => &mut this.go.name,
                };
                if let ui::Edit::Ignored = ui::edit_text(field, event, 60, |_| true, cx) {
                    return;
                }
                cx.stop_propagation();
                cx.notify();
            }))
    }

    fn act(&self, el: Stateful<Div>, action: &'static str, body: Value, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = self.go.enabled() || matches!(action, "reconnect" | "job-cancel");
        el.when(!enabled, |d| d.opacity(0.45)).on_click(cx.listener(move |this, _, _, cx| {
            if enabled {
                this.go_send(action, body.clone(), cx)
            }
        }))
    }

    fn ask_go(&self, el: Stateful<Div>, action: &'static str, body: Value, ask: Ask, cx: &mut Context<Self>) -> Stateful<Div> {
        el.when(!self.go.enabled(), |d| d.opacity(0.45)).on_click(cx.listener(move |this, _, _, cx| {
            if this.go.enabled() {
                this.go.confirm = Some((action, body.clone(), ask.clone()));
                cx.notify();
            }
        }))
    }

    fn routing(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        // Firmware Rustudio : le micro et les cinq sorties du PC, vers le casque, le micro de Windows et la sortie ligne.
        let firmware = self.rustudio_firmware();
        let outputs: Vec<&str> = labels(&v.meta["outputs"]).filter(|o| !firmware || matches!(*o, "Headphones" | "ChatMic" | "LineOut")).collect();
        let enabled = self.go.enabled();
        let vod = v.state["vod"]["mode"] == "no-music";
        let mut head = div().flex().items_end().pb(px(10.)).child(div().w(px(190.)));
        for &o in &outputs {
            let (name, sub, glyph) = output_info(o);
            head = head.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .child(ui::icon(glyph, 18., rgb(ui::ACCENT)))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(name))
                    .child(div().text_size(px(11.)).text_color(rgb(ui::TEXT3)).text_center().child(sub)),
            );
        }
        let mut matrix = ui::section(Icon::Route, ui::ACCENT, "Qui entend quoi", Some("Clique une case pour envoyer ou couper".into())).child(head);
        for (n, input) in labels(&v.meta["inputs"]).filter(|i| !firmware || !matches!(*i, "LineIn" | "Console")).enumerate() {
            let (name, sub, glyph) = input_info(input);
            let mut row = div()
                .flex()
                .items_center()
                .py(px(8.))
                .border_t_1()
                .border_color(alpha(ui::INK, if n == 0 { 0.1 } else { 0.06 }))
                .child(
                    div()
                        .w(px(190.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::chip(glyph, ui::ACCENT, 30.))
                        .child(div().flex().flex_col().child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child(name)).child(div().text_size(px(11.5)).text_color(rgb(ui::TEXT2)).child(sub))),
                );
            for &output in &outputs {
                let on = v.state["routing"][input][output] == true;
                let automatic = vod && v.meta["vod_output"] == output && input == "Music";
                // firmware Rustudio : le micro de Windows ne reçoit que la voix, pas les sons du PC
                let cell = if firmware && output == "ChatMic" && input != "Microphone" {
                    ui::caption("—").into_any_element()
                } else if automatic {
                    ui::badge("Auto", ui::ORANGE).into_any_element()
                } else {
                    self.act(ui::check_cell(SharedString::from(format!("route-{input}-{output}")), on, enabled), "route", json!({"input":input,"output":output,"enabled":!on}), cx).into_any_element()
                };
                row = row.child(div().flex_1().min_w(px(0.)).flex().justify_center().child(cell));
            }
            matrix = matrix.child(row);
        }
        let vod_card = ui::section(Icon::Music, ui::ORANGE, "Stream sans musique", None).child(ui::list_row(
            None,
            "Garder la musique hors des rediffusions",
            Some("La musique reste dans ton casque et dans le direct, mais pas dans l’enregistrement du Sampler (anti-DMCA).".into()),
            self.act(ui::toggle("vod-toggle", vod, enabled), "settings", json!({"vod":{"mode":if vod {"off"} else {"no-music"}}}), cx),
            true,
        ));
        div().flex().flex_col().gap(px(16.)).child(matrix).when(!firmware, |d| d.child(vod_card))
    }

    fn scenes(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let enabled = self.go.enabled();
        let active = v.state["scene"].as_str().unwrap_or("").to_owned();
        let mut tiles: Vec<AnyElement> = vec![];
        let mut names: Vec<(String, String)> = vec![];
        for (id, s) in entries(&v.meta["scenes"]) {
            let label = s["label"].as_str().unwrap_or(id).to_owned();
            names.push((id.clone(), label.clone()));
            let tile = ui::tile(SharedString::from(format!("scene-{id}")), Some((scene_icon(id), ui::ACCENT)), label, s["desc"].as_str().map(|d| d.to_owned().into()), active == *id, None);
            tiles.push(self.act(tile, "scene", json!({"name":id}), cx).into_any_element());
        }
        for (id, _) in entries(&v.state["user_scenes"]) {
            names.push((id.clone(), id.clone()));
            let name = id.clone();
            let t = ui::hover_t(&format!("scene-user-{id}"));
            let tile = ui::tile(SharedString::from(format!("scene-user-{id}")), Some((Icon::Bookmark, ui::GREEN)), id.clone(), Some("Ta scène enregistrée".into()), active == *id, None).child(
                div().flex().justify_end().opacity(t).child(
                    ui::round(SharedString::from(format!("scene-del-{id}")), Icon::Trash, 28., alpha(ui::RED, 0.1), rgb(ui::RED_TEXT)).on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.go.confirm = Some(("scene-delete", json!({"name": name.clone()}), Ask::new("Supprimer cette scène personnelle ?", "Supprimer").danger()));
                        cx.notify();
                    })),
                ),
            );
            tiles.push(self.act(tile, "scene", json!({"name":id}), cx).into_any_element());
        }
        let save = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.))
            .child(self.go_text("name", "Nom de ta scène", cx).flex_auto().min_w(px(156.)))
            .child(self.act(ui::pill("scene-save", Some(Icon::Plus), "Enregistrer l’état actuel", ui::Tone::Primary), "scene-save", json!({"name":self.go.name}), cx));
        let list = ui::section(Icon::Layers, ui::ACCENT, "Scènes", Some("Un clic applique faders, routage et micro".into())).child(ui::grid(tiles, 3)).child(save);
        let auto = &v.state["auto_scenes"];
        let on = auto["enabled"] == true;
        let rules = auto["rules"].as_array().cloned().unwrap_or_default();
        let fallback = auto["fallback"].clone();
        let label_of = |id: &str| names.iter().find(|(n, _)| n == id).map(|(_, l)| l.clone()).unwrap_or_else(|| id.to_owned());
        let app_label = |exe: &str| v.meta["known_apps"][exe].as_str().map(str::to_owned).unwrap_or_else(|| exe.to_owned());
        let mut automation = ui::section(Icon::Sparkles, 0x5856d6, "Scènes automatiques", None).child(ui::list_row(
            None,
            "Changer de scène selon l’appli ouverte",
            Some("Exemple : OBS s’ouvre, la scène Streaming s’applique toute seule.".into()),
            self.act(ui::toggle("auto-toggle", on, enabled), "auto-scenes", json!({"enabled":!on,"rules":rules,"fallback":fallback}), cx),
            true,
        ));
        for (i, rule) in rules.iter().enumerate() {
            let mut remaining = rules.clone();
            remaining.remove(i);
            automation = automation.child(ui::list_row(
                Some((Icon::ChevronRight, ui::TEXT3)),
                format!("{} → {}", app_label(rule["app"].as_str().unwrap_or("")), label_of(rule["scene"].as_str().unwrap_or(""))),
                None,
                self.act(ui::round(("auto-del", i), Icon::Trash, 30., alpha(0x767680, 0.), rgb(ui::TEXT2)), "auto-scenes", json!({"enabled":on,"rules":remaining,"fallback":fallback}), cx),
                false,
            ));
        }
        let apps: Vec<(String, String)> = entries(&v.meta["known_apps"]).map(|(exe, l)| (exe.clone(), l.as_str().unwrap_or(exe).to_owned())).collect();
        let picked_app = self.go.app_name.trim().to_owned();
        let app_text = if picked_app.is_empty() { "Choisir une appli".to_owned() } else { app_label(&picked_app) };
        let scene_text = self.go.auto_scene.as_deref().map(&label_of).unwrap_or_else(|| "Choisir une scène".into());
        let apps_for_pick = apps.clone();
        let names_for_pick = names.clone();
        let mut add = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().flex_1().child(self.dropdown("auto-app", app_text, apps.iter().map(|(e, l)| (l.clone(), *e == picked_app)).collect(), enabled, move |this, i, _| this.go.app_name = apps_for_pick[i].0.clone(), cx)))
            .child(ui::icon(Icon::ChevronRight, 16., rgb(ui::TEXT3)))
            .child(div().flex_1().child(self.dropdown("auto-scene", scene_text, names.iter().map(|(n, l)| (l.clone(), self.go.auto_scene.as_deref() == Some(n.as_str()))).collect(), enabled, move |this, i, _| this.go.auto_scene = Some(names_for_pick[i].0.clone()), cx)));
        if let Some(scene) = self.go.auto_scene.as_ref().filter(|_| !picked_app.is_empty()) {
            let mut next = rules.clone();
            next.push(json!({"app":picked_app,"scene":scene}));
            add = add.child(self.act(ui::pill("auto-add", Some(Icon::Plus), "Ajouter", ui::Tone::Primary), "auto-scenes", json!({"enabled":on,"rules":next,"fallback":fallback}), cx));
        }
        let fallback_names = names.clone();
        let rules_for_fallback = rules.clone();
        let fallback_text = fallback.as_str().map(label_of).unwrap_or_else(|| "Ne rien changer".into());
        let mut fallback_options = vec![("Ne rien changer".to_owned(), fallback.is_null())];
        fallback_options.extend(names.iter().map(|(n, l)| (l.clone(), fallback == n.as_str())));
        automation = automation
            .child(div().pt(px(6.)).child(ui::label("Nouvelle règle")))
            .child(add)
            .child(ui::list_row(
                None,
                "Quand aucune de ces applis n’est ouverte",
                None,
                div().w(px(220.)).child(self.dropdown("auto-fallback", fallback_text, fallback_options, enabled, move |this, i, cx| {
                    let target = if i == 0 { Value::Null } else { json!(fallback_names[i - 1].0) };
                    this.go_send("auto-scenes", json!({"enabled":on,"rules":rules_for_fallback,"fallback":target}), cx);
                }, cx)),
                false,
            ));
        div().flex().flex_col().gap(px(16.)).child(list).child(automation)
    }

    fn lighting(&self, v: &GoView, width: f32, cx: &mut Context<Self>) -> Div {
        let s = &v.state["lighting"];
        let enabled = self.go.enabled();
        let mut themes: Vec<AnyElement> = vec![];
        for (id, t) in entries(&v.meta["themes"]) {
            let mut dots = div().flex().flex_wrap().gap(px(5.));
            for c in labels(&t["preview"]) {
                dots = dots.child(ui::swatch(u32::from_str_radix(c, 16).unwrap_or(0), 16.));
            }
            let tile = ui::tile(SharedString::from(format!("theme-{id}")), None, t["label"].as_str().unwrap_or(id).to_owned(), None, s["theme"] == id.as_str(), None).child(dots);
            themes.push(self.act(tile, "theme", json!({"name":id}), cx).into_any_element());
        }
        let mut style = ui::segmented();
        for opt in labels(&v.meta["fader_styles"]) {
            style = style.child(self.act(ui::seg(SharedString::from(format!("style-{opt}")), light_label(opt).to_owned(), s["fader_style"]["A"] == opt), "lighting", json!({"fader_style":opt}), cx));
        }
        let animations: Vec<&str> = labels(&v.meta["animations"]).collect();
        let current = s["animation"].as_str().unwrap_or("none");
        let picks: Vec<String> = animations.iter().map(|a| a.to_string()).collect();
        let mut look = ui::section(Icon::Sun, ORANGE_SUN, "Thème", None)
            .child(ui::grid(themes, 3))
            .child(ui::label("Style des faders"))
            .child(style)
            .child(ui::list_row(
                None,
                "Animation",
                None,
                div().w(px(220.)).child(self.dropdown("animation", light_label(current).to_owned(), animations.iter().map(|a| (light_label(a).to_owned(), *a == current)).collect(), enabled, move |this, i, cx| {
                    this.go_send("lighting", json!({"animation": picks[i]}), cx)
                }, cx)),
                false,
            ));
        if current != "none" {
            for (key, label) in [("mod1", "Amplitude"), ("mod2", "Vitesse")] {
                look = look.child(self.slider_row(key, label, s[key].as_f64().unwrap_or(50.), (0., 100.), 1., |v| format!("{} %", v.round()), enabled, move |this, v, cx| this.go_send("lighting", json!({(key): v.round() as i64}), cx), cx));
            }
        }
        let hex = self.go.colour.trim().trim_start_matches('#').to_uppercase();
        let valid = hex.len() == 6 && hex.bytes().all(|c| c.is_ascii_hexdigit());
        let mut palette = div().flex().flex_wrap().gap(px(8.));
        for c in ["FF3B30", "FF9500", "FFCC00", "34C759", "00C7BE", "5AC8FA", "0071E3", "5856D6", "AF52DE", "FF2D55", "FFFFFF", "1D1D1F"] {
            let chosen = hex == c;
            let t = ui::hover_t(&format!("palette-{c}"));
            palette = palette.child(ui::hoverable(
                div()
                    .id(SharedString::from(format!("palette-{c}")))
                    .size(px(30.))
                    .rounded_full()
                    .cursor(gpui::CursorStyle::PointingHand)
                    .bg(ui::raw(u32::from_str_radix(c, 16).unwrap_or(0)))
                    .border_2()
                    .border_color(if chosen { rgb(ui::ACCENT) } else { ui::lerp(alpha(ui::INK, 0.12), alpha(ui::ACCENT, 0.6), t) })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.go.colour = c.to_owned();
                        cx.notify();
                    })),
                format!("palette-{c}"),
            ));
        }
        let mut colours = ui::section(Icon::Palette, 0xaf52de, "Couleurs des boutons", None)
            .child(palette)
            .child(div().flex().items_center().gap(px(10.)).child(self.go_text("colour", "Ou un code : 64D2FF", cx).flex_1()).when(valid, |d| d.child(ui::swatch(u32::from_str_radix(&hex, 16).unwrap_or(0), 36.))))
            .child(ui::hint(if valid { "Clique un carré ci-dessous pour lui donner cette couleur." } else { "Choisis une couleur, puis clique l’emplacement à colorer." }).text_size(px(12.)));
        for (n, (id, values)) in entries(&v.state["colours"]).enumerate() {
            let mut squares = div().flex().gap(px(8.));
            for (i, current) in values.as_array().into_iter().flatten().enumerate().take(2) {
                let fill = u32::from_str_radix(current.as_str().unwrap_or("000000"), 16).unwrap_or(0);
                let key = format!("colour-{id}-{i}");
                let t = if valid { ui::hover_t(&key) } else { 0. };
                let square = div()
                    .id(SharedString::from(key.clone()))
                    .size(px(28.))
                    .rounded(px(8.))
                    .bg(if t > 0.5 { ui::raw(u32::from_str_radix(&hex, 16).unwrap_or(fill)) } else { ui::raw(fill) })
                    .border_2()
                    .border_color(ui::lerp(alpha(ui::INK, 0.12), rgb(ui::ACCENT), t))
                    .when(valid, |d| d.cursor(gpui::CursorStyle::PointingHand));
                let values = if i == 0 { json!([hex, null]) } else { json!([null, hex]) };
                let square = ui::hoverable(square, key);
                squares = squares.child(if valid { self.act(square, "colour", json!({"target":id,"colours":values}), cx).into_any_element() } else { square.into_any_element() });
            }
            colours = colours.child(ui::list_row(None, colour_target(id), None, squares, n == 0));
        }
        ui::settings_columns(width, vec![look.into_any_element()], vec![colours.into_any_element()])
    }

    fn microphone(&self, v: &GoView, width: f32, cx: &mut Context<Self>) -> Div {
        if let Some(workspace) = self.mic_wizard_workspace(width, cx) {
            return workspace;
        }
        match self.mic_tab {
            MicTab::Input => self.go_mic_input(v, cx),
            MicTab::Filters => self.go_mic_filters(&v.state, width, cx),
            MicTab::Presets => self.mic_voice(v, width, cx),
            MicTab::Listen => self.mic_button_and_listen(v, width, cx),
        }
    }

    fn go_mic_input(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let s = &v.state;
        let mic = &s["mic"];
        let enabled = self.go.enabled();
        let kind = mic["type"].as_str().or(mic["kind"].as_str()).unwrap_or("dynamic").to_owned();
        let mut kinds = ui::segmented();
        for (k, name) in [("dynamic", "Dynamique"), ("condenser", "Statique · 48 V"), ("jack", "Jack 3,5 mm")] {
            let el = ui::seg(SharedString::from(format!("mic-{k}")), name, kind == k);
            kinds = kinds.child(if k == "condenser" && kind != k {
                self.ask_go(el, "mic", json!({"type":k,"suggest_gain":true}), Ask::new("Activer l’alimentation 48 V ?", "Activer").text("Seuls les micros statiques en ont besoin. Vérifie que ton micro et son câble la supportent avant de l’activer."), cx)
            } else {
                self.act(el, "mic", json!({"type":k,"suggest_gain":true}), cx)
            });
        }
        let models: Vec<(String, String)> = entries(&v.meta["mic_models"])
            .map(|(id, m)| (id.clone(), format!("{} {}", m["brand"].as_str().unwrap_or(""), m["name"].as_str().or(m["label"].as_str()).unwrap_or(id)).trim().to_owned()))
            .collect();
        let model = mic["model"].as_str().unwrap_or("");
        let model_text = models.iter().find(|(id, _)| id == model).map(|(_, l)| l.clone()).unwrap_or_else(|| "Choisir le modèle".into());
        let ids: Vec<String> = models.iter().map(|(id, _)| id.clone()).collect();
        let gain = ui::card_title(Some((Icon::Gauge, ui::ACCENT)), "Gain du micro", None);
        let gain = div().flex().flex_col().gap(px(14.))
            .child(div().flex().flex_wrap().items_center().justify_between().gap(px(12.))
                .child(gain)
                .child(mic_signal(self.go.level_shown, self.go.level)))
            .child(self.slider_row("mic-gain", "Préampli GoXLR", mic["gain_db"].as_f64().unwrap_or(0.),
                (0., 72.), 1., |v| format!("{v:.0} dB"), enabled,
                |this, v, cx| this.go_send("mic", json!({"gain_db": v.round() as i64}), cx), cx));

        let connection = div().flex().flex_col().gap(px(12.))
            .child(div().flex().flex_col().gap(px(6.))
                .child(ui::eyebrow("Modèle"))
                .child(div().w_full().max_w(px(340.)).child(self.dropdown("mic-model", model_text,
                    models.iter().map(|(id, l)| (l.clone(), id == model)).collect(), enabled,
                    move |this, i, cx| this.go_send("mic-model", json!({"model": ids[i]}), cx), cx))))
            .child(kinds)
            .when_some(labels(&v.meta["mic_models"][model]["tips"]).next(), |d, tip| {
                d.child(ui::caption(tip.to_owned()))
            });

        let open = self.go.mic_connection_open;
        let details = ui::pill("mic-connection", Some(if open { Icon::ChevronDown } else { Icon::ChevronRight }),
            "Modèle et alimentation", ui::Tone::Plain)
            .on_click(cx.listener(|this, _, _, cx| {
                this.go.mic_connection_open = !this.go.mic_connection_open;
                cx.notify();
            }));
        div().w_full().flex().flex_col().gap(px(18.))
            .child(gain)
            .child(ui::card().child(div().flex().child(details)).when(open, |d| d.child(connection)))
    }

    /// Onglet « Ta voix » : le préréglage à gauche ; à droite ce que Rustudio sait de ta voix et l'effet du préréglage.
    fn mic_voice(&self, v: &GoView, width: f32, cx: &mut Context<Self>) -> Div {
        let s = &v.state;
        let kind = s["mic"]["type"].as_str().or(s["mic"]["kind"].as_str()).unwrap_or("dynamic");
        let advised = v.meta["mic_type_preset"][kind].as_str().unwrap_or("");
        let mut presets: Vec<AnyElement> = vec![];
        for (id, p) in entries(&v.meta["mic_presets"]) {
            let tile = ui::tile(SharedString::from(format!("mic-preset-{id}")), Some((preset_icon(id), preset_colour(id))), p["label"].as_str().unwrap_or(id).to_owned(), p["desc"].as_str().map(|d| d.to_owned().into()), s["mic_preset"] == id.as_str(), (id == advised).then(|| ("Conseillé".into(), ui::GREEN)));
            presets.push(self.act(tile, "mic-preset", json!({"name":id}), cx).into_any_element());
        }
        let cal = &s["mic_cal"];
        let measured = s["voice_print"].is_object();
        let has_details = cal.is_object() || measured;
        let column_width = if has_details { ui::settings_column_width(width) } else { width };
        let columns = ui::columns_that_fit(column_width, 200., 10., 4);
        let applying = self.go.preset_pending.as_ref().and_then(|p| p["name"].as_str())
            .or(self.go.preset_in_flight.as_deref())
            .and_then(|id| v.meta["mic_presets"][id]["label"].as_str());
        let feedback = if let Some(label) = applying {
            (format!("Application de {label}…"), ui::State::Info)
        } else if v.ab {
            ("Écoute de la voix brute".into(), ui::State::Info)
        } else if let Some(label) = s["mic_preset"].as_str()
            .and_then(|id| v.meta["mic_presets"][id]["label"].as_str()) {
            (format!("{label} · Actif"), ui::State::Ok)
        } else {
            ("Réglages personnalisés".into(), ui::State::Info)
        };
        let voices = ui::card()
            .child(ui::status(feedback.0, feedback.1))
            .child(ui::grid(presets, columns))
            .when(s["mic_preset"] == "anti-bruit", |d| d.child(ui::caption("Pour atténuer le clavier pendant que tu parles dans tes applis, utilise la suppression du bruit (IA).")))
            .child(div().flex().flex_wrap().gap(px(8.))
                .child(ui::pill("mic-voice-setup", Some(Icon::Wave), "Analyser ma voix", ui::Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.open_mic_wizard(Step::Measure, cx))))
                .when(has_details, |d| d.child(ui::pill("mic-adjustments", Some(Icon::Activity), "Bilan vocal", ui::Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.open_mic_wizard(Step::Gain, cx))))));
        if !has_details { return voices; }
        let mut tuned = ui::section(Icon::Mic, ui::RED, "Ton réglage", None);
        if cal.is_object() {
            let when = cal["t"].as_f64().map(ago).unwrap_or_default();
            tuned = tuned
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_x(px(32.))
                        .gap_y(px(10.))
                        .child(ui::fact(format!("{} dB", s["mic"]["gain_db"].as_f64().unwrap_or(0.).round()), "Gain"))
                        .child(ui::fact(format!("{} dB", (cal["voice_db"].as_f64().unwrap_or(0.) - cal["noise_peak_db"].as_f64().unwrap_or(0.)).round()), "Voix au-dessus du bruit"))
                        .child(ui::fact(if s["voice_print"].is_object() { "Analysée" } else { "Non analysée" }, "Ta voix")),
                )
                .child(ui::caption(format!("Réglé {when}. Relance l’analyse si tu changes de micro, de place ou de pièce.")));
        }
        if measured {
            tuned = tuned.child(div().flex().child(self.ask_go(ui::pill("voice-forget", Some(Icon::Trash), "Oublier ma voix", ui::Tone::Plain), "voice-forget", json!({}), Ask::new("Oublier l’analyse de ta voix ?", "Oublier").text("Rustudio efface ce qu’il a mesuré de ta voix avec ce micro. Les sons repartent de réglages généraux, jusqu’à la prochaine analyse.").danger(), cx)));
        }
        let mut details = vec![tuned.into_any_element()];
        if measured {
            let preset = s["mic_preset"].as_str().filter(|p| *p != "brut" && *p != "custom").unwrap_or("studio");
            let chart_height = (column_width * 0.21).clamp(130., 220.);
            details.push(self.voice_chart(preset, chart_height).into_any_element());
        }
        ui::settings_columns(width, vec![voices.into_any_element()], details)
    }

    /// Onglet « Bouton et retour » : ce que fait le bouton du micro, et comment t'entendre.
    fn mic_button_and_listen(&self, v: &GoView, width: f32, cx: &mut Context<Self>) -> Div {
        let s = &v.state;
        let enabled = self.go.enabled();
        let mut mode = ui::segmented();
        for (m, label) in [("toggle", "Appui = couper"), ("hold", "Maintenir = couper"), ("ptt", "Maintenir = parler")] {
            mode = mode.child(self.act(ui::seg(SharedString::from(format!("mic-button-{m}")), label, s["mic_button"]["mode"] == m), "settings", json!({"mic_button":{"mode":m}}), cx));
        }
        let target = s["mic_button"]["target"].as_str().unwrap_or("all").to_owned();
        let targets: Vec<(&str, &str)> = rustudio_core::consts::MUTE_TARGET_LABELS.to_vec();
        let target_ids: Vec<String> = targets.iter().map(|(t, _)| t.to_string()).collect();
        let blink = s["mic_button"]["blink"] == true;
        let button_card = ui::section(Icon::MicOff, ui::RED, "Bouton du micro", None)
            .child(mode)
            .child(ui::list_row(None, "Il coupe le micro", None, div().w(px(200.)).child(self.dropdown("mic-target", targets.iter().find(|(t, _)| *t == target).map(|(_, l)| l.to_string()).unwrap_or_default(), targets.iter().map(|(t, l)| (l.to_string(), *t == target)).collect(), enabled, move |this, i, cx| this.go_send("settings", json!({"mic_button":{"target": target_ids[i]}}), cx), cx)), true))
            .child(ui::list_row(None, "Clignoter quand le micro est coupé", None, self.act(ui::toggle("mic-blink", blink, enabled), "settings", json!({"mic_button":{"blink":!blink}}), cx), false))
            .when(!self.rustudio_firmware(), |d| d.child(self.slider_row(
                "bleep", "Volume du bip de censure", s["bleep_db"].as_f64().unwrap_or(0.), (-36., 0.), 1.,
                |v| format!("{} dB", v.round()), enabled,
                |this, v, cx| this.go_send("dsp", json!({"bleep_db": v.round() as i64}), cx),
                cx,
            )));
        let mut compare = ui::segmented();
        let bypass = v.ab;
        compare = compare
            .child(self.act(ui::seg("mic-ab-processed", "Voix traitée", !bypass), "mic-ab", json!({"bypass":false}), cx))
            .child(self.act(ui::seg("mic-ab-raw", "Voix brute", bypass), "mic-ab", json!({"bypass":true}), cx));
        let listen = ui::section(Icon::Ear, ui::ACCENT, "Retour casque", None)
            .child(self.monitor_controls("mic-quick-monitor", enabled, cx))
            .child(ui::caption("Le retour direct utilise les filtres de la GoXLR. L’antibruit Windows agit sur le son envoyé aux applis."))
            .child(ui::subhead("Comparer"))
            .child(compare)
            .child(self.listen_panel(cx));
        div().flex().flex_col().child(ui::settings_columns(width, vec![button_card.into_any_element()], vec![listen.into_any_element()]))
    }

    fn headphones(&self, v: &GoView, width: f32, cx: &mut Context<Self>) -> Div {
        let eq = &v.state["hp_eq"];
        let enabled = self.go.enabled();
        let on = eq["enabled"] == true;
        let paused = eq["pause_live"] == true;
        let models: Vec<String> = entries(&v.meta["headphones"]).map(|(n, _)| n.clone()).collect();
        let current = eq["headphone"].as_str().unwrap_or("").to_owned();
        let mut options = vec![("Aucune correction".to_owned(), current.is_empty())];
        options.extend(models.iter().map(|m| (m.clone(), *m == current)));
        let picks = models.clone();
        let mut chips = div().flex().flex_wrap().gap(px(8.));
        for channel in labels(&v.meta["hp_channels"]) {
            let mut selected = eq["channels"].as_array().cloned().unwrap_or_default();
            let checked = selected.iter().any(|v| v == channel);
            if checked {
                selected.retain(|v| v != channel);
            } else {
                selected.push(json!(channel));
            }
            chips = chips.child(self.act(ui::pill(SharedString::from(format!("hp-channel-{channel}")), checked.then_some(Icon::Check), input_info(channel).0, if checked { ui::Tone::Soft } else { ui::Tone::Plain }), "hp-eq", json!({"channels":selected}), cx));
        }
        let chosen = eq["channels"].as_array().cloned().unwrap_or_default();
        // sortie Windows par défaut hors des voies cochées : l'égaliseur la couvre quand même
        let follows = v.hp_eq["default"].as_str().filter(|c| !chosen.iter().any(|x| x == c)).map(|c| input_info(c).0);
        let corrected = chosen.len() + follows.is_some() as usize;
        let status = match (on, follows) {
            (false, _) => "Coupé".to_owned(),
            (true, Some(name)) => format!("Actif sur {corrected} voies, dont {name}, ta sortie Windows"),
            (true, None) => format!("Actif sur {corrected} voie{}", if corrected > 1 { "s" } else { "" }),
        };
        // Les réglages qu'on touche souvent tiennent sur une ligne : marche/arrêt, casque, pause en live.
        let quick = ui::card().py(px(14.)).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(18.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(self.act(ui::toggle("hp-enabled", on, enabled), "hp-eq", json!({"enabled":!on}), cx))
                        .child(div().flex().flex_col().child(ui::title("Égaliseur du casque", 15.)).child(ui::caption(status))),
                )
                .child(div().flex_1())
                .child(
                    div().flex().items_center().gap(px(10.)).child(ui::label("Casque")).child(div().w(px(230.)).child(self.dropdown(
                        "hp-model",
                        if current.is_empty() { "Aucune correction".into() } else { current.clone() },
                        options,
                        enabled,
                        move |this, i, cx| {
                            let pick = if i == 0 { Value::Null } else { json!(picks[i - 1]) };
                            this.go_send("hp-eq", json!({"headphone": pick}), cx)
                        },
                        cx,
                    ))),
                )
                .child(div().flex().items_center().gap(px(10.)).child(ui::label("Pause en live")).child(self.act(ui::toggle("hp-pause", paused, enabled), "hp-eq", json!({"pause_live":!paused}), cx))),
        );
        // Styles en grille régulière, trois par rangée : icône et nom alignés, la description du choisi en dessous.
        let mut tiles: Vec<AnyElement> = vec![];
        let mut chosen_desc: Option<String> = None;
        for (id, st) in entries(&v.meta["hp_styles"]) {
            if let Some(required) = st["headphone"].as_str()
                && eq["headphone"] != required {
                    continue;
                }
            let selected = eq["style"] == id.as_str();
            if selected {
                chosen_desc = st["desc"].as_str().map(str::to_owned);
            }
            let label = st["label"].as_str().unwrap_or(id).to_owned();
            let tile = ui::tile(SharedString::from(format!("hp-style-{id}")), Some((style_icon(id), ui::TEXT2)), label, None, selected, None);
            tiles.push(self.act(tile, "hp-eq", json!({"style":id}), cx).into_any_element());
        }
        let style_row = div().grid().grid_cols(3).gap_x(px(6.)).gap_y(px(2.)).children(tiles);
        // Intensité du style choisi : 100 % = la courbe telle quelle, flèche pour y revenir.
        let has_style = eq["style"].as_str().is_some_and(|s| s != "neutre");
        let intensity = eq["intensity"].as_f64().unwrap_or(100.);
        let (shown, fraction) = self.parameter_view("param-hp-intensity", intensity, (0., 200.));
        let reset = ui::round("hp-intensity-reset", Icon::History, 26., alpha(0x767680, 0.), rgb(ui::TEXT2));
        let intensity_row = div()
            .mt(px(10.))
            .px(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(26.))
                    .child(div().text_size(px(13.)).text_color(rgb(ui::TEXT)).child("Intensité"))
                    .child(div().flex_1())
                    .when(shown.round() != 100., |d| {
                        d.child(reset.on_click(cx.listener(|this, _, _, cx| this.send_intensity(100, cx))))
                    })
                    .child(div().w(px(46.)).text_right().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(if shown.round() == 100. { ui::TEXT2 } else { ui::ACCENT })).child(format!("{:.0} %", shown))),
            )
            .child(
                self.param_area_with("hp-intensity", (0., 200.), 1., false, enabled, true, |this, v, cx| this.send_intensity(v.round() as i64, cx), cx)
                    .mt(px(2.))
                    .h(px(22.))
                    .w_full()
                    .child(ui::hslider(fraction, rgb(ui::ACCENT), ui::hover_t("param-hp-intensity"))),
            );
        let style_card = ui::section(Icon::Music, 0xff2d55, "Style de son", None)
            .child(style_row)
            .when(has_style, |d| d.child(intensity_row))
            .when_some(chosen_desc, |d, desc| d.child(ui::hint(desc).text_size(px(12.5)).mt(px(8.)).px(px(10.))));
        // Equalizer APO : état lisible (installé ? actif où ?), ce qu'il fait, et les actions qui ont un sens maintenant.
        let apo = &v.hp_eq;
        let installed = apo["apo"] == true;
        let setup = &apo["setup"];
        let running = setup["running"] == true;
        let branched: Vec<&str> = labels(&v.meta["hp_channels"]).filter(|c| apo["endpoints"][*c] == true).collect();
        let wanted: Vec<String> = chosen.iter().filter_map(|c| c.as_str().map(str::to_owned)).chain(apo["default"].as_str().map(str::to_owned)).collect();
        let missing = wanted.iter().any(|c| apo["endpoints"][c.as_str()] == false);
        let (dot, headline, detail) = if running {
            (ui::ACCENT, setup["step"].as_str().unwrap_or("En cours…").to_owned(), "Le son de Windows peut se couper environ 2 secondes.".to_owned())
        } else if !installed {
            (ui::ORANGE, "Pas installé".to_owned(), "Sans lui, la correction du casque et les styles de son ne s’appliquent pas.".to_owned())
        } else if branched.is_empty() {
            (ui::ORANGE, "Installé, mais branché sur aucune sortie".to_owned(), "Branche-le pour que la correction et les styles s’appliquent.".to_owned())
        } else {
            let names: Vec<&str> = branched.iter().map(|c| input_info(c).0).collect();
            (ui::GREEN, format!("Installé et actif sur {} sortie{}", names.len(), if names.len() > 1 { "s" } else { "" }), names.join(", "))
        };
        let status_row = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().size(px(9.)).flex_none().rounded_full().bg(rgb(dot)))
            .child(div().flex().flex_col().min_w(px(0.)).child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(headline)).child(ui::caption(detail)));
        let error = setup["error"].as_str().map(|e| div().text_size(px(12.5)).text_color(rgb(ui::RED_TEXT)).child(format!("Échec : {e}")));
        let mut actions = div().flex().flex_wrap().gap(px(8.));
        if !running && !installed {
            actions = actions.child(self.ask_go(ui::pill("hp-install", Some(Icon::Download), "Installer Equalizer APO", ui::Tone::Primary), "hp-eq-setup", json!({}), Ask::new("Installer Equalizer APO ?", "Installer")
                .text("Equalizer APO est un petit logiciel libre et gratuit qui ajoute un égaliseur au son de Windows. Rustudio s’en sert pour corriger ton casque et appliquer les styles de son : il ajuste les graves, les médiums et les aigus de tout ce que tu écoutes sur la GoXLR.")
                .text("Rustudio le télécharge depuis son site officiel, le branche sur les sorties de la GoXLR, puis redémarre le son de Windows : environ 2 secondes de silence. Tu peux le désinstaller ici à tout moment."), cx));
        }
        if !running && installed && (branched.is_empty() || missing) {
            actions = actions.child(self.ask_go(ui::pill("hp-install", Some(Icon::Plug), "Brancher sur les voies cochées", ui::Tone::Primary), "hp-eq-setup", json!({}), Ask::new("Brancher Equalizer APO sur les voies cochées ?", "Brancher")
                .text("La correction et le style s’appliqueront au son de ces sorties. Le son de Windows redémarre : environ 2 secondes de silence."), cx));
        }
        if !running && installed {
            if !branched.is_empty() {
                actions = actions.child(self.ask_go(ui::pill("hp-remove", Some(Icon::Close), "Débrancher", ui::Tone::Plain), "hp-eq-remove", json!({}), Ask::new("Débrancher Equalizer APO des sorties GoXLR ?", "Débrancher")
                    .text("La correction du casque et les styles de son ne s’appliqueront plus. Le son de Windows redémarre : environ 2 secondes de silence.")
                    .text("Le logiciel reste installé sur Windows : tu pourras le rebrancher d’un clic."), cx));
            }
            actions = actions.child(self.ask_go(ui::pill("hp-uninstall", Some(Icon::Trash), "Désinstaller de Windows", ui::Tone::Danger), "hp-eq-uninstall", json!({}), Ask::new("Désinstaller Equalizer APO ?", "Désinstaller")
                .text("Rustudio le débranche des sorties GoXLR puis le retire complètement de Windows. La correction du casque et les styles de son ne s’appliqueront plus.")
                .text("Le son de Windows redémarre : environ 2 secondes de silence. Tu pourras le réinstaller d’un clic.")
                .danger(), cx));
        }
        let channels_card = ui::section(Icon::Headphones, ui::ACCENT, "Equalizer APO", Some("Correction dans Windows".into()))
            .child(status_row)
            .children(error)
            .child(ui::hint("Un égaliseur libre pour Windows : il applique la correction de ton casque et le style choisi au son des voies cochées. Ta sortie Windows par défaut est toujours incluse.").text_size(px(12.5)))
            .when(installed && !running, |d| d.child(ui::label("Voies corrigées")).child(chips))
            .child(actions);
        let values = eq["bands"].as_array().cloned().unwrap_or_default();
        let mut bands = div().flex().gap(px(2.));
        for (i, freq) in v.meta["hp_bands"].as_array().into_iter().flatten().enumerate() {
            let Some(current) = values.get(i).and_then(Value::as_f64) else { continue };
            let all = values.clone();
            let below = div().flex().justify_center().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(hz(freq.as_f64().unwrap_or(0.)));
            bands = bands.child(self.eq_column(&format!("hp-band-{i}"), current, (-12., 12.), 0.5, below.into_any_element(), enabled, move |this, v, cx| {
                let mut next = all.clone();
                next[i] = json!(v);
                this.go_send("hp-eq", json!({"bands": next}), cx)
            }, cx));
        }
        let eq_card = ui::section(Icon::Activity, ui::ACCENT, "Égaliseur 10 bandes", Some("Fais glisser chaque bande".into())).child(bands);
        let mut custom = ui::section(Icon::Bookmark, ui::GREEN, "Mes réglages de casque", None).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.))
                .child(self.go_text("name", "Nom du réglage", cx).flex_auto().min_w(px(156.)))
                .child(self.act(ui::pill("hp-save", Some(Icon::Check), "Enregistrer", ui::Tone::Primary), "hp-eq-save", json!({"name":self.go.name}), cx)),
        );
        for (n, (name, _)) in entries(&eq["custom"]).enumerate() {
            let style = format!("custom:{name}");
            let active = eq["style"] == style.as_str();
            custom = custom.child(ui::list_row(
                None,
                name.clone(),
                active.then(|| "Appliqué".into()),
                div()
                    .flex()
                    .gap(px(6.))
                    .child(self.act(ui::pill(SharedString::from(format!("hp-apply-{name}")), None, "Appliquer", if active { ui::Tone::Soft } else { ui::Tone::Plain }), "hp-eq", json!({"style":style}), cx))
                    .child(self.ask_go(ui::round(SharedString::from(format!("hp-delete-{name}")), Icon::Trash, 32., alpha(0x767680, 0.), rgb(ui::TEXT2)), "hp-eq-delete", json!({"name":name}), Ask::new("Supprimer ce réglage de casque ?", "Supprimer").danger(), cx)),
                n == 0,
            ));
        }
        // Barre rapide en haut, puis le son à gauche (style, courbe) et les réglages rares à droite.
        div().child(quick.mb(px(16.))).child(ui::settings_columns(
            width,
            vec![style_card.into_any_element(), eq_card.into_any_element()],
            vec![custom.into_any_element(), channels_card.into_any_element()],
        ))
    }

    pub(crate) fn run_latency_test(&mut self, cx: &mut Context<Self>) {
        if self.go.busy || !self.go.enabled() {
            return;
        }
        self.go.busy = true;
        self.go.latency_running = true;
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move { crate::win_session::latency_test(&client) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.busy = false;
                this.go.latency_running = false;
                this.go.latency_test = Some(result.unwrap_or_else(|error| json!({"status": "error", "error": error})));
                this.resume_mic_preset(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn latency_card(&self, cx: &mut Context<Self>) -> Div {
        let ms = |v: f64| format!("{} ms", crate::mixer::fmt_num(v));
        let mut card = ui::section(Icon::Activity, ui::ACCENT, "Latence", None)
            .child(hint("Mise à jour chaque seconde tant que cette page est ouverte.").text_size(px(12.)));
        let live = self.go.latency.as_ref();
        let usb = live.and_then(|l| l["usb"]["avg_ms"].as_f64());
        card = card.child(ui::list_row(
            Some((Icon::Plug, ui::TEXT2)),
            "Réponse aux réglages",
            Some("Temps pour que la GoXLR confirme un réglage. N’ajoute rien au son.".into()),
            match usb {
                Some(v) => ui::status(ms(v), if v < 100. { ui::State::Ok } else { ui::State::Warn }).into_any_element(),
                None => ui::caption("—").into_any_element(),
            },
            true,
        ));
        // le pilote GoXLR annonce 0 ms de latence propre : seule la période de Windows est un chiffre fiable
        if let Some(period) = live.and_then(|l| l["windows"]["output"]["period_ms"].as_f64()) {
            card = card.child(ui::list_row(
                Some((Icon::Wave, ui::TEXT2)),
                "Cycle audio de Windows",
                Some("Windows échange le son par paquets de cette durée.".into()),
                ui::status(ms(period), ui::State::Info),
                false,
            ));
        }
        if self.rustudio_firmware() {
            return card;
        }
        card = card.child(hint("« Mesurer » joue trois sons aigus très courts, envoyés seulement vers le Stream Mix, et chronomètre leur retour. Évite pendant un live.").text_size(px(12.)));
        card = if self.go.latency_running {
            card.child(ui::status("Mesure en cours…", ui::State::Info))
        } else {
            card.child(ui::pill("latency-run", Some(Icon::Play), "Mesurer", ui::Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.run_latency_test(cx))))
        };
        if let Some(r) = self.go.latency_test.as_ref() {
            if let (Some("ok"), Some(total)) = (r["status"].as_str(), r["total_ms"].as_f64()) {
                let (state, advice) = if total <= 40. {
                    (ui::State::Ok, "Très bon : imperceptible en jeu comme en appel.")
                } else if total <= 80. {
                    (ui::State::Info, "Correct : se remarque seulement si tu t’écoutes en direct ou joues d’un instrument.")
                } else {
                    (ui::State::Warn, "Élevé : un tampon audio trop grand dans le pilote GoXLR peut en être la cause.")
                };
                let detail = format!(
                    "{}Aller {} · retour {} · de {} à {} sur {} essai(s)",
                    if r["simulated"] == true { "Démonstration · " } else { "" },
                    ms(r["out_ms"].as_f64().unwrap_or(0.)),
                    ms(r["back_ms"].as_f64().unwrap_or(0.)),
                    ms(r["min_ms"].as_f64().unwrap_or(total)),
                    ms(r["max_ms"].as_f64().unwrap_or(total)),
                    r["trials"].as_u64().unwrap_or(1)
                );
                card = card
                    .child(ui::list_row(Some((Icon::Gauge, ui::TEXT2)), "Aller-retour mesuré", Some(detail.into()), ui::status(ms(total), state), false))
                    .child(hint(advice).text_size(px(12.)));
            } else {
                let text = match r["status"].as_str() {
                    Some("no-signal") => "Le son de mesure n’est pas revenu : vérifie que la voie Sample n’est ni coupée ni à zéro.".to_owned(),
                    _ => format!("Mesure impossible : {}", r["error"].as_str().unwrap_or("erreur inconnue")),
                };
                card = card.child(ui::notice(false, text));
            }
        }
        card
    }

    pub(crate) fn run_sound_test(&mut self, cx: &mut Context<Self>) {
        if self.go.busy || !self.go.enabled() {
            return;
        }
        self.go.busy = true;
        self.go.sound_test = None;
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move { crate::win_session::sound_test(&client) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.busy = false;
                match result {
                    Ok(data) => {
                        this.go.sound_test = Some(data);
                        this.go.notice = None;
                    }
                    Err(error) => this.go.notice = Some((false, error)),
                }
                this.resume_mic_preset(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Le verdict et les actions utiles restent visibles quand les outils sont repliés.
    fn diagnostic_view(&self, width: f32, cx: &mut Context<Self>) -> Div {
        let url = self.client.overlay_url();
        let mut copy_text: Option<String> = None;
        let mut state = ui::section(Icon::Gauge, ui::GREEN, "Connexions et Windows", None);
        let mut problems: Vec<AnyElement> = vec![];
        let mut checked = false;
        if let Some(d) = self.go.diagnostics.as_ref() {
            checked = true;
            let report = &d["support"]["report"];
            copy_text = Some(serde_json::to_string_pretty(report).unwrap_or_default());
            let engine_ok = report["moteur"]["etat"].as_str().is_some_and(|e| e.contains("ok") || e.contains("connect") || e.contains("Démo"));
            let cuts = report["coupures_usb_24h"]["nombre"].as_u64();
            let mut rows: Vec<(Icon, String, String, ui::State)> = vec![
                (Icon::Power, "Moteur".into(), match report["moteur"]["etat"].as_str().unwrap_or("inconnu") { "ok" => "En marche".to_owned(), other => other.to_owned() }, if engine_ok { ui::State::Ok } else { ui::State::Warn }),
            ];
            // les coupures sont comptées par le pilote officiel : rien à montrer sous firmware Rustudio
            if !self.rustudio_firmware() {
                rows.push((Icon::Plug, "Coupures USB (24 h)".into(), cuts.map(|n| if n == 0 { "Aucune".to_owned() } else { n.to_string() }).unwrap_or_else(|| "Non mesurées".into()), if cuts.unwrap_or(0) == 0 { ui::State::Ok } else { ui::State::Warn }));
            }
            if d["usb"]["found"] == true {
                let cpu = d["usb"]["controller_cpu"] == true;
                rows.push((Icon::Gauge, "Contrôleur USB".into(), if cpu { "Relié au processeur".into() } else { "À vérifier".into() }, if cpu { ui::State::Ok } else { ui::State::Warn }));
            }
            if d["glitches"]["readable"] == true {
                rows.push((Icon::History, "Journal Windows".into(), format!("{} évènement(s)", d["glitches"]["total"]), ui::State::Info));
            }
            let kernel = &d["glitches"]["kernel"];
            if let Some(mb) = kernel["nonpaged_mb"].as_u64() {
                let high = kernel["high"] == true;
                rows.push((Icon::Gauge, "Mémoire noyau de Windows".into(), kernel_size(mb), if high { ui::State::Warn } else { ui::State::Ok }));
            }
            let mut first = true;
            for (glyph, title, value, st) in rows {
                if st == ui::State::Warn {
                    let detail = (title == "Mémoire noyau de Windows").then(|| kernel_advice(
                        kernel["nonpaged_mb"].as_u64().unwrap_or(0), kernel["uptime_h"].as_u64().unwrap_or(0),
                    ).into());
                    problems.push(ui::list_row(Some((glyph, ui::ORANGE)), title, detail, ui::status(value, st), problems.is_empty()).into_any_element());
                } else {
                    state = state.child(ui::list_row(Some((glyph, ui::TEXT2)), title, None, ui::status(value, st), first));
                    first = false;
                }
            }
            if self.rustudio_firmware() && d["firmware"].is_object() {
                let firmware = firmware_verdict(&d["firmware"]);
                if firmware.state == ui::State::Warn {
                    problems.push(ui::list_row(Some((Icon::Activity, ui::ORANGE)), "Firmware Rustudio", None,
                        ui::status(firmware.label, firmware.state), problems.is_empty()).into_any_element());
                }
            }
            for (i, check) in d["echo"]["checks"].as_array().into_iter().flatten().enumerate() {
                // le moteur envoie le titre dans « t » et l'explication dans « d »
                let label = check["t"].as_str().or(check["title"].as_str()).unwrap_or("Routage").to_owned();
                let detail = check["d"].as_str().map(|d| SharedString::from(d.to_owned()));
                let ok = check["ok"] == true;
                if ok {
                    state = state.child(ui::list_row(Some((Icon::Route, ui::TEXT2)), label, detail, ui::status("Bon", ui::State::Ok), first));
                    first = false;
                    continue;
                }
                // un réglage à corriger monte dans le verdict, avec son bouton
                let fix = &check["fix"];
                let trailing = if let Some(action) = ACTIONS.iter().copied().find(|a| fix["action"] == *a) {
                    let text = fix["label"].as_str().unwrap_or("Corriger").to_owned();
                    self.act(ui::pill(("diag-fix", i), Some(Icon::Check), text, ui::Tone::Primary), action, fix["body"].clone(), cx).into_any_element()
                } else {
                    ui::status("À vérifier", ui::State::Warn).into_any_element()
                };
                problems.push(ui::list_row(Some((Icon::Route, ui::ORANGE)), label, detail, trailing, problems.is_empty()).into_any_element());
            }
        }
        // verdict : la première chose qu'on lit
        let (glyph, colour, headline) = if !checked && self.go.diagnostic_error.is_some() {
            (Icon::Alert, ui::ORANGE, "Vérification indisponible".to_owned())
        } else if !checked {
            (Icon::Refresh, ui::TEXT3, "Vérification en cours…".to_owned())
        } else if problems.is_empty() {
            (Icon::Check, ui::GREEN, "Aucun problème détecté".to_owned())
        } else {
            let n = problems.len();
            (Icon::Alert, ui::ORANGE, format!("{n} point{} à vérifier", if n > 1 { "s" } else { "" }))
        };
        let actions = div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .child(ui::pill("diag-read", Some(Icon::Refresh), if self.go.diagnostic_loading { "Lecture…" } else { "Actualiser" }, ui::Tone::Plain)
                .on_click(cx.listener(|this, _, _, cx| this.refresh_diag(cx))));
        let verdict = ui::card()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .flex_1()
                            .min_w(px(260.))
                            .child(ui::icon(glyph, 26., rgb(colour)))
                            .child(ui::title(headline, 22.)),
                    )
                    .child(actions),
            )
            .when_some(self.go.diagnostic_error.as_ref(), |d, error| d.child(ui::caption(error.clone())))
            .when(!problems.is_empty(), |d| d.child(div().flex().flex_col().children(problems)));
        let open = self.go.diagnostic_details.is_open();
        let details_button = ui::pill("diag-details", Some(if open { Icon::ChevronDown } else { Icon::ChevronRight }),
            "Détails et outils", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                let panel = &mut this.go.diagnostic_details;
                panel.set(!panel.is_open(), Instant::now());
                cx.notify();
            }));
        let details = self.go.diagnostic_details.body(|| self.diagnostic_tools(
            width, checked.then_some(state), copy_text, url, cx,
        ));
        div().flex().flex_col().gap(px(16.))
            .child(verdict)
            .child(div().child(div().flex().child(details_button)).children(details))
    }

    fn diagnostic_tools(&self, width: f32, state: Option<Div>, copy_text: Option<String>, url: String, cx: &mut Context<Self>) -> Div {
        let mut test = ui::section(Icon::Volume, ui::ACCENT, "Test des voies audio", None)
            .child(hint("Rustudio joue un son dans chaque voie et vérifie qu’il arrive sans coupure ni saut.").text_size(px(12.)))
            .child(ui::pill("sound-test", Some(Icon::Play), "Lancer le test", ui::Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.run_sound_test(cx))));
        if let Some(result) = self.go.sound_test.as_ref() {
            let dropouts = result["loop"]["dropouts"].as_u64().unwrap_or(0);
            test = test.child(ui::list_row(None, "Boucle audio", Some(format!("{} coupure(s) · {} saut(s)", dropouts, result["loop"]["jumps"])).map(Into::into), ui::status(result["loop"]["status"].as_str().unwrap_or("indisponible").to_owned(), if dropouts == 0 { ui::State::Ok } else { ui::State::Warn }), true));
            if let Some(error) = result["loop"]["error"].as_str() {
                test = test.child(ui::notice(false, error.to_owned()));
            }
            for row in result["scan"]["channels"].as_array().into_iter().flatten() {
                test = test.child(ui::list_row(None, row["name"].as_str().unwrap_or("Voie audio").to_owned(), None, ui::caption(row["status"].as_str().unwrap_or("mesurée").to_owned()), false));
            }
        }
        let obs = ui::section(Icon::Monitor, 0x5856d6, "Incrustation OBS", None)
            .child(hint("Ajoute une « Source navigateur » dans OBS avec ce lien : la pastille du micro s’affiche dans ton live.").text_size(px(12.)))
            .child(ui::list_row(None, url.clone(), None, ui::pill("obs-url", Some(Icon::Bookmark), "Copier", ui::Tone::Plain).on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(url.clone())))), true));
        // Firmware Rustudio : le test des voies vise les voies du pilote officiel, absentes ici.
        let mut right = vec![self.latency_card(cx).into_any_element()];
        if let Some(card) = self.firmware_card() {
            right.insert(0, card.into_any_element());
        }
        if !self.rustudio_firmware() {
            right.push(test.into_any_element());
        }
        right.push(self.echo_panel(cx).into_any_element());
        let mut tools = div().flex().flex_wrap().gap(px(8.))
            .child(self.act(ui::pill("diag-reconnect", Some(Icon::Plug), "Reconnecter la GoXLR", ui::Tone::Plain), "reconnect", json!({}), cx));
        if let Some(copy) = copy_text {
            tools = tools.child(ui::pill("diag-copy", Some(Icon::Bookmark), "Copier le rapport anonyme", ui::Tone::Plain)
                .on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))));
        }
        let left = state.into_iter().chain([obs]).map(IntoElement::into_any_element).collect();
        div().pt(px(12.)).flex().flex_col().gap(px(16.))
            .child(tools)
            .child(ui::settings_columns(width, left, right))
    }

    /// Compteurs du firmware Rustudio : ce que le pilote officiel comptait, mesuré par la Mini elle-même.
    fn firmware_card(&self) -> Option<Div> {
        if !self.rustudio_firmware() {
            return None;
        }
        let f = self.go.diagnostics.as_ref().map(|d| &d["firmware"]).filter(|f| f.is_object())?;
        let v = firmware_verdict(f);
        let mut card = ui::section(Icon::Activity, ui::GREEN, "Firmware Rustudio", Some(if v.streaming { "Son en cours".into() } else { "Aucun son en cours".into() }))
            .child(ui::status(v.label, v.state));
        let missed_sub = if v.missed > 0 && v.state == ui::State::Ok {
            "1 à 3 paquets à la fermeture d’un flux Windows sont normaux."
        } else {
            "Depuis l’ouverture du son par Windows."
        };
        let underrun_val = if v.overruns > 0 {
            format!("{} (+{} sat.)", v.underruns, v.overruns)
        } else {
            v.underruns.to_string()
        };
        let (loop_val, loop_sub) = firmware_loop_reading(v.max_loop_us, v.slow_loops);
        let rows = [
            (Icon::Plug, "Paquets audio perdus", v.missed.to_string(), missed_sub),
            (Icon::Wave, "Échantillons corrigés", v.corrections.to_string(), "Sautés ou répétés pour rattraper un retard."),
            (Icon::Gauge, "Manques de son", underrun_val, "La puce n’avait plus rien à jouer."),
            (Icon::Activity, "Boucle la plus longue", loop_val, loop_sub),
        ];
        for (i, (glyph, title, value, sub)) in rows.into_iter().enumerate() {
            card = card.child(ui::list_row(Some((glyph, ui::TEXT2)), title, Some(sub.into()), ui::caption(value), i == 0));
        }
        Some(card)
    }

    /// Demande de confirmation GoXLR, affichée en fenêtre par-dessus la page.
    pub(crate) fn go_confirm_box(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (action, body, ask) = self.go.confirm.clone()?;
        let (glyph, colour) = if ask.danger { (Icon::Trash, ui::RED) } else { (Icon::Alert, ui::ORANGE) };
        let cancel = ui::pill("go-cancel", None, "Annuler", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
            this.go.clear_confirm();
            cx.notify();
        }));
        let confirm = ui::pill("go-confirm", None, ask.label, if ask.danger { ui::Tone::Danger } else { ui::Tone::Primary }).on_click(cx.listener(move |this, _, _, cx| {
            this.go.clear_confirm();
            this.go_send(action, body.clone(), cx);
        }));
        Some(ui::dialog(glyph, colour, ask.title, ask.text.into_iter().map(SharedString::from).collect(), vec![cancel.into_any_element(), confirm.into_any_element()]))
    }

    pub fn go_controls(&self, section: Section, width: f32, cx: &mut Context<Self>) -> Div {
        // bloc simple : la page se met en page en une passe (une colonne flexbox multiplie les mesures de tout ce qu'elle contient)
        let mut root = div();
        if section == Section::Diagnostic {
            return root.child(self.diagnostic_view(width, cx));
        }
        let Some(view) = self.go.view.as_ref() else {
            return root.child(ui::card().items_center().py(px(36.)).child(ui::chip(Icon::Refresh, ui::ACCENT, 52.)).child(ui::title("Lecture des réglages", 17.)).child(hint("En attente des réglages de la GoXLR Mini.")));
        };
        if view.device.status != "ready" {
            root = root.child(ui::notice(false, "GoXLR indisponible : réglages désactivés jusqu’à son retour.").mb(px(16.)));
        }
        root.child(match section {
            Section::Routing => self.routing(view, cx),
            Section::Scenes => self.scenes(view, cx),
            Section::Light => self.lighting(view, width, cx),
            Section::Microphone => self.microphone(view, width, cx),
            Section::Headphones => self.headphones(view, width, cx),
            Section::Diagnostic => unreachable!(),
        })
    }
}

const ORANGE_SUN: u32 = 0xff9500;

pub(crate) fn hz(f: f64) -> String {
    if f >= 1000. { format!("{} kHz", fmt_num(f / 1000.)) } else { format!("{} Hz", f.round()) }
}

fn output_info(o: &str) -> (&'static str, &'static str, Icon) {
    match o {
        "Headphones" => ("Casque", "Ce que tu entends", Icon::Headphones),
        "BroadcastMix" => ("Stream", "OBS et enregistrement", Icon::Radio),
        "StreamMix2" => ("Stream 2", "Deuxième mix", Icon::Radio),
        "ChatMic" => ("Discord", "Ce qu’entendent tes amis", Icon::Chat),
        "Sampler" => ("Sampler", "Enregistrement des samples", Icon::Lines),
        "LineOut" => ("Enceintes", "Prise Line Out", Icon::Volume),
        _ => ("Sortie", "", Icon::Volume),
    }
}

fn input_info(i: &str) -> (&'static str, &'static str, Icon) {
    match i {
        "Microphone" | "Mic" => ("Micro", "Ta voix", Icon::Mic),
        "LineIn" => ("Entrée ligne", "Prise Line In", Icon::Plug),
        "Console" => ("Console", "Prise console", Icon::Game),
        "System" => ("Système", "Sons de Windows", Icon::Monitor),
        "Game" => ("Jeu", "Tes jeux", Icon::Game),
        "Chat" => ("Discord", "Appels et vocal", Icon::Chat),
        "Music" => ("Musique", "Spotify, YouTube…", Icon::Music),
        "Samples" | "Sample" => ("Samples", "Sons du sampler", Icon::Lines),
        _ => ("Entrée", "", Icon::Volume),
    }
}

fn scene_icon(id: &str) -> Icon {
    match id {
        "streaming" => Icon::Radio,
        "stream-safe" => Icon::Music,
        "gaming" => Icon::Game,
        "discord" => Icon::Chat,
        "podcast" => Icon::Podcast,
        "ecoute" => Icon::Headphones,
        _ => Icon::Layers,
    }
}

fn preset_icon(id: &str) -> Icon {
    match id {
        "brut" | "naturel" => Icon::Leaf,
        "studio" => Icon::Podcast,
        "radio" => Icon::Tower,
        "stream" => Icon::Radio,
        "podcast" => Icon::Mic,
        _ => Icon::Keyboard,
    }
}
fn preset_colour(id: &str) -> u32 {
    match id {
        "brut" | "naturel" => 0x34c759,
        "studio" => 0x5856d6,
        "radio" => 0xff9500,
        "stream" => 0xff2d55,
        "podcast" => 0x0071e3,
        _ => 0x8e8e93,
    }
}
fn style_icon(id: &str) -> Icon {
    match id {
        "neutre" => Icon::Leaf,
        id if id.contains("basse") || id.contains("rap") => Icon::Volume,
        "jeu" | "competitif" => Icon::Game,
        "voix" => Icon::Mic,
        "film" | "immersion" => Icon::Monitor,
        _ => Icon::Music,
    }
}
pub fn verify(client: &Client) -> Result<(), String> {
    if client.is_live() {
        return Err("Les essais GoXLR exigent un moteur simulé.".into());
    }
    let s = client.go_view()?;
    let route = s.state["routing"]["Chat"]["ChatMic"] == true;
    client.go_command(
        "route",
        json!({"input":"Chat","output":"ChatMic","enabled":!route}),
    )?;
    if client.go_view()?.state["routing"]["Chat"]["ChatMic"] != !route {
        return Err("Routage non relu.".into());
    }
    client.go_command("scene-save", json!({"name":"Scène démo GPUI"}))?;
    client.go_command("scene", json!({"name":"Scène démo GPUI"}))?;
    if client.go_view()?.state["scene"] != "Scène démo GPUI" {
        return Err("Scène non relue.".into());
    }
    client.go_command("theme", json!({"name":"voies"}))?;
    client.go_command("lighting", json!({"mod1":37,"mod2":52}))?;
    client.go_command(
        "colour",
        json!({"target":"MicrophoneMute","colours":["64D2FF"]}),
    )?;
    let s = client.go_view()?;
    if s.state["lighting"]["mod1"] != 37
        || s.state["lighting"]["mod2"] != 52
        || s.state["colours"]["MicrophoneMute"][0] != "64D2FF"
    {
        return Err("Éclairage non relu.".into());
    }
    client.go_command("mic", json!({"gain_db":26}))?;
    client.go_command("dsp", json!({"eq":{"90":3}}))?;
    client.go_command("hp-eq", json!({"style":"neutre"}))?;
    client.go_command("hp-eq-save", json!({"name":"Casque de démo"}))?;
    let s = client.go_view()?;
    if s.state["mic"]["gain_db"] != 26
        || s.state["dsp"]["eq"]["90"] != 3
        || s.state["hp_eq"]["custom"].get("Casque de démo").is_none()
    {
        return Err("Réglages audio non relus.".into());
    }
    let d = client.diagnostics()?;
    if d["support"]["windows"] != "Démonstration" || d["glitches"]["simulated"] != true || d["glitches"]["kernel"]["high"] != false {
        return Err("Diagnostic hors démo.".into());
    }
    crate::voice::analyse(client, &std::sync::atomic::AtomicBool::new(false), true)?;
    if client.get("/api/modules/goxlr-mini/voice")?["summary"].is_null() {
        return Err("L’empreinte vocale simulée n’a pas été relue.".into());
    }
    println!(
        "OK : routage, scènes, éclairage, réglages micro et casque, diagnostic de démonstration."
    );
    Ok(())
}

/// Plage affichée par le niveau d'entrée du micro, en dB.
const LEVEL_FLOOR: f64 = -72.;
/// Vumètre : temps de montée et de retombée (secondes), puis rythme d'affichage (60 images/s suffisent à l'œil).
const LEVEL_RISE_S: f64 = 0.035;
const LEVEL_FALL_S: f64 = 0.28;
pub(crate) const LEVEL_FRAME: std::time::Duration = std::time::Duration::from_micros(16_600);

/// « il y a 5 min », « il y a 3 h », « il y a 2 jours » depuis un instant epoch en secondes.
fn ago(t: f64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(t);
    let s = (now - t).max(0.);
    if s < 90. {
        "à l’instant".into()
    } else if s < 3600. {
        format!("il y a {} min", (s / 60.).round())
    } else if s < 86_400. {
        format!("il y a {} h", (s / 3600.).round())
    } else {
        let d = (s / 86_400.).round();
        format!("il y a {d} jour{}", if d > 1. { "s" } else { "" })
    }
}

/// « 420 Mo » ou « 6,1 Go ».
fn kernel_size(mb: u64) -> String {
    if mb < 1024 {
        format!("{mb} Mo")
    } else {
        format!("{},{} Go", mb / 1024, mb % 1024 * 10 / 1024)
    }
}

/// Conseil affiché quand la mémoire noyau de Windows est anormalement haute.
fn kernel_advice(mb: u64, uptime_h: u64) -> String {
    let since = match uptime_h {
        0..=47 => format!("{uptime_h} h"),
        h => format!("{} jours", h / 24),
    };
    format!(
        "Windows retient {} de mémoire noyau, allumé depuis {since}. À ce niveau, il peut retarder le son de toutes les cartes son et provoquer des micro-coupures, sans défaut de ta GoXLR. Redémarrer Windows remet cette mémoire à zéro.",
        kernel_size(mb)
    )
}

#[derive(PartialEq, Eq)]
struct FirmwareVerdict {
    label: &'static str,
    state: ui::State,
    missed: u64,
    corrections: u64,
    underruns: u64,
    overruns: u64,
    max_loop_us: u64,
    slow_loops: u64,
    streaming: bool,
}

/// Évalue les compteurs du firmware Rustudio sans fausse alerte sur les 1 à 3 paquets « missed » normaux
/// quand Windows ferme un flux audio (arrêt ISO avant SET_INTERFACE alt 0).
fn firmware_verdict(f: &Value) -> FirmwareVerdict {
    let n = |k: &str| f[k].as_u64().unwrap_or(0);
    let streaming = f["streaming"] == true;
    let packets = n("packets");
    let missed = n("missed");
    let underruns = n("underruns");
    let overruns = n("overruns");
    let corrections = n("drops") + n("repeats") + n("jumps");
    let slow_loops = n("slow_loops");
    let max_loop_us = n("max_loop_us");
    let benign_close_missed = missed <= 3 && underruns == 0 && overruns == 0 && corrections == 0 && (!streaming || slow_loops == 0);
    let has_cuts = underruns > 0 || overruns > 0 || corrections > 0 || (missed > 0 && !benign_close_missed);
    let (label, state) = if has_cuts {
        ("Coupures relevées", ui::State::Warn)
    } else if !streaming && packets == 0 {
        ("Prêt (aucun son lu)", ui::State::Ok)
    } else {
        ("Son sans coupure", ui::State::Ok)
    };
    FirmwareVerdict { label, state, missed, corrections, underruns, overruns, max_loop_us, slow_loops, streaming }
}

fn firmware_loop_reading(max_loop_us: u64, slow_loops: u64) -> (String, &'static str) {
    if slow_loops == 0 && max_loop_us > 125 {
        (format!("{max_loop_us} µs (hors audio)"), "Pic de démarrage ou hors flux : aucune boucle lente pendant le son.")
    } else if slow_loops > 0 {
        (
            format!("{max_loop_us} µs ({slow_loops} lente{})", if slow_loops > 1 { "s" } else { "" }),
            "Au-delà de 125 µs, un paquet USB peut se perdre.",
        )
    } else {
        (format!("{max_loop_us} µs"), "Au-delà de 125 µs, un paquet USB peut se perdre.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memoire_noyau_en_clair() {
        assert_eq!(kernel_size(640), "640 Mo");
        assert_eq!(kernel_size(1024), "1,0 Go");
        assert_eq!(kernel_size(6246), "6,0 Go");
        let advice = kernel_advice(6246, 62);
        assert!(advice.contains("6,0 Go") && advice.contains("2 jours") && !advice.contains("  "), "{advice}");
        assert!(kernel_advice(4096, 20).contains("depuis 20 h"));
    }

    #[test]
    fn verdict_firmware_distingue_fermeture_normale_et_vraies_coupures() {
        // 1 a 3 missed a la fermeture du flux sans underrun/correction : sain
        let close_ok = firmware_verdict(&json!({"streaming": false, "packets": 12000, "missed": 2, "underruns": 0, "drops": 0, "repeats": 0, "jumps": 0, "overruns": 0, "slow_loops": 0, "max_loop_us": 420}));
        assert!(close_ok.state == ui::State::Ok);
        assert_eq!(close_ok.label, "Son sans coupure");
        assert!(firmware_loop_reading(close_ok.max_loop_us, close_ok.slow_loops).0.contains("hors audio"));
        // Au repos apres boot (0 paquet, pic initial > 125 us, 0 slow_loops) : pret
        let boot_idle = firmware_verdict(&json!({"streaming": false, "packets": 0, "missed": 0, "underruns": 0, "slow_loops": 0, "max_loop_us": 600}));
        assert!(boot_idle.state == ui::State::Ok);
        assert_eq!(boot_idle.label, "Prêt (aucun son lu)");
        // Vraies coupures : underrun, overrun, correction, >= 4 missed, ou missed + slow_loops en streaming
        for bad in [
            json!({"streaming": true, "packets": 5000, "missed": 4, "underruns": 0, "slow_loops": 0}),
            json!({"streaming": true, "packets": 5000, "missed": 1, "underruns": 0, "slow_loops": 2}),
            json!({"streaming": true, "packets": 5000, "missed": 0, "underruns": 1}),
            json!({"streaming": false, "packets": 5000, "missed": 0, "overruns": 1}),
            json!({"streaming": true, "packets": 5000, "missed": 0, "drops": 1}),
        ] {
            let v = firmware_verdict(&bad);
            assert!(v.state == ui::State::Warn, "{bad}");
            assert_eq!(v.label, "Coupures relevées", "{bad}");
        }
        assert!(firmware_loop_reading(180, 3).0.contains("3 lentes"));
        assert_eq!(firmware_loop_reading(42, 0).0, "42 µs");
    }
}


#[cfg(test)]
mod monitor_tests {
    use super::*;

    #[test]
    fn depart_a_cinq_et_niveau_jusqua_cent_pour_cent() {
        assert_eq!(monitor_percent(MONITOR_START), 5.);
        assert_eq!(monitor_level(-1.), 3);
        assert_eq!(monitor_percent(monitor_level(42.)), 42.);
        assert_eq!(monitor_level(100.), 255);
        assert_eq!(monitor_level(101.), 255);
    }
}
