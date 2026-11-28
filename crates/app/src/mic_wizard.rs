//! Réglages détaillés du micro GoXLR, avec analyse de la voix sur demande.
use crate::desktop::hint;
use crate::mic_filters::{MicFilter, mic_filter_columns, mic_signal, softer_compressor};
use crate::mixer::{Mixer, fmt_num};
use crate::ui::{self, Icon, Tone, alpha, rgb};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, div, prelude::*, px};
use rustudio_core::{
    consts::{COMP_ATTACK_MS, COMP_RELEASE_MS, EQ_BANDS, GATE_MS, RATIOS, eq_default_freq, eq_range},
    dsp, micchain,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Gain,
    Compare,
    Measure,
}

const TABS: [(Step, Icon, &str); 3] = [
    (Step::Gain, Icon::Activity, "Bilan"),
    (Step::Compare, Icon::Headphones, "Comparer"),
    (Step::Measure, Icon::Wave, "Analyse"),
];

/// Copie complète des réglages micro avant l'ouverture de l'assistant, pour restaurer à l'identique si on annule.
#[derive(Clone, Debug)]
pub struct MicSnapshot {
    pub mic_type: String,
    pub mic_model: Option<String>,
    pub gain_db: i64,
    pub mic_preset: String,
    pub fx: Value,
    pub dsp: Value,
    pub mic_ns: Option<Value>,
    pub mic_monitor: i64,
}

impl MicSnapshot {
    pub fn capture(state: &Value) -> Self {
        let mic = &state["mic"];
        let mic_ns = state["mic_ns"].is_object().then(|| {
            json!({
                "enabled": state["mic_ns"]["enabled"] == true,
                "strength": state["mic_ns"]["strength"].as_i64().unwrap_or(90),
                "grace_ms": state["mic_ns"]["grace_ms"].as_i64().unwrap_or(200),
                "device": state["mic_ns"]["device"].clone(),
            })
        });
        Self {
            mic_type: mic["type"].as_str().or(mic["kind"].as_str()).unwrap_or("dynamic").to_owned(),
            mic_model: mic["model"].as_str().map(str::to_owned),
            gain_db: mic["gain_db"].as_i64().unwrap_or(30),
            mic_preset: state["mic_preset"].as_str().unwrap_or("brut").to_owned(),
            fx: state["fx"].clone(),
            dsp: state["dsp"].clone(),
            mic_ns,
            mic_monitor: state["volumes"]["MicMonitor"].as_i64().unwrap_or(0),
        }
    }
}

/// Prise de ta voix brute (la lecture du texte), et le gain du micro au moment de la prise.
struct Take {
    samples: Arc<Vec<f32>>,
    rate: u32,
    gain_db: i32,
}

pub struct MicWizard {
    pub step: Step,
    /// Sauvegarde de départ pour le bouton « Annuler et restaurer ».
    pub snapshot: Option<MicSnapshot>,
    /// Une mesure tourne ; l'annuler l'arrête dans la seconde.
    cancel: Option<Arc<AtomicBool>>,
    /// Tâche du moteur en cours, relue en direct : phase, consigne, avancement.
    job: Value,
    error: Option<String>,
    chosen: Option<String>,
    take: Option<Take>,
    playing: Option<String>,
    /// Numéro de la mesure en cours : une mesure annulée qui finit plus tard ne touche plus à rien.
    run: u64,
    /// Numéro de la lecture audio en cours : une réouverture n'est jamais touchée par une ancienne lecture.
    play_run: u64,
}

impl Drop for MicWizard {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Default for MicWizard {
    fn default() -> Self {
        Self::new()
    }
}

impl MicWizard {
    pub fn new() -> Self {
        MicWizard {
            step: Step::Gain,
            snapshot: None,
            cancel: None,
            job: Value::Null,
            error: None,
            chosen: None,
            take: None,
            playing: None,
            run: 0,
            play_run: 0,
        }
    }

    fn busy(&self) -> bool {
        self.cancel.is_some()
    }
}

fn measure_prompt(job: &Value, now: f64) -> (&'static str, &'static str, Option<u64>) {
    let phase = job["phase"].as_str();
    let (title, detail) = match phase {
        Some("silence") => ("Reste silencieux", "Rustudio mesure le bruit de la pièce."),
        Some("reading") if job["retry"].as_u64().unwrap_or(0) > 0 => (
            "Parle normalement", "Le gain a été baissé. Continue avec tes propres mots.",
        ),
        Some("reading") => ("Parle normalement", "Parle comme en appel, avec tes propres mots."),
        Some("loud") => ("Parle plus fort", "Hausse un peu la voix pour vérifier la marge avant saturation."),
        Some("prepare") | None => ("Prépare-toi", "La mesure commence par quelques secondes de silence."),
        _ => ("Analyse en cours", "Rustudio calcule le gain et le bilan de ta voix."),
    };
    let countdown = job["phase_end"].as_f64()
        .map(|deadline| deadline - now)
        .filter(|left| *left > 0. && matches!(phase, Some("silence" | "reading" | "loud" | "prepare")))
        .map(|left| left.ceil() as u64);
    (title, detail, countdown)
}

fn note_state(note: &Value) -> ui::State {
    match note["lvl"].as_str() {
        Some("warn") => ui::State::Warn,
        Some("ok") => ui::State::Ok,
        _ => ui::State::Info,
    }
}

fn note_text(note: &Value) -> Option<String> {
    let text = note["t"].as_str()?;
    // Les moteurs déjà installés renvoient encore l’ancienne formulation.
    if let Some((frequency, _)) = text.strip_prefix("Ronflement à ").and_then(|s| s.split_once(" : ")) {
        return Some(format!("Ronflement à {frequency}. Éloigne le câble du micro des alimentations et des multiprises."));
    }
    if text.starts_with("Ta voix a saturé pendant l’enregistrement :") {
        return Some("Ta voix a saturé pendant la mesure. Vérifie le gain, puis relance la mesure si nécessaire.".into());
    }
    Some(text.into())
}

fn is_cancelled_error(e: &str) -> bool {
    matches!(e, "Analyse annulée." | "Mesure annulée.")
}

/// Réglages d'un préréglage tels qu'ils partiraient vers la GoXLR (traitements coupés neutres).
fn profile_dsp(info: Option<&Value>, preset: &str) -> Option<dsp::Dsp> {
    let p = &info?["profiles"][preset];
    if !p.is_object() {
        return None;
    }
    let d = dsp::normalize_dsp(&p["dsp"], None);
    let fx = dsp::normalize_fx(&p["fx"]);
    Some(dsp::effective_dsp(&d, &fx))
}

/// La dernière lecture valide du texte dans la prise complète (celle analysée par voice::analyze, y compris
/// après un rattrapage de saturation).
fn reading_part(take: &crate::voice::Take, timeline: &Value) -> Option<Take> {
    let t0 = take.t0_ms as f64 / 1000.;
    let at = |t: f64| (((t - t0).max(0.) * take.rate as f64) as usize).min(take.samples.len());
    timeline.as_array()?.iter().rev().find_map(|step| {
        if step["phase"] != "reading" {
            return None;
        }
        let (a, b) = (at(step["start"].as_f64()?), at(step["end"].as_f64()?));
        (b > a + take.rate as usize).then(|| Take {
            samples: Arc::new(take.samples[a..b].to_vec()),
            rate: take.rate,
            gain_db: step["gain_db"].as_i64().unwrap_or(0) as i32,
        })
    })
}

/// Fréquence affichée proprement (« 90 Hz », « 3 kHz », « 3,5 kHz »).
fn fmt_hz(f: f64) -> String {
    if f < 1000. {
        format!("{} Hz", f.round())
    } else {
        let k = (f / 100.).round() / 10.;
        let s = if (k - k.round()).abs() < 1e-9 { format!("{}", k.round()) } else { format!("{k:.1}") };
        format!("{} kHz", s.replace('.', ","))
    }
}

fn fmt_gate_ms(idx: f64) -> String {
    let i = idx.round().clamp(0., (GATE_MS.len() - 1) as f64) as usize;
    let ms = GATE_MS[i];
    if ms >= 1000 {
        format!("{} s", fmt_num(f64::from(ms) / 1000.))
    } else {
        format!("{ms} ms")
    }
}

fn fmt_comp_attack(idx: f64) -> String {
    let i = idx.round().clamp(0., (COMP_ATTACK_MS.len() - 1) as f64) as usize;
    let ms = COMP_ATTACK_MS[i];
    if ms == 0 { "0,1 ms (instantanée)".into() } else { format!("{ms} ms") }
}

fn fmt_comp_release(idx: f64) -> String {
    let i = idx.round().clamp(0., (COMP_RELEASE_MS.len() - 1) as f64) as usize;
    let ms = COMP_RELEASE_MS[i];
    if ms >= 1000 {
        format!("{} s", fmt_num(f64::from(ms) / 1000.))
    } else {
        format!("{ms} ms")
    }
}

/// Ne garde que les notes qui concernent l'égaliseur (les notes de-esser, gate et compresseur ont leur propre étape).
fn filter_eq_notes(notes: &[Value]) -> Vec<String> {
    notes
        .iter()
        .filter_map(|n| n["t"].as_str())
        .filter(|t| {
            !t.starts_with("Sifflantes")
                && !t.starts_with("Noise gate")
                && !t.starts_with("Pièce calme")
                && !t.starts_with("Compresseur")
        })
        .map(String::from)
        .collect()
}

/// Dose rapide correspondant à l'état actuel du compresseur (aucune si le compresseur est coupé).
fn selected_dose(comp_on: bool, mic_preset: &str, ratio: f64, thr: f64) -> Option<&'static str> {
    if !comp_on {
        return None;
    }
    match mic_preset {
        "antenne" => Some("skyrock"),
        "stream" => Some("stream"),
        "naturel" => Some("doux"),
        _ if ratio >= 9.5 || (ratio >= 8.5 && thr <= -24.) => Some("skyrock"),
        _ if ratio >= 7. => Some("stream"),
        _ => Some("doux"),
    }
}

