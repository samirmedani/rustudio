use crate::{
    backend::Client,
    desktop::hint,
    mixer::Mixer,
    ui::{self, Icon},
};
use gpui::{Context, Div, div, prelude::*, px, rgb};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const MODULE: &str = "/api/modules/goxlr-mini";

fn take(client: &Client, cancel: &AtomicBool, quick: bool) -> Result<Value, String> {
    // assistant : 6 s de silence (ta pièce), 3 phrases de 5 s, 4 s plus fort
    let options = if quick {
        json!({"silence_s":1.5,"reading_s":8,"loud_s":1.5})
    } else {
        json!({"silence_s":6,"reading_s":15,"loud_s":4})
    };
    let mode = client.live_state()?;
    if mode["connected"] != true || mode["job"]["running"] == true {
        return Err("La GoXLR doit être connectée et disponible pour la mesure.".into());
    }
    if mode["mic"]["muted"] == true || mode["mic"]["partial"].as_str() == Some("chat") {
        return Err("Réactive le micro pour lancer la mesure.".into());
    }
    let start = client.post_module(MODULE, "voice-print", options)?;
    let id = start["job"].as_f64().ok_or("L’analyse n’a pas démarré.")?;
    let deadline = Instant::now() + Duration::from_secs(58);
    let outcome = (|| {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Analyse annulée.".into());
            }
            if Instant::now() >= deadline {
                return Err("La mesure n’a pas abouti dans le délai prévu.".into());
            }
            let live = client.live_state()?;
            if live["boot"] != mode["boot"] {
                return Err("Le moteur a redémarré.".into());
            }
            let job = &live["job"];
            if job["started"].as_f64() != Some(id) {
                return Err("La session de mesure a changé.".into());
            }
            if job["running"] != true {
                if let Some(error) = job["error"].as_str() {
                    return Err(error.into());
                }
                if job["result"]["audio"] == "wanted" {
                    return Ok(job["result"].clone());
                }
                return Err("Aucun enregistrement attendu par le moteur.".into());
            }
            std::thread::sleep(Duration::from_millis(170));
        }
    })();
    if outcome.is_err()
        && client.live_state().is_ok_and(|state| {
            state["boot"] == mode["boot"]
                && state["job"]["started"].as_f64() == Some(id)
                && state["job"]["running"] == true
        })
    {
        let _ = client.post_module(MODULE, "job-cancel", json!({}));
    }
    outcome
}

/// Prise brute de l'empreinte, gardée par l'assistant pour la réécouter : échantillons, fréquence, début (ms epoch).
pub struct Take {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub t0_ms: u64,
}

/// Empreinte vocale : le moteur mène les étapes, l'appli enregistre « Chat Mic » puis envoie le son.
/// Renvoie l'analyse, l'étape de lecture (début, fin en epoch s, gain) et la prise brute.
pub fn analyse(client: &Client, cancel: &AtomicBool, quick: bool) -> Result<(Value, Value, Option<Take>), String> {
    if !client.is_live() {
        let job = take(client, cancel, quick)?;
        if cancel.load(Ordering::Relaxed) {
            return Err("Analyse annulée.".into());
        }
        return Ok((client.send_voice(None)?, job["timeline"].clone(), None));
    }
    let stop = AtomicBool::new(false);
    let (tx, rx) = mpsc::sync_channel(1);
    let (run, bytes) = std::thread::scope(|scope| {
        let record = scope.spawn(|| rustudio_windows_desktop::soundcheck::capture_voice(&stop, tx));
        let result = (|| {
            let (time, rate) = rx
                .recv_timeout(Duration::from_secs(4))
                .map_err(|_| "Impossible d’ouvrir l’entrée Chat Mic.")?;
            let job = take(client, cancel, quick)?;
            Ok::<_, String>((time, rate, job))
        })();
        stop.store(true, Ordering::Relaxed);
        (
            result,
            record
                .join()
                .unwrap_or_else(|_| Err("Capture du micro interrompue.".into())),
        )
    });
    let (time, rate, job) = run?;
    let bytes = bytes?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Analyse annulée.".into());
    }
    let samples: Vec<f32> = bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
    let result = client.send_voice(Some((time, rate, bytes)))?;
    Ok((result, job["timeline"].clone(), Some(Take { samples, rate, t0_ms: time })))
}

/// Réécoute (8 s), graphique de voix et test d'écho : complément de la page Micro et du diagnostic.
#[derive(Default)]
pub struct VoiceExtras {
    pub recording: Option<Arc<AtomicBool>>,
    pub take: Option<Arc<(Vec<f32>, u32)>>,
    pub playing: bool,
    pub info: Option<Value>,
    pub loading: bool,
    pub reload: bool,
    pub echo: Option<Value>,
    pub echo_running: bool,
    pub note: Option<String>,
}

const REC_SECONDS: f64 = 8.0;

