//! Assistant « Régler mon micro » : le seul chemin pour régler le micro, de A à Z.
//!
//! 1. Niveau : le moteur trouve le micro, écoute ta voix puis ta pièce, et cale un premier gain (`voice-setup`).
//! 2. Ta voix : un silence, un texte à lire, une phrase plus forte. L'appli enregistre la voix brute ; le moteur
//!    l'analyse et cale le gain, le noise gate et l'égaliseur sur ta voix mesurée.
//! 3. Vérifier : le niveau en direct, le gain à portée de main, et ta prise à réécouter telle qu'elle arrive.
//! 4. Ton son : chaque préréglage, recalculé sur ta voix, s'écoute sur ta propre prise avant d'être choisi.
//!    Le calcul est celui du firmware Rustudio, au bit près (`micchain::render`).
//! 5. Terminé : ce qui est réglé.
//!
//! En bas, à chaque étape : le niveau du micro en direct et « M'écouter en direct » (retour dans le casque).
//! Aucun son n'est joué sans un clic.
use crate::desktop::hint;
use crate::mixer::Mixer;
use crate::ui::{self, Icon, Tone, alpha, rgb};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, div, prelude::*, px};
use rustudio_core::{dsp, micchain};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

/// Trois phrases de live, une toutes les 5 secondes pendant la lecture (durées envoyées au moteur par voice::take).
const SENTENCES: [&str; 3] = [
    "« Salut tout le monde, bienvenue sur le live ! »",
    "« Aujourd’hui, on règle mon micro pour qu’il sonne parfaitement. »",
    "« Si vous m’entendez bien, dites-le-moi dans le chat. »",
];
const SENTENCE_S: f64 = 5.0;
const LOUD_SENTENCE: &str = "« Oh non, c’est pas possible, on l’avait ! »";
/// Volume du retour de ta voix dans le casque pendant l'assistant (sur 255).
const LISTEN_LEVEL: i64 = 191;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Mic,
    Measure,
    Result,
    Sound,
    Done,
}

const STEPS: [(Step, &str); 5] = [(Step::Mic, "Ton micro"), (Step::Measure, "Mesure"), (Step::Result, "Réglage"), (Step::Sound, "Ton son"), (Step::Done, "Terminé")];

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
        MicWizard { step: Step::Mic, cancel: None, job: Value::Null, error: None, chosen: None, take: None, playing: None, listening: false, run: 0, listen_run: 0 }
    }

    fn busy(&self) -> bool {
        self.cancel.is_some()
    }
}

const CANCELLED: &str = "Mesure annulée.";

/// Réglages d'un préréglage tels qu'ils partiraient vers la GoXLR (traitements coupés neutres).
fn profile_dsp(info: Option<&Value>, preset: &str) -> Option<dsp::Dsp> {
    let p = &info?["profiles"][preset];
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
    (b > a + take.rate as usize).then(|| Take { samples: Arc::new(take.samples[a..b].to_vec()), rate: take.rate, gain_db: step["gain_db"].as_i64().unwrap_or(0) as i32 })
}

impl Mixer {
    pub fn open_mic_wizard(&mut self, cx: &mut Context<Self>) {
        self.go_to(crate::desktop::Page::Go(crate::goxlr::Section::Microphone), cx);
        self.wizard = Some(MicWizard::new());
        cx.notify();
    }