/// Paramètres (seuil, ratio, attaque, relâchement, compensation, de-esser) d'une dose rapide,
/// calés sur l'empreinte vocale si elle est valide pour le micro actuel.
fn radio_dose_params(info: Option<&Value>, dose: &str) -> (i64, i64, i64, i64, i64, i64) {
    let (preset_key, fallback) = match dose {
        "doux" => ("naturel", (-16, 6, 5, 11, 3, 15)),   // 2:1 doux
        "stream" => ("stream", (-20, 8, 2, 10, 5, 25)),  // 3,2:1 live
        _ => ("antenne", (-24, 10, 2, 10, 8, 40)),       // 5,6:1 Radio FM / Skyrock
    };
    if let Some(info) = info.filter(|i| i["valid"] == true) {
        let p = &info["profiles"][preset_key]["dsp"];
        if p.is_object() {
            let d = dsp::normalize_dsp(p, None);
            return (
                d.comp.threshold as i64,
                d.comp.ratio as i64,
                d.comp.attack as i64,
                d.comp.release as i64,
                d.comp.makeup as i64,
                d.deesser as i64,
            );
        }
    }
    fallback
}

/// Formatage lisible d'un ratio de compresseur (index GoXLR -> « 5,6:1 »).
fn ratio_text(idx: f64) -> String {
    let i = idx.round().clamp(0., (RATIOS.len() - 1) as f64) as usize;
    let r = RATIOS[i];
    if (r - r.round()).abs() < 0.05 {
        format!("{}:1", r.round() as i32)
    } else {
        format!("{r:.1}:1").replace('.', ",")
    }
}

/// Formatage lisible d'un gain d'EQ en dB (« +4 dB », « 0 dB », « -3 dB »).
fn eq_db_text(v: f64) -> String {
    let n = v.round() as i64;
    if n > 0 { format!("+{n} dB") } else { format!("{n} dB") }
}

/// Rôle vulgarisé de chacune des 6 bandes de l'égaliseur GoXLR Mini.
fn eq_band_role(band: &str, fitted_layout: bool) -> (&'static str, &'static str) {
    match band {
        "90" if fitted_layout => ("Coupe-bas", "Souffle & chocs"),
        "90" => ("Graves", "Coffre radio"),
        "250" if fitted_layout => ("Chaleur", "Voix proche"),
        "250" => ("Bas-médiums", "Chaleur"),
        "500" => ("Anti-carton", "Netteté"),
        "1000" => ("Médiums", "Corps"),
        "3000" => ("Présence", "Clarté"),
        _ => ("Air", "Brillance"),
    }
}

/// Pastille d'activité DSP en direct (réduction mesurée en dB par le firmware Rustudio).
pub(crate) fn dsp_meter_pill(label: &'static str, reduction_db: f64, enabled: bool, active_colour: u32) -> Div {
    let db = reduction_db.abs();
    let working = enabled && db >= 0.4;
    let text = if !enabled {
        "coupé".to_owned()
    } else if working {
        format!("-{} dB", fmt_num(db))
    } else {
        "prêt".to_owned()
    };
    let fg = if !enabled {
        ui::TEXT3
    } else if working {
        active_colour
    } else {
        ui::TEXT2
    };
    div()
        .px(px(10.))
        .py(px(5.))
        .rounded(px(8.))
        .bg(if working { alpha(active_colour, 0.14) } else { alpha(ui::INK, 0.04) })
        .border_1()
        .border_color(if working { alpha(active_colour, 0.3) } else { alpha(ui::INK, 0.06) })
        .flex()
        .items_center()
        .gap(px(6.))
        .child(div().size(px(6.)).rounded_full().bg(rgb(if !enabled { ui::TEXT3 } else if working { active_colour } else { ui::GREEN })))
        .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(label))
        .child(div().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(fg)).child(text))
}

impl Mixer {
    pub fn open_mic_wizard(&mut self, step: Step, cx: &mut Context<Self>) {
        self.voice.cancel_preview();
        if !self.winmic.hardware_active {
            self.go_to(crate::desktop::Page::Go(crate::goxlr::Section::Microphone), cx);
        }
        let mut w = MicWizard::new();
        w.step = step;
        if let Some(v) = self.go.view.as_ref() {
            w.snapshot = Some(MicSnapshot::capture(&v.state));
            if let Some(p) = v.state["mic_preset"]
                .as_str()
                .filter(|p| dsp::is_preset(p) && *p != "brut")
            {
                w.chosen = Some(p.to_owned());
            }
        }
        self.wizard = Some(w);
        self.page_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.load_voice_info(cx);
        cx.notify();
    }