impl Mixer {
    pub fn load_voice_info(&mut self, cx: &mut Context<Self>) {
        if self.voice.loading {
            self.voice.reload = true;
            return;
        }
        self.voice.loading = true;
        let client = self.client.clone();
        let job = cx.background_executor().spawn(async move { client.get("/api/modules/goxlr-mini/voice") });
        cx.spawn(async move |this, cx| {
            let r = job.await;
            let _ = this.update(cx, |this, cx| {
                this.voice.loading = false;
                if let Ok(info) = r {
                    this.voice.info = Some(info);
                }
                if std::mem::take(&mut this.voice.reload) {
                    this.load_voice_info(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn record_take(&mut self, cx: &mut Context<Self>) {
        if self.voice.recording.is_some() || !self.client.is_live() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.voice.recording = Some(stop.clone());
        self.voice.note = None;
        let job = cx.background_executor().spawn(async move { rustudio_windows_desktop::soundcheck::record_mic(REC_SECONDS, stop) });
        cx.spawn(async move |this, cx| {
            let r = job.await;
            let _ = this.update(cx, |this, cx| {
                this.voice.recording = None;
                match r {
                    Ok((x, rate)) if !x.is_empty() => this.voice.take = Some(Arc::new((x, rate))),
                    Ok(_) => this.voice.note = Some("Aucun son reçu de Chat Mic.".into()),
                    Err(e) => this.voice.note = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn play_take(&mut self, cx: &mut Context<Self>) {
        let Some(take) = self.voice.take.clone() else { return };
        if self.voice.playing {
            return;
        }
        self.voice.playing = true;
        let job = cx.background_executor().spawn(async move { rustudio_windows_desktop::soundcheck::play_default(take.0.clone(), take.1) });
        cx.spawn(async move |this, cx| {
            let r = job.await;
            let _ = this.update(cx, |this, cx| {
                this.voice.playing = false;
                if let Err(e) = r {
                    this.voice.note = Some(e);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn run_echo(&mut self, cx: &mut Context<Self>) {
        if self.voice.echo_running {
            return;
        }
        let chat_muted = self.go.view.as_ref().is_some_and(|v| v.state["mutes"]["Chat"] == true);
        if chat_muted {
            self.voice.echo = Some(json!({"status": "error", "error": "La voie Chat (Discord) est coupée : réactive-la le temps du test."}));
            cx.notify();
            return;
        }
        self.voice.echo_running = true;
        self.voice.echo = None;
        let live = self.client.is_live();
        let job = cx.background_executor().spawn(async move {
            if live {
                rustudio_windows_desktop::soundcheck::echo_test()
            } else {
                json!({"status": "ok", "level": "none", "simulated": true})
            }
        });
        cx.spawn(async move |this, cx| {
            let r = job.await;
            let _ = this.update(cx, |this, cx| {
                this.voice.echo_running = false;
                this.voice.echo = Some(r);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn echo_panel(&self, cx: &mut Context<Self>) -> Div {
        let mut p = ui::section(Icon::Chat, ui::ACCENT, "Test d’écho", None)
            .child(hint("Rustudio joue trois bips dans la voie Discord (tu les entends dans le casque) et écoute ton micro. Reste silencieux, et pas pendant un live."));
        if self.voice.echo_running {
            p = p.child(ui::status("Test en cours…", ui::State::Info));
        } else {
            p = p.child(ui::pill("echo-run", Some(Icon::Play), "Lancer le test", ui::Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.run_echo(cx))));
        }
        if let Some(r) = self.voice.echo.as_ref() {
            let loop_route = self.go.view.as_ref().is_some_and(|v| v.state["routing"]["Chat"]["ChatMic"] == true);
            let text = if r["status"] != "ok" {
                format!("Test impossible : {}", r["error"].as_str().unwrap_or("erreur inconnue"))
            } else {
                match r["level"].as_str() {
                    Some("strong") => format!(
                        "Écho fort : les bips reviennent dans ton micro ({} ms plus tard, {} dB). {}",
                        r["delay_ms"].as_f64().unwrap_or(0.).max(0.).round(),
                        r["db"].as_f64().unwrap_or(0.).round(),
                        if loop_route { "Cause : Discord est renvoyé vers Discord (routage Chat → Chat Mic)." } else { "Vérifie le micro de Windows et le routage de Chat." }
                    ),
                    Some("light") => format!("Léger écho ({} dB) : le casque s’entend dans le micro. Baisse un peu le casque ou éloigne le micro.", r["db"].as_f64().unwrap_or(0.).round()),
                    _ => format!("{}Aucun écho : ton micro n’a rien capté des bips.", if r["simulated"] == true { "Démonstration · " } else { "" }),
                }
            };
            p = p.child(ui::notice(r["level"] != "strong", text));
        }
        p
    }

    pub fn listen_panel(&self, cx: &mut Context<Self>) -> Div {
        let mut p = div().flex().flex_col().gap(px(10.)).child(ui::subhead("Comme les autres t’entendent"))
            .child(hint("Enregistre 8 secondes de « Chat Mic », puis réécoute ta voix telle que la reçoivent Discord et ton stream."));
        if !self.client.is_live() {
            return p.child(hint("Démonstration : aucun micro du PC n’est enregistré."));
        }
        let mut actions = div().flex().items_center().gap(px(8.));
        if let Some(stop) = self.voice.recording.clone() {
            actions = actions.child(hint("Enregistrement… parle comme en live.")).child(
                ui::pill("listen-stop", Some(Icon::Stop), "Arrêter", ui::Tone::Plain).on_click(cx.listener(move |_, _, _, _| stop.store(true, Ordering::Relaxed))),
            );
        } else {
            actions = actions.child(ui::pill("listen-rec", Some(Icon::Mic), if self.voice.take.is_some() { "Refaire" } else { "Enregistrer 8 s" }, ui::Tone::Primary)
                .on_click(cx.listener(|this, _, _, cx| this.record_take(cx))));
            if self.voice.take.is_some() {
                actions = actions.child(ui::pill("listen-play", Some(Icon::Play), if self.voice.playing { "Lecture…" } else { "Réécouter" }, ui::Tone::Plain)
                    .on_click(cx.listener(|this, _, _, cx| this.play_take(cx))));
            }
        }
        p = p.child(actions);
        if let Some(n) = &self.voice.note {
            p = p.child(hint(n.clone()));
        }
        p
    }

    /// Ta voix avant et avec le préréglage actif, par bande de fréquence.
    pub fn voice_chart(&self, preset: &str) -> Div {
        let mut p = ui::section(Icon::Activity, ui::ACCENT, "Ta voix et le préréglage", None);
        let Some(info) = self.voice.info.as_ref() else {
            return p.child(hint(if self.voice.loading { "Lecture…" } else { "Lance « Régler mon micro » pour voir ta voix." }));
        };
        let chart = &info["profiles"][preset]["chart"];
        let (Some(bands), Some(voice), Some(after)) = (chart["bands"].as_array(), chart["voice"].as_array(), chart["after"].as_array()) else {
            return p.child(hint("Lance « Régler mon micro » : ta voix apparaîtra ici, avant et après le préréglage."));
        };
        let values: Vec<f64> = voice.iter().chain(after.iter()).filter_map(Value::as_f64).collect();
        if bands.is_empty() || values.is_empty() {
            return p.child(hint("Lance « Régler mon micro » : ta voix apparaîtra ici, avant et après le préréglage."));
        }
        let (lo, hi) = values.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let span = (hi - lo).max(1.0);
        let height = |v: Option<f64>| gpui::relative((((v.unwrap_or(lo) - lo) / span) * 0.94 + 0.04) as f32);
        let step = (bands.len() / 32).max(1);
        // barres sur toute la largeur : gris ta voix, bleu avec le préréglage
        let mut bars = div().flex().items_end().gap(px(4.)).h(px(130.)).w_full();
        for i in (0..bands.len()).step_by(step) {
            bars = bars.child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_end()
                    .gap(px(1.))
                    .child(div().flex_1().h(height(voice.get(i).and_then(Value::as_f64))).bg(rgb(0x8e8e93)).rounded_sm())
                    .child(div().flex_1().h(height(after.get(i).and_then(Value::as_f64))).bg(rgb(0x0071e3)).rounded_sm()),
            );
        }
        let first = bands.first().and_then(Value::as_f64).unwrap_or(50.);
        let last = bands.last().and_then(Value::as_f64).unwrap_or(16000.);
        let legend = |colour: u32, text: &'static str| div().flex().items_center().gap(px(6.)).child(div().size(px(8.)).rounded_full().bg(rgb(colour))).child(ui::caption(text));
        p = p.child(bars).child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(legend(0x8e8e93, "Ta voix"))
                .child(legend(0x0071e3, "Avec le préréglage"))
                .child(div().flex_1())
                .child(ui::caption(format!("{} Hz → {} kHz", first.round(), (last / 1000.).round()))),
        );
        // ce que l'empreinte a mesuré, en faits lisibles plutôt qu'en mots isolés
        let sm = &info["summary"];
        let mut facts: Vec<(String, &'static str)> = vec![];
        if let Some(t) = sm["voice_type"].as_str() {
            let hz = sm["f0_hz"].as_f64().map(|f| format!(" · {} Hz", f.round())).unwrap_or_default();
            facts.push((format!("{}{hz}", capital(t)), "Ta voix"));
        }
        if let Some(t) = sm["sibilance"].as_str() {
            facts.push((capital(t), "Sifflantes"));
        }
        if let Some(t) = sm["room"].as_str() {
            facts.push((capital(t), "Ta pièce"));
        }
        if let Some(d) = sm["snr_db"].as_f64() {
            facts.push((format!("{} dB", d.round()), "Voix au-dessus du bruit"));
        }
        if !facts.is_empty() {
            p = p.child(div().flex().flex_wrap().gap_x(px(32.)).gap_y(px(12.)).pt(px(6.)).children(facts.into_iter().map(|(v, l)| ui::fact(v, l))));
        }
        p
    }
}

/// « grave » → « Grave ».
pub(crate) fn capital(text: &str) -> String {
    let mut c = text.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