    fn close_mic_wizard(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.wizard.as_mut() {
            if let Some(cancel) = w.cancel.as_ref() {
                cancel.store(true, Ordering::Relaxed);
            }
            w.listen_run += 1; // la boucle d'écoute en cours s'arrête à sa prochaine vérification
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
        // avancement : la tâche du moteur, relue à côté de l'enregistrement
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
                // mesure annulée (ou remplacée) entre-temps : son résultat n'a plus lieu d'être
                if this.wizard.as_ref().is_none_or(|w| w.run != run) {
                    return;
                }
                if let Some(w) = this.wizard_mut() {
                    match result {
                        Ok((_, timeline, take)) => {
                            w.take = take.as_ref().and_then(|t| reading_part(t, &timeline));
                            w.step = Step::Result;
                            Self::wizard_end(w, None);
                            ok = true;
                        }
                        Err(e) => Self::wizard_end(w, Some(e)),
                    }
                }
                // mesure réussie : le son conseillé pour ce micro, calculé sur ta voix, est appliqué tout de suite
                // (noise gate au-dessus de ta pièce, compresseur à ton niveau) ; l'étape suivante permet de l'ajuster
                if ok {
                    let kind = this.go.view.as_ref().and_then(|v| v.state["mic"]["type"].as_str().map(String::from)).unwrap_or_else(|| "dynamic".into());
                    let advised = this.go.view.as_ref().and_then(|v| v.meta["mic_type_preset"][kind.as_str()].as_str().map(String::from)).unwrap_or_else(|| "stream".into());
                    let ns_ready = this.go.view.as_ref().is_some_and(|v| v.mic_ns["ready"] == true);
                    let client = this.client.clone();
                    let work = cx.background_executor().spawn(async move {
                        let _ = client.go_command("mic-preset", json!({"name": advised}));
                        // suppression du bruit installée : elle s'active avec le reste
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
        // la prise suit le gain : baissé de 4 dB depuis, elle se réécoute 4 dB plus bas
        let scale = 10f32.powf((gain_now - take.gain_db) as f32 / 20.);
        let (samples, rate) = (take.samples.clone(), take.rate);
        let chain = match preset.as_deref() {
            // « actuel » : les réglages de la GoXLR tels qu'ils sont, gain et noise gate ajustés compris
            Some("actuel") => self.go.view.as_ref().map(|v| micchain::design(&dsp::effective_dsp(&dsp::normalize_dsp(&v.state["dsp"], None), &dsp::normalize_fx(&v.state["fx"])))),
            Some(p) => match profile_dsp(self.voice.info.as_ref(), p) {
                Some(d) => Some(micchain::design(&d)),
                None => {
                    if let Some(w) = self.wizard_mut() {
                        w.error = Some("Ce son n’est pas encore calculé sur ta voix : refais l’étape « Ta voix ».".into());
                    }
                    cx.notify();
                    return;
                }
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

    fn wizard_goto(&mut self, step: Step, cx: &mut Context<Self>) {
        if let Some(w) = self.wizard_mut() {
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
        w.listen_run += 1; // une boucle d'écoute encore en cours s'arrêtera : son numéro ne correspond plus
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

    // ---------- affichage ----------

    pub fn mic_wizard_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let w = self.wizard.as_ref()?;
        let body = match w.step {
            Step::Mic => self.wizard_mic_view(cx),
            Step::Measure => self.wizard_measure_view(w, cx),
            Step::Result => self.wizard_result_view(w, cx),
            Step::Sound => self.wizard_sound_view(w, cx),
            Step::Done => self.wizard_done_view(cx),
        };
        let close = ui::round("wizard-close", Icon::Close, 28., alpha(ui::INK, 0.06), rgb(ui::TEXT2)).on_click(cx.listener(|this, _, _, cx| this.close_mic_wizard(cx)));
        let head = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::icon(Icon::Mic, 22., rgb(ui::RED)))
            .child(div().flex_1().flex().flex_col().child(div().text_size(px(19.)).font_weight(FontWeight::BOLD).child("Régler mon micro")).child(ui::caption("Deux minutes, en te guidant à chaque instant.")))
            .child(close);
        let mut stepper = div().flex().gap(px(6.));
        let current = STEPS.iter().position(|(s, _)| *s == w.step);
        for (i, (_, label)) in STEPS.iter().enumerate() {
            let state = match current {
                Some(c) if i < c => 2,
                Some(c) if i == c => 1,
                _ => 0,
            };
            let colour = if state == 2 { ui::GREEN } else { ui::ACCENT };
            stepper = stepper.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(div().h(px(4.)).rounded_full().bg(if state == 0 { alpha(ui::INK, 0.08) } else { alpha(colour, 1.) }))
                    .child(div().text_size(px(11.5)).font_weight(if state == 1 { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).text_color(rgb(if state == 0 { ui::TEXT3 } else { ui::TEXT2 })).child(*label)),
            );
        }
        let mut root = div().p(px(26.)).rounded(px(18.)).flex().flex_col().gap(px(20.)).child(head).child(stepper).child(body);
        if let Some(e) = &w.error {
            root = root.child(ui::notice(false, e.clone()));
        }
        Some(root.child(self.wizard_live_bar(w, cx)))
    }

    /// En bas de chaque étape : le niveau du micro en direct et l'écoute de ta voix dans le casque.
    fn wizard_live_bar(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        div()
            .pt(px(16.))
            .border_t_1()
            .border_color(alpha(ui::INK, 0.08))
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
                    .child(div().flex().flex_col().child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("M’écouter en direct")).child(ui::caption("Ta voix réglée, dans le casque")))
                    .child(ui::toggle("wizard-listen", w.listening, true).on_click(cx.listener(|this, _, _, cx| this.wizard_toggle_listen(cx)))),
            )
    }

    fn wizard_actions(&self, items: Vec<AnyElement>) -> Div {
        div().flex().justify_end().items_center().gap(px(8.)).children(items)
    }

    /// Étape 1 : quel micro (choisi par toi dans la liste, jamais deviné), et comment se placer.
    fn wizard_mic_view(&self, cx: &mut Context<Self>) -> Div {
        let Some(v) = self.go.view.as_ref() else { return div() };
        let current = v.state["mic"]["model"].as_str().unwrap_or("").to_owned();
        let kind_label = |k: &str| match k {
            "condenser" => "Statique · alimenté en 48 V",
            "jack" => "Jack 3,5 mm",
            _ => "Dynamique · sans alimentation",
        };
        let mut tiles: Vec<AnyElement> = vec![];
        for (id, m) in crate::goxlr::entries(&v.meta["mic_models"]) {
            let brand = m["brand"].as_str().unwrap_or("");
            let name = m["name"].as_str().unwrap_or(id);
            let title = if brand.is_empty() { name.to_owned() } else { format!("{brand} {name}") };
            let key = id.clone();
            let tile = ui::tile(SharedString::from(format!("wizard-model-{id}")), None, title, Some(kind_label(m["type"].as_str().unwrap_or("dynamic")).into()), current == *id, None)
                .on_click(cx.listener(move |this, _, _, cx| this.go_send("mic-model", json!({"model": key}), cx)));
            tiles.push(tile.into_any_element());
        }
        let check = |glyph: Icon, text: &'static str| div().flex().items_start().gap(px(10.)).child(div().flex_none().mt(px(1.)).child(ui::icon(glyph, 16., rgb(ui::ACCENT)))).child(hint(text).flex_1().min_w(px(0.)));
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(div().flex().flex_col().gap(px(6.)).child(ui::label("Quel est ton micro ?")).child(ui::grid(tiles, 3)))
            .child(ui::caption("Pas dans la liste ? Son nom est écrit sur le micro ou sa boîte. Un micro qui ne marche qu’avec l’alimentation 48 V est « statique » (micros de studio en forme de stylo ou de gros cylindre) ; un micro qui marche sans est « dynamique » (Shure SM7B, SM58, RØDE PodMic…). Choisis alors « Autre micro » du bon type."))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(ui::label("Avant de commencer"))
                    .child(check(Icon::Mic, "Place-toi comme en live : 10 à 15 cm du micro, face à lui."))
                    .child(check(Icon::Headphones, "Garde ton casque : rien ne doit sortir des enceintes."))
                    .child(check(Icon::VolumeOff, "Laisse ton PC et tes ventilateurs tourner comme d’habitude : Rustudio doit les entendre pour les couper.")),
            )
            .child(hint("La mesure dure 25 secondes : d’abord un silence complet, puis trois phrases à lire, puis une phrase plus forte. L’écran te dit à chaque instant quoi faire."))
            .child(self.wizard_actions(vec![ui::pill("wizard-start", Some(Icon::Play), "Lancer la mesure", Tone::Primary)
                .when(!self.go.enabled() || current.is_empty(), |d| d.opacity(0.45))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let chosen = this.go.view.as_ref().is_some_and(|v| v.state["mic"]["model"].is_string());
                    if !chosen {
                        if let Some(w) = this.wizard_mut() {
                            w.error = Some("Choisis d’abord ton micro dans la liste.".into());
                        }
                        cx.notify();
                    } else if this.go.enabled() {
                        this.wizard_voice(cx);
                    }
                }))
                .into_any_element()]))
    }

    /// Étape 2 : la mesure. Une seule consigne à la fois, en grand, avec le temps qui reste.
    fn wizard_measure_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        if !w.busy() {
            return div().flex().flex_col().gap(px(12.)).child(hint("La mesure s’est arrêtée. Relance-la quand tu es prêt.")).child(self.wizard_actions(vec![
                ui::pill("wizard-back-mic", None, "Retour", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Mic, cx))).into_any_element(),
                ui::pill("wizard-measure-retry", Some(Icon::Refresh), "Recommencer", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx))).into_any_element(),
            ]));
        }
        let phase = w.job["phase"].as_str().unwrap_or("");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
        let left = w.job["phase_end"].as_f64().map(|e| (e - now).max(0.));
        let retry = w.job["retry"].as_u64().unwrap_or(0) > 0;
        // titre, consigne en gros, explication, couleur
        let (glyph, title, big, why, colour): (Icon, &str, String, &str, u32) = match phase {
            "silence" => (Icon::VolumeOff, "Silence complet", "Ne dis plus rien".into(), "Respire doucement, ne touche à rien. J’écoute ta pièce : ventilateurs, PC, rue. C’est ce bruit-là que le réglage va couper.", ui::ORANGE),
            "reading" => {
                let elapsed = left.map(|l| (3. * SENTENCE_S - l).max(0.)).unwrap_or(0.);
                let i = ((elapsed / SENTENCE_S) as usize).min(SENTENCES.len() - 1);
                (Icon::Mic, if retry { "Ça saturait : gain baissé, relis" } else { "Lis à voix normale" }, SENTENCES[i].into(), "Comme en live, à ta distance habituelle. La phrase change toute seule.", ui::ACCENT)
            }
            "loud" => (Icon::Bolt, "Plus fort, comme quand tu t’enflammes", LOUD_SENTENCE.into(), "Je vérifie que ta voix ne sature jamais, même quand tu cries.", ui::RED),
            "prepare" => (Icon::Headphones, "Prépare-toi", "Le silence commence".into(), "Dans un instant : ne parle plus.", ui::ACCENT),
            _ => (Icon::Sparkles, "Analyse de ta voix", "Presque fini…".into(), "Rustudio calcule ton gain, ton noise gate et ton égaliseur.", ui::ACCENT),
        };
        let mut b = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .py(px(10.))
            .child(div().flex().items_center().gap(px(8.)).child(ui::icon(glyph, 18., rgb(colour))).child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(colour)).child(title)))
            .child(div().text_center().text_size(px(26.)).line_height(px(34.)).font_weight(FontWeight::BOLD).child(big))
            .child(div().max_w(px(520.)).text_center().child(hint(why)));
        if phase == "reading" {
            let elapsed = left.map(|l| (3. * SENTENCE_S - l).max(0.)).unwrap_or(0.);
            let i = ((elapsed / SENTENCE_S) as usize).min(SENTENCES.len() - 1);
            b = b.child(div().flex().gap(px(6.)).children((0..SENTENCES.len()).map(|n| div().w(px(26.)).h(px(4.)).rounded_full().bg(if n <= i { rgb(ui::ACCENT) } else { alpha(ui::INK, 0.12) }))));
        }
        if let (Some(l), true) = (left, matches!(phase, "silence" | "reading" | "loud" | "prepare")) {
            b = b.child(div().text_size(px(40.)).font_weight(FontWeight::BOLD).text_color(rgb(colour)).child(format!("{}", l.ceil() as i64)));
        }
        b.child(
            div().w_full().flex().justify_end().child(
                ui::pill("wizard-cancel", Some(Icon::Close), "Annuler", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_cancel(cx))),
            ),
        )
    }

    /// Étape 3 : ce que la mesure a trouvé, appliqué, et ajustable à l'oreille.
    fn wizard_result_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        let Some(v) = self.go.view.as_ref() else { return div() };
        let s = &v.state;
        let enabled = self.go.enabled();
        let gain = s["mic"]["gain_db"].as_f64().unwrap_or(0.);
        let cal = &s["mic_cal"];
        let noise = cal["noise_peak_db"].as_f64();
        let (room, room_sub) = match noise {
            Some(n) if n > -42. => ("Bruyante", "Ventilateurs ou PC bien audibles"),
            Some(n) if n > -54. => ("Bruit de fond", "Un léger souffle s’entend"),
            Some(_) => ("Calme", "Presque rien entre tes phrases"),
            None => ("—", "Non mesurée"),
        };
        let gate_on = s["fx"]["gate"] == true;
        let threshold = s["dsp"]["gate"]["threshold"].as_f64().unwrap_or(-40.);
        let playing = w.playing.clone();
        let label = |key: &str, text: &'static str| if playing.as_deref() == Some(key) { "Lecture…" } else { text };
        let mut b = div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(40.))
                    .gap_y(px(10.))
                    .child(ui::fact(room, format!("Ta pièce · {room_sub}")))
                    .child(ui::fact(format!("{} dB", gain.round()), "Gain du micro"))
                    .child(ui::fact(if gate_on { format!("{} dB", threshold.round()) } else { "Coupé".into() }, "Noise gate")),
            )
            .child(hint("Dans l’ordre d’OBS : le gain, puis la suppression du bruit (l’IA enlève ventilateurs et souffle, même pendant que tu parles), puis le noise gate (il coupe tout entre tes phrases). Active « M’écouter en direct » en bas : tais-toi, tu ne dois plus rien entendre ; parle, ta voix doit passer entière."))
            .child(ui::subhead("Gain"))
            .child(self.slider_row("wizard-gain", "Sensibilité du micro", gain, (0., 72.), 1., |v| format!("{} dB", v.round()), enabled, |this, v, cx| this.go_send("mic", json!({"gain_db": v.round() as i64}), cx), cx))
            .child(self.ns_panel(v, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().child(ui::subhead("Noise gate")))
                    .child(ui::toggle("wizard-gate", gate_on, enabled).on_click(cx.listener(move |this, _, _, cx| this.go_send("fx", json!({"gate": !gate_on}), cx)))),
            )
            .when(gate_on, |d| {
                d.child(self.slider_row("wizard-gate-threshold", "Seuil : plus haut coupe plus de bruit", threshold, (-59., 0.), 1., |v| format!("{} dB", v.round()), enabled, |this, v, cx| this.go_send("dsp", json!({"gate":{"threshold": v.round() as i64}}), cx), cx))
            });
        if w.take.is_some() {
            b = b.child(ui::subhead("Réécouter ta prise")).child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(ui::pill("wizard-play-raw", Some(Icon::Play), label("avant", "Brute"), Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_play(None, cx))))
                    .child(ui::pill("wizard-play-now", Some(Icon::Play), label("actuel", "Avec mes réglages"), Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_play(Some("actuel".into()), cx)))),
            );
        }
        b.child(self.wizard_actions(vec![
            ui::pill("wizard-measure-again", Some(Icon::Refresh), "Refaire la mesure", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_voice(cx))).into_any_element(),
            ui::pill("wizard-to-sound", Some(Icon::ChevronRight), "Choisir mon son", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Sound, cx))).into_any_element(),
        ]))
    }

    /// Étape 4 : chaque son, à écouter sur ta prise, puis à choisir.
    fn wizard_sound_view(&self, w: &MicWizard, cx: &mut Context<Self>) -> Div {
        let state = self.go.view.as_ref().map(|v| &v.state);
        let current = w.chosen.clone().or_else(|| state.and_then(|s| s["mic_preset"].as_str()).map(String::from)).unwrap_or_default();
        let kind = state.and_then(|s| s["mic"]["type"].as_str()).unwrap_or("dynamic");
        let advised = self.go.view.as_ref().and_then(|v| v.meta["mic_type_preset"][kind].as_str()).unwrap_or("");
        let can_play = w.take.is_some() && self.voice.info.is_some();
        let mut rows: Vec<AnyElement> = vec![];
        for p in dsp::MIC_PRESETS.iter().filter(|p| p.key != "brut") {
            let id = p.key.to_owned();
            let play_id = id.clone();
            let playing = w.playing.as_deref() == Some(p.key);
            let tile = ui::tile(SharedString::from(format!("wizard-preset-{id}")), None, p.label.to_owned(), Some(p.desc.into()), current == p.key, (p.key == advised).then(|| ("Conseillé".into(), ui::GREEN)))
                .on_click(cx.listener(move |this, _, _, cx| this.wizard_choose(id.clone(), cx)));
            let play = can_play.then(|| {
                ui::round(SharedString::from(format!("wizard-play-{}", p.key)), if playing { Icon::Wave } else { Icon::Play }, 34., alpha(ui::ACCENT, 0.1), rgb(ui::ACCENT))
                    .on_click(cx.listener(move |this, _, _, cx| this.wizard_play(Some(play_id.clone()), cx)))
            });
            rows.push(div().flex().items_center().gap(px(8.)).child(tile.flex_1().min_w(px(0.))).children(play).into_any_element());
        }
        let raw_playing = w.playing.as_deref() == Some("avant");
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(hint(if can_play {
                "Chaque son est calculé sur ta voix. ▶ fait entendre ta prise avec ce son, exactement comme ta GoXLR la traitera ; un clic sur le nom le choisit."
            } else {
                "Un clic sur un son le choisit."
            }))
            .child(ui::grid(rows, 2))
            .when(can_play, |d| {
                d.child(
                    div().flex().items_center().gap(px(10.)).child(
                        ui::pill("wizard-play-before", Some(if raw_playing { Icon::Wave } else { Icon::Play }), if raw_playing { "Lecture…" } else { "Comparer : sans traitement" }, Tone::Plain)
                            .on_click(cx.listener(|this, _, _, cx| this.wizard_play(None, cx))),
                    ),
                )
            })
            .child(self.wizard_actions(vec![
                ui::pill("wizard-sound-back", None, "Retour", Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Result, cx))).into_any_element(),
                ui::pill("wizard-to-done", Some(Icon::Check), "C’est parfait", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.wizard_goto(Step::Done, cx))).into_any_element(),
            ]))
    }

    fn wizard_done_view(&self, cx: &mut Context<Self>) -> Div {
        let s = self.go.view.as_ref().map(|v| &v.state);
        let preset = s.and_then(|s| s["mic_preset"].as_str()).and_then(dsp::preset).map(|p| p.label).unwrap_or("Réglages personnalisés");
        let gain = s.and_then(|s| s["mic"]["gain_db"].as_f64()).unwrap_or(0.);
        let on: Vec<&str> = [("eq", "égaliseur"), ("gate", "noise gate"), ("comp", "compresseur"), ("deesser", "de-esser")]
            .into_iter()
            .filter(|(k, _)| s.is_some_and(|s| s["fx"][*k] == true))
            .map(|(_, l)| l)
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(div().flex().items_center().gap(px(10.)).child(ui::icon(Icon::Check, 22., rgb(ui::GREEN))).child(ui::title("Ton micro est prêt", 20.)))
            .child(
                div()
                    .flex()
                    .gap_x(px(40.))
                    .child(ui::fact(preset, "Son"))
                    .child(ui::fact(format!("{} dB", gain.round()), "Gain"))
                    .child(ui::fact(if on.is_empty() { "Aucun".to_owned() } else { on.len().to_string() }, "Traitements actifs")),
            )
            .child(hint(if on.is_empty() { "Ta voix part telle quelle.".to_owned() } else { format!("Actifs : {}.", on.join(", ")) }))
            .child(hint("Discord, OBS et tes jeux reçoivent directement la voix réglée. Tout reste ajustable dans la page Micro ; relance l’assistant si tu changes de micro ou de pièce."))
            .child(self.wizard_actions(vec![ui::pill("wizard-finish", Some(Icon::Check), "Terminer", Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.close_mic_wizard(cx))).into_any_element()]))
    }
}
