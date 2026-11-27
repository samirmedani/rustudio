//! Studio Micro guidé « Régler mon micro » : espace dédié pleine page en 8 étapes pour configurer
//! son micro pas à pas, avec explications simples, contrôles complets (Gain, Noise Gate, IA RNNoise,
//! Égaliseur 6 bandes + fréquences, Compresseur complet, De-esser, Limiteur), écoute directe Avant / Après,
//! vumètres DSP en temps réel et annulation avec restauration intégrale des anciens réglages.
use crate::desktop::hint;
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
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

/// Trois phrases de live/radio, une toutes les 5 secondes pendant la lecture.
const SENTENCES: [&str; 3] = [
    "« Salut tout le monde, bienvenue en direct sur le live ! »",
    "« Aujourd’hui, on règle mon micro étape par étape pour avoir un vrai son radio. »",
    "« Si vous m’entendez bien chaud et bien propre, dites-le dans le chat ! »",
];
const SENTENCE_S: f64 = 5.0;
const LOUD_SENTENCE: &str = "« Oh non, c’est pas possible, on l’avait presque ! »";
/// Volume du retour de ta voix dans le casque pendant l'assistant (sur 255).
const LISTEN_LEVEL: i64 = 185;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Mic,
    Measure,
    Gain,
    Noise,
    Eq,
    Radio,
    Compare,
    Done,
}

const STEPS: [(Step, &str); 8] = [
    (Step::Mic, "1. Micro"),
    (Step::Measure, "2. Mesure"),
    (Step::Gain, "3. Gain"),
    (Step::Noise, "4. Anti-bruit"),
    (Step::Eq, "5. Égaliseur"),
    (Step::Radio, "6. Radio FM"),
    (Step::Compare, "7. Comparer"),
    (Step::Done, "8. Résumé"),
];

impl Step {
    pub fn index(self) -> usize {
        STEPS.iter().position(|(s, _)| *s == self).unwrap_or(0)
    }

    #[cfg(test)]
    pub fn next(self) -> Option<Step> {
        STEPS.get(self.index() + 1).map(|(s, _)| *s)
    }

    #[cfg(test)]
    pub fn prev(self) -> Option<Step> {
        self.index().checked_sub(1).and_then(|i| STEPS.get(i)).map(|(s, _)| *s)
    }
}

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
    listening: bool,
    /// Génération partagée de la boucle d'écoute : incrémentée à chaque arrêt pour garantir zéro fuite audio.
    listen_gen: Arc<AtomicU64>,
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
        self.listen_gen.fetch_add(1, Ordering::SeqCst);
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
            step: Step::Mic,
            snapshot: None,
            cancel: None,
            job: Value::Null,
            error: None,
            chosen: None,
            take: None,
            playing: None,
            listening: false,
            listen_gen: Arc::new(AtomicU64::new(0)),
            run: 0,
            play_run: 0,
        }
    }

    pub fn is_listening(&self) -> bool {
        self.listening
    }

    fn busy(&self) -> bool {
        self.cancel.is_some()
    }
}

fn is_cancelled_error(e: &str) -> bool {
    matches!(e, "Analyse annulée." | "Mesure annulée.")
}

