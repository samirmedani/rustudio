//! Réglages du micro Windows et suivi de son effet de capture dans la session utilisateur.
use crate::{
    desktop::{Page, hint},
    goxlr::Section,
    mic_filters::{MicFilter, MicTab, mic_filter_columns, mic_signal},
    mixer::Mixer,
    ui::{self, Icon, State, Tone, alpha, rgb},
};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Window, div, prelude::*, px};
use rustudio_windows_desktop::{audio, soundcheck, spectrum};
use rustudio_windows_mic as wm;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const MODULE: &str = "/api/modules/windows-mic";
/// Durée de l'enregistrement de test.
const TEST_SECS: f64 = 6.;
/// Fréquences de la courbe de l'égaliseur (échelle logarithmique, 50 Hz à 16 kHz).
const CURVE_POINTS: usize = 40;

/// Confirmation en attente : chacune redémarre le son de Windows ou retire un réglage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Ask {
    Setup,
    Noise(bool),
    LegacyNoise,
    LegacyNoiseRemove,
    Remove(bool),
}

#[derive(Clone, Debug, Default, PartialEq)]
enum Test {
    #[default]
    Idle,
    Recording,
    Playing,
    Done,
    Failed(String),
}

#[derive(Default)]
pub struct WinMicPage {
    /// État du module lu au moteur (réglages, micros, Equalizer APO, suppression du bruit).
    view: Option<Value>,
    /// Micros par défaut de Windows (identifiants complets) : applis et appels.
    defaults: (Option<String>, Option<String>),
    /// Volume Windows du micro affiché ; en simulation, une valeur locale.
    level: Option<audio::Level>,
    windows_options: bool,
    pub(crate) prefer_goxlr: bool,
    pub(crate) hardware_active: bool,
    level_busy: bool,
    level_pending: VecDeque<(String, f32)>,
    pub(crate) busy: bool,
    pending: VecDeque<(&'static str, Value)>,
    revision: u64,
    refreshing: bool,
    reading_meters: bool,
    notice: Option<(bool, String)>,
    monitor: Option<spectrum::Monitor>,
    monitored_source: Option<spectrum::Source>,
    frame: spectrum::Frame,
    shown_db: Option<f64>,
    active: bool,
    poll_gen: u64,
    test: Test,
    test_stop: Arc<AtomicBool>,
    ask: Option<(Ask, String)>,
}

/// Identifiant IMMDevice d'une entrée de Windows, depuis son identifiant de registre « {…} ».
fn endpoint_id(guid: &str) -> String {
    format!("{{0.0.1.00000000}}.{}", guid.to_lowercase())
}

fn same_input(id: Option<&String>, guid: &str) -> bool {
    id.and_then(|i| i.rsplit('.').next()).is_some_and(|g| g.eq_ignore_ascii_case(guid))
}

fn capture_label(capture: &Value) -> Option<String> {
    let name = capture["name"].as_str()?;
    if capture["goxlr"] == true && !name.to_lowercase().contains("goxlr") {
        Some(format!("GoXLR Mini · {name}"))
    } else {
        Some(name.to_owned())
    }
}

/// Fusionne un réglage en attente avec le suivant : un glissement rapide ne perd aucune bande.
fn merge(dst: &mut Value, src: &Value) {
    let (Some(d), Some(s)) = (dst.as_object_mut(), src.as_object()) else { return };
    for (k, v) in s {
        match (d.get_mut(k), v) {
            (Some(old), Value::Object(_)) if old.is_object() => merge(old, v),
            _ => {
                d.insert(k.clone(), v.clone());
            }
        }
    }
}

fn preset_icon(id: &str) -> (Icon, u32) {
    match id {
        "natural" => (Icon::Leaf, ui::GREEN),
        "warm" => (Icon::Sun, ui::ORANGE),
        "radio" => (Icon::Radio, 0xff2d55),
        "clear" => (Icon::Chat, ui::ACCENT),
        _ => (Icon::Wave, 0x5856d6),
    }
}

impl WinMicPage {
    #[cfg(debug_assertions)]
    pub(crate) fn load_layout_fixture(&mut self, mut state: Value, hardware: bool) {
        let selected = state["live"]["captures"].as_array().into_iter().flatten()
            .find(|c| c["goxlr"] == hardware).map(|c| c["guid"].clone()).unwrap_or(Value::Null);
        state["state"]["selected"] = selected;
        state["live"]["native"]["attached"] = json!(!hardware);
        state["live"]["installed"] = json!(!hardware);
        self.view = Some(state);
        self.hardware_active = hardware;
    }

    fn live(&self) -> &Value {
        static NULL: Value = Value::Null;
        self.view.as_ref().map(|v| &v["live"]).unwrap_or(&NULL)
    }
    fn selected(&self) -> Option<String> {
        self.view.as_ref()?["state"]["selected"].as_str().map(str::to_owned)
    }
    fn voice(&self) -> wm::Voice {
        self.view.as_ref().and_then(|v| serde_json::from_value(v["state"]["voice"].clone()).ok()).unwrap_or_default()
    }
    fn capture(&self, guid: &str) -> Option<&Value> {
        self.live()["captures"].as_array()?.iter().find(|c| c["guid"].as_str().is_some_and(|g| g.eq_ignore_ascii_case(guid)))
    }
    fn noise_input(&self) -> Option<(String, String)> {
        let guid = self.selected()?;
        let capture = self.capture(&guid)?;
        if self.hardware_active && capture["goxlr"] != true {
            return None;
        }
        Some((guid, capture_label(capture)?))
    }

    fn legacy_noise_ready(&self) -> bool {
        self.live()["legacy"] == true && self.live()["apo"] == true && self.live()["plugin"] == true
    }
    fn goxlr_guid(&self) -> Option<String> {
        if let Some(g) = self.selected().filter(|g| self.capture(g).is_some_and(|c| c["goxlr"] == true)) {
            return Some(g);
        }
        self.live()["captures"].as_array()?.iter().find(|c| c["goxlr"] == true)?["guid"].as_str().map(str::to_owned)
    }
    fn hardware_choice(&self, status: Option<&str>) -> bool {
        if status.is_some_and(|s| s != "ready") { return false; }
        let selected = self.selected();
        self.prefer_goxlr || selected.as_deref().and_then(|g| self.capture(g)).is_some_and(|c| c["goxlr"] == true)
            || (status == Some("ready") && self.view.is_some() && selected.is_none())
    }
    fn confirm(&mut self, action: Ask) {
        self.ask = self.selected().map(|g|(action,g));
    }
    fn setup_running(&self) -> bool {
        self.live()["setup"]["running"] == true
    }

    fn queue_level(&mut self, guid: String, scalar: f32) {
        if let Some((_, value)) = self.level_pending.iter_mut().find(|(id, _)| id.eq_ignore_ascii_case(&guid)) {
            *value = scalar;
        } else {
            self.level_pending.push_back((guid, scalar));
        }
    }