    /// Enregistrer et terminer : garde les nouveaux réglages, coupe systématiquement le retour casque et l'A/B.
    fn finish_mic_wizard(&mut self, cx: &mut Context<Self>) {
        self.voice.cancel_preview();
        self.go.end_monitor();
        let mut cancel_job = false;
        let mut restore_monitor_zero = true;
        if let Some(w) = self.wizard.as_mut() {
            if let Some(cancel) = w.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
                cancel_job = true;
            }
            w.play_run += 1;
            if let Some(snap) = w.snapshot.as_ref() {
                restore_monitor_zero = snap.mic_monitor == 0;
            }
        }
        self.wizard = None;
        self.param_held.clear();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            if cancel_job {
                let _ = client.go_command("job-cancel", json!({}));
            }
            let _ = client.stop_mic_listen();
            let _ = client.go_command("mic-ab", json!({"bypass": false}));
            if restore_monitor_zero {
                let _ = client.go_command("volume", json!({"channel": "MicMonitor", "value": 0}));
            }
            client.go_view().ok()
        });
        cx.spawn(async move |this, cx| {
            let view = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.receive(view);
                this.go.notice = Some((true, "Réglages micro enregistrés.".into()));
                this.load_voice_info(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Annuler et restaurer : remet tous les réglages micro d'avant l'assistant, coupe le retour et ferme.
    fn cancel_mic_wizard(&mut self, cx: &mut Context<Self>) {
        self.voice.cancel_preview();
        self.go.end_monitor();
        let mut cancel_job = false;
        let snapshot = if let Some(w) = self.wizard.as_mut() {
            if let Some(cancel) = w.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
                cancel_job = true;
            }
            w.play_run += 1;
            w.snapshot.clone()
        } else {
            None
        };
        self.wizard = None;
        self.param_held.clear();
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            if cancel_job {
                let _ = client.go_command("job-cancel", json!({}));
            }
            let _ = client.stop_mic_listen();
            let _ = client.go_command("mic-ab", json!({"bypass": false}));
            if let Some(snap) = snapshot {
                let _ = client.go_command("volume", json!({"channel": "MicMonitor", "value": snap.mic_monitor}));
                if let Some(model) = snap.mic_model.as_deref() {
                    let _ = client.go_command("mic-model", json!({"model": model}));
                }
                let _ = client.go_command("mic", json!({"type": snap.mic_type, "gain_db": snap.gain_db}));
                if snap.mic_preset == "brut" {
                    let _ = client.go_command("mic-preset", json!({"name": "brut"}));
                } else if dsp::is_preset(&snap.mic_preset) {
                    let _ = client.go_command("mic-preset", json!({"name": snap.mic_preset}));
                } else {
                    let _ = client.go_command("dsp", snap.dsp);
                    let _ = client.go_command("fx", snap.fx);
                }
                if let Some(ns) = snap.mic_ns {
                    let _ = client.go_command("mic-ns", ns);
                }
            } else {
                let _ = client.go_command("volume", json!({"channel": "MicMonitor", "value": 0}));
            }
            client.go_view().ok()
        });
        cx.spawn(async move |this, cx| {
            let view = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.receive(view);
                this.go.notice = Some((true, "Anciens réglages micro restaurés.".into()));
                this.load_voice_info(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Recharge les anciens réglages dans l’onglet Comparer.
    fn wizard_apply_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(snap) = self.wizard.as_ref().and_then(|w| w.snapshot.clone()) else {
            return;
        };
        let mut cmds: Vec<(&'static str, Value)> = vec![
            ("mic-ab", json!({"bypass": false})),
            ("mic", json!({"type": snap.mic_type, "gain_db": snap.gain_db})),
        ];
        if snap.mic_preset == "brut" {
            cmds.push(("mic-preset", json!({"name": "brut"})));
        } else if dsp::is_preset(&snap.mic_preset) {
            cmds.push(("mic-preset", json!({"name": snap.mic_preset})));
        } else {
            cmds.push(("dsp", snap.dsp));
            cmds.push(("fx", snap.fx));
        }
        if let Some(ns) = snap.mic_ns {
            cmds.push(("mic-ns", ns));
        }
        self.param_held.clear();
        self.wizard_send_seq(cmds, true, cx);
    }

    /// Enchaîne plusieurs commandes GoXLR dans une seule tâche et recharge l'état puis, si demandé, l'analyse vocale.
    fn wizard_send_seq(&mut self, cmds: Vec<(&'static str, Value)>, refresh_voice: bool, cx: &mut Context<Self>) {
        if self.go.busy || !self.go.enabled() {
            return;
        }
        self.go.busy = true;
        self.go.notice = None;
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let mut outcome = Ok(());
            for (action, body) in cmds {
                if let Err(e) = client.go_command(action, body) {
                    outcome = Err(e);
                    break;
                }
            }
            (outcome, client.go_view().ok())
        });
        cx.spawn(async move |this, cx| {
            let (outcome, view) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.busy = false;
                this.go.receive(view);
                match outcome {
                    Ok(()) => {
                        if let Some(w) = this.wizard_mut() {
                            w.error = None;
                        }
                        if refresh_voice {
                            this.load_voice_info(cx);
                        }
                    }
                    Err(e) => {
                        if let Some(w) = this.wizard_mut() {
                            w.error = Some(e);
                        } else {
                            this.go.notice = Some((false, e));
                        }
                    }
                }
                this.resume_mic_preset(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Commande au moteur sans attendre ni bloquer la page (arrêt d'une mesure).
    fn send_quiet(&mut self, action: &'static str, body: Value, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = client.go_command(action, body);
            })
            .detach();
    }

    /// Relit l’état de la GoXLR après la mesure.
    fn go_refresh(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move { client.go_view().ok() });
        cx.spawn(async move |this, cx| {
            let view = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.receive(view);
                cx.notify();
            });
        })
        .detach();
    }

    fn wizard_mut(&mut self) -> Option<&mut MicWizard> {
        self.wizard.as_mut()
    }

    /// Démarre une mesure : la marque en cours, efface l'erreur précédente, renvoie son drapeau d'annulation.
    fn wizard_begin(&mut self, step: Step) -> Option<(Arc<AtomicBool>, u64)> {
        let w = self.wizard_mut()?;
        if w.busy() {
            return None;
        }
        w.run += 1;
        let run = w.run;
        let cancel = Arc::new(AtomicBool::new(false));
        w.cancel = Some(cancel.clone());
        w.error = None;
        w.job = Value::Null;
        w.step = step;
        Some((cancel, run))
    }

    /// Fin d'une mesure : une annulation n'est pas une erreur.
    fn wizard_end(w: &mut MicWizard, error: Option<String>) {
        w.cancel = None;
        w.job = Value::Null;
        w.error = error.filter(|e| !is_cancelled_error(e));
    }

    /// Avancement d'une tâche du moteur, affiché au fil de l'eau.
    fn wizard_follow(&mut self, rx: std::sync::mpsc::Receiver<Value>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(120)).await;
                let mut last = None;
                let mut closed = false;
                loop {
                    match rx.try_recv() {
                        Ok(job) => last = Some(job),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            closed = true;
                            break;
                        }
                    }
                }
                let alive = this
                    .update(cx, |this, cx| {
                        if let (Some(job), Some(w)) = (last, this.wizard_mut())
                            && w.busy()
                        {
                            w.job = job;
                            cx.notify();
                        }
                        this.wizard.is_some()
                    })
                    .unwrap_or(false);
                if closed || !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Analyse facultative en 25 s. Le moteur cadence les consignes et l’appli enregistre la voix.
    fn wizard_voice(&mut self, cx: &mut Context<Self>) {
        if !self.go.enabled() {
            return;
        }
        // Coupe toujours le retour casque avant d'enregistrer pour éviter toute repisse.
        if self.monitor_on() {
            self.cut_monitor(cx);
        }
        let Some((cancel, run)) = self.wizard_begin(Step::Measure) else { return };
        let client = self.client.clone();
        let watcher = self.client.clone();
        let stop = cancel.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Ok(s) = watcher.live_state()
                    && s["job"]["running"] == true
                    && tx.send(s["job"].clone()).is_err()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        self.wizard_follow(rx, cx);
        let done = cancel.clone();
        let work = cx.background_executor().spawn(async move { crate::voice::analyse(&client, &cancel, false) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            done.store(true, Ordering::Relaxed);
            let _ = this.update(cx, |this, cx| {
                if this.wizard.as_ref().is_none_or(|w| w.run != run) {
                    return;
                }
                match result {
                    Ok((info, timeline, take)) => {
                        this.voice.info = Some(info);
                        if let Some(w) = this.wizard_mut() {
                            w.take = take.as_ref().and_then(|t| reading_part(t, &timeline));
                            w.step = Step::Gain;
                            Self::wizard_end(w, None);
                        }
                    }
                    Err(e) => {
                        if let Some(w) = this.wizard_mut() {
                            Self::wizard_end(w, Some(e));
                        }
                    }
                }
                this.go_refresh(cx);
                this.load_voice_info(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Réécoute de ta prise : brute (« avant ») ou passée par la chaîne d'un préréglage, au gain actuel du micro.
    fn wizard_play(&mut self, preset: Option<String>, cx: &mut Context<Self>) {
        if !self.client.is_live() || self.monitor_on() || self.voice.preview_busy() { return; }
        let Some(w) = self.wizard.as_ref() else { return };
        let Some(take) = w.take.as_ref() else { return };
        if w.playing.is_some() {
            return;
        }
        let gain_now = self.go.view.as_ref().and_then(|v| v.state["mic"]["gain_db"].as_i64()).unwrap_or(take.gain_db as i64) as i32;
        let scale = 10f32.powf((gain_now - take.gain_db) as f32 / 20.);
        let (samples, rate) = (take.samples.clone(), take.rate);
        let after = preset.is_some();
        let chain = match preset.as_deref() {
            Some("actuel") | None => {
                let Some(v) = self.go.view.as_ref() else { return };
                micchain::design(&dsp::effective_dsp(
                    &dsp::normalize_dsp(&v.state["dsp"], None),
                    &dsp::normalize_fx(&v.state["fx"]),
                ))
            }
            Some(p) => micchain::design(&profile_dsp(self.voice.info.as_ref(), p)
                .unwrap_or_else(|| dsp::preset_dsp(p))),
        };
        let Some(w) = self.wizard_mut() else { return };
        w.play_run += 1;
        let play_run = w.play_run;
        w.playing = Some(preset.clone().unwrap_or_else(|| "avant".into()));
        w.error = None;
        let stop = Arc::new(AtomicBool::new(false));
        self.voice.playback = Some(stop.clone());
        let playing = stop.clone();
        let work = cx.background_executor().spawn(async move {
            let raw: Vec<f32> = samples.iter().map(|x| (x * scale).clamp(-1., 1.)).collect();
            let raw = if rate == 48_000 { raw } else { rustudio_windows_desktop::soundcheck::resample(&raw, rate, 48_000) };
            let processed = micchain::render(&chain, &raw);
            let out = micchain::match_voice_level(&raw, &processed, after).map_err(str::to_owned)?;
            if stop.load(Ordering::Relaxed) { return Ok(()); }
            rustudio_windows_desktop::soundcheck::play_voice_preview(out, 48_000, stop)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.voice.playback.as_ref().is_some_and(|stop| Arc::ptr_eq(stop, &playing)) {
                    this.voice.playback = None;
                }
                if let Some(w) = this.wizard_mut().filter(|w| w.play_run == play_run) {
                    w.playing = None;
                    if let Err(e) = result {
                        w.error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn wizard_choose(&mut self, preset: String, cx: &mut Context<Self>) {
        if let Some(w) = self.wizard_mut() {
            w.chosen = Some(preset.clone());
        }
        self.param_held.clear();
        self.wizard_send_seq(vec![("mic-ab", json!({"bypass": false})), ("mic-preset", json!({"name": preset}))], false, cx);
    }

    /// Applique une courbe d'EQ complète sans écraser le Noise Gate ni le compresseur.
    fn apply_mic_eq_preset(&mut self, preset: &str, cx: &mut Context<Self>) {
        self.param_held.clear();
        if preset == "flat" {
            let zero: serde_json::Map<String, Value> = EQ_BANDS.iter().map(|(b, _, _)| ((*b).to_owned(), json!(0))).collect();
            let freqs: serde_json::Map<String, Value> = EQ_BANDS.iter().map(|(b, _, _)| ((*b).to_owned(), json!(eq_default_freq(b)))).collect();
            self.wizard_send_seq(vec![("fx", json!({"eq": true})), ("dsp", json!({"eq": zero, "eq_freq": freqs}))], false, cx);
            return;
        }
        let d = profile_dsp(self.voice.info.as_ref(), preset).unwrap_or_else(|| dsp::preset_dsp(preset));
        self.wizard_send_seq(
            vec![("fx", json!({"eq": true})), ("dsp", json!({"eq": d.eq, "eq_freq": d.eq_freq}))],
            false,
            cx,
        );
    }

    /// Applique un préréglage rapide de Noise Gate sans toucher aux autres blocs.
    fn apply_mic_gate_preset(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        let (thr, att, attack, release) = match mode {
            "doux" => (-52, 55, 0, 21),
            "clavier" => (-36, 100, 0, 15),
            _ => (-46, 80, 0, 18),
        };
        self.param_held.clear();
        self.wizard_send_seq(
            vec![
                ("fx", json!({"gate": true})),
                ("dsp", json!({"gate": {"enabled": true, "threshold": thr, "attenuation": att, "attack": attack, "release": release}})),
            ],
            false,
            cx,
        );
    }

    /// Applique uniquement une dose de dynamique (Compresseur + De-esser) sans écraser l'EQ ni le Noise Gate.
    fn apply_mic_radio_dose(&mut self, dose: &'static str, cx: &mut Context<Self>) {
        let (thr, ratio, attack, release, makeup, deess) = radio_dose_params(self.voice.info.as_ref(), dose);
        self.param_held.clear();
        self.wizard_send_seq(
            vec![
                ("fx", json!({"comp": true, "deesser": deess > 0})),
                (
                    "dsp",
                    json!({
                        "comp": {"threshold": thr, "ratio": ratio, "attack": attack, "release": release, "makeup": makeup},
                        "deesser": deess
                    }),
                ),
            ],
            false,
            cx,
        );
    }

    fn wizard_goto(&mut self, step: Step, cx: &mut Context<Self>) {
        if let Some(w) = self.wizard_mut() {
            if w.busy() {
                return;
            }
            w.step = step;
            w.error = None;
        }
        self.page_scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    /// Bascule instantanée Avant (voix brute sans filtre) / Après (traitements actifs) en direct.
    fn wizard_toggle_ab(&mut self, bypass: bool, cx: &mut Context<Self>) {
        self.wizard_send_seq(vec![("mic-ab", json!({"bypass": bypass, "seconds": 60}))], false, cx);
    }

    /// Annule la mesure et laisse l’onglet Analyse ouvert.
    fn wizard_cancel_measure(&mut self, cx: &mut Context<Self>) {
        let Some(w) = self.wizard_mut() else { return };
        if let Some(cancel) = w.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        w.run += 1;
        Self::wizard_end(w, None);
        self.send_quiet("job-cancel", json!({}), cx);
        cx.notify();
    }

    /// Réécoute Avant / Après accessible dès qu’une prise existe.
    fn wizard_replay_bar(&self, w: &MicWizard, cx: &mut Context<Self>) -> Option<Div> {
        w.take.as_ref()?;
        let playing = w.playing.as_deref();
        let monitoring = self.monitor_on();
        Some(
            div()
                .p(px(12.))
                .rounded(px(12.))
                .bg(alpha(ui::INK, 0.035))
                .border_1()
                .border_color(alpha(ui::INK, 0.06))
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(200.))
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child("Réécoute sur ta prise enregistrée"))
                        .child(ui::caption(if monitoring {
                            "Coupe le retour en direct pour réécouter la prise."
                        } else {
                            "Compare la même prise à volume égal."
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            ui::pill(
                                "wizard-replay-raw",
                                Some(if playing == Some("avant") { Icon::Wave } else { Icon::Play }),
                                if playing == Some("avant") { "Lecture…" } else { "Avant (brut)" },
                                Tone::Plain,
                            )
                            .when(monitoring, |d| d.opacity(0.45))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(stop) = &this.voice.playback {
                                    stop.store(true, Ordering::Relaxed);
                                } else {
                                    this.wizard_play(None, cx);
                                }
                            })),
                        )
                        .child(
                            ui::pill(
                                "wizard-replay-now",
                                Some(if playing == Some("actuel") { Icon::Wave } else { Icon::Play }),
                                if playing == Some("actuel") { "Lecture…" } else { "Après (réglé)" },
                                Tone::Primary,
                            )
                            .when(monitoring, |d| d.opacity(0.45))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(stop) = &this.voice.playback {
                                    stop.store(true, Ordering::Relaxed);
                                } else {
                                    this.wizard_play(Some("actuel".into()), cx);
                                }
                            })),
                        ),
                ),
        )
    }

    pub fn mic_wizard_workspace(&self, width: f32, cx: &mut Context<Self>) -> Option<Div> {
        let w = self.wizard.as_ref()?;
        let body = match w.step {
            Step::Gain => self.wizard_gain_view(w, width, cx),
            Step::Compare => self.wizard_compare_view(w, width, cx),
            Step::Measure => self.wizard_measure_view(w, cx),
        };
        if w.busy() {
            return Some(body);
        }

        let mut tabs = ui::tabs();
        for (index, (target, icon, label)) in TABS.into_iter().enumerate() {
            tabs = tabs.child(
                ui::tab(("mic-detail-tab", index), icon, label, w.step == target)
                    .on_click(cx.listener(move |this, _, _, cx| this.wizard_goto(target, cx))),
            );
        }

        Some(div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(ui::title("Bilan vocal", 19.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .child(ui::pill("wizard-cancel-all", Some(Icon::History), "Annuler les changements", Tone::Plain)
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_mic_wizard(cx))))
                            .child(ui::pill("wizard-finish", Some(Icon::Check), "Terminer", Tone::Primary)
                                .on_click(cx.listener(|this, _, _, cx| this.finish_mic_wizard(cx)))),
                    ),
            )
            .child(tabs)
            .when(w.step != Step::Measure, |d| d.child(self.wizard_live_console(w, cx)))
            .when_some(w.error.as_ref(), |d, error| d.child(ui::notice(false, error.clone())))
            .child(body))
    }

    /// Bandeau de contrôle en direct : vumètre d'entrée, activité DSP temps réel, écoute casque et bascule Avant/Après.
    fn wizard_live_console(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        let enabled = !w.busy() && w.playing.is_none() && self.go.enabled();
        let bypass = self.go.view.as_ref().is_some_and(|v| v.ab);
        let fx = self.go.view.as_ref().map(|v| &v.state["fx"]);
        let mut bar = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(18.))
                    .child(div().flex_1().min_w(px(240.)).child(mic_signal(self.go.level_shown, self.go.level)))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .p(px(3.))
                                    .rounded(px(9.))
                                    .bg(alpha(ui::INK, 0.05))
                                    .child(
                                        ui::pill("wizard-ab-raw", None, "Voix brute", if bypass { Tone::Warn } else { Tone::Plain })
                                            .h(px(28.))
                                            .when(!enabled, |d| d.opacity(0.45))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if enabled { this.wizard_toggle_ab(true, cx); }
                                            })),
                                    )
                                    .child(
                                        ui::pill("wizard-ab-fx", None, "Son traité", if !bypass { Tone::Primary } else { Tone::Plain })
                                            .h(px(28.))
                                            .when(!enabled, |d| d.opacity(0.45))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if enabled { this.wizard_toggle_ab(false, cx); }
                                            })),
                                    ),
                            )
                            .child(self.monitor_controls("wizard-live-monitor", enabled, cx))
                    ),
            );
        if let Some(m) = self.go.voice_meters {
            let is_on = |k: &str| fx.is_some_and(|f| f[k] == true) && !bypass;
            bar = bar.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::caption("Traitements en direct :"))
                    .child(dsp_meter_pill("Noise Gate", m[0], is_on("gate"), ui::GREEN))
                    .child(dsp_meter_pill("Compresseur", m[1], is_on("comp"), ui::ACCENT))
                    .child(dsp_meter_pill("De-esser", m[2], is_on("deesser"), ui::ORANGE))
                    .child(dsp_meter_pill("Limiteur", m[3], !bypass, ui::RED)),
            );
        }
        bar
    }

    fn wizard_measure_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        if !w.busy() {
            return ui::section(Icon::Wave, ui::ACCENT, "Analyser ma voix", None)
                .child(hint("La mesure prend environ 25 s et ajuste le gain. Elle commence par quelques secondes de silence, puis tu parles avec tes propres mots."))
                .child(
                    div().flex().child(
                        ui::pill("wizard-measure-retry", Some(Icon::Play), "Démarrer la mesure", Tone::Primary)
                            .when(!self.go.enabled(), |d| d.opacity(0.45))
                            .on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx))),
                    ),
                );
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.);
        let (instruction, detail, countdown) = measure_prompt(&w.job, now);
        let progress = w.job["progress"].as_f64().unwrap_or_default() as f32;

        div()
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(ui::title("Mesure de ta voix", 19.))
                    .child(ui::pill("wizard-cancel-measure", Some(Icon::Close), "Annuler la mesure", Tone::Plain)
                        .on_click(cx.listener(|this, _, _, cx| this.wizard_cancel_measure(cx)))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .p(px(20.))
                    .rounded(px(12.))
                    .bg(alpha(ui::INK, 0.04))
                    .child(ui::progress(progress, ui::ACCENT).overflow_hidden())
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_start()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(240.))
                                    .child(ui::title(instruction, 26.))
                                    .child(hint(detail)),
                            )
                            .when_some(countdown, |d, seconds| {
                                d.child(div().flex_none().text_size(px(26.)).font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(ui::ACCENT)).child(format!("{seconds} s")))
                            }),
                    )
                    .child(mic_signal(self.go.level_shown, self.go.level)),
            )
    }

    /// Bilan vocal vulgarisé + réglage complet du Gain d'entrée.
    fn wizard_gain_view(&self, w: &MicWizard, width: f32, cx: &mut Context<Self>) -> Div {
        let Some(v) = self.go.view.as_ref() else { return div() };
        let s = &v.state;
        let enabled = self.go.enabled();
        let gain = s["mic"]["gain_db"].as_f64().unwrap_or(0.);
        let mic_kind = s["mic"]["type"].as_str();
        let cal = &s["mic_cal"];
        let cal_matches = cal.is_object() && cal["type"].as_str() == mic_kind;
        let cal_gain = cal_matches.then(|| cal["gain_db"].as_i64()).flatten();
        let noise = cal_matches
            .then(|| {
                let shift = gain - cal["gain_db"].as_f64().unwrap_or(gain);
                cal["noise_peak_db"].as_f64().map(|n| n + shift)
            })
            .flatten();
        let (room, room_sub) = match noise {
            Some(n) if n > -42. => ("Bruyante", "PC ou ventilateurs audibles"),
            Some(n) if n > -54. => ("Souffle léger", "Un léger fond sonore s’entend"),
            Some(_) => ("Calme", "Très peu de bruit entre tes phrases"),
            None => ("Non mesurée", "L’analyse peut la mesurer"),
        };
        let valid_info = self.voice.info.as_ref().filter(|i| i["valid"] == true);
        let summary = valid_info.map(|i| &i["summary"]);
        let voice_label = summary
            .and_then(|u| u["voice_type"].as_str())
            .map(crate::voice::capital)
            .unwrap_or_else(|| if valid_info.is_some() { "Analysée".into() } else { "Non mesurée".into() });
        let f0 = summary
            .and_then(|u| u["f0_hz"].as_f64())
            .map(|f| format!("Fondamentale à {} Hz", f.round()))
            .unwrap_or_else(|| if valid_info.is_some() { "Timbre mesuré".into() } else { "Disponible après l’analyse".into() });
        let sib = summary
            .and_then(|u| u["sibilance"].as_str())
            .map(crate::voice::capital)
            .unwrap_or_else(|| "Non mesurées".into());
        let snr = summary
            .and_then(|u| u["snr_db"].as_f64())
            .map(|d| format!("{} dB", d.round()))
            .unwrap_or_else(|| "Non mesuré".into());
        let notes = valid_info
            .and_then(|i| i["notes"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|note| Some(ui::notice_state(note_state(note), note_text(note)?)));

        let diag = ui::section(Icon::Activity, ui::ACCENT, "Bilan de ta voix et ta pièce", None).child(
            div()
                .flex()
                .flex_wrap()
                .gap_x(px(32.))
                .gap_y(px(12.))
                .child(ui::fact(room, room_sub))
                .child(ui::fact(voice_label, f0))
                .child(ui::fact(sib, "Tes « S » (sifflantes)"))
                .child(ui::fact(snr, "Voix au-dessus du bruit")),
        ).children(notes);

        let gain_minus = (gain.round() as i64 - 3).clamp(0, 72);
        let gain_plus = (gain.round() as i64 + 3).clamp(0, 72);
        let mut quick = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(
                ui::pill("wizard-gain-m3", Some(Icon::Minus), "-3 dB", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.param_held.clear();
                        this.wizard_send_seq(vec![("mic", json!({"gain_db": gain_minus}))], true, cx);
                    })),
            )
            .child(
                ui::pill("wizard-gain-p3", Some(Icon::Plus), "+3 dB", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.param_held.clear();
                        this.wizard_send_seq(vec![("mic", json!({"gain_db": gain_plus}))], true, cx);
                    })),
            );
        if let Some(cg) = cal_gain {
            quick = quick.child(
                ui::pill("wizard-gain-cal", Some(Icon::Sparkles), format!("Gain mesuré ({cg} dB)"), Tone::Soft)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.param_held.clear();
                        this.wizard_send_seq(vec![("mic", json!({"gain_db": cg}))], true, cx);
                    })),
            );
        }

        let mut gain_card = ui::section(Icon::Gauge, ui::GREEN, "Gain d’entrée", None)
            .child(self.slider_row(
                "wizard-gain",
                "Sensibilité du préampli",
                gain,
                (0., 72.),
                1.,
                |val| format!("{} dB", val.round()),
                enabled,
                |this, val, cx| this.wizard_send_seq(vec![("mic", json!({"gain_db": val.round() as i64}))], true, cx),
                cx,
            ))
            .child(quick)
            .child(ui::caption("Parle normalement et vise la zone verte du vumètre, entre −18 et −6 dB."));
        if let Some(cg) = cal_gain
            && (gain.round() as i64 - cg).abs() >= 10
        {
            gain_card = gain_card.child(ui::notice(
                false,
                format!("Ton gain ({} dB) est très éloigné de ta mesure ({cg} dB) : si le son paraît faible ou trop fort, clique sur « Gain mesuré ({cg} dB) ».", gain.round()),
            ));
        }

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(ui::settings_columns(width, vec![gain_card.into_any_element()], vec![diag.into_any_element()]))
            .children(self.wizard_replay_bar(w, cx))
    }

    /// Tous les filtres de la GoXLR, avec une seule section développée à la fois.
    pub(crate) fn go_mic_filters(&self, s: &Value, width: f32, cx: &mut Context<Self>) -> Div {
        let enabled = self.go.enabled();
        let gate = self.go_filter_toggle("gate", s, enabled, cx);
        let eq = self.go_filter_toggle("eq", s, enabled, cx);
        let comp = self.go_filter_toggle("comp", s, enabled, cx);
        let deesser = self.go_filter_toggle("deesser", s, enabled, cx);
        let mut list = div().w_full().child(self.winmic_noise(width, cx))
            .child(self.mic_filter_row(MicFilter::Gate, width, true, gate, |cx| self.mic_gate_settings(s, width, cx), cx))
            .child(self.mic_filter_row(MicFilter::Eq, width, true, eq, |cx| self.mic_eq_settings(s, cx), cx))
            .child(self.mic_filter_row(MicFilter::Compressor, width, true, comp, |cx| self.mic_comp_settings(s, width, cx), cx))
            .child(self.mic_filter_row(MicFilter::Deesser, width, true, deesser, |cx| self.mic_deess_settings(s, cx), cx));

        if self.rustudio_firmware() {
            let chain = micchain::design(&dsp::effective_dsp(
                &dsp::normalize_dsp(&s["dsp"], None), &dsp::normalize_fx(&s["fx"]),
            ));
            let bypass = self.go.view.as_ref().is_some_and(|v| v.ab);
            let text = if chain.active() && !bypass { "Auto · −1 dBFS" } else { "En veille" };
            list = list.child(self.mic_filter_row(MicFilter::Limiter, width, false, ui::caption(text), |_| div(), cx));
        }

        list
    }

    fn go_filter_toggle(&self, key: &'static str, s: &Value, enabled: bool, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let on = s["fx"][key] == true;
        ui::toggle(SharedString::from(format!("mic-fx-{key}")), on, enabled)
            .on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({(key): !on}), cx)))
    }

    fn mic_gate_settings(&self, s: &Value, width: f32, cx: &mut Context<Self>) -> Div {
        let enabled = self.go.enabled();
        let on = s["fx"]["gate"] == true;
        let gate = &s["dsp"]["gate"];
        let presets = div().flex().flex_wrap().items_center().gap(px(8.)).children([
            ("doux", "Doux"), ("equilibre", "Équilibré"), ("clavier", "Anti-clavier"),
        ].into_iter().map(|(key, title)| {
            ui::pill(SharedString::from(format!("mic-gate-preset-{key}")), None, title, Tone::Plain)
                .when(enabled, |d| d.on_click(cx.listener(move |this, _, _, cx| this.apply_mic_gate_preset(key, cx))))
                .when(!enabled, |d| d.opacity(0.45))
        }));
        let controls = [
            ("threshold", "Seuil d’ouverture", -46., (-59., 0.), 1., eq_db_text as fn(f64) -> String),
            ("attenuation", "Atténuation", 80., (0., 100.), 5., |x| format!("{x:.0} %")),
            ("attack", "Attaque", 0., (0., (GATE_MS.len() - 1) as f64), 1., fmt_gate_ms),
            ("release", "Relâchement", 18., (0., (GATE_MS.len() - 1) as f64), 1., fmt_gate_ms),
        ];
        let sliders = controls.into_iter().map(|(field, title, fallback, range, step, format)| {
            self.slider_row(&format!("goxlr-gate-{field}"), title, gate[field].as_f64().unwrap_or(fallback), range,
                step, format, enabled && on,
                move |this, x, cx| this.go_send("dsp", json!({"gate": {(field): x.round() as i64}}), cx), cx)
                .into_any_element()
        }).collect();

        div().flex().flex_col().gap(px(12.))
            .child(presets)
            .child(ui::grid(sliders, mic_filter_columns(width)))
    }

    /// Console d'égaliseur 6 bandes (faders verticaux + fréquences réglables + courbe visuelle).
    fn mic_eq_settings(&self, s: &Value, cx: &mut Context<Self>) -> Div {
        let enabled = self.go.enabled();
        let eq_on = s["fx"]["eq"] == true;
        let dsp = &s["dsp"];
        let g90 = dsp["eq"]["90"].as_f64().unwrap_or(0.);
        let f250 = dsp["eq_freq"]["250"].as_f64().unwrap_or(250.);
        let fitted_layout = f250 < 230. || g90 < 0.;

        let preset_key = s["mic_preset"].as_str().filter(|p| dsp::is_preset(p) && *p != "brut");
        let active_key = preset_key.unwrap_or("antenne");
        let eq_notes: Vec<String> = self
            .voice
            .info
            .as_ref()
            .filter(|i| i["valid"] == true)
            .and_then(|i| i["profiles"][active_key]["notes"].as_array())
            .map(|arr| filter_eq_notes(arr))
            .unwrap_or_default();

        let quick_eq = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(ui::caption("Courbe de départ :"))
            .child(
                ui::pill("wizard-eq-antenne", Some(Icon::Radio), "Radio FM", Tone::Soft)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.apply_mic_eq_preset("antenne", cx))),
            )
            .child(
                ui::pill("wizard-eq-studio", Some(Icon::Podcast), "Studio propre", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.apply_mic_eq_preset("studio", cx))),
            )
            .child(
                ui::pill("wizard-eq-podcast", Some(Icon::Mic), "Chaleur podcast", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.apply_mic_eq_preset("podcast", cx))),
            )
            .child(
                ui::pill("wizard-eq-stream", Some(Icon::Sparkles), "Voix claire", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.apply_mic_eq_preset("stream", cx))),
            )
            .child(
                ui::pill("wizard-eq-flat", Some(Icon::History), "Plat (0 dB)", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.apply_mic_eq_preset("flat", cx))),
            );

        let mut bands = div().flex().gap(px(6.)).pt(px(6.));
        for (band, _, _) in EQ_BANDS {
            let band_str = band.to_owned();
            let freq = dsp["eq_freq"][band].as_f64().unwrap_or_else(|| eq_default_freq(band));
            let (lo, hi) = eq_range(band).unwrap_or((20., 20000.));
            let down = (freq / 1.12).round().clamp(lo, hi);
            let up = (freq * 1.12).round().clamp(lo, hi);
            let (role, sub) = eq_band_role(band, fitted_layout);
            let (b1, b2, b3) = (band_str.clone(), band_str.clone(), band_str.clone());
            let below = div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(2.))
                .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(ui::TEXT)).child(role))
                .child(div().text_size(px(10.5)).text_color(rgb(ui::TEXT3)).child(sub))
                .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::ACCENT)).child(fmt_hz(freq)))
                .child(
                    div()
                        .mt(px(3.))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(
                            ui::round(SharedString::from(format!("wiz-eqf-dn-{band}")), Icon::ChevronLeft, 22., alpha(0x767680, 0.08), rgb(ui::TEXT2))
                                .when(enabled && eq_on, |d| {
                                    d.on_click(cx.listener(move |this, _, _, cx| this.go_send("dsp", json!({"eq_freq": {(b1.clone()): down}}), cx)))
                                }),
                        )
                        .child(
                            ui::round(SharedString::from(format!("wiz-eqf-up-{band}")), Icon::ChevronRight, 22., alpha(0x767680, 0.08), rgb(ui::TEXT2))
                                .when(enabled && eq_on, |d| {
                                    d.on_click(cx.listener(move |this, _, _, cx| this.go_send("dsp", json!({"eq_freq": {(b2.clone()): up}}), cx)))
                                }),
                        ),
                );
            bands = bands.child(self.eq_column(
                &format!("wizard-eq-{band}"),
                dsp["eq"][band].as_f64().unwrap_or(0.),
                (-9., 9.),
                1.,
                below.into_any_element(),
                enabled && eq_on,
                move |this, val, cx| this.go_send("dsp", json!({"eq": {(b3.clone()): val.round() as i64}}), cx),
                cx,
            ));
        }

        div().flex().flex_col().gap(px(10.))
            .child(quick_eq)
            .child(bands)
            .children(eq_notes.into_iter().map(ui::caption))
    }

    fn mic_comp_settings(&self, s: &Value, width: f32, cx: &mut Context<Self>) -> Div {
        let enabled = self.go.enabled();
        let on = s["fx"]["comp"] == true;
        let comp = &s["dsp"]["comp"];
        let active_dose = selected_dose(on, s["mic_preset"].as_str().unwrap_or_default(),
            comp["ratio"].as_f64().unwrap_or(9.), comp["threshold"].as_f64().unwrap_or(-22.));
        let presets = div().flex().flex_wrap().items_center().gap(px(8.))
            .child(ui::caption("Compression et sifflantes"))
            .children([("doux", "Doux"), ("stream", "Stream"), ("skyrock", "Radio FM")].into_iter().map(|(key, title)| {
                ui::pill(SharedString::from(format!("mic-comp-preset-{key}")), None, title,
                    if active_dose == Some(key) { Tone::Soft } else { Tone::Plain })
                    .when(enabled, |d| d.on_click(cx.listener(move |this, _, _, cx| this.apply_mic_radio_dose(key, cx))))
                    .when(!enabled, |d| d.opacity(0.45))
            }));
        let controls = [
            ("threshold", "Seuil", -22., (-40., 0.), eq_db_text as fn(f64) -> String),
            ("ratio", "Ratio", 9., (0., 14.), ratio_text),
            ("makeup", "Gain de compensation", 6., (-6., 24.), eq_db_text),
            ("attack", "Attaque", 2., (0., (COMP_ATTACK_MS.len() - 1) as f64), fmt_comp_attack),
            ("release", "Relâchement", 10., (0., (COMP_RELEASE_MS.len() - 1) as f64), fmt_comp_release),
        ];
        let sliders = controls.into_iter().map(|(field, title, fallback, range, format)| {
            self.slider_row(&format!("goxlr-comp-{field}"), title, comp[field].as_f64().unwrap_or(fallback), range,
                1., format, enabled && on,
                move |this, x, cx| this.go_send("dsp", json!({"comp": {(field): x.round() as i64}}), cx), cx)
                .into_any_element()
        }).collect();

        div().flex().flex_col().gap(px(12.))
            .child(presets)
            .when(on, |d| d.children(self.mic_softer_compressor(s, cx)))
            .child(ui::grid(sliders, mic_filter_columns(width)))
    }

    fn mic_softer_compressor(&self, s: &Value, cx: &mut Context<Self>) -> Option<Div> {
        let current: dsp::Comp = serde_json::from_value(s["dsp"]["comp"].clone()).ok()?;
        let enabled = self.go.enabled() && !self.go.busy;
        if let Some((before, applied)) = self.go.softer_compressor_undo.as_ref().filter(|(_, after)| after == &current) {
            let (before, applied) = (before.clone(), applied.clone());
            return Some(div().flex().flex_wrap().items_center().gap(px(8.))
                .child(ui::caption("Compression adoucie"))
                .child(ui::pill("mic-revert-softer-comp", Some(Icon::History), "Rétablir la compression", Tone::Plain)
                    .when(enabled, |d| d.on_click(cx.listener(move |this, _, _, cx| {
                        if this.go.busy || this.go.view.as_ref().is_none_or(|v| v.state["dsp"]["comp"] != json!(applied)) { return; }
                        this.go_send("dsp", json!({"comp": before}), cx);
                    })))));
        }
        let cal: rustudio_core::state::MicCal = serde_json::from_value(s["mic_cal"].clone()).ok()?;
        if s["mic"]["type"].as_str() != Some(cal.kind.as_str()) { return None; }
        let gain = i32::try_from(s["mic"]["gain_db"].as_i64()?).ok()?;
        let next = softer_compressor(&current, &cal, gain)?;
        Some(div().flex().flex_wrap().items_center().gap(px(8.))
            .child(ui::pill("mic-softer-comp", Some(Icon::Wave), "Adoucir la compression", Tone::Soft)
                .when(enabled, |d| d.on_click(cx.listener(move |this, _, _, cx| {
                    if this.go.busy || this.go.view.as_ref().is_none_or(|v| {
                        v.state["dsp"]["comp"] != json!(current) || v.state["fx"]["comp"] != true
                            || v.state["mic"]["gain_db"] != gain || v.state["mic"]["type"] != cal.kind
                    }) { return; }
                    this.go.softer_compressor_undo = Some((current.clone(), next.clone()));
                    this.go_send("dsp", json!({"comp": next}), cx);
                }))))
            .child(ui::caption("Réduit les remontées de souffle liées à la compensation.")))
    }

    fn mic_deess_settings(&self, s: &Value, cx: &mut Context<Self>) -> Div {
        self.slider_row("goxlr-deesser", "Intensité", s["dsp"]["deesser"].as_f64().unwrap_or(30.),
            (0., 100.), 5., |x| format!("{x:.0} %"), self.go.enabled() && s["fx"]["deesser"] == true,
            |this, x, cx| this.go_send("dsp", json!({"deesser": x.round() as i64}), cx), cx)
    }

    /// Comparateur final Avant / Après (en direct + prise + anciens réglages) et bibliothèque de styles.
    fn wizard_compare_view(&self, w: &MicWizard, width: f32, cx: &mut Context<Self>) -> Div {
        let state = self.go.view.as_ref().map(|v| &v.state);
        let current = state.and_then(|s| s["mic_preset"].as_str()).unwrap_or("custom").to_owned();
        let bypass = self.go.view.as_ref().is_some_and(|v| v.ab);
        let can_play = w.take.is_some();
        let playing = w.playing.as_deref();
        let has_snap = w.snapshot.is_some();

        let mut rows: Vec<AnyElement> = vec![];
        for p in dsp::MIC_PRESETS.iter().filter(|p| p.key != "brut") {
            let id = p.key.to_owned();
            let play_id = id.clone();
            let is_playing = playing == Some(p.key);
            let badge = if p.key == "antenne" { Some(("Style Skyrock".into(), ui::ORANGE)) } else { None };
            let tile = ui::tile(
                SharedString::from(format!("wizard-preset-{id}")),
                None,
                p.label.to_owned(),
                Some(p.desc.into()),
                current == p.key && !bypass,
                badge,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.wizard_choose(id.clone(), cx)));
            let play = can_play.then(|| {
                ui::round(
                    SharedString::from(format!("wizard-play-{}", p.key)),
                    if is_playing { Icon::Wave } else { Icon::Play },
                    34.,
                    alpha(ui::ACCENT, 0.1),
                    rgb(ui::ACCENT),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.wizard_play(Some(play_id.clone()), cx)))
            });
            rows.push(div().flex().items_center().gap(px(8.)).child(tile.flex_1().min_w(px(0.))).children(play).into_any_element());
        }

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(has_snap, |d| d.child(div().flex().child(
                ui::pill("wizard-cmp-old", Some(Icon::History), "Recharger mes anciens réglages", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_snapshot(cx))),
            )))
            .children(self.wizard_replay_bar(w, cx))
            .child(
                ui::section(Icon::Sparkles, ui::ACCENT, "Préréglages", None)
                    .child(ui::grid(rows, ui::columns_that_fit(width - 40., 320., 10., 2))),
            );
        if !can_play {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(ui::caption("Compare en direct au casque ou lance une analyse pour enregistrer une prise."))
                    .child(
                        ui::pill("wizard-compare-remeasure", Some(Icon::Wave), "Analyser ma voix", Tone::Plain)
                            .when(!self.go.enabled(), |d| d.opacity(0.45))
                            .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Measure, cx))),
                    ),
            );
        }

        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse_facultative_et_consignes_sans_texte_a_lire() {
        assert_eq!(MicWizard::new().step, Step::Gain);
        assert_eq!(TABS.last().map(|(tab, _, _)| *tab), Some(Step::Measure));
        assert_eq!(measure_prompt(&Value::Null, 100.).2, None);
        let reading = json!({"phase": "reading", "phase_end": 108.2});
        assert_eq!(measure_prompt(&reading, 100.).0, "Parle normalement");
        assert_eq!(measure_prompt(&reading, 100.).2, Some(9));
        assert_eq!(measure_prompt(&reading, 109.).2, None);
        let retry = json!({"phase":"reading","retry":1});
        assert_eq!(measure_prompt(&retry, 100.).1, "Le gain a été baissé. Continue avec tes propres mots.");
        assert!(note_state(&json!({"lvl":"warn"})) == ui::State::Warn);
        assert!(note_state(&json!({"lvl":"tip"})) == ui::State::Info);
        let legacy = json!({"lvl":"warn", "t":"Ronflement à 60 Hz : éloigne le câble du micro."});
        assert!(note_text(&legacy).unwrap().starts_with("Ronflement à 60 Hz. Éloigne"));
    }

    #[test]
    fn formatages_lisibles_et_zero_tiret_cadratin() {
        assert_eq!(ratio_text(10.), "5,6:1");
        assert_eq!(ratio_text(9.), "4:1");
        assert_eq!(eq_db_text(4.), "+4 dB");
        assert_eq!(eq_db_text(-3.), "-3 dB");
        assert_eq!(eq_db_text(0.), "0 dB");
        assert_eq!(fmt_gate_ms(0.), "10 ms");
        assert_eq!(fmt_gate_ms(35.), "1 s");
        assert_eq!(fmt_comp_attack(0.), "0,1 ms (instantanée)");
        assert_eq!(fmt_comp_release(16.), "1 s");
        for (_, _, label) in TABS {
            assert!(!label.contains('\u{2014}') && !label.contains('\u{2013}'));
        }
        assert_eq!(fmt_hz(90.), "90 Hz");
        assert_eq!(fmt_hz(3000.), "3 kHz");
        assert_eq!(fmt_hz(3500.), "3,5 kHz");
    }

    #[test]
    fn snapshot_capture_tous_les_reglages_initiaux() {
        let state = json!({
            "mic": {"type": "condenser", "model": "rode-nt1a", "gain_db": 56},
            "mic_preset": "studio",
            "fx": {"eq": true, "gate": true, "comp": true, "deesser": true},
            "dsp": {"deesser": 20},
            "mic_ns": {"enabled": false, "strength": 77, "grace_ms": 200, "device": "{aaaaaaaa-0000-0000-0000-000000000001}"},
            "volumes": {"MicMonitor": 0}
        });
        let snap = MicSnapshot::capture(&state);
        assert_eq!(snap.mic_type, "condenser");
        assert_eq!(snap.mic_model.as_deref(), Some("rode-nt1a"));
        assert_eq!(snap.gain_db, 56);
        assert_eq!(snap.mic_preset, "studio");
        assert_eq!(snap.mic_monitor, 0);
        assert_eq!(snap.mic_ns.unwrap(), state["mic_ns"]);
    }

    #[test]
    fn reading_part_prend_la_derniere_lecture_apres_rattrapage_saturation() {
        let take = crate::voice::Take {
            samples: (0..48_000 * 12).map(|i| i as f32 / 1000.).collect(),
            rate: 48_000,
            t0_ms: 100_000,
        };
        let timeline = json!([
            {"phase": "silence", "start": 100.0, "end": 102.0, "gain_db": 45},
            {"phase": "reading", "start": 102.0, "end": 104.0, "gain_db": 45},
            {"phase": "reading", "start": 105.0, "end": 111.0, "gain_db": 35},
            {"phase": "loud", "start": 111.0, "end": 112.0, "gain_db": 35}
        ]);
        let part = reading_part(&take, &timeline).expect("lecture trouvee");
        assert_eq!(part.gain_db, 35, "doit prendre la derniere lecture non saturee");
        assert_eq!(part.samples.len(), 6 * 48_000);
    }

    #[test]
    fn notes_eq_filtrees_et_doses_coherentes() {
        let notes = vec![
            json!({"lvl": "ok", "t": "+3 dB à 180 Hz : chaleur « Radio FM »."}),
            json!({"lvl": "ok", "t": "−3 dB à 380 Hz : médiums allégés pour « Radio FM »."}),
            json!({"lvl": "ok", "t": "Sifflantes normales : de-esser à 40 %."}),
            json!({"lvl": "ok", "t": "Noise gate à −48 dB : 24 dB de bruit en moins entre tes phrases."}),
            json!({"lvl": "ok", "t": "Compresseur 4:1 dès −26 dB (attaque 3 ms, relâchement 115 ms), +8 dB de compensation : ta voix reste régulière."}),
        ];
        assert_eq!(filter_eq_notes(&notes).len(), 2);
        assert_eq!(selected_dose(false, "antenne", 10., -26.), None);
        assert_eq!(selected_dose(true, "antenne", 9., -20.), Some("skyrock"));
        assert_eq!(selected_dose(true, "custom", 8., -20.), Some("stream"));
        assert_eq!(selected_dose(true, "custom", 6., -16.), Some("doux"));
        assert!(is_cancelled_error("Analyse annulée."));
        assert!(is_cancelled_error("Mesure annulée."));
        assert!(!is_cancelled_error("GoXLR déconnectée."));
        assert_eq!(radio_dose_params(None, "doux"), (-16, 6, 5, 11, 3, 15));
        assert_eq!(radio_dose_params(None, "skyrock"), (-24, 10, 2, 10, 8, 40));
    }
}
