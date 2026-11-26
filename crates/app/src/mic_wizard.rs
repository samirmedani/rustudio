//! Onboarding guidé « Régler mon micro » : parcours pas à pas et vulgarisé en 8 étapes pour obtenir
//! un vrai son radio (style Skyrock) réglé sur ta voix et ta pièce, main dans la main.
//!
//! 1. Micro & placement : choix du micro et bons réflexes (distance, angle, casque, ventilateurs).
//! 2. Mesure (25 s) : silence de la pièce, trois phrases normales, une phrase forte ; le moteur calibre
//!    le gain et calcule l'empreinte vocale pendant que l'appli enregistre ta prise brute.
//! 3. Bilan & Gain : lecture claire de ce que la mesure a trouvé (pièce, timbre, sifflantes, coups de
//!    souffle) et réglage du gain d'entrée sans jargon.
//! 4. Anti-bruit : les deux boucliers expliqués (IA RNNoise pendant que tu parles + Noise Gate entre
//!    tes phrases), réglables et écoutables sur ta prise.
//! 5. Sculpter ta voix (EQ) : chaleur radio, propreté anti-carton, présence devant la musique, brillance
//!    avec les explications calculées sur TA voix.
//! 6. Effet Radio FM (Skyrock) : compresseur, de-esser et limiteur expliqués simplement, avec 3 doses
//!    rapides (Doux, Stream, Radio FM) et curseurs accessibles.
//! 7. Comparer & Choisir : écoute directe Avant (voix brute) vs Après (ton son final) et sélection parmi
//!    tous les styles recalculés sur ta voix (Radio FM Skyrock mis en avant).
//! 8. C'est prêt : résumé complet de la chaîne active et astuce Discord / OBS pour ne pas doubler les filtres.
use crate::desktop::hint;
use crate::mixer::Mixer;
use crate::ui::{self, Icon, Tone, alpha, rgb};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, div, prelude::*, px};
use rustudio_core::{consts::RATIOS, dsp, micchain};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
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
const LISTEN_LEVEL: i64 = 191;

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
    (Step::Eq, "5. Timbre"),
    (Step::Radio, "6. Radio FM"),
    (Step::Compare, "7. Comparer"),
    (Step::Done, "8. Prêt"),
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

/// Prise de ta voix brute (la lecture du texte), et le gain du micro au moment de la prise.
struct Take {
    samples: Arc<Vec<f32>>,
    rate: u32,
    gain_db: i32,
}

pub struct MicWizard {
    pub step: Step,
    /// une mesure tourne ; l'annuler l'arrête dans la seconde
    cancel: Option<Arc<AtomicBool>>,
    /// tâche du moteur en cours, relue en direct : phase, consigne, avancement
    job: Value,
    error: Option<String>,
    chosen: Option<String>,
    take: Option<Take>,
    playing: Option<String>,
    listening: bool,
    /// numéro de la mesure en cours : une mesure annulée qui finit plus tard ne touche plus à rien
    run: u64,
    /// numéro de la boucle d'écoute en cours : un arrêt/relance rapide n'en laisse jamais deux tourner
    listen_run: u64,
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
            cancel: None,
            job: Value::Null,
            error: None,
            chosen: None,
            take: None,
            playing: None,
            listening: false,
            run: 0,
            listen_run: 0,
        }
    }

    fn busy(&self) -> bool {
        self.cancel.is_some()
    }
}

const CANCELLED: &str = "Mesure annulée.";

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