    fn preview_settings(&mut self, body: &Value) {
        let Some(guid) = self.selected() else { return };
        if body.get("selected").is_some() || !body["device"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(&guid)) {
            return;
        }
        let base = wm::Settings {
            selected: Some(guid.clone()),
            mics: [(guid.to_ascii_lowercase(), self.voice())].into(),
        };
        if let Ok(next) = wm::update(body, &base)
            && let Some(view) = self.view.as_mut()
        {
            view["state"]["voice"] = json!(next.voice(&guid));
        }
    }
}

impl Mixer {
    /// Un seul flux de mesures pour la source affichée. L’ancien lien « mic » cible la GoXLR.
    pub(crate) fn winmic_sync_source(&mut self, cx: &mut Context<Self>) {
        let visible = self.page == Page::WinMic;
        let status = self.device("goxlr-mini").map(|d| d.status.as_str());
        let hardware = visible && self.winmic.hardware_choice(status);
        let previous = std::mem::replace(&mut self.winmic.hardware_active, hardware);
        self.client.set_go_visible(hardware || matches!(self.page, Page::Go(_)));
        if hardware && !previous { self.load_voice_info(cx); }
        if previous && !hardware {
            if !self.page.is_goxlr() && self.go.owns_monitor() { self.cut_monitor(cx); }
            self.voice.cancel_preview();
            self.go.clear_confirm();
        }
        self.winmic_sync_monitor();
        if visible && self.winmic.prefer_goxlr && !self.winmic.busy
            && let Some(target) = self.winmic.goxlr_guid()
        {
            self.winmic.prefer_goxlr = false;
            if self.winmic.selected().as_ref() != Some(&target) {
                self.winmic_send("settings", json!({"selected": target}), cx);
            }
        }
    }
    /// Ouvre ou ferme la page : vumètre (capture partagée) et relecture de l'état une fois par seconde.
    pub(crate) fn winmic_visibility(&mut self, visible: bool, cx: &mut Context<Self>) {
        let was = self.winmic.active;
        self.winmic.active = visible;
        if !visible {
            self.winmic.test_stop.store(true, Ordering::Relaxed);
            return;
        }
        if self.winmic.monitor.is_none() {
            self.winmic.monitor = Some(spectrum::Monitor::new(!self.client.is_live()));
        }
        if was {
            return;
        }
        self.winmic.poll_gen = self.winmic.poll_gen.wrapping_add(1);
        let token = self.winmic.poll_gen;
        self.winmic_refresh(cx);
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let mut tick = 0u32;
            loop {
                executor.timer(Duration::from_millis(50)).await;
                tick = tick.wrapping_add(1);
                let keep = this
                    .update(cx, |this, cx| {
                        if !this.winmic.active || this.winmic.poll_gen != token {
                            return false;
                        }
                        if let Some(m) = &this.winmic.monitor {
                            let frame = m.frame();
                            if frame.version != this.winmic.frame.version {
                                let db = (frame.ready && frame.peak_db > spectrum::FLOOR).then_some(f64::from(frame.peak_db));
                                // la barre retombe doucement, la valeur suit la mesure
                                this.winmic.shown_db = match (db, this.winmic.shown_db) {
                                    (Some(d), Some(s)) if d < s => Some(s - (s - d) * 0.25),
                                    (d, _) => d,
                                };
                                this.winmic.frame = frame;
                                cx.notify();
                            }
                        }
                        if tick % 2 == 0 { this.winmic_read_meters(cx); }
                        if tick % 20 == 0 {
                            this.winmic_refresh(cx);
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    /// Le vumètre écoute le seul micro affiché.
    fn winmic_sync_monitor(&mut self) {
        let Some(m) = &self.winmic.monitor else { return };
        let source = self.winmic.selected().filter(|_| self.winmic.active && !self.winmic.hardware_active).map(|g| spectrum::Source {
            id: if self.client.is_live() { endpoint_id(&g) } else { "demo-mic".into() },
            output: false,
        });
        if self.winmic.monitored_source == source { return; }
        self.winmic.monitored_source = source.clone();
        self.winmic.frame = spectrum::Frame::default();
        self.winmic.shown_db = None;
        match source {
            Some(s) => m.configure(vec![s.clone()], Some(s)),
            None => m.stop(),
        }
    }

    fn winmic_read_meters(&mut self, cx: &mut Context<Self>) {
        if self.winmic.hardware_active || self.winmic.reading_meters || self.winmic.live()["native"]["attached"] != true { return; }
        let Some(guid) = self.winmic.selected() else { return };
        self.winmic.reading_meters = true;
        let client = self.client.clone();
        // Les accolades d'un GUID ne sont pas normalisées de la même façon par tous les clients HTTP.
        let path = format!("{MODULE}/meters?device={}",guid.trim_matches(['{','}']));
        let work = cx.background_executor().spawn(async move { client.get(&path).ok() });
        cx.spawn(async move |this,cx| {
            let levels = work.await.and_then(|v|v.get("levels").cloned()).unwrap_or(Value::Null);
            let _ = this.update(cx, |this,cx| {
                this.winmic.reading_meters=false;
                if this.winmic.active && this.winmic.selected().as_ref() == Some(&guid)
                    && let Some(view) = this.winmic.view.as_mut()
                    && view["live"]["native"].is_object() && view["live"]["native"]["levels"] != levels {
                    view["live"]["native"]["levels"] = levels;
                    cx.notify();
                }
            });
        }).detach();
    }

    /// Relit l'état du module, les micros par défaut et le volume Windows du micro affiché.
    pub(crate) fn winmic_refresh(&mut self, cx: &mut Context<Self>) {
        if self.winmic.busy || self.winmic.refreshing { return; }
        self.winmic.refreshing = true;
        let rev = self.winmic.revision;
        let client = self.client.clone();
        let live = client.is_live();
        let work = cx.background_executor().spawn(async move {
            let view = client.get(&format!("{MODULE}/state")).ok();
            let selected = view.as_ref().and_then(|v| v["state"]["selected"].as_str().map(str::to_owned));
            let (defaults, level) = if live {
                let inv = audio::inventory();
                let pick = |key: &str| inv["inputs"].as_array().into_iter().flatten().find(|r| r[key] == true).and_then(|r| r["id"].as_str().map(str::to_owned));
                ((pick("default"), pick("communications")), selected.as_deref().and_then(|g| audio::level(&endpoint_id(g))))
            } else {
                ((None, None), None)
            };
            (view, defaults, level)
        });
        cx.spawn(async move |this, cx| {
            let (view, defaults, level) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.winmic.refreshing = false;
                if this.winmic.busy || !this.winmic.active || rev != this.winmic.revision {
                    return;
                }
                let before = this.winmic.selected();
                this.winmic.view = view;
                this.winmic.defaults = defaults;
                if live {
                    if !this.winmic.level_busy {
                        this.winmic.level = level;
                    }
                } else if this.winmic.level.is_none() {
                    this.winmic.level = Some(audio::Level { scalar: 0.7, db: Some(-4.5), range: Some((-60., 12.)), muted: false });
                }
                if before != this.winmic.selected() {
                    this.winmic.ask = None;
                    this.winmic.test_stop.store(true, Ordering::Relaxed);
                }
                this.winmic_sync_source(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Envoie une commande au module ; pendant un envoi, les réglages suivants se fusionnent et partent ensuite.
    pub(crate) fn finish_microphone_profile(&mut self, cx: &mut Context<Self>) {
        self.winmic.revision = self.winmic.revision.wrapping_add(1);
        self.winmic.busy = false;
        if let Some((action, body)) = self.winmic.pending.pop_front() {
            self.winmic_send(action, body, cx);
        } else if self.winmic.active {
            self.winmic_refresh(cx);
        }
    }

    pub(crate) fn winmic_send(&mut self, action: &'static str, mut body: Value, cx: &mut Context<Self>) {
        if body.get("device").is_none() && body.get("selected").is_none() {
            if let Some(g)=self.winmic.selected(){body["device"]=json!(g);}
        }
        if self.winmic.busy {
            match self.winmic.pending.back_mut() {
                Some((a, b)) if *a == "settings" && action == "settings" && b.get("device").is_some() && b["device"] == body["device"] && b.get("selected").is_none() && body.get("selected").is_none() && b.get("preset").is_none() && body.get("preset").is_none() => merge(b, &body),
                _ => self.winmic.pending.push_back((action, body)),
            }
            return;
        }
        self.winmic.busy = true;
        self.winmic.revision = self.winmic.revision.wrapping_add(1);
        self.winmic.notice = None;
        // l'écran suit tout de suite le réglage, sans attendre le moteur
        if action == "settings" {
            self.winmic.preview_settings(&body);
        }
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            match client.post_module(MODULE, action, body) {
                Ok(view) if view["state"].is_object() && view["live"].is_object() => (Ok(()), Some(view)),
                result => (result.map(|_| ()), client.get(&format!("{MODULE}/state")).ok()),
            }
        });
        cx.spawn(async move |this, cx| {
            let (result, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.winmic.busy = false;
                let before = this.winmic.selected();
                if view.is_some() {
                    this.winmic.view = view;
                }
                if let Err(e) = result {
                    this.winmic.notice = Some((false, e));
                }
                if before != this.winmic.selected() {
                    this.winmic.ask = None;
                    this.winmic.level = None;
                    this.winmic.test_stop.store(true, Ordering::Relaxed);
                    this.winmic_refresh(cx);
                }
                this.winmic_sync_source(cx);
                if let Some((a, b)) = this.winmic.pending.pop_front() {
                    this.winmic_send(a, b, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Volume Windows : appliqué dans la session de l'utilisateur, un réglage à la fois, le dernier gagne.
    fn winmic_set_level(&mut self, scalar: f32, cx: &mut Context<Self>) {
        let Some(guid) = self.winmic.selected() else { return };
        if let Some(l) = self.winmic.level.as_mut() {
            l.scalar = scalar;
            l.db = None;
        }
        if self.client.is_live() {
            self.winmic_write_level(guid, scalar, cx);
        }
        cx.notify();
    }

    fn winmic_write_level(&mut self, guid: String, scalar: f32, cx: &mut Context<Self>) {
        if self.winmic.level_busy {
            self.winmic.queue_level(guid, scalar);
            return;
        }
        self.winmic.level_busy = true;
        let id = endpoint_id(&guid);
        let work = cx.background_executor().spawn(async move {
            let r = audio::set_level(&id, scalar);
            (r, audio::level(&id))
        });
        cx.spawn(async move |this, cx| {
            let (r, level) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.winmic.level_busy = false;
                match r {
                    Ok(()) if this.winmic.selected().as_ref() == Some(&guid)
                        && !this.winmic.level_pending.iter().any(|(g, _)| g == &guid) => this.winmic.level = level,
                    Ok(()) => {}
                    Err(e) => this.winmic.notice = Some((false, format!("Windows refuse le volume du micro {guid} ({e})."))),
                }
                if let Some((target, value)) = this.winmic.level_pending.pop_front() {
                    this.winmic_write_level(target, value, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn winmic_windows(&mut self, what: &'static str, cx: &mut Context<Self>) {
        let Some(guid) = self.winmic.selected() else { return };
        if !self.client.is_live() {
            if what == "mute"
                && let Some(l) = self.winmic.level.as_mut()
            {
                l.muted = !l.muted;
            }
            cx.notify();
            return;
        }
        let id = endpoint_id(&guid);
        let muted = self.winmic.level.is_some_and(|l| l.muted);
        let work = cx.background_executor().spawn(async move {
            let r = if what == "mute" { audio::set_mute(&id, !muted) } else { audio::set_default_input(&id) };
            (r, audio::level(&id))
        });
        cx.spawn(async move |this, cx| {
            let (r, level) = work.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(()) if this.winmic.selected().as_ref() == Some(&guid) => {
                        this.winmic.level = level.or(this.winmic.level);
                        if what == "default" {
                            this.winmic.notice = Some((true, "Windows utilise maintenant ce micro, pour les applis comme pour les appels.".into()));
                        }
                    }
                    Ok(()) => {}
                    Err(e) => this.winmic.notice = Some((false, format!("Windows refuse : {e}"))),
                }
                this.winmic_refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Enregistre quelques secondes du micro tel que les applis le reçoivent, puis le rejoue sur la sortie de Windows.
    fn winmic_test(&mut self, cx: &mut Context<Self>) {
        if matches!(self.winmic.test, Test::Recording | Test::Playing) {
            self.winmic.test_stop.store(true, Ordering::Relaxed);
            return;
        }
        let Some(guid) = self.winmic.selected() else { return };
        if !self.client.is_live() {
            self.winmic.test = Test::Failed("Le test enregistre un vrai micro : disponible hors simulation.".into());
            cx.notify();
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.winmic.test_stop = stop.clone();
        self.winmic.test = Test::Recording;
        cx.notify();
        let id = endpoint_id(&guid);
        let rec = cx.background_executor().spawn({
            let stop = stop.clone();
            async move { soundcheck::record_input(&id, TEST_SECS, stop) }
        });
        cx.spawn(async move |this, cx| {
            let recorded = rec.await;
            let take = match recorded {
                Ok((samples, rate)) if !samples.is_empty() => Some((samples, rate)),
                Ok(_) => None,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        this.winmic.test = Test::Failed(e);
                        cx.notify();
                    });
                    return;
                }
            };
            let cancelled = stop.load(Ordering::Relaxed);
            let go_on = this
                .update(cx, |this, cx| {
                    this.winmic.test = if cancelled || take.is_none() { Test::Idle } else { Test::Playing };
                    cx.notify();
                    !cancelled && take.is_some()
                })
                .unwrap_or(false);
            let Some((samples, rate)) = take.filter(|_| go_on) else { return };
            let played = cx.background_executor().spawn(async move { soundcheck::play_voice_preview(samples, rate, stop) }).await;
            let _ = this.update(cx, |this, cx| {
                this.winmic.test = match played {
                    Ok(()) => Test::Done,
                    Err(e) => Test::Failed(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Confirmation liée à l'identifiant du micro affiché lors du clic.
    pub(crate) fn winmic_confirm_box(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (ask, guid) = self.winmic.ask.clone()?;
        if self.winmic.selected().as_deref() != Some(guid.as_str()) {
            return None;
        }
        let (title, label, mut text): (&str, &str, Vec<SharedString>) = match ask {
            Ask::Setup => ("Traiter la voix de ce micro ?", "Activer", vec![
                "Rustudio branche son effet sur ce micro. Les applications qui utilisent les effets Windows reçoivent ta voix réglée.".into(),
            ]),
            Ask::Noise(on) => (if on {"Activer la suppression du bruit ?"} else {"Couper la suppression du bruit ?"}, "Appliquer", vec![
                "Le filtre RNNoise fonctionne sur ton PC. Il ajoute environ 20 ms à la dynamique pendant son utilisation.".into(),
            ]),
            Ask::LegacyNoise => {
                let mut text = vec!["Rustudio installe RNNoise et Equalizer APO si nécessaire, puis active le filtre sur cette entrée.".into()];
                if let Some(other) = self.winmic.live()["ns"]["elsewhere"].as_str() {
                    text.push(format!("Le filtre Equalizer APO sera déplacé depuis « {other} ».").into());
                }
                ("Activer la suppression du bruit ?", "Activer", text)
            }
            Ask::LegacyNoiseRemove => ("Retirer la suppression du bruit ?", "Retirer", vec![
                "Rustudio retire Equalizer APO de cette entrée.".into(),
            ]),
            Ask::Remove(_) => ("Retirer Rustudio de ce micro ?", "Retirer", vec![
                "Ta voix repart telle que Windows la capte : sans égaliseur ni suppression du bruit. Tes réglages restent gardés pour plus tard.".into(),
            ]),
        };
        if let Some(name) = self.winmic.capture(&guid).and_then(capture_label) {
            text.insert(0, format!("Micro sélectionné : {name}").into());
        }
        if self.winmic.live()["legacy"] == true && matches!(ask, Ask::Setup | Ask::Noise(_)) {
            text.push("Les réglages Rustudio de ce micro passent sur le nouvel effet. Les autres périphériques gardent leur configuration.".into());
        }
        if ask != Ask::LegacyNoise || !self.winmic.legacy_noise_ready() {
            text.push("Le son de Windows redémarre : environ 2 secondes de silence.".into());
        }
        let remove = matches!(ask, Ask::Remove(_) | Ask::LegacyNoiseRemove);
        let cancel = ui::pill("winmic-ask-cancel", None, "Annuler", Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
            this.winmic.ask = None;
            cx.notify();
        }));
        let ok = ui::pill("winmic-ask-ok", None, label, if remove { Tone::Danger } else { Tone::Primary }).on_click(cx.listener(move |this, _, _, cx| {
            this.winmic.ask = None;
            if this.winmic.selected().as_deref() != Some(guid.as_str()) {
                cx.notify();
                return;
            }
            match ask {
                Ask::Setup => this.winmic_send("attach", json!({"device":guid}), cx),
                Ask::Noise(on) => this.winmic_send("native-noise", json!({"device":guid,"enabled": on}), cx),
                Ask::LegacyNoise => this.winmic_send("noise", json!({"device":guid,"enabled":true}), cx),
                Ask::LegacyNoiseRemove => this.winmic_send("remove", json!({"device":guid}), cx),
                Ask::Remove(native) => this.winmic_send(if native {"detach"} else {"remove"}, json!({"device":guid}), cx),
            }
        }));
        Some(ui::dialog(if remove { Icon::Alert } else { Icon::Mic }, if remove { ui::ORANGE } else { ui::ACCENT }, title, text, vec![cancel.into_any_element(), ok.into_any_element()]))
    }

    pub(crate) fn winmic_view(&self, width: f32, window: &Window, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let spacious = width >= 900.;
        let mut root = div().w_full().min_w(px(0.)).flex().flex_col().gap(px(if spacious { 16. } else { 12. }));
        let selected = w.selected();
        if !w.hardware_active && selected.is_none() {
            let text = if w.view.is_none() { "Lecture des micros de Windows…" } else { "Aucun micro détecté." };
            return root.child(ui::card().child(hint(text)));
        }
        if w.hardware_active && self.wizard.is_some() {
            return root.child(self.go_controls(Section::Microphone, width, cx));
        }
        if let Some((ok, text)) = &w.notice {
            root = root.child(ui::notice(*ok, text.clone()));
        }
        if w.hardware_active && let Some((false, text)) = &self.go.notice {
            root = root.child(ui::notice(false, text.clone()));
        }
        let setup = &w.live()["setup"];
        if setup["running"] == true {
            root = root.child(ui::status(setup["step"].as_str().unwrap_or("En cours…").to_owned(), State::Info));
        } else if let Some(e) = setup["error"].as_str() {
            root = root.child(ui::notice(false, e.to_owned()));
        }

        let source = selected.as_deref().filter(|id| {
            !w.hardware_active || w.capture(id).is_some_and(|c| c["goxlr"] == true)
        });
        let preferred = match self.motion.target() {
            Some(crate::motion::Destination::Microphone(tab)) => tab,
            _ => self.mic_tab,
        };
        let mut tabs = Vec::with_capacity(MicTab::ALL.len());
        let mut selected_tab = 0;
        for (index, (tab, icon, label)) in MicTab::ALL.into_iter().enumerate() {
            if tab == preferred { selected_tab = index; }
            tabs.push(ui::motion_tab(("windows-mic-tab", index), icon, label, preferred == tab)
                .text_size(px(if spacious { 17. } else { 15. }))
                .h(px(if spacious { 46. } else { 40. }))
                .on_click(cx.listener(move |this, _, _, cx| this.navigate_to(crate::motion::Destination::Microphone(tab), cx)))
                .into_any_element());
        }
        let tabs = self.motion.tabs(crate::motion::Surface::Microphone, selected_tab, tabs);

        let body = if w.hardware_active {
            let hardware = self.go_controls(Section::Microphone, width, cx);
            if self.mic_tab == MicTab::Input && let Some(guid) = source {
                div().flex().flex_col().gap(px(18.)).child(hardware).child(self.winmic_go_options(guid, cx))
            } else {
                hardware
            }
        } else {
            match self.mic_tab {
                MicTab::Input => source.map_or_else(div, |id| self.winmic_input(id, cx)),
                MicTab::Filters => self.winmic_filters(width, cx),
                MicTab::Presets => self.winmic_voice(width, cx),
                MicTab::Listen => self.winmic_test_card(cx),
            }
        };
        let body = if self.mic_tab == MicTab::Input || (self.mic_tab == MicTab::Listen && !w.hardware_active) {
            div().w_full().max_w(px(840.)).mx_auto().child(body)
        } else {
            body
        };
        root.child(self.winmic_selector(source, cx)).child(tabs)
            .child(body.opacity(self.surface_opacity(crate::motion::Surface::Microphone, window)))
    }

    fn winmic_go_options(&self, id: &str, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let native = &w.live()["native"];
        let toggle = ui::pill("winmic-windows-options", Some(Icon::Sliders),
            if w.windows_options { "Masquer les options Windows" } else { "Options Windows" }, Tone::Plain)
            .on_click(cx.listener(|this, _, _, cx| { this.winmic.windows_options = !this.winmic.windows_options; cx.notify(); }));
        ui::card().child(div().flex().child(toggle)).when(w.windows_options, |d| {
            d.child(self.winmic_input(id, cx))
                .when(native["safe_mode"] == true, |d| d.child(ui::status("Effet Windows en mode de secours.", State::Warn)))
                .when(native["attached"] == true || native["lost"] == true, |d| {
                    d.child(div().flex().child(ui::pill("winmic-remove-goxlr-apo", None, "Retirer l’effet Windows", Tone::Plain)
                        .on_click(cx.listener(|this, _, _, cx| { this.winmic.confirm(Ask::Remove(true)); cx.notify(); }))))
                })
        })
    }

    fn winmic_selector(&self, guid: Option<&str>, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let mut caps: Vec<(Option<String>, String)> = w.live()["captures"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some((Some(c["guid"].as_str()?.to_owned()), capture_label(c)?)))
            .collect();
        if w.goxlr_guid().is_none() && self.device("goxlr-mini").is_some_and(|d| d.status == "ready") {
            caps.push((None, "Micro de la GoXLR Mini".into()));
        }
        let chosen = |id: &Option<String>| match (id.as_deref(), guid) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (None, None) => true,
            _ => false,
        };
        let current = if w.hardware_active {
            "GoXLR Mini".into()
        } else {
            caps.iter().find(|(g, _)| chosen(g)).map(|(_, n)| n.clone()).unwrap_or_else(|| "Choisir un micro".into())
        };
        let options = caps.iter().map(|(g, n)| (n.clone(), chosen(g))).collect();
        div().w_full().max_w(px(380.)).child(self.dropdown("winmic-device", current, options, !w.busy, move |this, i, cx| {
                if let Some(g) = &caps[i].0 {
                    this.winmic.prefer_goxlr = false;
                    this.winmic_send("settings", json!({"selected":g}), cx);
                } else {
                    this.winmic.prefer_goxlr = true;
                    this.winmic_sync_source(cx);
                    cx.notify();
                }
            }, cx))
    }

    fn winmic_input(&self, guid: &str, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let is_default = same_input(w.defaults.0.as_ref(), guid);
        let is_calls = same_input(w.defaults.1.as_ref(), guid);
        let mut windows = div().flex().flex_wrap().items_center().gap(px(10.));
        if self.client.is_live() && is_default && is_calls {
            windows = windows.child(ui::status("Micro de Windows, pour les applis et les appels", State::Ok));
        } else if self.client.is_live() {
            let text = if is_default { "Micro de Windows, mais pas pour les appels" } else if is_calls { "Micro des appels seulement" } else { "Windows utilise un autre micro" };
            windows = windows
                .child(ui::status(text, State::Warn))
                .child(ui::pill("winmic-default", Some(Icon::Check), "Utiliser ce micro dans Windows", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.winmic_windows("default", cx))));
        } else {
            windows = windows.child(ui::status("Simulation : Windows n’est pas modifié", State::Info));
        }
        let level = w.level;
        let muted = level.is_some_and(|l| l.muted);
        let volume = level.map(|l| f64::from(l.scalar) * 100.).unwrap_or(0.);
        let db = level.and_then(|l| l.db);
        let voice = w.voice();
        let can_adjust = voice.enabled && !w.busy && !w.setup_running();
        div().flex().flex_col().gap(px(14.))
            .child(div().flex().flex_wrap().items_center().justify_between().gap(px(12.))
                .child(ui::card_title(Some((Icon::Gauge, ui::ACCENT)), "Niveau dans Windows", None))
                .when(!w.hardware_active, |d| d.child(mic_signal(
                    if muted { None } else { w.shown_db },
                    (!muted && w.frame.ready).then_some(f64::from(w.frame.peak_db.max(-72.))),
                ))))
            .child(self.slider_row(
                "winmic-volume", "Volume du micro dans Windows", volume, (0., 100.), 1.,
                move |v| match db {
                    Some(d) => format!("{} % · {} dB", v.round(), fmt_db(f64::from(d))),
                    None => format!("{} %", v.round()),
                },
                level.is_some(), |this, v, cx| this.winmic_set_level((v / 100.) as f32, cx), cx,
            ))
            .when(!w.hardware_active && w.live()["installed"] == true, |d| d.child(self.slider_row(
                "winmic-gain", "Gain ajouté par Rustudio", f64::from(voice.gain_db),
                (f64::from(wm::GAIN_RANGE.0), f64::from(wm::GAIN_RANGE.1)), 1.,
                |x| format!("{} dB", fmt_db(x)), can_adjust,
                |this, x, cx| this.winmic_send("settings", json!({"gain_db": x.round() as i64}), cx), cx,
            )))
            .child(div().flex().flex_wrap().items_center().gap(px(12.))
                .child(ui::pill("winmic-mute", Some(if muted { Icon::MicOff } else { Icon::Mic }),
                    if muted { "Réactiver dans Windows" } else { "Couper dans Windows" },
                    if muted { Tone::Warn } else { Tone::Plain })
                    .on_click(cx.listener(|this, _, _, cx| this.winmic_windows("mute", cx))))
                .child(windows))
    }

    /// Ta voix : marche, préréglages, gain et coupe-bas.
    fn winmic_voice(&self, width: f32, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let v = w.voice();
        let installed = w.live()["installed"] == true;
        let busy = w.busy || w.setup_running();
        let mut p = ui::section(Icon::Wave, 0x5856d6, "Préréglages", None)
            .child(if busy {
                ui::status("Envoi des réglages…", State::Info)
            } else if v.enabled && installed {
                ui::status("Traitement actif sur cette entrée", State::Ok)
            } else {
                ui::status("Traitement en pause", State::Info)
            });
        if w.live()["native"]["attached"] != true {
            p = p
                .child(hint("Donne à ta voix plus de corps ou de clarté et retire les grondements du bureau, pour Discord, OBS et tes jeux à la fois. Rustudio règle l’entrée du micro dans Windows, sans logiciel à garder ouvert."))
                .child(div().flex().child(ui::pill("winmic-setup", Some(Icon::Plug), if installed {"Passer à l’effet Rustudio"} else {"Activer le traitement de ma voix"}, Tone::Primary).on_click(cx.listener(move |this, _, _, cx| {
                    if !busy {
                        this.winmic.confirm(Ask::Setup);
                        cx.notify();
                    }
                }))));
            if !installed {return p;}
        }
        let on = v.enabled;
        p = p.child(ui::list_row(None, if on { "Traitement actif" } else { "Traitement coupé" }, Some("Coupé, ta voix part telle que le micro la capte.".into()), ui::toggle("winmic-enabled", on, !busy).on_click(cx.listener(move |this, _, _, cx| this.winmic_send("settings", json!({"enabled": !on}), cx))), true));
        let mut tiles: Vec<AnyElement> = vec![];
        for preset in &wm::PRESETS {
            let selected = v.preset.as_deref() == Some(preset.id);
            let id = preset.id;
            tiles.push(
                ui::tile(SharedString::from(format!("winmic-preset-{id}")), Some(preset_icon(id)), preset.label, Some(preset.desc.into()), selected, None)
                    .on_click(cx.listener(move |this, _, _, cx| this.winmic_send("settings", json!({"preset": id, "enabled": true}), cx)))
                    .into_any_element(),
            );
        }
        p.child(ui::grid(tiles, ui::columns_that_fit(width, 200., 10., 3)))
    }

    /// Égaliseur 5 bandes et sa courbe.
    fn winmic_eq(&self, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let v = w.voice();
        let on = v.enabled && w.live()["installed"] == true;
        let busy = w.busy || w.setup_running();
        let mut bands = div().flex().gap(px(4.));
        for (i, band) in wm::BANDS.iter().enumerate() {
            let key = band.key;
            let below = div()
                .child(div().flex().justify_center().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(ui::TEXT)).child(band.label))
                .child(div().flex().justify_center().text_size(px(11.)).text_color(rgb(ui::TEXT3)).child(crate::goxlr::hz(band.hz)))
                .child(div().mt(px(4.)).px(px(2.)).w_full().min_w(px(0.)).whitespace_normal().text_center().text_size(px(11.)).line_height(px(14.)).text_color(rgb(ui::TEXT2)).child(band.hint));
            bands = bands.child(self.eq_column(&format!("winmic-eq-{key}"), f64::from(v.eq[i]), (f64::from(wm::EQ_RANGE.0), f64::from(wm::EQ_RANGE.1)), 1., below.into_any_element(), on && !busy, move |this, x, cx| this.winmic_send("settings", json!({"eq": {(key): x.round() as i64}}), cx), cx));
        }
        div().flex().flex_col().gap(px(10.))
            .child(curve(&v).when(!on, |d| d.opacity(0.4)))
            .child(bands.when(!on, |d| d.opacity(0.45)))
    }

    fn winmic_filters(&self, width: f32, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let voice = w.voice();
        let d = voice.dynamics;
        let attached = w.live()["native"]["attached"] == true;
        let active = attached && voice.enabled && !w.setup_running();
        let installed = w.live()["installed"] == true;
        let voice_active = voice.enabled && installed;
        let busy = w.busy || w.setup_running();
        let mut list = div().w_full();
        if !attached {
            list = list.child(div().flex().items_center().gap(px(12.)).flex_wrap().pb(px(8.))
                .child(ui::caption("Le traitement Rustudio est nécessaire pour régler la dynamique."))
                .child(ui::pill("winmic-filter-setup", Some(Icon::Plug), "Activer le traitement", Tone::Plain)
                    .when(!busy, |d| d.on_click(cx.listener(|this, _, _, cx| {
                        this.winmic.confirm(Ask::Setup);
                        cx.notify();
                    })))));
        } else if !voice.enabled {
            list = list.child(div().flex().items_center().gap(px(12.)).pb(px(8.))
                .child(ui::caption("Traitement micro en pause"))
                .child(ui::pill("winmic-filter-enable", Some(Icon::Play), "Activer", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.winmic_send("settings", json!({"enabled":true}), cx)))));
        }
        if w.live()["native"]["safe_mode"] == true {
            list = list.child(ui::status("Windows a rétabli les effets précédents.", State::Warn));
        }
        let hpf = voice.hpf;
        let hpf_toggle = ui::toggle("winmic-hpf", hpf, voice_active && !busy)
            .when(voice_active && !busy, |d| d.on_click(cx.listener(move |this, _, _, cx| {
                this.winmic_send("settings", json!({"hpf": !hpf}), cx);
            })));
        list = list.child(self.winmic_noise(width, cx))
            .child(self.mic_filter_row(MicFilter::HighPass, width, true, hpf_toggle, |cx| {
                self.slider_row("winmic-hpf-hz", "Fréquence de coupure", f64::from(voice.hpf_hz),
                    (f64::from(wm::HPF_RANGE.0), f64::from(wm::HPF_RANGE.1)), 5.,
                    |x| format!("{x:.0} Hz"), voice_active && hpf && !busy,
                    |this, x, cx| this.winmic_send("settings", json!({"hpf_hz": x.round() as i64}), cx), cx)
            }, cx));

        let rows = [
            (MicFilter::Gate, Some("gate"), d.gate, Some(1), vec![
                ("threshold", "Seuil d’ouverture", d.threshold, (-80., -15.), "dBFS"),
                ("depth", "Atténuation", d.depth, (6., 80.), "dB"),
                ("hold_ms", "Maintien", d.hold_ms, (0., 500.), "ms"),
            ]),
            (MicFilter::Eq, None, voice_active, None, vec![]),
            (MicFilter::Compressor, Some("compressor"), d.compressor, Some(2), vec![
                ("comp_db", "Seuil", d.comp_db, (-48., -3.), "dBFS"),
                ("ratio", "Ratio", d.ratio, (1., 20.), ":1"),
                ("attack_ms", "Attaque", d.attack_ms, (1., 80.), "ms"),
                ("release_ms", "Relâchement", d.release_ms, (10., 600.), "ms"),
                ("makeup", "Gain après compression", d.makeup, (0., 12.), "dB"),
            ]),
            (MicFilter::Deesser, None, d.deesser > 0, None, vec![
                ("deesser", "Intensité", d.deesser, (0., 100.), "%"),
            ]),
            (MicFilter::Limiter, Some("limiter"), d.limiter, Some(4), vec![
                ("ceiling", "Plafond", d.ceiling, (-12., 0.), "dBFS"),
            ]),
        ];
        for (filter, key, on, meter, fields) in rows {
            let action = if let Some(key) = key {
                ui::toggle(SharedString::from(format!("winmic-dyn-{key}")), on, active)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if active { this.winmic_send("settings", json!({"dynamics": {(key): !on}}), cx); }
                    })).into_any_element()
            } else if filter == MicFilter::Eq {
                let text = if voice.eq.iter().any(|&value| value != 0) && voice_active { "Actif" } else { "Neutre" };
                ui::caption(text).into_any_element()
            } else {
                ui::caption(format!("{} %", d.deesser)).into_any_element()
            };
            list = list.child(self.mic_filter_row(filter, width, true, action, |cx| {
                if filter == MicFilter::Eq {
                    return self.winmic_eq(cx);
                }
                let sliders = fields.into_iter().map(|(field, label, value, range, unit)| {
                    self.slider_row(field, label, value.into(), range, 1., move |x| {
                        if unit == ":1" { format!("{x:.0}:1") } else { format!("{} {unit}", fmt_db(x)) }
                    }, active && (on || key.is_none()), move |this, x, cx| {
                        this.winmic_send("settings", json!({"dynamics": {(field): x.round() as i32}}), cx);
                    }, cx).into_any_element()
                }).collect();
                let reduction = meter.and_then(|i| w.live()["native"]["levels"].get(i)).and_then(Value::as_f64);
                div().flex().flex_col().gap(px(8.))
                    .when_some(reduction.filter(|_| on && active), |d, db| {
                        d.child(ui::caption(format!("Réduction en direct  {} dB", fmt_db(db))))
                    })
                    .child(ui::grid(sliders, mic_filter_columns(width)))
            }, cx));
        }
        list
    }

    pub(crate) fn winmic_noise_active(&self) -> bool {
        let w = &self.winmic;
        w.noise_input().is_some() && w.live()["ns"]["enabled"] == true
            && (!w.hardware_active || w.live()["native"]["attached"] == true || w.legacy_noise_ready())
    }

    /// Le sélecteur « Entrée micro » détermine aussi le micro traité par RNNoise.
    pub(crate) fn winmic_noise(&self, width: f32, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let Some((guid, _)) = w.noise_input() else {
            return self.mic_filter_row(MicFilter::Noise, width, false, ui::caption("Entrée indisponible"), |_| div(), cx);
        };
        let live = w.live();
        let ns = &live["ns"];
        let native = live["native"]["attached"] == true || live["native"]["requested"] == true;
        let legacy_go = w.hardware_active && !native;
        let busy = w.busy || w.setup_running();
        let on = self.winmic_noise_active();
        let needs_install = legacy_go && !w.legacy_noise_ready();
        let target = guid.clone();
        let change = cx.listener(move |this, _, _, cx| {
            if this.winmic.busy || this.winmic.setup_running() || this.winmic.selected().as_deref() != Some(target.as_str()) {
                return;
            }
            if on && !native {
                this.winmic_send("noise", json!({"device":target, "enabled":false}), cx);
            } else {
                this.winmic.ask = Some((if legacy_go { Ask::LegacyNoise } else { Ask::Noise(!on) }, target.clone()));
                cx.notify();
            }
        });
        let action = if needs_install {
            ui::pill("winmic-ns-install", Some(Icon::Download), "Installer", Tone::Plain)
                .when(busy, |d| d.opacity(0.45)).on_click(change).into_any_element()
        } else {
            ui::toggle("winmic-ns", on, !busy).on_click(change).into_any_element()
        };
        self.mic_filter_row(MicFilter::Noise, width, true, action, |cx| {
            let mut details = div().flex().flex_col().gap(px(10.));
            if !native && let Some(other) = ns["elsewhere"].as_str() {
                details = details.child(ui::caption(format!("Antibruit actif sur une autre entrée · {other}")));
            }
            if on {
                let action = if native { "native-noise" } else { "noise" };
                let target = guid.clone();
                let grace_target = guid.clone();
                details = details
                    .child(self.slider_row("winmic-ns-strength", "Intensité", ns["strength"].as_f64().unwrap_or(90.),
                        (50., 99.), 1., |x| format!("{x:.0} %"), !busy,
                        move |this, x, cx| this.winmic_send(action, json!({"device":target, "strength":x.round() as i64}), cx), cx))
                    .child(self.slider_row("winmic-ns-grace", "Maintien en fin de phrase", ns["grace_ms"].as_f64().unwrap_or(200.),
                        (0., 500.), 10., |x| format!("{x:.0} ms"), !busy,
                        move |this, x, cx| this.winmic_send(action, json!({"device":grace_target, "grace_ms":x.round() as i64}), cx), cx));
            } else {
                details = details.child(ui::caption(if needs_install {
                    "Installe l’antibruit pour cette entrée."
                } else {
                    "Active l’antibruit pour régler son intensité."
                }));
            }
            if legacy_go && !needs_install {
                details = details.child(div().flex().child(ui::pill("winmic-ns-remove", Some(Icon::Close), "Retirer de ce micro", Tone::Plain)
                    .when(!busy, |d| d.on_click(cx.listener(move |this, _, _, cx| {
                        if !this.winmic.busy && !this.winmic.setup_running() && this.winmic.selected().as_deref() == Some(guid.as_str()) {
                            this.winmic.ask = Some((Ask::LegacyNoiseRemove, guid.clone()));
                            cx.notify();
                        }
                    })))));
            }
            details
        }, cx)
    }

    /// Test : quelques secondes enregistrées telles que les applis les reçoivent, puis réécoutées.
    fn winmic_test_card(&self, cx: &mut Context<Self>) -> Div {
        let w = &self.winmic;
        let (text, state) = match &w.test {
            Test::Idle => ("Parle quelques secondes comme sur Discord, puis réécoute-toi.".to_owned(), None),
            Test::Recording => (format!("Enregistrement ({} s)… parle maintenant.", TEST_SECS as u32), Some(State::Warn)),
            Test::Playing => ("Réécoute en cours sur ta sortie Windows…".to_owned(), Some(State::Info)),
            Test::Done => ("Voilà ce que reçoivent tes applis. Ajuste et recommence.".to_owned(), Some(State::Ok)),
            Test::Failed(e) => (format!("Test impossible : {e}"), Some(State::Error)),
        };
        let running = matches!(w.test, Test::Recording | Test::Playing);
        let installed = w.live()["installed"] == true;
        let mut p = ui::section(Icon::Ear, ui::ORANGE, "Te réécouter", None)
            .child(match state {
                Some(s) => ui::status(text, s),
                None => hint(text),
            })
            .child(div().flex().items_center().gap(px(6.)).child(ui::pill("winmic-test", Some(if running { Icon::Stop } else { Icon::Play }), if running { "Arrêter" } else { "Enregistrer puis réécouter" }, if running { Tone::Plain } else { Tone::Primary }).on_click(cx.listener(|this, _, _, cx| this.winmic_test(cx)))))
            .child(ui::caption("Avec un casque, pour ne pas reprendre le son des enceintes."));
        if installed || w.live()["native"]["requested"] == true || w.live()["native"]["safe_mode"] == true {
            p = p.child(
                div().pt(px(6.)).border_t_1().border_color(alpha(ui::INK, 0.07)).flex().items_center().gap(px(10.)).child(ui::caption("Rustudio est branché sur ce micro dans Windows.").flex_1().min_w(px(0.))).child(ui::pill("winmic-remove", Some(Icon::Close), "Retirer", Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                    if !this.winmic.setup_running() {
                        let n=&this.winmic.live()["native"];
                        this.winmic.confirm(Ask::Remove(n["requested"]==true || n["attached"]==true || n["safe_mode"]==true));
                        cx.notify();
                    }
                }))),
            );
        }
        p
    }
}

/// « −4,5 », « +3 », « 0 ».
fn fmt_db(v: f64) -> String {
    let s = crate::mixer::fmt_num(v);
    if v > 0.05 { format!("+{s}") } else { s.replace('-', "−") }
}

/// Courbe de l'égaliseur en barres : au-dessus du trait, ce qui est renforcé ; en dessous, ce qui est atténué.
fn curve(v: &wm::Voice) -> Div {
    const HALF: f32 = 44.;
    const SPAN: f64 = 12.;
    let freqs: Vec<f64> = (0..CURVE_POINTS).map(|i| 50. * (16000f64 / 50.).powf(i as f64 / (CURVE_POINTS - 1) as f64)).collect();
    let gain = f64::from(v.gain_db);
    // le gain décale toute la courbe : on montre la forme, le gain a son propre curseur
    let db: Vec<f64> = v.response_db(&freqs).into_iter().map(|d| d - gain).collect();
    let mut bars = div().flex().items_center().gap(px(3.)).h(px(HALF * 2.)).w_full();
    for d in db {
        // sous 0,3 dB : rien à montrer, le trait du milieu suffit
        let flat = d.abs() < 0.3;
        let h = (d.abs() / SPAN).min(1.) as f32 * HALF;
        let colour = if d >= 0. { ui::ACCENT } else { ui::ORANGE };
        let up = div().h(px(HALF)).flex().flex_col().justify_end().child(div().when(!flat && d > 0., |b| b.h(px(h)).rounded_t(px(2.)).bg(rgb(colour))));
        let down = div().h(px(HALF)).flex().flex_col().justify_start().child(div().when(!flat && d < 0., |b| b.h(px(h)).rounded_b(px(2.)).bg(rgb(colour))));
        bars = bars.child(div().flex_1().min_w(px(0.)).flex().flex_col().child(up).child(down));
    }
    let axis = div().flex().justify_between().text_size(px(11.)).text_color(rgb(ui::TEXT3)).child("Graves").child("Médiums").child("Aigus");
    div().flex().flex_col().gap(px(6.)).child(div().relative().child(bars).child(div().absolute().left_0().right_0().top(px(HALF)).h(px(1.)).bg(alpha(ui::INK, 0.12)))).child(axis)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "{aaaaaaaa-0000-0000-0000-000000000001}";
    const B: &str = "{bbbbbbbb-0000-0000-0000-000000000002}";

    #[test]
    fn lantibruit_vise_lentree_affichee_et_jamais_la_camera_depuis_la_goxlr() {
        let mut page = WinMicPage {
            hardware_active: true,
            view: Some(json!({"state":{"selected":A}, "live":{"ns":{"elsewhere":"C920"},"captures":[
                {"guid":A,"goxlr":true,"name":"Microphone USB Audio"}, {"guid":B,"goxlr":false,"name":"C920"}
            ]}})),
            ..WinMicPage::default()
        };
        assert_eq!(page.noise_input(), Some((A.into(), "GoXLR Mini · Microphone USB Audio".into())));
        page.view.as_mut().unwrap()["state"]["selected"] = json!(B);
        assert_eq!(page.noise_input(), None);
        page.hardware_active = false;
        assert_eq!(page.noise_input(), Some((B.into(), "C920".into())));
    }

    #[test]
    fn une_seule_chaine_selon_le_micro_et_les_commandes_disponibles() {
        let mut page = WinMicPage {
            view: Some(json!({"state":{"selected": A}, "live":{"captures":[
                {"guid": A,"goxlr":true},{"guid":B,"goxlr":false}
            ]}})),
            ..Default::default()
        };
        assert_eq!(page.goxlr_guid().as_deref(), Some(A));
        assert!(page.hardware_choice(Some("ready")));
        assert!(!page.hardware_choice(Some("audio")));
        assert!(!page.hardware_choice(Some("disconnected")));
        page.view.as_mut().unwrap()["state"]["selected"] = json!(B);
        assert!(!page.hardware_choice(Some("ready")));
        page.prefer_goxlr = true;
        assert!(page.hardware_choice(Some("ready")));
        page.prefer_goxlr = false;
        page.view = Some(json!({"state":{"selected":null},"live":{"captures":[]}}));
        assert!(page.hardware_choice(Some("ready")));
        assert!(!page.hardware_choice(Some("disconnected")));
    }

    #[test]
    fn le_dernier_volume_est_conserve_pour_chaque_entree() {
        let mut page = WinMicPage::default();
        page.queue_level(A.into(), 0.3);
        page.queue_level(B.into(), 0.7);
        page.queue_level(A.to_uppercase(), 0.5);
        assert_eq!(page.level_pending.pop_front(), Some((A.into(), 0.5)));
        assert_eq!(page.level_pending.pop_front(), Some((B.into(), 0.7)));
        assert!(page.level_pending.is_empty());
    }

    #[test]
    fn une_commande_retardee_ne_modifie_pas_le_micro_affiche() {
        let mut page = WinMicPage {
            view: Some(json!({"state":{"selected":B, "voice":wm::Voice::default()}})),
            ..WinMicPage::default()
        };
        let original = page.voice();
        page.preview_settings(&json!({"device": A, "gain_db": 6}));
        assert_eq!(page.voice(), original);
        page.preview_settings(&json!({"device": B, "preset": "streamer"}));
        let mut expected = original;
        expected.apply_preset("streamer").unwrap();
        assert_eq!(page.voice(), expected);
        page.preview_settings(&json!({"selected": A}));
        assert_eq!(page.voice(), expected);
    }
}
