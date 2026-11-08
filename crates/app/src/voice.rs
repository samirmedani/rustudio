use crate::{
    backend::Client,
    desktop::{hint, panel},
    mixer::{Mixer, button},
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
    let options = if quick {
        json!({"silence_s":1.5,"reading_s":8,"loud_s":1.5})
    } else {
        json!({})
    };
    let mode = client.live_state()?;
    if mode["connected"] != true || mode["job"]["running"] == true {
        return Err("La GoXLR doit être connectée et disponible pour la mesure.".into());
    }
    if mode["mic"]["muted"] == true || mode["mic"]["partial"].is_string() {
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

pub fn analyse(client: &Client, cancel: &AtomicBool, quick: bool) -> Result<Value, String> {
    if !client.is_live() {
        take(client, cancel, quick)?;
        return client.send_voice(None);
    }
    let stop = AtomicBool::new(false);
    let (tx, rx) = mpsc::sync_channel(1);
    let (run, bytes) = std::thread::scope(|scope| {
        let record = scope.spawn(|| rustudio_windows_desktop::soundcheck::capture_voice(&stop, tx));
        let result = (|| {
            let (time, rate) = rx
                .recv_timeout(Duration::from_secs(4))
                .map_err(|_| "Impossible d’ouvrir l’entrée Chat Mic.")?;
            take(client, cancel, false)?;
            Ok::<_, String>((time, rate))
        })();
        stop.store(true, Ordering::Relaxed);
        (
            result,
            record
                .join()
                .unwrap_or_else(|_| Err("Capture du micro interrompue.".into())),
        )
    });
    let (time, rate) = run?;
    client.send_voice(Some((time, rate, bytes?)))
}

impl Mixer {
    fn run_voice_print(&mut self, cx: &mut Context<Self>) {
        if !self.go.enabled() || self.go.voice_cancel.is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.go.voice_cancel = Some(cancel.clone());
        self.go.notice = None;
        self.go.busy = true;
        let client = self.client.clone();
        let work = cx
            .background_executor()
            .spawn(async move { analyse(&client, &cancel, false) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.go.voice_cancel = None;
                this.go.busy = false;
                this.go.notice = Some(match result {
                    Ok(_) => (true, "Empreinte calculée et enregistrée.".into()),
                    Err(error) => (false, error),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn voice_panel(&self, cx: &mut Context<Self>) -> Div {
        let mut panel = panel()
            .child(crate::ui::heading("Empreinte vocale"))
            .child(hint(if self.client.is_live() {
                "Mesure via Chat Mic : le son est analysé localement et reste en mémoire."
            } else {
                "Une voix synthétique est utilisée. Aucun microphone du PC n’est lu."
            }));
        if let Some(cancel) = self.go.voice_cancel.as_ref() {
            let cancel = cancel.clone();
            let j = &self.go.job;
            panel = panel.child(hint(
                j["step"].as_str().unwrap_or("Préparation…").to_owned(),
            ));
            if j["phase"] == "reading" {
                panel=panel.child(div().child("Ce matin, le ciel était clair. Je prends le temps de lire cette phrase. Les sons passent doucement, puis ma voix reprend un rythme naturel."));
            } else if j["phase"] == "loud" {
                panel = panel.child(div().child("Parle plus fort : « On y va, c’est parti ! »"));
            }
            panel = panel.child(
                button(
                    "voice-print-cancel".into(),
                    "Annuler la mesure".into(),
                    false,
                )
                .on_click(cx.listener(move |_, _, _, _| cancel.store(true, Ordering::Relaxed))),
            );
        } else if self.go.enabled() {
            panel = panel.child(
                button(
                    "voice-print-start".into(),
                    "Créer mon empreinte".into(),
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| this.run_voice_print(cx))),
            );
        }
        panel
    }
}


/// Réécoute (8 s), graphique de voix et test d'écho : complément de la page Micro et du diagnostic.
#[derive(Default)]
pub struct VoiceExtras {
    pub recording: Option<Arc<AtomicBool>>,
    pub take: Option<Arc<(Vec<f32>, u32)>>,
    pub playing: bool,
    pub info: Option<Value>,
    pub loading: bool,
    pub echo: Option<Value>,
    pub echo_running: bool,
    pub note: Option<String>,
}

const REC_SECONDS: f64 = 8.0;

impl Mixer {
    pub fn load_voice_info(&mut self, cx: &mut Context<Self>) {
        if self.voice.loading {
            return;
        }
        self.voice.loading = true;
        let client = self.client.clone();
        let job = cx.background_executor().spawn(async move { client.get("/api/modules/goxlr-mini/voice") });
        cx.spawn(async move |this, cx| {
            let r = job.await;
            let _ = this.update(cx, |this, cx| {
                this.voice.loading = false;
                this.voice.info = r.ok();
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
        let mut p = panel().child(crate::ui::heading("Test d’écho"))
            .child(hint("Rustudio joue trois bips dans la voie Discord (tu les entends dans le casque) et écoute ton micro. Reste silencieux, et pas pendant un live."));
        if self.voice.echo_running {
            p = p.child(hint("Test en cours…"));
        } else {
            p = p.child(button("echo-run".into(), "Lancer le test".into(), true).on_click(cx.listener(|this, _, _, cx| this.run_echo(cx))));
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
            p = p.child(div().text_sm().child(text));
        }
        p
    }

    pub fn listen_panel(&self, cx: &mut Context<Self>) -> Div {
        let mut p = panel().child(crate::ui::heading("M’écouter"))
            .child(hint("Enregistre 8 secondes de « Chat Mic », puis réécoute ta voix telle que la reçoivent Discord et ton stream."));
        if !self.client.is_live() {
            return p.child(hint("Démonstration : aucun micro du PC n’est enregistré."));
        }
        let mut actions = div().flex().gap_2();
        if let Some(stop) = self.voice.recording.clone() {
            actions = actions.child(hint("Enregistrement… parle comme en live.")).child(
                button("listen-stop".into(), "Arrêter".into(), false).on_click(cx.listener(move |_, _, _, _| stop.store(true, Ordering::Relaxed))),
            );
        } else {
            actions = actions.child(button("listen-rec".into(), if self.voice.take.is_some() { "Refaire".into() } else { "Enregistrer 8 s".into() }, true)
                .on_click(cx.listener(|this, _, _, cx| this.record_take(cx))));
            if self.voice.take.is_some() {
                actions = actions.child(button("listen-play".into(), if self.voice.playing { "Lecture…".into() } else { "Réécouter".into() }, false)
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
        let mut p = panel().child(crate::ui::heading("Ta voix et le préréglage"));
        let Some(info) = self.voice.info.as_ref() else {
            return p.child(hint(if self.voice.loading { "Lecture…" } else { "Crée ton empreinte pour voir ta voix." }));
        };
        let chart = &info["profiles"][preset]["chart"];
        let (Some(bands), Some(voice), Some(after)) = (chart["bands"].as_array(), chart["voice"].as_array(), chart["after"].as_array()) else {
            return p.child(hint("Crée ton empreinte vocale pour voir ta voix avant et après le préréglage."));
        };
        let values: Vec<f64> = voice.iter().chain(after.iter()).filter_map(Value::as_f64).collect();
        let (lo, hi) = values.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let span = (hi - lo).max(1.0);
        let height = |v: Option<f64>| px((((v.unwrap_or(lo) - lo) / span) * 110.0 + 4.0) as f32);
        let step = (bands.len() / 24).max(1);
        let mut bars = div().flex().items_end().gap_1().h(px(130.));
        for i in (0..bands.len()).step_by(step) {
            bars = bars.child(div().flex().items_end().gap(px(1.))
                .child(div().w(px(5.)).h(height(voice.get(i).and_then(Value::as_f64))).bg(rgb(0xc7c7cc)).rounded_sm())
                .child(div().w(px(5.)).h(height(after.get(i).and_then(Value::as_f64))).bg(rgb(0x0071e3)).rounded_sm()));
        }
        let first = bands.first().and_then(Value::as_f64).unwrap_or(50.);
        let last = bands.last().and_then(Value::as_f64).unwrap_or(16000.);
        p = p.child(bars).child(hint(format!("{} Hz → {} kHz · gris : ta voix · bleu : avec le préréglage", first.round(), (last / 1000.).round())));
        for s in info["summary"].as_object().into_iter().flatten().take(6) {
            if let Some(t) = s.1.as_str() {
                p = p.child(hint(t.to_owned()));
            }
        }
        p
    }
}