/// La lecture du texte dans la prise complète : de son début à sa fin, d'après les étapes du moteur.
fn reading_part(take: &crate::voice::Take, timeline: &Value) -> Option<Take> {
    let step = timeline.as_array()?.iter().find(|s| s["phase"] == "reading")?;
    let t0 = take.t0_ms as f64 / 1000.;
    let at = |t: f64| (((t - t0).max(0.) * take.rate as f64) as usize).min(take.samples.len());
    let (a, b) = (at(step["start"].as_f64()?), at(step["end"].as_f64()?));
    (b > a + take.rate as usize).then(|| Take {
        samples: Arc::new(take.samples[a..b].to_vec()),
        rate: take.rate,
        gain_db: step["gain_db"].as_i64().unwrap_or(0) as i32,
    })
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

/// Formatage lisible d'un ratio de compresseur (index GoXLR -> « 6:1 »).
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

impl Mixer {
    pub fn open_mic_wizard(&mut self, cx: &mut Context<Self>) {
        self.go_to(crate::desktop::Page::Go(crate::goxlr::Section::Microphone), cx);
        self.wizard = Some(MicWizard::new());
        if self.voice.info.is_none() {
            self.load_voice_info(cx);
        }
        cx.notify();
    }

    fn close_mic_wizard(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.wizard.as_mut() {
            if let Some(cancel) = w.cancel.as_ref() {
                cancel.store(true, Ordering::Relaxed);
            }
            w.listen_run += 1;
            if w.listening {
                self.send_quiet("mic-listen", json!({"seconds": 0}), cx);
            }
        }
        self.wizard = None;
        self.go_refresh(cx);
        cx.notify();
    }

    /// Commande au moteur sans attendre ni bloquer la page (écoute en direct, arrêt d'une mesure).
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
        w.error = error.filter(|e| e != CANCELLED);
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

    /// Étape 2 : la mesure guidée ; le moteur mène les étapes, l'appli enregistre la voix brute.
    fn wizard_voice(&mut self, cx: &mut Context<Self>) {
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
                if let Some(w) = this.wizard_mut() {
                    match result {
                        Ok((_, timeline, take)) => {
                            w.take = take.as_ref().and_then(|t| reading_part(t, &timeline));
                            w.step = Step::Gain;
                            Self::wizard_end(w, None);
                            ok = true;
                        }
                        Err(e) => Self::wizard_end(w, Some(e)),
                    }
                }
                // Mesure réussie : on applique directement le profil Radio FM calculé sur TA voix comme base
                // propre (gain, noise gate, EQ, compresseur, de-esser), puis chaque étape permet d'ajuster.
                if ok {
                    let ns_ready = this.go.view.as_ref().is_some_and(|v| v.mic_ns["ready"] == true);
                    if let Some(w) = this.wizard_mut() {
                        w.chosen = Some("antenne".into());
                    }
                    let client = this.client.clone();
                    let work = cx.background_executor().spawn(async move {
                        let _ = client.go_command("mic-preset", json!({"name": "antenne"}));
                        if ns_ready {
                            let _ = client.go_command("mic-ns", json!({"enabled": true}));
                        }
                        client.go_view().ok()
                    });
                    cx.spawn(async move |this, cx| {
                        let view = work.await;
                        let _ = this.update(cx, |this, cx| {
                            this.go.receive(view);
                            cx.notify();
                        });
                    })
                    .detach();
                } else {
                    this.go_refresh(cx);
                }
                this.voice.info = None;
                this.load_voice_info(cx);
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
            Some("actuel") => self.go.view.as_ref().map(|v| {
                micchain::design(&dsp::effective_dsp(
                    &dsp::normalize_dsp(&v.state["dsp"], None),
                    &dsp::normalize_fx(&v.state["fx"]),
                ))
            }),
            Some(p) => match profile_dsp(self.voice.info.as_ref(), p) {
                Some(d) => Some(micchain::design(&d)),
                None => Some(micchain::design(&dsp::preset_dsp(p))),
            },
            None => None,
        };
        if let Some(w) = self.wizard_mut() {
            w.playing = Some(preset.clone().unwrap_or_else(|| "avant".into()));
            w.error = None;
        }
        let work = cx.background_executor().spawn(async move {
            let raw: Vec<f32> = samples.iter().map(|x| (x * scale).clamp(-1., 1.)).collect();
            let out = match chain {
                Some(p) => micchain::render(&p, &raw),
                None => raw,
            };
            rustudio_windows_desktop::soundcheck::play_default(out, rate)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(w) = this.wizard_mut() {
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
        self.go_send("mic-preset", json!({"name": preset}), cx);
    }

    /// Applique uniquement une dose de dynamique (Compresseur + De-esser) sans écraser l'EQ ni le Noise Gate.
    fn wizard_apply_radio_dose(&mut self, dose: &'static str, cx: &mut Context<Self>) {
        let (thr, ratio, attack, release, makeup, deess) = match dose {
            "doux" => (-16, 4, 5, 11, 3, 15),       // 2:1 doux
            "stream" => (-20, 8, 2, 10, 5, 25),     // 3.2:1 live
            _ => (-26, 10, 2, 10, 9, 45),           // 6:1 Radio FM / Skyrock
        };
        self.go_send("fx", json!({"comp": true, "deesser": true}), cx);
        self.go_send(
            "dsp",
            json!({
                "comp": {"threshold": thr, "ratio": ratio, "attack": attack, "release": release, "makeup": makeup},
                "deesser": deess
            }),
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
        cx.notify();
    }

    /// L'écoute en direct est un bail de 20 s renouvelé toutes les 8 s tant qu'elle est allumée et l'assistant
    /// ouvert : appli fermée, plantée ou relancée, le retour s'arrête tout seul en quelques secondes.
    fn wizard_toggle_listen(&mut self, cx: &mut Context<Self>) {
        let Some(w) = self.wizard_mut() else { return };
        w.listening = !w.listening;
        let on = w.listening;
        w.listen_run += 1;
        let run = w.listen_run;
        if !on {
            self.send_quiet("mic-listen", json!({"seconds": 0}), cx);
            cx.notify();
            return;
        }
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            loop {
                let c = client.clone();
                cx.background_executor()
                    .spawn(async move {
                        let _ = c.go_command("mic-listen", json!({"seconds": 20, "level": LISTEN_LEVEL}));
                    })
                    .await;
                cx.background_executor().timer(Duration::from_secs(8)).await;
                let still = this
                    .update(cx, |this, _| this.wizard.as_ref().is_some_and(|w| w.listening && w.listen_run == run))
                    .unwrap_or(false);
                if !still {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Annuler : la mesure s'arrête tout de suite côté moteur, et l'écran rend la main sans attendre.
    fn wizard_cancel(&mut self, cx: &mut Context<Self>) {
        let Some(w) = self.wizard_mut() else { return };
        if let Some(cancel) = w.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        w.run += 1;
        Self::wizard_end(w, None);
        w.error = Some("Mesure annulée : relance-la quand tu es prêt.".into());
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
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child("Entends la différence sur ta propre voix"))
                        .child(ui::caption("Compare ton enregistrement brut avec tes réglages en cours.")),
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

    // ---------- affichage ----------

    pub fn mic_wizard_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let w = self.wizard.as_ref()?;
        let (body, actions) = match w.step {
            Step::Mic => self.wizard_mic_view(cx),
            Step::Measure => self.wizard_measure_view(w, cx),
            Step::Gain => self.wizard_gain_view(w, cx),
            Step::Noise => self.wizard_noise_view(w, cx),
            Step::Eq => self.wizard_eq_view(w, cx),
            Step::Radio => self.wizard_radio_view(w, cx),
            Step::Compare => self.wizard_compare_view(w, cx),
            Step::Done => self.wizard_done_view(cx),
        };
        let step_num = w.step.index() + 1;
        let close = ui::round("wizard-close", Icon::Close, 28., alpha(ui::INK, 0.06), rgb(ui::TEXT2))
            .on_click(cx.listener(|this, _, _, cx| this.close_mic_wizard(cx)));
        let head = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::icon(Icon::Radio, 22., rgb(ui::ACCENT)))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(19.)).font_weight(FontWeight::BOLD).child("Assistant micro : ton son radio pas à pas"))
                    .child(ui::caption(format!("Étape {step_num} sur {} : on règle tout ensemble, sans jargon.", STEPS.len()))),
            )
            .child(close);

        let can_jump = !w.busy() && self.go.view.as_ref().is_some_and(|v| v.state["mic"]["model"].is_string());
        let current = w.step.index();
        let mut stepper = div().flex().gap(px(5.));
        for (i, (st, label)) in STEPS.iter().enumerate() {
            let state = if i < current { 2 } else if i == current { 1 } else { 0 };
            let colour = if state == 2 { ui::GREEN } else { ui::ACCENT };
            let target = *st;
            let clickable = can_jump && target != Step::Measure && target != w.step;
            let item = div()
                .id(SharedString::from(format!("wizard-step-{i}")))
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(5.))
                .when(clickable, |d| d.cursor(gpui::CursorStyle::PointingHand).on_click(cx.listener(move |this, _, _, cx| this.wizard_goto(target, cx))))
                .child(div().h(px(4.)).rounded_full().bg(if state == 0 { alpha(ui::INK, 0.09) } else { alpha(colour, 1.) }))
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .font_weight(if state == 1 { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                        .text_color(rgb(if state == 0 { ui::TEXT3 } else if state == 1 { ui::TEXT } else { ui::TEXT2 }))
                        .child(*label),
                );
            stepper = stepper.child(item);
        }

        let scroll_body = div()
            .id("wizard-scroll-body")
            .max_h(px(480.))
            .overflow_y_scroll()
            .pr(px(4.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(body);

        let mut root = div()
            .p(px(24.))
            .rounded(px(18.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(head)
            .child(stepper)
            .child(scroll_body);
        if let Some(e) = &w.error {
            root = root.child(ui::notice(false, e.clone()));
        }
        let footer = div()
            .pt(px(14.))
            .border_t_1()
            .border_color(alpha(ui::INK, 0.08))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.wizard_live_bar(w, cx))
            .child(actions);
        Some(root.child(footer))
    }

    /// En bas de chaque étape : le niveau du micro en direct et l'écoute de ta voix dans le casque.
    fn wizard_live_bar(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(20.))
            .child(div().flex_1().min_w(px(0.)).child(crate::goxlr::level_meter(self.go.level_shown, self.go.level)))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child("M’écouter en direct"))
                            .child(ui::caption("Entends tes changements au casque")),
                    )
                    .child(ui::toggle("wizard-listen", w.listening, true).on_click(cx.listener(|this, _, _, cx| this.wizard_toggle_listen(cx)))),
            )
    }

    fn wizard_nav(&self, left: Option<AnyElement>, right: Vec<AnyElement>) -> Div {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .child(div().flex().items_center().gap(px(8.)).children(left))
            .child(div().flex().items_center().gap(px(8.)).children(right))
    }

    /// Étape 1 : choix du micro + bons réflexes physiques (les 50 % gratuits d'un gros son radio).
    fn wizard_mic_view(&self, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let current = v.state["mic"]["model"].as_str().unwrap_or("").to_owned();
        let has_print = v.state["voice_print"].is_object();
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
            .on_click(cx.listener(move |this, _, _, cx| this.go_send("mic-model", json!({"model": key}), cx)));
            tiles.push(tile.into_any_element());
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
        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(lesson(
                Icon::Radio,
                ui::ACCENT,
                "Objectif : un son chaud, propre et percutant comme à la radio",
                "On va régler ton micro dans l’ordre des ingénieurs son : placement, gain, anti-bruit, égaliseur, puis le secret des radios FM (compresseur, de-esser et limiteur). Chaque bouton est expliqué simplement.",
            ))
            .child(div().flex().flex_col().gap(px(6.)).child(ui::label("1. Choisis ton micro")).child(ui::grid(tiles, 3)))
            .child(ui::caption("Pas dans la liste ? Un micro qui marche sans courant 48 V est « dynamique » (Shure SM7B, PodMic, MV7…). Un micro qui demande le 48 V est « statique » (AT2020, NT1…). Choisis alors « Autre micro » du bon type."))
            .child(
                div()
                    .p(px(14.))
                    .rounded(px(12.))
                    .bg(alpha(ui::INK, 0.03))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(ui::label("2. Le placement radio (50 % du résultat gratuit)"))
                    .child(tip(Icon::Mic, "Distance de 3 à 4 doigts (10 cm)", "Plus tu es proche, plus ta voix a naturellement des graves chauds (l’effet radio) et moins on entend ta pièce."))
                    .child(tip(Icon::Sparkles, "Oriente le micro légèrement de côté (30°)", "Parle juste à côté de la grille plutôt que droit dedans : ça supprime les coups de souffle sur les « P » et les « B »."))
                    .child(tip(Icon::Headphones, "Casque sur les oreilles et PC allumé normalement", "Coupe tes enceintes et laisse tes ventilateurs tourner comme d’habitude pour que Rustudio apprenne à les effacer.")),
            );

        let left = has_print.then(|| {
            ui::pill("wizard-skip-measure", Some(Icon::Sliders), "Garder ma dernière mesure", Tone::Plain)
                .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Gain, cx)))
                .into_any_element()
        });
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
            let body = div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(lesson(
                    Icon::Mic,
                    ui::ORANGE,
                    "La mesure a été interrompue",
                    "Relance-la quand tu es prêt : 5 secondes de silence, puis 3 phrases à voix normale, puis 1 phrase forte.",
                ));
            let nav = self.wizard_nav(
                Some(ui::pill("wizard-back-mic", Some(Icon::ChevronLeft), "Retour", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Mic, cx))).into_any_element()),
                vec![ui::pill("wizard-measure-retry", Some(Icon::Refresh), "Relancer la mesure", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx))).into_any_element()],
            );
            return (body, nav);
        }
        let phase = w.job["phase"].as_str().unwrap_or("");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
        let left = w.job["phase_end"].as_f64().map(|e| (e - now).max(0.));
        let retry = w.job["retry"].as_u64().unwrap_or(0) > 0;
        let (glyph, title, big, why, colour): (Icon, &str, String, &str, u32) = match phase {
            "silence" => (
                Icon::VolumeOff,
                "Phase 1/3 : Silence complet",
                "Ne dis plus rien".into(),
                "Respire calmement, ne touche ni au clavier ni à la souris. Rustudio mesure le bruit de ta pièce (PC, ventilateurs) pour savoir exactement quoi couper.",
                ui::ORANGE,
            ),
            "reading" => {
                let elapsed = left.map(|l| (3. * SENTENCE_S - l).max(0.)).unwrap_or(0.);
                let i = ((elapsed / SENTENCE_S) as usize).min(SENTENCES.len() - 1);
                (
                    Icon::Mic,
                    if retry { "Ça saturait : gain baissé, relis" } else { "Phase 2/3 : Lis à voix normale" },
                    SENTENCES[i].into(),
                    "Parle comme en vocal ou en stream, à 10 cm du micro. La phrase change automatiquement.",
                    ui::ACCENT,
                )
            }
            "loud" => (
                Icon::Bolt,
                "Phase 3/3 : Parle plus fort (comme quand tu t’enflammes)",
                LOUD_SENTENCE.into(),
                "Rustudio vérifie que ta voix ne sature jamais, même sur un fou rire ou une action chaude en jeu.",
                ui::RED,
            ),
            "prepare" => (Icon::Headphones, "Prépare-toi", "Le silence commence dans un instant".into(), "Pose les mains loin du clavier et reste silencieux.", ui::ACCENT),
            _ => (Icon::Sparkles, "Calcul de ton son sur mesure", "Analyse en cours…".into(), "Rustudio calcule ton gain, ton noise gate, ton égaliseur et ta compression radio.", ui::GREEN),
        };
        let mut b = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .py(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon(glyph, 18., rgb(colour)))
                    .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(colour)).child(title)),
            )
            .child(div().text_center().text_size(px(24.)).line_height(px(32.)).font_weight(FontWeight::BOLD).child(big))
            .child(div().max_w(px(560.)).text_center().child(hint(why)));
        if phase == "reading" {
            let elapsed = left.map(|l| (3. * SENTENCE_S - l).max(0.)).unwrap_or(0.);
            let i = ((elapsed / SENTENCE_S) as usize).min(SENTENCES.len() - 1);
            b = b.child(div().flex().gap(px(6.)).children((0..SENTENCES.len()).map(|n| {
                div().w(px(32.)).h(px(4.)).rounded_full().bg(if n <= i { rgb(ui::ACCENT) } else { alpha(ui::INK, 0.12) })
            })));
        }
        if let (Some(l), true) = (left, matches!(phase, "silence" | "reading" | "loud" | "prepare")) {
            b = b.child(div().text_size(px(42.)).font_weight(FontWeight::BOLD).text_color(rgb(colour)).child(format!("{}", l.ceil() as i64)));
        }
        let nav = self.wizard_nav(
            None,
            vec![ui::pill("wizard-cancel", Some(Icon::Close), "Annuler la mesure", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_cancel(cx))).into_any_element()],
        );
        (b, nav)
    }

    /// Étape 3 : Bilan vocal vulgarisé + réglage du Gain d'entrée.
    fn wizard_gain_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let s = &v.state;
        let enabled = self.go.enabled();
        let gain = s["mic"]["gain_db"].as_f64().unwrap_or(0.);
        let cal = &s["mic_cal"];
        let noise = cal["noise_peak_db"].as_f64();
        let (room, room_sub) = match noise {
            Some(n) if n > -42. => ("Bruyante", "PC ou ventilateurs très audibles"),
            Some(n) if n > -54. => ("Souffle léger", "Un léger fond sonore s’entend"),
            Some(_) => ("Calme", "Très peu de bruit entre tes phrases"),
            None => ("Non mesurée", "Lance l’étape 2 pour la mesurer"),
        };
        let summary = self.voice.info.as_ref().map(|i| &i["summary"]);
        let voice_label = summary.and_then(|u| u["voice_type"].as_str()).unwrap_or("Analysée").to_owned();
        let f0 = summary.and_then(|u| u["f0_hz"].as_f64()).map(|f| format!("Fondamentale à {} Hz", f.round())).unwrap_or_else(|| "Timbre mesuré".into());
        let sib = summary.and_then(|u| u["sibilance"].as_str()).unwrap_or("Normales").to_owned();
        let plos = summary.and_then(|u| u["plosives"].as_u64()).unwrap_or(0);
        let tips: Vec<String> = self
            .voice
            .info
            .as_ref()
            .and_then(|i| i["notes"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|n| n["t"].as_str().map(String::from))
            .collect();

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(lesson(
                Icon::Gauge,
                ui::ACCENT,
                "Le Gain : c’est le volume de départ de ton micro",
                "Trop bas, il faut pousser le son derrière et ça ramène du souffle. Trop haut, ta voix sature et craque dès que tu ris ou cries. Règle d’or : quand tu parles normalement, la barre en bas doit bouger dans le vert, jamais dans le rouge.",
            ))
            .child(
                div()
                    .p(px(14.))
                    .rounded(px(12.))
                    .bg(alpha(ui::INK, 0.03))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(ui::label("Ce que Rustudio a entendu sur ta prise"))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_x(px(32.))
                            .gap_y(px(10.))
                            .child(ui::fact(room, format!("Ta pièce : {room_sub}")))
                            .child(ui::fact(voice_label, f0))
                            .child(ui::fact(sib, "Tes « S » (sifflantes)"))
                            .child(ui::fact(if plos == 0 { "Aucun".into() } else { format!("{plos} détecté(s)") }, "Souffles « P / B »")),
                    ),
            );
        for t in tips {
            body = body.child(ui::notice(true, t));
        }
        body = body
            .child(ui::subhead("Sensibilité du micro (préampli GoXLR)"))
            .child(self.slider_row(
                "wizard-gain",
                "Gain d’entrée : déjà calé par la mesure, ajuste-le si la barre touche le rouge",
                gain,
                (0., 72.),
                1.,
                |val| format!("{} dB", val.round()),
                enabled,
                |this, val, cx| this.go_send("mic", json!({"gain_db": val.round() as i64}), cx),
                cx,
            ))
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            Some(
                ui::pill("wizard-gain-remeasure", Some(Icon::Refresh), "Refaire la mesure", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx)))
                    .into_any_element(),
            ),
            vec![
                ui::pill("wizard-gain-next", Some(Icon::ChevronRight), "Suivant : nettoyer les bruits", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Noise, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 4 : Nettoyage anti-bruit (IA RNNoise + Noise Gate expliqués simplement).
    fn wizard_noise_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let s = &v.state;
        let enabled = self.go.enabled();
        let gate_on = s["fx"]["gate"] == true;
        let threshold = s["dsp"]["gate"]["threshold"].as_f64().unwrap_or(-45.);
        let att = s["dsp"]["gate"]["attenuation"].as_f64().unwrap_or(80.);

        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(lesson(
                Icon::Shield,
                ui::GREEN,
                "Deux boucliers contre le bruit, chacun a son rôle",
                "1) L’IA anti-bruit efface ventilateurs et souffle même pendant que tu parles. 2) Le Noise Gate est une porte automatique : il coupe clavier, souris et bruits de la pièce dès que tu arrêtes de parler. Teste avec « M’écouter en direct » en bas : tais-toi, silence complet ; parle, aucun mot ne doit être mangé.",
            ))
            .child(ui::subhead("Bouclier 1 : Suppression du bruit par IA (RNNoise)"))
            .child(self.ns_panel(v, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(ui::subhead("Bouclier 2 : Noise Gate (porte entre tes phrases)"))
                            .child(ui::caption("Ferme le micro quand tu ne parles pas, sans jamais couper ta voix.")),
                    )
                    .child(ui::toggle("wizard-gate", gate_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"gate": !gate_on}), cx)))),
            )
            .when(gate_on, |d| {
                d.child(self.slider_row(
                    "wizard-gate-threshold",
                    "Seuil d’ouverture (plus haut = coupe plus fort, baisse si le début de tes mots saute)",
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
                    "Force de coupure (80 % naturel, 100 % silence absolu contre un clavier bruyant)",
                    att,
                    (0., 100.),
                    5.,
                    |val| format!("{} %", val.round()),
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"gate": {"attenuation": val.round() as i64}}), cx),
                    cx,
                ))
            })
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            Some(
                ui::pill("wizard-noise-back", Some(Icon::ChevronLeft), "Retour", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Gain, cx)))
                    .into_any_element(),
            ),
            vec![
                ui::pill("wizard-noise-next", Some(Icon::ChevronRight), "Suivant : sculpter ta voix", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Eq, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 5 : Sculpter ta voix (Égaliseur vulgarisé en 4 curseurs humains).
    fn wizard_eq_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let s = &v.state;
        let enabled = self.go.enabled();
        let eq_on = s["fx"]["eq"] == true;
        let eq = &s["dsp"]["eq"];
        let warmth = eq["90"].as_f64().unwrap_or(0.);
        let boxiness = eq["250"].as_f64().unwrap_or(0.);
        let presence = eq["3000"].as_f64().unwrap_or(0.);
        let air = eq["8000"].as_f64().unwrap_or(0.);

        // Notes expliquant ce que Rustudio a calculé sur la voix pour le profil Radio FM.
        let active_key = w.chosen.as_deref().unwrap_or("antenne");
        let eq_notes: Vec<String> = self
            .voice
            .info
            .as_ref()
            .and_then(|i| i["profiles"][active_key]["notes"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|n| n["t"].as_str().map(String::from))
            .take(3)
            .collect();

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(lesson(
                Icon::Sliders,
                ui::ACCENT,
                "L’Égaliseur : la recette du timbre radio en 4 ingrédients",
                "L’égaliseur dose les graves et les aigus de ta voix. Pour un son radio FM : un peu de chaleur en bas, on creuse le côté « boîte en carton » au milieu, puis on ajoute présence et brillance pour passer proprement devant la musique.",
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(ui::subhead("Égaliseur sur mesure"))
                            .child(ui::caption("Déjà calculé sur ton empreinte vocale : ajuste librement à l’oreille.")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                ui::pill("wizard-eq-skyrock", Some(Icon::Sparkles), "Recalculer Radio FM", Tone::Plain)
                                    .on_click(cx.listener(|this, _, _, cx| this.wizard_choose("antenne".into(), cx))),
                            )
                            .child(ui::toggle("wizard-eq-toggle", eq_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"eq": !eq_on}), cx)))),
                    ),
            );
        if !eq_notes.is_empty() {
            let mut list = div().p(px(12.)).rounded(px(10.)).bg(alpha(ui::INK, 0.03)).flex().flex_col().gap(px(4.)).child(ui::caption("Ce que Rustudio a ajusté sur ta voix :"));
            for n in eq_notes {
                list = list.child(div().text_size(px(12.)).text_color(rgb(ui::TEXT2)).child(format!("• {n}")));
            }
            body = body.child(list);
        }
        body = body
            .when(eq_on, |d| {
                d.child(self.slider_row(
                    "wizard-eq-warmth",
                    "Chaleur & coffre radio (90 Hz : voix grave et proche façon antenne)",
                    warmth,
                    (-9., 9.),
                    1.,
                    eq_db_text,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"eq": {"90": val.round() as i64}}), cx),
                    cx,
                ))
                .child(self.slider_row(
                    "wizard-eq-mud",
                    "Propreté anti-carton (250 à 500 Hz : baisser un peu éclaircit tout de suite)",
                    boxiness,
                    (-9., 9.),
                    1.,
                    eq_db_text,
                    enabled,
                    |this, val, cx| {
                        let v500 = (val * 0.65).round() as i64;
                        this.go_send("dsp", json!({"eq": {"250": val.round() as i64, "500": v500}}), cx)
                    },
                    cx,
                ))
                .child(self.slider_row(
                    "wizard-eq-presence",
                    "Présence & clarté des mots (3 kHz : fait sortir ta voix devant la musique)",
                    presence,
                    (-9., 9.),
                    1.,
                    eq_db_text,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"eq": {"3000": val.round() as i64}}), cx),
                    cx,
                ))
                .child(self.slider_row(
                    "wizard-eq-air",
                    "Brillance & air studio (8 kHz : côté net et haute définition)",
                    air,
                    (-9., 9.),
                    1.,
                    eq_db_text,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"eq": {"8000": val.round() as i64}}), cx),
                    cx,
                ))
            })
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            Some(
                ui::pill("wizard-eq-back", Some(Icon::ChevronLeft), "Retour", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Noise, cx)))
                    .into_any_element(),
            ),
            vec![
                ui::pill("wizard-eq-next", Some(Icon::ChevronRight), "Suivant : l’effet Skyrock", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Radio, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 6 : Le secret du son radio Skyrock (Compresseur + De-esser + Limiteur).
    fn wizard_radio_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> (Div, Div) {
        let Some(v) = self.go.view.as_ref() else { return (div(), div()) };
        let s = &v.state;
        let enabled = self.go.enabled();
        let comp_on = s["fx"]["comp"] == true;
        let deess_on = s["fx"]["deesser"] == true;
        let comp = &s["dsp"]["comp"];
        let thr = comp["threshold"].as_f64().unwrap_or(-22.);
        let ratio = comp["ratio"].as_f64().unwrap_or(9.);
        let makeup = comp["makeup"].as_f64().unwrap_or(6.);
        let deess = s["dsp"]["deesser"].as_f64().unwrap_or(30.);

        let dose_selected = if ratio >= 9.5 && thr <= -23. {
            "skyrock"
        } else if ratio >= 7. {
            "stream"
        } else {
            "doux"
        };

        let doses = ui::grid(
            vec![
                ui::tile(
                    "wizard-dose-doux",
                    Some((Icon::Leaf, ui::GREEN)),
                    "Doux (Naturel)",
                    Some("Lisse juste les petits écarts, dynamique très libre.".into()),
                    dose_selected == "doux",
                    None,
                )
                .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_radio_dose("doux", cx)))
                .into_any_element(),
                ui::tile(
                    "wizard-dose-stream",
                    Some((Icon::Headphones, ui::ACCENT)),
                    "Présent (Stream)",
                    Some("Voix bien calée devant le jeu, sans excès.".into()),
                    dose_selected == "stream",
                    None,
                )
                .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_radio_dose("stream", cx)))
                .into_any_element(),
                ui::tile(
                    "wizard-dose-skyrock",
                    Some((Icon::Radio, ui::ORANGE)),
                    "Radio FM (Skyrock)",
                    Some("Compression dense 5,6:1 + brillance maîtrisée : gros son d’antenne.".into()),
                    dose_selected == "skyrock",
                    Some(("Son Skyrock".into(), ui::ORANGE)),
                )
                .on_click(cx.listener(|this, _, _, cx| this.wizard_apply_radio_dose("skyrock", cx)))
                .into_any_element(),
            ],
            3,
        );

        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(lesson(
                Icon::Radio,
                ui::ORANGE,
                "Le vrai secret de Skyrock : le trio Compresseur + De-esser + Limiteur",
                "Le compresseur rapproche tes chuchotements et calme tes cris pour garder ta voix dense et collée devant. Le de-esser adoucit les « S » agressifs quand on pousse les aigus. Et un limiteur à -1 dB empêche automatiquement toute saturation.",
            ))
            .child(div().flex().flex_col().gap(px(6.)).child(ui::label("Choisis l’intensité radio en un clic")).child(doses))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().child(ui::subhead("Compresseur (densité et puissance)")))
                    .child(ui::toggle("wizard-comp-toggle", comp_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"comp": !comp_on}), cx)))),
            )
            .when(comp_on, |d| {
                d.child(self.slider_row(
                    "wizard-comp-thr",
                    "Seuil (plus bas = voix plus compressée et plus « collée » devant)",
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
                    "Force de compression (4:1 à 6:1 recommandé pour l’effet radio FM)",
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
                    "Puissance de sortie (remonte le volume global après compression)",
                    makeup,
                    (-6., 18.),
                    1.,
                    eq_db_text,
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"comp": {"makeup": val.round() as i64}}), cx),
                    cx,
                ))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().child(ui::subhead("De-esser (douceur sur les « S » et « CH »)")))
                    .child(ui::toggle("wizard-deess-toggle", deess_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"deesser": !deess_on}), cx)))),
            )
            .when(deess_on, |d| {
                d.child(self.slider_row(
                    "wizard-deess",
                    "Adoucissement des sifflantes (30 à 50 % idéal avec la brillance radio)",
                    deess,
                    (0., 100.),
                    5.,
                    |val| format!("{} %", val.round()),
                    enabled,
                    |this, val, cx| this.go_send("dsp", json!({"deesser": val.round() as i64}), cx),
                    cx,
                ))
            })
            .children(self.wizard_replay_bar(w, cx));

        let nav = self.wizard_nav(
            Some(
                ui::pill("wizard-radio-back", Some(Icon::ChevronLeft), "Retour", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Eq, cx)))
                    .into_any_element(),
            ),
            vec![
                ui::pill("wizard-radio-next", Some(Icon::ChevronRight), "Suivant : comparer Avant / Après", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Compare, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 7 : Comparateur final Avant / Après & bibliothèque de styles recalculés sur ta voix.
    fn wizard_compare_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> (Div, Div) {
        let state = self.go.view.as_ref().map(|v| &v.state);
        let current = state.and_then(|s| s["mic_preset"].as_str()).unwrap_or("custom").to_owned();
        let can_play = w.take.is_some();
        let playing = w.playing.as_deref();

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
                current == p.key,
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
            .gap(px(14.))
            .child(lesson(
                Icon::Ear,
                ui::GREEN,
                "Le test final à l’oreille : Avant vs Après",
                "Compare ta voix brute (sans aucun filtre) avec ton son réglé. Tu peux garder les réglages sur mesure que tu viens de sculpter, ou cliquer sur un style complet recalculé sur ta voix (▶ pour l’écouter avant de choisir).",
            ));
        if can_play {
            body = body.child(
                div()
                    .p(px(14.))
                    .rounded(px(12.))
                    .bg(alpha(ui::ACCENT, 0.06))
                    .border_1()
                    .border_color(alpha(ui::ACCENT, 0.2))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child("Comparaison directe sur ta prise"))
                            .child(ui::caption(if current == "custom" { "Ton réglage personnalisé des étapes 3 à 6 est actif." } else { "Le style sélectionné ci-dessous est actif sur ta GoXLR." })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                ui::pill(
                                    "wizard-play-before",
                                    Some(if playing == Some("avant") { Icon::Wave } else { Icon::Play }),
                                    if playing == Some("avant") { "Lecture…" } else { "Avant (voix brute)" },
                                    Tone::Plain,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.wizard_play(None, cx))),
                            )
                            .child(
                                ui::pill(
                                    "wizard-play-after",
                                    Some(if playing == Some("actuel") { Icon::Wave } else { Icon::Play }),
                                    if playing == Some("actuel") { "Lecture…" } else { "Après (ton son)" },
                                    Tone::Primary,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.wizard_play(Some("actuel".into()), cx))),
                            ),
                    ),
            );
        }
        body = body.child(ui::subhead("Styles calculés sur ta voix")).child(ui::grid(rows, 2));

        let nav = self.wizard_nav(
            Some(
                ui::pill("wizard-compare-back", Some(Icon::ChevronLeft), "Retour", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Radio, cx)))
                    .into_any_element(),
            ),
            vec![
                ui::pill("wizard-to-done", Some(Icon::Check), "Valider ce son", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Done, cx)))
                    .into_any_element(),
            ],
        );
        (body, nav)
    }

    /// Étape 8 : Récapitulatif final + conseils Discord / OBS pour garder le son radio intact.
    fn wizard_done_view(&self, cx: &mut Context<Self>) -> (Div, Div) {
        let v = self.go.view.as_ref();
        let s = v.map(|v| &v.state);
        let preset = s.and_then(|s| s["mic_preset"].as_str()).and_then(dsp::preset).map(|p| p.label).unwrap_or("Sur mesure");
        let gain = s.and_then(|s| s["mic"]["gain_db"].as_f64()).unwrap_or(0.);
        let ns_on = v.is_some_and(|v| v.mic_ns["ready"] == true && v.state["mic_ns"]["enabled"] == true);
        let mut on: Vec<&str> = [("eq", "Égaliseur"), ("gate", "Noise gate"), ("comp", "Compresseur"), ("deesser", "De-esser")]
            .into_iter()
            .filter(|(k, _)| s.is_some_and(|s| s["fx"][*k] == true))
            .map(|(_, l)| l)
            .collect();
        if !on.is_empty() {
            on.push("Coupe-bas & Limiteur");
        }
        if ns_on {
            on.insert(0, "IA anti-bruit");
        }

        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(ui::icon(Icon::Check, 24., rgb(ui::GREEN)))
                    .child(ui::title("Ton son radio est prêt !", 20.)),
            )
            .child(
                div()
                    .p(px(14.))
                    .rounded(px(12.))
                    .bg(alpha(ui::INK, 0.03))
                    .flex()
                    .flex_wrap()
                    .gap_x(px(36.))
                    .gap_y(px(10.))
                    .child(ui::fact(preset, "Style actif"))
                    .child(ui::fact(format!("{} dB", gain.round()), "Gain micro"))
                    .child(ui::fact(if ns_on { "Active" } else { "Coupée" }, "IA anti-bruit"))
                    .child(ui::fact(format!("{} blocs", on.len()), "Chaîne active")),
            )
            .child(hint(if on.is_empty() {
                "Ta voix part telle quelle, sans traitement.".to_owned()
            } else {
                format!("Chaîne active dans ta GoXLR : {}.", on.join(", "))
            }))
            .child(lesson(
                Icon::Chat,
                ui::ACCENT,
                "Dernier réflexe important dans Discord et OBS",
                "Dans Discord (Paramètres, Voix et vidéo), désactive Krisp, la suppression d’écho et le contrôle automatique du gain. Ta GoXLR traite déjà ta voix en studio : empiler les filtres de Discord par-dessus écraserait toute la chaleur radio.",
            ))
            .child(hint("Tout reste modifiable à tout moment dans la page Micro. Relance cet assistant si tu changes de micro ou de pièce."));

        let nav = self.wizard_nav(
            Some(
                ui::pill("wizard-done-back", Some(Icon::ChevronLeft), "Revenir au comparateur", Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Compare, cx)))
                    .into_any_element(),
            ),
            vec![
                ui::pill("wizard-finish", Some(Icon::Check), "Terminer", Tone::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.close_mic_wizard(cx)))
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
    fn formatages_lisibles_sans_tiret_cadratin() {
        assert_eq!(ratio_text(10.), "5,6:1");
        assert_eq!(ratio_text(9.), "4:1");
        assert_eq!(eq_db_text(4.), "+4 dB");
        assert_eq!(eq_db_text(-3.), "-3 dB");
        assert_eq!(eq_db_text(0.), "0 dB");
        for (_, label) in STEPS {
            assert!(!label.contains('\u{2014}') && !label.contains('\u{2013}'));
        }
    }
}