/// Empreinte vocale valable pour le micro actuellement sélectionné (même logique que State::voice_for_mic).
fn voice_print_valid(state: &Value) -> bool {
    let vp = &state["voice_print"];
    if !vp.is_object() {
        return false;
    }
    let mic = &state["mic"];
    let Some(kind) = mic["type"].as_str() else {
        return false;
    };
    if vp["mic_type"].as_str() != Some(kind) {
        return false;
    }
    match (vp["model"].as_str(), mic["model"].as_str()) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
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

/// Encadré pédagogique clair qui explique « pourquoi on fait ça » sans jargon technique.
fn lesson(glyph: Icon, colour: u32, title: &str, text: impl Into<SharedString>) -> Div {
    div()
        .p(px(14.))
        .rounded(px(12.))
        .bg(alpha(colour, 0.08))
        .border_1()
        .border_color(alpha(colour, 0.22))
        .flex()
        .items_start()
        .gap(px(12.))
        .child(div().flex_none().mt(px(2.)).child(ui::icon(glyph, 18., rgb(colour))))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(ui::TEXT)).child(title.to_owned()))
                .child(div().text_size(px(12.5)).line_height(px(18.)).text_color(rgb(ui::TEXT2)).child(text.into())),
        )
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
fn dsp_meter_pill(label: &'static str, reduction_db: f64, enabled: bool, active_colour: u32) -> Div {
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
    pub fn open_mic_wizard(&mut self, cx: &mut Context<Self>) {
        self.go_to(crate::desktop::Page::Go(crate::goxlr::Section::Microphone), cx);
        let mut w = MicWizard::new();
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

    /// Coupe immédiatement l'état local d'écoute de l'assistant (appelé aussi par cut_monitor).
    pub(crate) fn stop_wizard_listen_state(&mut self) {
        if let Some(w) = self.wizard.as_mut() {
            w.listening = false;
            w.listen_gen.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Enregistrer et terminer : garde les nouveaux réglages, coupe systématiquement le retour casque et l'A/B.
    fn finish_mic_wizard(&mut self, cx: &mut Context<Self>) {
        let mut cancel_job = false;
        let mut restore_monitor_zero = true;
        if let Some(w) = self.wizard.as_mut() {
            if let Some(cancel) = w.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
                cancel_job = true;
            }
            w.listening = false;
            w.listen_gen.fetch_add(1, Ordering::SeqCst);
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
            let _ = client.set_mic_listen(0, None);
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
        let mut cancel_job = false;
        let snapshot = if let Some(w) = self.wizard.as_mut() {
            if let Some(cancel) = w.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
                cancel_job = true;
            }
            w.listening = false;
            w.listen_gen.fetch_add(1, Ordering::SeqCst);
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
            let _ = client.set_mic_listen(0, None);
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

    /// Applique temporairement les anciens réglages sauvegardés (comparaison sur l'étape 7).
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

    /// Relit l'état de la GoXLR (gain, préréglage, traitements) après une étape.
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

    /// Étape 2 : mesure guidée 25 s ; le moteur mène les phases, l'appli enregistre la voix brute.
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
                let mut ok = false;
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
                            ok = true;
                        }
                    }
                    Err(e) => {
                        if let Some(w) = this.wizard_mut() {
                            Self::wizard_end(w, Some(e));
                        }
                    }
                }
                // Mesure réussie : on applique le profil Radio FM calculé sur ta voix (adapté aux micros brillants),
                // sans jamais forcer l'IA RNNoise contre ton gré (tu la règles librement à l'étape 4).
                if ok {
                    if let Some(w) = this.wizard_mut() {
                        w.chosen = Some("antenne".into());
                    }
                    this.param_held.clear();
                    let client = this.client.clone();
                    let work = cx.background_executor().spawn(async move {
                        let _ = client.go_command("mic-ab", json!({"bypass": false}));
                        let _ = client.go_command("mic-preset", json!({"name": "antenne"}));
                        client.go_view().ok()
                    });
                    cx.spawn(async move |this, cx| {
                        let view = work.await;
                        let _ = this.update(cx, |this, cx| {
                            this.go.receive(view);
                            this.load_voice_info(cx);
                            cx.notify();
                        });
                    })
                    .detach();
                } else {
                    this.go_refresh(cx);
                    this.load_voice_info(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Réécoute de ta prise : brute (« avant ») ou passée par la chaîne d'un préréglage, au gain actuel du micro.
    fn wizard_play(&mut self, preset: Option<String>, cx: &mut Context<Self>) {
        let Some(w) = self.wizard.as_ref() else { return };
        let Some(take) = w.take.as_ref() else { return };
        if w.playing.is_some() {
            return;
        }
        let gain_now = self.go.view.as_ref().and_then(|v| v.state["mic"]["gain_db"].as_i64()).unwrap_or(take.gain_db as i64) as i32;
        let scale = 10f32.powf((gain_now - take.gain_db) as f32 / 20.);
        let (samples, rate) = (take.samples.clone(), take.rate);
        let chain = match preset.as_deref() {
            Some("actuel") => {
                let Some(v) = self.go.view.as_ref() else { return };
                Some(micchain::design(&dsp::effective_dsp(
                    &dsp::normalize_dsp(&v.state["dsp"], None),
                    &dsp::normalize_fx(&v.state["fx"]),
                )))
            }
            Some(p) => match profile_dsp(self.voice.info.as_ref(), p) {
                Some(d) => Some(micchain::design(&d)),
                None => Some(micchain::design(&dsp::preset_dsp(p))),
            },
            None => None,
        };
        let Some(w) = self.wizard_mut() else { return };
        w.play_run += 1;
        let play_run = w.play_run;
        w.playing = Some(preset.clone().unwrap_or_else(|| "avant".into()));
        w.error = None;
        let work = cx.background_executor().spawn(async move {
            let raw: Vec<f32> = samples.iter().map(|x| (x * scale).clamp(-1., 1.)).collect();
            let (out, play_rate) = match chain {
                Some(p) => {
                    let src = if rate == 48_000 { raw } else { rustudio_windows_desktop::soundcheck::resample(&raw, rate, 48_000) };
                    (micchain::render(&p, &src), 48_000)
                }
                None => (raw, rate),
            };
            rustudio_windows_desktop::soundcheck::play_default(out, play_rate)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
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
    fn wizard_apply_eq_preset(&mut self, preset: &str, cx: &mut Context<Self>) {
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
    fn wizard_apply_gate_preset(&mut self, mode: &'static str, cx: &mut Context<Self>) {
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
    fn wizard_apply_radio_dose(&mut self, dose: &'static str, cx: &mut Context<Self>) {
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

    /// Écoute en direct parfaitement synchronisée avec le moteur et sans course critique :
    /// si le retour est déjà actif (assistant ou page Micro), un clic le coupe net.
    fn wizard_toggle_listen(&mut self, cx: &mut Context<Self>) {
        if self.wizard.as_ref().is_some_and(MicWizard::busy) {
            return;
        }
        if self.monitor_on() {
            self.cut_monitor(cx);
            return;
        }
        let Some(w) = self.wizard_mut() else { return };
        w.listening = true;
        let my_gen = w.listen_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let gen_arc = w.listen_gen.clone();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            loop {
                if gen_arc.load(Ordering::SeqCst) != my_gen {
                    break;
                }
                let c = client.clone();
                let g = gen_arc.clone();
                let view = cx
                    .background_executor()
                    .spawn(async move {
                        if g.load(Ordering::SeqCst) != my_gen {
                            return None;
                        }
                        let _ = c.set_mic_listen(20, Some(LISTEN_LEVEL));
                        // Vérifie juste après l'envoi : si un arrêt a eu lieu entre-temps, coupe immédiatement.
                        if g.load(Ordering::SeqCst) != my_gen {
                            let _ = c.set_mic_listen(0, None);
                            return None;
                        }
                        c.go_view().ok()
                    })
                    .await;
                if let Some(v) = view {
                    let _ = this.update(cx, |this, cx| {
                        this.go.receive(Some(v));
                        cx.notify();
                    });
                }
                cx.background_executor().timer(Duration::from_secs(8)).await;
                let still = gen_arc.load(Ordering::SeqCst) == my_gen
                    && this
                        .update(cx, |this, _| this.wizard.as_ref().is_some_and(|w| w.listening))
                        .unwrap_or(false);
                if !still {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Bascule instantanée Avant (voix brute sans filtre) / Après (traitements actifs) en direct.
    fn wizard_toggle_ab(&mut self, bypass: bool, cx: &mut Context<Self>) {
        self.wizard_send_seq(vec![("mic-ab", json!({"bypass": bypass, "seconds": 60}))], false, cx);
    }

    /// Annuler la mesure en cours : arrête immédiatement le moteur et revient à l'étape 1 sans casser tes réglages.
    fn wizard_cancel_measure(&mut self, cx: &mut Context<Self>) {
        let Some(w) = self.wizard_mut() else { return };
        if let Some(cancel) = w.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        w.run += 1;
        Self::wizard_end(w, None);
        w.step = Step::Mic;
        self.send_quiet("job-cancel", json!({}), cx);
        cx.notify();
    }

    /// Barre rapide de réécoute Avant / Après affichée dans les étapes de réglage dès qu'une prise existe.
    fn wizard_replay_bar(&self, w: &MicWizard, cx: &mut Context<Self>) -> Option<Div> {
        w.take.as_ref()?;
        let playing = w.playing.as_deref();
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
                        .child(ui::caption("Compare ton enregistrement brut avec tes réglages actuels.")),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            ui::pill(
                                "wizard-replay-raw",
                                Some(if playing == Some("avant") { Icon::Wave } else { Icon::Play }),
                                if playing == Some("avant") { "Lecture…" } else { "Avant (brut)" },
                                Tone::Plain,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.wizard_play(None, cx))),
                        )
                        .child(
                            ui::pill(
                                "wizard-replay-now",
                                Some(if playing == Some("actuel") { Icon::Wave } else { Icon::Play }),
                                if playing == Some("actuel") { "Lecture…" } else { "Après (réglé)" },
                                Tone::Primary,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.wizard_play(Some("actuel".into()), cx))),
                        ),
                ),
        )
    }

    // ---------- espace pleine page ----------

    /// Affiche l'assistant directement dans la page Micro (pleine largeur, défilement natif, jamais tronqué).
    pub fn mic_wizard_workspace(&self, width: f32, cx: &mut Context<Self>) -> Option<Div> {
        let w = self.wizard.as_ref()?;
        let (body, actions) = match w.step {
            Step::Mic => self.wizard_mic_view(width, cx),
            Step::Measure => self.wizard_measure_view(w, cx),
            Step::Gain => self.wizard_gain_view(w, width, cx),
            Step::Noise => self.wizard_noise_view(w, width, cx),
            Step::Eq => self.wizard_eq_view(w, width, cx),
            Step::Radio => self.wizard_radio_view(w, width, cx),
            Step::Compare => self.wizard_compare_view(w, width, cx),
            Step::Done => self.wizard_done_view(cx),
        };
        let step_num = w.step.index() + 1;
        let monitor_on = self.monitor_on();

        let head = div()
            .p(px(18.))
            .rounded(px(16.))
            .bg(alpha(ui::PAPER, 0.92))
            .border_1()
            .border_color(alpha(ui::INK, 0.08))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(ui::chip(Icon::Radio, ui::ACCENT, 40.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_size(px(19.)).font_weight(FontWeight::BOLD).child("Studio Micro · Configuration guidée"))
                                    .child(ui::caption(format!("Étape {step_num} sur {} : chaque réglage est expliqué, modifiable en direct et annulable.", STEPS.len()))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .when(monitor_on, |d| {
                                d.child(
                                    ui::pill("wizard-head-stop-mon", Some(Icon::VolumeOff), "Couper le retour", Tone::Danger)
                                        .on_click(cx.listener(|this, _, _, cx| this.cut_monitor(cx))),
                                )
                            })
                            .child(
                                ui::pill("wizard-cancel-all", Some(Icon::History), "Annuler et restaurer", Tone::Plain)
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_mic_wizard(cx))),
                            ),
                    ),
            )
            .child(self.wizard_stepper(w, cx))
            .child(self.wizard_live_console(w, cx));

        let mut root = div().flex().flex_col().gap(px(18.)).child(head);
        if let Some(e) = &w.error {
            root = root.child(ui::notice(false, e.clone()));
        }
        root = root.child(body).child(
            div()
                .pt(px(16.))
                .border_t_1()
                .border_color(alpha(ui::INK, 0.08))
                .child(actions),
        );
        Some(root)
    }

    fn wizard_stepper(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        let can_jump = !w.busy() && self.go.view.as_ref().is_some_and(|v| v.state["mic"]["model"].is_string());
        let current = w.step.index();
        let mut stepper = div().flex().gap(px(6.));
        for (i, (st, label)) in STEPS.iter().enumerate() {
            let state = if i < current { 2 } else if i == current { 1 } else { 0 };
            let colour = if state == 2 { ui::GREEN } else { ui::ACCENT };
            let target = *st;
            let clickable = can_jump && target != Step::Measure && target != w.step;
            let item = div()
                .id(SharedString::from(format!("wizard-step-{i}")))
                .flex_1()
                .min_w(px(0.))
                .py(px(4.))
                .flex()
                .flex_col()
                .gap(px(5.))
                .when(clickable, |d| d.cursor(gpui::CursorStyle::PointingHand).on_click(cx.listener(move |this, _, _, cx| this.wizard_goto(target, cx))))
                .child(div().h(px(4.)).rounded_full().bg(if state == 0 { alpha(ui::INK, 0.09) } else { alpha(colour, 1.) }))
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.5))
                        .font_weight(if state == 1 { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                        .text_color(rgb(if state == 0 { ui::TEXT3 } else if state == 1 { ui::ACCENT } else { ui::TEXT2 }))
                        .child(*label),
                );
            stepper = stepper.child(item);
        }
        stepper
    }

    /// Bandeau de contrôle en direct : vumètre d'entrée, activité DSP temps réel, écoute casque et bascule Avant/Après.
    fn wizard_live_console(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        let monitor_on = self.monitor_on();
        let enabled = !w.busy() && self.go.enabled();
        let bypass = self.go.view.as_ref().is_some_and(|v| v.ab);
        let fx = self.go.view.as_ref().map(|v| &v.state["fx"]);
        let mut bar = div()
            .p(px(12.))
            .rounded(px(12.))
            .bg(alpha(ui::INK, 0.03))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(18.))
                    .child(div().flex_1().min_w(px(240.)).child(crate::goxlr::level_meter(self.go.level_shown, self.go.level)))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(12.))
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
                                            .on_click(cx.listener(|this, _, _, cx| this.wizard_toggle_ab(true, cx))),
                                    )
                                    .child(
                                        ui::pill("wizard-ab-fx", None, "Son traité", if !bypass { Tone::Primary } else { Tone::Plain })
                                            .h(px(28.))
                                            .when(!enabled, |d| d.opacity(0.45))
                                            .on_click(cx.listener(|this, _, _, cx| this.wizard_toggle_ab(false, cx))),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child("M’écouter au casque"))
                                            .child(ui::caption(if monitor_on { "Retour actif" } else { "Coupé" })),
                                    )
                                    .child(
                                        ui::toggle("wizard-listen", monitor_on, enabled)
                                            .on_click(cx.listener(|this, _, _, cx| this.wizard_toggle_listen(cx))),
                                    ),
                            ),
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

    fn wizard_nav(&self, left: Vec<AnyElement>, right: Vec<AnyElement>) -> Div {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .child(div().flex().flex_wrap().items_center().gap(px(8.)).children(left))
            .child(div().flex().flex_wrap().items_center().gap(px(8.)).children(right))
    }

    /// Étape 1 : choix du micro, branchement et bons réflexes physiques.
    fn wizard_mic_view(&self, width: f32, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let current = v.state["mic"]["model"].as_str().unwrap_or("").to_owned();
        let kind = v.state["mic"]["type"].as_str().or(v.state["mic"]["kind"].as_str()).unwrap_or("dynamic").to_owned();
        let has_print = voice_print_valid(&v.state);
        let kind_label = |k: &str| match k {
            "condenser" => "Statique (48 V)",
            "jack" => "Jack 3,5 mm",
            _ => "Dynamique (sans 48 V)",
        };
        let mut tiles: Vec<AnyElement> = vec![];
        for (id, m) in crate::goxlr::entries(&v.meta["mic_models"]) {
            let brand = m["brand"].as_str().unwrap_or("");
            let name = m["name"].as_str().unwrap_or(id);
            let title = if brand.is_empty() { name.to_owned() } else { format!("{brand} {name}") };
            let key = id.clone();
            let tile = ui::tile(
                SharedString::from(format!("wizard-model-{id}")),
                None,
                title,
                Some(kind_label(m["type"].as_str().unwrap_or("dynamic")).into()),
                current == *id,
                None,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.wizard_send_seq(vec![("mic-model", json!({"model": key.clone()}))], true, cx);
            }));
            tiles.push(tile.into_any_element());
        }
        let mut kinds = ui::segmented();
        for (k, label) in [("dynamic", "Dynamique"), ("condenser", "Statique · 48 V"), ("jack", "Jack 3,5 mm")] {
            let k_str = k.to_owned();
            kinds = kinds.child(
                ui::seg(SharedString::from(format!("wizard-kind-{k}")), label, kind == k)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.wizard_send_seq(vec![("mic", json!({"type": k_str.clone(), "suggest_gain": true}))], true, cx);
                    })),
            );
        }
        let tip = |glyph: Icon, title: &'static str, text: &'static str| {
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(div().flex_none().mt(px(2.)).child(ui::icon(glyph, 16., rgb(ui::ACCENT))))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(hint(text)),
                )
        };
        let model_tips: Vec<String> = v.meta["mic_models"][&current]["tips"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect();

        let col_w = if ui::columns_that_fit(width, 420., 16., 2) == 2 { (width - 16.) * 0.5 } else { width };
        let pick_card = ui::section(Icon::Mic, ui::ACCENT, "1. Choisis ton micro", Some("Type et 48 V réglés automatiquement".into()))
            .child(ui::grid(tiles, ui::columns_that_fit(col_w, 195., 10., 3)))
            .child(ui::subhead("Type de prise"))
            .child(kinds)
            .child(ui::caption("Pas dans la liste ? Un micro qui marche sans courant 48 V est dynamique (SM7B, PodMic). Un micro qui demande le 48 V est statique (NT1-A, AT2020)."));

        let mut place_card = ui::section(Icon::Sparkles, ui::GREEN, "2. Le placement radio (50 % du son gratuit)", None)
            .child(tip(Icon::Mic, "Distance de 10 à 15 cm (3 à 4 doigts)", "Plus tu es proche, plus ta voix a des graves chauds naturels et moins on entend ta pièce."))
            .child(tip(Icon::Sparkles, "Oriente le micro légèrement de côté (30°)", "Parle juste à côté de la grille plutôt que droit dedans : ça supprime les coups d’air sur les « P » et les « B »."))
            .child(tip(Icon::Headphones, "Casque sur les oreilles et PC normal", "Coupe tes enceintes et laisse tes ventilateurs tourner comme d’habitude pour une mesure réaliste."));
        for mt in model_tips {
            place_card = place_card.child(ui::notice(true, mt));
        }

        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(lesson(
                Icon::Radio,
                ui::ACCENT,
                "Objectif : un son chaud, propre et percutant, réglé main dans la main",
                "On règle ton micro dans l’ordre d’un vrai studio : placement, gain, anti-bruit, égaliseur 6 bandes, puis compresseur, de-esser et limiteur. À tout moment tu peux écouter Avant / Après ou cliquer sur « Annuler et restaurer ».",
            ))
            .child(ui::settings_columns(width, vec![pick_card.into_any_element()], vec![place_card.into_any_element()]));

        let skip_label = if has_print { "Garder ma mesure et régler en direct" } else { "Passer la mesure et régler en direct" };
        let left = vec![
            ui::pill("wizard-skip-measure", Some(Icon::Sliders), skip_label, Tone::Plain)
                .when(current.is_empty(), |d| d.opacity(0.45))
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.go.view.as_ref().is_some_and(|v| v.state["mic"]["model"].is_string()) {
                        this.wizard_goto(Step::Gain, cx);
                    } else if let Some(w) = this.wizard_mut() {
                        w.error = Some("Choisis d’abord ton micro dans la liste ci-dessus.".into());
                        cx.notify();
                    }
                }))
                .into_any_element(),
        ];
        let start = ui::pill("wizard-start", Some(Icon::Play), "Étape 2 : mesurer ma voix (25 s)", Tone::Primary)
            .when(!self.go.enabled() || current.is_empty(), |d| d.opacity(0.45))
            .on_click(cx.listener(move |this, _, _, cx| {
                let chosen = this.go.view.as_ref().is_some_and(|v| v.state["mic"]["model"].is_string());
                if !chosen {
                    if let Some(w) = this.wizard_mut() {
                        w.error = Some("Choisis d’abord ton micro dans la liste ci-dessus.".into());
                    }
                    cx.notify();
                } else if this.go.enabled() {
                    this.wizard_voice(cx);
                }
            }))
            .into_any_element();
        (body, self.wizard_nav(left, vec![start]))
    }

    /// Étape 2 : mesure guidée 25 s (silence, 3 phrases normales, 1 phrase forte).
    fn wizard_measure_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> (Div, Div) {
        if !w.busy() {
            let enabled = self.go.enabled();
            let body = div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(lesson(
                    Icon::Mic,
                    ui::ORANGE,
                    "Mesure interrompue ou non lancée",
                    "Relance-la quand tu es prêt (6 s de silence, 3 phrases normales, 1 phrase forte), ou passe directement au réglage manuel du gain.",
                ));
            let nav = self.wizard_nav(
                vec![
                    ui::pill("wizard-back-mic", Some(Icon::ChevronLeft), "Retour au micro", Tone::Plain)
                        .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Mic, cx)))
                        .into_any_element(),
                    ui::pill("wizard-measure-skip", Some(Icon::Sliders), "Régler sans mesure", Tone::Plain)
                        .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Gain, cx)))
                        .into_any_element(),
                ],
                vec![
                    ui::pill("wizard-measure-retry", Some(Icon::Refresh), "Relancer la mesure", Tone::Primary)
                        .when(!enabled, |d| d.opacity(0.45))
                        .on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx)))
                        .into_any_element(),
                ],
            );
            return (body, nav);
        }
        let phase = w.job["phase"].as_str().unwrap_or("");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
        let left = w.job["phase_end"].as_f64().map(|e| (e - now).max(0.));
        let progress = w.job["progress"].as_f64().unwrap_or(0.) as f32;
        let retry = w.job["retry"].as_u64().unwrap_or(0) > 0;
        let (glyph, title, big, why, colour): (Icon, &str, String, &str, u32) = match phase {
            "silence" => (
                Icon::VolumeOff,
                "Phase 1/3 : Silence complet",
                "Ne dis plus rien".into(),
                "Respire calmement, sans toucher clavier ni souris. Rustudio mesure le bruit de ta pièce.",
                ui::ORANGE,
            ),
            "reading" => {
                let elapsed = left.map(|l| (3. * SENTENCE_S - l).max(0.)).unwrap_or(0.);
                let i = ((elapsed / SENTENCE_S) as usize).min(SENTENCES.len() - 1);
                (
                    Icon::Mic,
                    if retry { "Ça saturait : gain baissé, relis" } else { "Phase 2/3 : Lis à voix normale" },
                    SENTENCES[i].into(),
                    "Parle comme en vocal ou en stream, à 10 à 15 cm du micro. La phrase change automatiquement.",
                    ui::ACCENT,
                )
            }
            "loud" => (
                Icon::Bolt,
                "Phase 3/3 : Parle plus fort (comme sur une grosse action)",
                LOUD_SENTENCE.into(),
                "Rustudio vérifie que ta voix ne sature jamais, même sur un rire ou un cri.",
                ui::RED,
            ),
            "prepare" => (Icon::Headphones, "Prépare-toi", "Le silence commence dans un instant".into(), "Pose les mains loin du clavier et reste silencieux.", ui::ACCENT),
            _ => (Icon::Sparkles, "Calcul de ton son sur mesure", "Analyse en cours…".into(), "Rustudio ajuste ton gain, ton noise gate, ton égaliseur et ta compression.", ui::GREEN),
        };
        let mut b = ui::card()
            .items_center()
            .gap(px(16.))
            .py(px(28.))
            .child(div().w_full().max_w(px(520.)).child(ui::progress(progress, colour)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon(glyph, 18., rgb(colour)))
                    .child(div().text_size(px(14.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(colour)).child(title)),
            )
            .child(div().max_w(px(720.)).text_center().text_size(px(26.)).line_height(px(34.)).font_weight(FontWeight::BOLD).child(big))
            .child(div().max_w(px(580.)).text_center().child(hint(why)));
        if phase == "reading" {
            let elapsed = left.map(|l| (3. * SENTENCE_S - l).max(0.)).unwrap_or(0.);
            let i = ((elapsed / SENTENCE_S) as usize).min(SENTENCES.len() - 1);
            b = b.child(div().flex().gap(px(8.)).children((0..SENTENCES.len()).map(|n| {
                div().w(px(38.)).h(px(5.)).rounded_full().bg(if n <= i { rgb(ui::ACCENT) } else { alpha(ui::INK, 0.12) })
            })));
        }
        if let (Some(l), true) = (left, matches!(phase, "silence" | "reading" | "loud" | "prepare")) {
            b = b.child(div().text_size(px(46.)).font_weight(FontWeight::BOLD).text_color(rgb(colour)).child(format!("{} s", l.ceil() as i64)));
        }
        let nav = self.wizard_nav(
            vec![],
            vec![
                ui::pill("wizard-cancel-measure", Some(Icon::Close), "Annuler la mesure", Tone::Danger)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_cancel_measure(cx)))
                    .into_any_element(),
            ],
        );
        (b, nav)
    }

    /// Étape 3 : Bilan vocal vulgarisé + réglage complet du Gain d'entrée.
    fn wizard_gain_view(&self, w: &MicWizard, width: f32, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
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
            None => ("Non mesurée", "Lance l’étape 2 pour la mesurer"),
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
            .unwrap_or_else(|| if valid_info.is_some() { "Timbre mesuré".into() } else { "Passe par l’étape 2".into() });
        let sib = summary
            .and_then(|u| u["sibilance"].as_str())
            .map(crate::voice::capital)
            .unwrap_or_else(|| "Non mesurées".into());
        let snr = summary
            .and_then(|u| u["snr_db"].as_f64())
            .map(|d| format!("{} dB", d.round()))
            .unwrap_or_else(|| "Non mesuré".into());
        let tips: Vec<String> = valid_info
            .and_then(|i| i["notes"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|n| n["t"].as_str().map(String::from))
            .collect();

        let mut diag = ui::section(Icon::Activity, ui::ACCENT, "Bilan de ta voix et ta pièce", None).child(
            div()
                .flex()
                .flex_wrap()
                .gap_x(px(32.))
                .gap_y(px(12.))
                .child(ui::fact(room, room_sub))
                .child(ui::fact(voice_label, f0))
                .child(ui::fact(sib, "Tes « S » (sifflantes)"))
                .child(ui::fact(snr, "Voix au-dessus du bruit")),
        );
        for t in tips {
            diag = diag.child(ui::notice(true, t));
        }

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

        let mut gain_card = ui::section(Icon::Gauge, ui::GREEN, "Gain d’entrée (préampli GoXLR)", Some("Vise la zone verte (-18 à -6 dB)".into()))
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
            .child(ui::caption("Règle d’or : parle normalement en regardant la barre tout en haut. Elle doit danser dans le vert, jamais toucher le rouge quand tu ris."));
        if let Some(cg) = cal_gain
            && (gain.round() as i64 - cg).abs() >= 10
        {
            gain_card = gain_card.child(ui::notice(
                false,
                format!("Ton gain ({} dB) est très éloigné de ta mesure ({cg} dB) : si le son paraît faible ou trop fort, clique sur « Gain mesuré ({cg} dB) ».", gain.round()),
            ));
        }

        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(lesson(
                Icon::Gauge,
                ui::ACCENT,
                "Le Gain : la base propre avant tous les filtres",
                "Trop bas, le compresseur devra pousser le souffle derrière. Trop haut, ta voix sature dès que tu hausses le ton. Cale d’abord un niveau propre dans le vert : tout le reste sonnera 2 fois mieux.",
            ))
            .child(ui::settings_columns(width, vec![gain_card.into_any_element()], vec![diag.into_any_element()]))
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            vec![
                ui::pill("wizard-gain-back", Some(Icon::ChevronLeft), "Retour : Micro", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Mic, cx)))
                    .into_any_element(),
                ui::pill("wizard-gain-remeasure", Some(Icon::Refresh), "Refaire la mesure", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx)))
                    .into_any_element(),
            ],
            vec![
                ui::pill("wizard-gain-next", Some(Icon::ChevronRight), "Suivant : anti-bruit", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Noise, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 4 : Nettoyage anti-bruit complet (Noise Gate 4 paramètres + IA RNNoise sans forçage).
    fn wizard_noise_view(&self, w: &MicWizard, width: f32, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let s = &v.state;
        let enabled = self.go.enabled();
        let gate_on = s["fx"]["gate"] == true;
        let gate = &s["dsp"]["gate"];
        let threshold = gate["threshold"].as_f64().unwrap_or(-46.);
        let att = gate["attenuation"].as_f64().unwrap_or(80.);
        let attack = gate["attack"].as_f64().unwrap_or(0.);
        let release = gate["release"].as_f64().unwrap_or(18.);
        let is_condenser = s["mic"]["type"].as_str() == Some("condenser");

        let gate_presets = div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .child(
                ui::pill("wizard-gate-doux", None, "Doux (pièce calme)", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_gate_preset("doux", cx))),
            )
            .child(
                ui::pill("wizard-gate-equilibre", None, "Équilibré", Tone::Soft)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_gate_preset("equilibre", cx))),
            )
            .child(
                ui::pill("wizard-gate-clavier", None, "Anti-clavier fort", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_gate_preset("clavier", cx))),
            );

        let gate_card = ui::section(Icon::Shield, ui::GREEN, "Bouclier 1 : Noise Gate (porte entre tes phrases)", Some("Traité dans la GoXLR, 0 latence".into()))
            .child(ui::list_row(
                None,
                if gate_on { "Noise Gate actif" } else { "Noise Gate coupé" },
                Some("Ferme le micro dès que tu arrêtes de parler.".into()),
                ui::toggle("wizard-gate", gate_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"gate": !gate_on}), cx))),
                true,
            ))
            .when(gate_on, |d| {
                d.child(gate_presets)
                    .child(self.slider_row(
                        "wizard-gate-threshold",
                        "Seuil d’ouverture (baisse si le début de tes mots est mangé)",
                        threshold,
                        (-59., 0.),
                        1.,
                        |val| format!("{} dB", val.round()),
                        enabled,
                        |this, val, cx| this.go_send("dsp", json!({"gate": {"threshold": val.round() as i64}}), cx),
                        cx,
                    ))
                    .child(self.slider_row(
                        "wizard-gate-att",
                        "Force de coupure (70 à 85 % naturel, 100 % silence absolu)",
                        att,
                        (0., 100.),
                        5.,
                        |val| format!("{} %", val.round()),
                        enabled,
                        |this, val, cx| this.go_send("dsp", json!({"gate": {"attenuation": val.round() as i64}}), cx),
                        cx,
                    ))
                    .child(self.slider_row(
                        "wizard-gate-attack",
                        "Attaque (ouverture : 10 ms recommandé pour ne rien couper)",
                        attack,
                        (0., (GATE_MS.len() - 1) as f64),
                        1.,
                        fmt_gate_ms,
                        enabled,
                        |this, val, cx| this.go_send("dsp", json!({"gate": {"attack": val.round() as i64}}), cx),
                        cx,
                    ))
                    .child(self.slider_row(
                        "wizard-gate-release",
                        "Relâchement (fermeture après ta voix : 180 à 300 ms naturel)",
                        release,
                        (0., (GATE_MS.len() - 1) as f64),
                        1.,
                        fmt_gate_ms,
                        enabled,
                        |this, val, cx| this.go_send("dsp", json!({"gate": {"release": val.round() as i64}}), cx),
                        cx,
                    ))
            });

        let mut ns_col = div().flex().flex_col().gap(px(12.)).child(self.ns_panel(v, cx));
        if is_condenser {
            ns_col = ns_col.child(ui::notice(
                true,
                "Conseil micro statique (ex. NT1-A) : dans une pièce calme, laisser l’IA coupée préserve toute la finesse naturelle de ta voix. Active-la surtout si ton PC ou ton clavier s’entend pendant que tu parles.",
            ));
        }

        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(lesson(
                Icon::Shield,
                ui::GREEN,
                "Deux boucliers complémentaires contre le bruit",
                "Le Noise Gate coupe les bruits entre tes phrases sans toucher au timbre de ta voix. L’IA anti-bruit (RNNoise) retire ventilateurs et clavier même pendant que tu parles. Active « M’écouter au casque » en haut pour ajuster finement.",
            ))
            .child(ui::settings_columns(width, vec![gate_card.into_any_element()], vec![ns_col.into_any_element()]))
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            vec![
                ui::pill("wizard-noise-back", Some(Icon::ChevronLeft), "Retour : Gain", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Gain, cx)))
                    .into_any_element(),
            ],
            vec![
                ui::pill("wizard-noise-next", Some(Icon::ChevronRight), "Suivant : égaliseur 6 bandes", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Eq, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 5 : Console d'égaliseur 6 bandes (faders verticaux + fréquences réglables + courbe visuelle).
    fn wizard_eq_view(&self, w: &MicWizard, width: f32, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let s = &v.state;
        let enabled = self.go.enabled();
        let eq_on = s["fx"]["eq"] == true;
        let dsp = &s["dsp"];
        let g90 = dsp["eq"]["90"].as_f64().unwrap_or(0.);
        let f250 = dsp["eq_freq"]["250"].as_f64().unwrap_or(250.);
        let fitted_layout = f250 < 230. || g90 < 0.;

        let preset_key = s["mic_preset"].as_str().filter(|p| dsp::is_preset(p) && *p != "brut");
        let active_key = preset_key.or(w.chosen.as_deref()).unwrap_or("antenne");
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
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_eq_preset("antenne", cx))),
            )
            .child(
                ui::pill("wizard-eq-studio", Some(Icon::Podcast), "Studio propre", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_eq_preset("studio", cx))),
            )
            .child(
                ui::pill("wizard-eq-podcast", Some(Icon::Mic), "Chaleur podcast", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_eq_preset("podcast", cx))),
            )
            .child(
                ui::pill("wizard-eq-stream", Some(Icon::Sparkles), "Voix claire", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_eq_preset("stream", cx))),
            )
            .child(
                ui::pill("wizard-eq-flat", Some(Icon::History), "Plat (0 dB)", Tone::Plain)
                    .when(!enabled, |d| d.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_eq_preset("flat", cx))),
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

        let eq_console = ui::section(Icon::Sliders, ui::ACCENT, "Console égaliseur 6 bandes", Some("Glisse les curseurs · flèches pour la fréquence".into()))
            .child(ui::list_row(
                None,
                if eq_on { "Égaliseur actif" } else { "Égaliseur coupé" },
                Some("Chaque bande agit de -9 à +9 dB sur une zone précise de ta voix.".into()),
                ui::toggle("wizard-eq-toggle", eq_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"eq": !eq_on}), cx))),
                true,
            ))
            .child(quick_eq)
            .child(bands.when(!eq_on, |d| d.opacity(0.4)));

        let mut analysis_right: Vec<AnyElement> = vec![];
        if !eq_notes.is_empty() {
            let mut list = ui::section(Icon::Sparkles, ui::GREEN, "Pourquoi Rustudio a réglé ton EQ ainsi", None);
            for n in eq_notes {
                list = list.child(div().text_size(px(12.5)).line_height(px(18.)).text_color(rgb(ui::TEXT2)).child(format!("• {n}")));
            }
            analysis_right.push(list.into_any_element());
        } else {
            analysis_right.push(
                ui::section(Icon::Sparkles, ui::GREEN, "Repères rapides d’égaliseur", None)
                    .child(hint("• Coupe-bas (90 Hz) : baisse pour retirer chocs de bureau et grondements."))
                    .child(hint("• Chaleur (250 Hz) : monte légèrement (+2 à +3 dB) pour une voix plus proche."))
                    .child(hint("• Anti-carton (500 Hz) : baisse (-2 à -3 dB) pour éclaircir le timbre."))
                    .child(hint("• Présence (3 kHz) & Air (8 kHz) : dose la clarté ; reste léger sur un micro statique brillant."))
                    .into_any_element(),
            );
        }

        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(lesson(
                Icon::Sliders,
                ui::ACCENT,
                "L’Égaliseur : sculpter ton timbre sans dénaturer ta voix",
                "Coupe les grondements sous ta voix, ajoute juste ce qu’il faut de chaleur, nettoie le côté carton vers 400 à 600 Hz, et dose présence et brillance à l’oreille. Si un micro statique paraît trop brillant, baisse légèrement 3 kHz ou 8 kHz.",
            ))
            .child(eq_console)
            .child(ui::settings_columns(width, vec![self.voice_chart(active_key).into_any_element()], analysis_right))
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            vec![
                ui::pill("wizard-eq-back", Some(Icon::ChevronLeft), "Retour : Anti-bruit", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Noise, cx)))
                    .into_any_element(),
            ],
            vec![
                ui::pill("wizard-eq-next", Some(Icon::ChevronRight), "Suivant : compresseur et effet radio", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Radio, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 6 : Le trio Compresseur complet (5 contrôles) + De-esser + Limiteur.
    fn wizard_radio_view(&self, w: &MicWizard, width: f32, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let s = &v.state;
        let enabled = self.go.enabled();
        let comp_on = s["fx"]["comp"] == true;
        let deess_on = s["fx"]["deesser"] == true;
        let comp = &s["dsp"]["comp"];
        let thr = comp["threshold"].as_f64().unwrap_or(-22.);
        let ratio = comp["ratio"].as_f64().unwrap_or(9.);
        let attack = comp["attack"].as_f64().unwrap_or(2.);
        let release = comp["release"].as_f64().unwrap_or(10.);
        let makeup = comp["makeup"].as_f64().unwrap_or(6.);
        let deess = s["dsp"]["deesser"].as_f64().unwrap_or(30.);

        let mic_preset = s["mic_preset"].as_str().unwrap_or("custom");
        let dose_selected = selected_dose(comp_on, mic_preset, ratio, thr);

        let doses = ui::grid(
            vec![
                ui::tile(
                    "wizard-dose-doux",
                    Some((Icon::Leaf, ui::GREEN)),
                    "Doux (Naturel)",
                    Some("Lisse juste les petits écarts, dynamique très libre.".into()),
                    dose_selected == Some("doux"),
                    None,
                )
                .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_radio_dose("doux", cx)))
                .into_any_element(),
                ui::tile(
                    "wizard-dose-stream",
                    Some((Icon::Headphones, ui::ACCENT)),
                    "Présent (Stream)",
                    Some("Voix bien calée devant le jeu, sans excès.".into()),
                    dose_selected == Some("stream"),
                    None,
                )
                .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_radio_dose("stream", cx)))
                .into_any_element(),
                ui::tile(
                    "wizard-dose-skyrock",
                    Some((Icon::Radio, ui::ORANGE)),
                    "Radio FM (Skyrock)",
                    Some("Compression dense 5,6:1 + sifflantes tenues : gros son d’antenne.".into()),
                    dose_selected == Some("skyrock"),
                    Some(("Son Skyrock".into(), ui::ORANGE)),
                )
                .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_radio_dose("skyrock", cx)))
                .into_any_element(),
            ],
            ui::columns_that_fit(width - 40., 220., 10., 3),
        );

        let comp_card = ui::section(Icon::Gauge, ui::ACCENT, "Compresseur complet (densité et puissance)", None)
            .child(ui::list_row(
                None,
                if comp_on { "Compresseur actif" } else { "Compresseur coupé" },
                Some("Rapproche tes phrases douces et tes éclats de voix.".into()),
                ui::toggle("wizard-comp-toggle", comp_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"comp": !comp_on}), cx))),
                true,
            ))
            .when(comp_on, |d| {
                d.child(self.slider_row(
                    "wizard-comp-thr",
                    "Seuil (plus bas = voix plus compressée et plus collée devant)",
                    thr,
                    (-40., 0.),
                    1.,
                    |val| format!("{} dB", val.round()),
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"comp": {"threshold": val.round() as i64}}), cx),
                    cx,
                ))
                .child(self.slider_row(
                    "wizard-comp-ratio",
                    "Ratio / Force (3:1 naturel, 4:1 à 5,6:1 radio FM)",
                    ratio,
                    (0., 14.),
                    1.,
                    ratio_text,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"comp": {"ratio": val.round() as i64}}), cx),
                    cx,
                ))
                .child(self.slider_row(
                    "wizard-comp-makeup",
                    "Gain de compensation (remonte le volume après compression)",
                    makeup,
                    (-6., 24.),
                    1.,
                    eq_db_text,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"comp": {"makeup": val.round() as i64}}), cx),
                    cx,
                ))
                .child(self.slider_row(
                    "wizard-comp-attack",
                    "Attaque (rapide = son radio serré ; plus lent = plus naturel)",
                    attack,
                    (0., (COMP_ATTACK_MS.len() - 1) as f64),
                    1.,
                    fmt_comp_attack,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"comp": {"attack": val.round() as i64}}), cx),
                    cx,
                ))
                .child(self.slider_row(
                    "wizard-comp-release",
                    "Relâchement (100 à 230 ms fluide ; trop court = pompage)",
                    release,
                    (0., (COMP_RELEASE_MS.len() - 1) as f64),
                    1.,
                    fmt_comp_release,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"comp": {"release": val.round() as i64}}), cx),
                    cx,
                ))
            });

        let deess_card = ui::section(Icon::Sparkles, ui::ORANGE, "De-esser (douceur sur les « S » et « CH »)", None)
            .child(ui::list_row(
                None,
                if deess_on { "De-esser actif" } else { "De-esser coupé" },
                Some("Calme les sifflantes qui percent les oreilles au casque.".into()),
                ui::toggle("wizard-deess-toggle", deess_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"deesser": !deess_on}), cx))),
                true,
            ))
            .when(deess_on, |d| {
                d.child(self.slider_row(
                    "wizard-deess",
                    "Intensité (25 à 45 % conseillé, plus haut sur micro brillant)",
                    deess,
                    (0., 100.),
                    5.,
                    |val| format!("{} %", val.round()),
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"deesser": val.round() as i64}), cx),
                    cx,
                ))
            });

        let limiter_card = ui::section(Icon::Shield, ui::GREEN, "Limiteur anti-saturation", Some(if self.rustudio_firmware() { "Actif à -1 dB".into() } else { "Marge de sortie".into() }))
            .child(hint(if self.rustudio_firmware() {
                "Le firmware Rustudio embarque un limiteur matériel en bout de chaîne à -1 dB : même sur un cri, le son reste propre et sans distorsion."
            } else {
                "La compensation est bornée pour garder de la marge sur tes pics et éviter la saturation numérique."
            }));

        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(lesson(
                Icon::Radio,
                ui::ORANGE,
                "Le secret d’un son d’antenne : Compresseur + De-esser + Limiteur",
                "Choisis une dose rapide en un clic, puis ajuste finement l’attaque, le relâchement, la compensation et le de-esser en observant les indicateurs en direct tout en haut.",
            ))
            .child(doses)
            .child(ui::settings_columns(
                width,
                vec![comp_card.into_any_element()],
                vec![deess_card.into_any_element(), limiter_card.into_any_element()],
            ))
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            vec![
                ui::pill("wizard-radio-back", Some(Icon::ChevronLeft), "Retour : Égaliseur", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Eq, cx)))
                    .into_any_element(),
            ],
            vec![
                ui::pill("wizard-radio-next", Some(Icon::ChevronRight), "Suivant : comparer Avant / Après", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Compare, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 7 : Comparateur final Avant / Après (en direct + prise + anciens réglages) et bibliothèque de styles.
    fn wizard_compare_view(&self, w: &MicWizard, width: f32, cx: &mut Context<Self>) -> (Div, Div) {
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

        let live_ab = ui::section(Icon::Ear, ui::GREEN, "Test à l’oreille : compare ton son", None).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(
                    ui::pill("wizard-cmp-raw", Some(Icon::Mic), "Voix brute (sans filtre)", if bypass { Tone::Warn } else { Tone::Plain })
                        .on_click(cx.listener(|this, _, _, cx| this.wizard_toggle_ab(true, cx))),
                )
                .child(
                    ui::pill("wizard-cmp-now", Some(Icon::Radio), "Mon nouveau son", if !bypass { Tone::Primary } else { Tone::Plain })
                        .on_click(cx.listener(|this, _, _, cx| this.wizard_toggle_ab(false, cx))),
                )
                .when(has_snap, |d| {
                    d.child(
                        ui::pill("wizard-cmp-old", Some(Icon::History), "Recharger mes anciens réglages", Tone::Plain)
                            .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_snapshot(cx))),
                    )
                }),
        );

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(lesson(
                Icon::Ear,
                ui::GREEN,
                "Le juge final, c’est ton oreille",
                "Active « M’écouter au casque » tout en haut pour comparer instantanément ta voix brute, ton nouveau son et même tes anciens réglages. Tu peux aussi tester d’autres styles recalculés sur ta voix ci-dessous.",
            ))
            .child(live_ab)
            .children(self.wizard_replay_bar(w, cx))
            .child(
                ui::section(Icon::Sparkles, ui::ACCENT, "Tous les styles calculés sur ta voix", None)
                    .child(ui::grid(rows, ui::columns_that_fit(width - 40., 320., 10., 2))),
            );
        if !can_play {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(ui::caption("Aucune prise vocale enregistrée dans cette session : tu peux comparer en direct au casque ou enregistrer une prise."))
                    .child(
                        ui::pill("wizard-compare-remeasure", Some(Icon::Refresh), "Enregistrer une prise (25 s)", Tone::Plain)
                            .when(!self.go.enabled(), |d| d.opacity(0.45))
                            .on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx))),
                    ),
            );
        }

        let nav = self.wizard_nav(
            vec![
                ui::pill("wizard-compare-back", Some(Icon::ChevronLeft), "Retour : Radio FM", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Radio, cx)))
                    .into_any_element(),
            ],
            vec![
                ui::pill("wizard-to-done", Some(Icon::Check), "Voir le résumé final", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Done, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 8 : Récapitulatif final + conseils Discord / OBS + boutons Enregistrer et Annuler/Restaurer.
    fn wizard_done_view(&self, cx: &mut Context<Self>) -> (Div, Div) {
        let v = self.go.view.as_ref();
        let s = v.map(|v| &v.state);
        let preset = s.and_then(|s| s["mic_preset"].as_str()).and_then(dsp::preset).map(|p| p.label).unwrap_or("Sur mesure");
        let gain = s.and_then(|s| s["mic"]["gain_db"].as_f64()).unwrap_or(0.);
        let comp = s.map(|s| &s["dsp"]["comp"]);
        let comp_desc = if s.is_some_and(|s| s["fx"]["comp"] == true) {
            format!(
                "{} dès {} dB (+{} dB)",
                ratio_text(comp.and_then(|c| c["ratio"].as_f64()).unwrap_or(0.)),
                comp.and_then(|c| c["threshold"].as_f64()).unwrap_or(0.).round(),
                comp.and_then(|c| c["makeup"].as_f64()).unwrap_or(0.).round()
            )
        } else {
            "Coupé".into()
        };
        let gate_desc = if s.is_some_and(|s| s["fx"]["gate"] == true) {
            format!(
                "{} dB ({} %)",
                s.and_then(|s| s["dsp"]["gate"]["threshold"].as_f64()).unwrap_or(-46.).round(),
                s.and_then(|s| s["dsp"]["gate"]["attenuation"].as_f64()).unwrap_or(80.).round()
            )
        } else {
            "Coupé".into()
        };
        let ns_on = v.is_some_and(|v| v.mic_ns["ready"] == true && v.state["mic_ns"]["enabled"] == true);
        let goxlr_active = s.is_some_and(|s| {
            micchain::design(&dsp::effective_dsp(
                &dsp::normalize_dsp(&s["dsp"], None),
                &dsp::normalize_fx(&s["fx"]),
            ))
            .active()
        });
        let mut on: Vec<&str> = [("eq", "Égaliseur"), ("gate", "Noise gate"), ("comp", "Compresseur"), ("deesser", "De-esser")]
            .into_iter()
            .filter(|(k, _)| s.is_some_and(|s| s["fx"][*k] == true))
            .map(|(_, l)| l)
            .collect();
        if self.rustudio_firmware() && goxlr_active {
            on.push("Coupe-bas & Limiteur");
        }
        if ns_on {
            on.insert(0, "IA anti-bruit");
        }

        let summary_card = ui::section(Icon::Check, ui::GREEN, "Ton son micro est prêt !", Some("Le retour casque sera coupé automatiquement en terminant".into()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(36.))
                    .gap_y(px(12.))
                    .child(ui::fact(preset, "Style actif"))
                    .child(ui::fact(format!("{} dB", gain.round()), "Gain micro"))
                    .child(ui::fact(gate_desc, "Noise Gate"))
                    .child(ui::fact(comp_desc, "Compresseur"))
                    .child(ui::fact(if ns_on { "Active" } else { "Coupée" }, "IA anti-bruit")),
            )
            .child(hint(if on.is_empty() {
                "Ta voix part telle quelle, sans traitement.".to_owned()
            } else {
                format!("Chaîne active : {}.", on.join(", "))
            }));

        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(summary_card)
            .child(lesson(
                Icon::Chat,
                ui::ACCENT,
                "Dernier réflexe important dans Discord et OBS",
                "Dans Discord (Paramètres, Voix et vidéo), désactive Krisp, la suppression d’écho et le contrôle automatique du gain. Ta GoXLR traite déjà ta voix en studio : empiler les filtres de Discord par-dessus écraserait toute la chaleur.",
            ))
            .child(hint("Clique sur « Enregistrer et terminer » pour garder ces réglages et couper le retour micro, ou sur « Annuler et restaurer » pour revenir exactement comme avant."));

        let nav = self.wizard_nav(
            vec![
                ui::pill("wizard-done-back", Some(Icon::ChevronLeft), "Retour : Comparer", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Compare, cx)))
                    .into_any_element(),
                ui::pill("wizard-done-cancel", Some(Icon::History), "Annuler et restaurer mes anciens réglages", Tone::Danger)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_mic_wizard(cx)))
                    .into_any_element(),
            ],
            vec![
                ui::pill("wizard-finish", Some(Icon::Check), "Enregistrer et terminer", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.finish_mic_wizard(cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parcours_huit_etapes_complet() {
        let mut s = Step::Mic;
        let mut seen = vec![s];
        while let Some(next) = s.next() {
            assert_eq!(next.prev(), Some(s));
            seen.push(next);
            s = next;
        }
        assert_eq!(seen.len(), 8);
        assert_eq!(seen.first(), Some(&Step::Mic));
        assert_eq!(seen.last(), Some(&Step::Done));
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
        for (_, label) in STEPS {
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
            "mic_ns": {"enabled": false, "strength": 77, "grace_ms": 200},
            "volumes": {"MicMonitor": 0}
        });
        let snap = MicSnapshot::capture(&state);
        assert_eq!(snap.mic_type, "condenser");
        assert_eq!(snap.mic_model.as_deref(), Some("rode-nt1a"));
        assert_eq!(snap.gain_db, 56);
        assert_eq!(snap.mic_preset, "studio");
        assert_eq!(snap.mic_monitor, 0);
        assert_eq!(snap.mic_ns.unwrap()["enabled"], false);
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
    fn empreinte_valide_seulement_pour_le_micro_selectionne() {
        let st = json!({
            "mic": {"type": "condenser", "model": "rode-nt1a"},
            "voice_print": {"mic_type": "condenser", "model": "rode-nt1a"}
        });
        assert!(voice_print_valid(&st));
        let other_model = json!({
            "mic": {"type": "condenser", "model": "at2020"},
            "voice_print": {"mic_type": "condenser", "model": "rode-nt1a"}
        });
        assert!(!voice_print_valid(&other_model));
        let other_type = json!({
            "mic": {"type": "dynamic", "model": "shure-sm7b"},
            "voice_print": {"mic_type": "condenser", "model": "rode-nt1a"}
        });
        assert!(!voice_print_valid(&other_type));
        let no_model = json!({
            "mic": {"type": "condenser", "model": null},
            "voice_print": {"mic_type": "condenser", "model": "rode-nt1a"}
        });
        assert!(voice_print_valid(&no_model));
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
