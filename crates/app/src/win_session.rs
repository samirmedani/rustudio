use crate::{
    backend::Client,
    desktop::hint,
    mixer::Mixer,
    ui::{self, Icon},
};
use gpui::{Context, Div, div, prelude::*, px};
use rustudio_windows_desktop::{audio, osd, soundcheck};
use serde_json::{Value, json};

pub fn inventory(live: bool) -> Value {
    if live {
        let mut v = audio::inventory();
        v["issues"] = audio::status()["issues"].clone();
        return v;
    }
    json!({"status":"ok","outputs":[
        {"id":"demo-system","name":"System (GoXLR de démonstration)","default":true,"communications":false},
        {"id":"demo-chat","name":"Chat (GoXLR de démonstration)","default":false,"communications":true},
        {"id":"demo-game","name":"Game (GoXLR de démonstration)"},
        {"id":"demo-music","name":"Music (GoXLR de démonstration)"},
        {"id":"demo-sample","name":"Sample (GoXLR de démonstration)"}],
        "inputs":[{"id":"demo-mic","name":"Chat Mic (GoXLR de démonstration)","default":true,"communications":true},
        {"id":"demo-stream","name":"Broadcast Stream Mix (GoXLR de démonstration)"},
        {"id":"demo-vod","name":"VOD (GoXLR de démonstration)"}]})
}

#[derive(Default)]
pub struct WinPage {
    pub inventory: Option<Value>,
    busy: bool,
    confirm: bool,
    pub(crate) note: Option<String>,
    pub(crate) monitor: Option<rustudio_windows_desktop::spectrum::Monitor>,
    pub(crate) frame: rustudio_windows_desktop::spectrum::Frame,
    pub(crate) selected: Option<rustudio_windows_desktop::spectrum::Source>,
    pub(crate) paused: bool,
    pub(crate) guide_hidden: bool,
    active: bool,
}
impl WinPage {
    pub fn cancel(&mut self) {
        self.confirm = false;
    }
    pub fn receive(&mut self,data:Value) {self.inventory=Some(data);self.sync_monitor();}
    pub(crate) fn sync_monitor(&mut self) {
        use rustudio_windows_desktop::spectrum::{Source,Frame};
        let mut sources=vec![];let mut preferred=None;
        if let Some(data)=&self.inventory {
            for(key,output)in [("outputs",true),("inputs",false)] {
                for row in data[key].as_array().into_iter().flatten(){
                    let Some(id)=row["id"].as_str()else{continue};let s=Source{id:id.into(),output};
                    if preferred.is_none()||output&&row["default"]==true {preferred=Some(s.clone());}
                    sources.push(s);
                }
            }
        }
        if !self.selected.as_ref().is_some_and(|s|sources.contains(s)) {self.selected=preferred;self.frame=Frame::default();}
        if let Some(monitor)=&self.monitor {
            if self.active&&!self.paused {monitor.configure(sources,self.selected.clone());}else{monitor.stop();}
        }
    }
}

impl Mixer {
    pub(crate) fn audio_visibility(&mut self,visible:bool,cx:&mut Context<Self>) {
        self.win.active=visible;
        if visible&&self.win.monitor.is_none() {
            self.win.monitor=Some(rustudio_windows_desktop::spectrum::Monitor::new(!self.client.is_live()));
            let executor=cx.background_executor().clone();
            cx.spawn(async move|this,cx|{
                loop {executor.timer(std::time::Duration::from_millis(50)).await;
                    if this.update(cx,|this,cx|{
                        if this.page!=crate::desktop::Page::WindowsAudio{return;}
                        if let Some(m)=&this.win.monitor {
                            let mut frame=m.frame();
                            if frame.version!=this.win.frame.version {
                                if frame.selected!=this.win.selected {frame.ready=false;frame.bands.fill(rustudio_windows_desktop::spectrum::FLOOR);frame.peak_db=rustudio_windows_desktop::spectrum::FLOOR;}
                                this.win.frame=frame;cx.notify();
                            }
                        }
                    }).is_err(){break;}
                }
            }).detach();
        }
        self.win.sync_monitor();
    }
    /// Demande de confirmation du son Windows, en fenêtre par-dessus la page.
    pub(crate) fn win_confirm_box(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.win.confirm {
            return None;
        }
        let cancel = ui::pill("windows-fix-cancel", None, "Annuler", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
            this.win.cancel();
            cx.notify();
        }));
        let confirm = ui::pill("windows-fix-ok", None, "Corriger", ui::Tone::Primary).on_click(cx.listener(|this, _, _, cx| this.fix_audio(cx)));
        Some(ui::dialog(
            Icon::Alert,
            ui::ORANGE,
            "Corriger le son de Windows ?",
            vec![
                "Windows utilisera Chat Mic comme micro. Les sorties déjà sur la GoXLR sont gardées ; les autres passent sur System, ou Chat pour les appels.".into(),
                "Les choix propres à Discord et OBS ne changent pas.".into(),
            ],
            vec![cancel.into_any_element(), confirm.into_any_element()],
        ))
    }

    pub fn win_audio(&self, window: &gpui::Window, cx: &mut Context<Self>) -> Div {
        let mut root = div().w_full().min_w(px(0.)).whitespace_normal().flex().flex_col().gap(px(16.));
        for issue in self.win.inventory.as_ref().and_then(|v| v["issues"].as_array()).into_iter().flatten() {
            let name = issue["name"].as_str().unwrap_or("un autre périphérique");
            let role = if issue["role"] == "communications" { "pour les appels" } else { "par défaut" };
            let text = match issue["kind"].as_str() {
                Some("capture-loop") => format!("Windows utilise « {name} » {role} comme micro : tes amis peuvent recevoir le mix du stream ou un écho. Pour leur envoyer ton micro, choisis Chat Mic."),
                Some("capture-other") => format!("Le micro de Windows {role} est « {name} » : la GoXLR n’est pas utilisée."),
                Some("capture-none") => format!("Aucun micro Windows {role}."),
                Some("render-other") => format!("Le son {role} part sur « {name} » au lieu de la GoXLR."),
                _ => format!("Aucune sortie Windows {role}."),
            };
            root = root.child(ui::notice(false, text));
        }
        root=root.child(self.audio_guide(f32::from(window.viewport_size().width)>1240.,cx)).child(self.audio_spectrum(cx));
        let mut cards: Vec<gpui::AnyElement> = vec![];
        if let Some(data) = self.win.inventory.as_ref().filter(|v| v["status"] == "ok") {
            for (key, label, glyph) in [("outputs", "Sorties · du PC vers ton casque", Icon::Headphones), ("inputs", "Entrées · vers tes applis", Icon::Mic)] {
                let mut group = ui::section(glyph, ui::ACCENT, label, None).w_full().min_w(px(0.)).gap(px(10.));
                group=group.child(ui::caption(if key=="outputs"{"Le PC envoie le son vers ces voies. Clique une ligne pour voir son spectre."}else{"Choisis une entrée pour la tester, même si aucune appli ne l’utilise encore."}));
                for (n, row) in data[key].as_array().into_iter().flatten().enumerate() {
                    group = group.child(self.audio_endpoint(row,key=="outputs",n,cx));
                }
                if data[key].as_array().is_none_or(Vec::is_empty) {
                    group = group.child(hint("Aucun périphérique actif."));
                }
                cards.push(group.into_any_element());
            }
        } else {
            cards.push(ui::card().child(hint("Inventaire audio indisponible. Nouvelle lecture automatique.")).into_any_element());
        }
        root = root.child(div().grid().grid_cols(2).gap(px(16.)).children(cards));
        if self.client.is_live() {
            root = root.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(ui::pill("windows-open", Some(Icon::External), "Paramètres de son de Windows", ui::Tone::Plain).on_click(cx.listener(|_, _, _, cx| cx.open_url("ms-settings:sound"))))
                    .when(!self.win.busy, |d| {
                        d.child(ui::pill("windows-fix", Some(Icon::Check), "Configurer Windows pour la GoXLR", ui::Tone::Primary).on_click(cx.listener(|this, _, _, cx| {
                            this.win.confirm = true;
                            cx.notify();
                        })))
                    }),
            );
        }
        if let Some(note)=&self.win.note {root=root.child(ui::hint(note.clone()));}
        root
    }

    fn fix_audio(&mut self, cx: &mut Context<Self>) {
        if !self.client.is_live() || self.win.busy {
            return;
        }
        self.win.confirm = false;
        self.win.busy = true;
        let work = cx
            .background_executor()
            .spawn(async move { (audio::fix(), audio::inventory()) });
        cx.spawn(async move |this, cx| {
            let (n, state) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.win.busy = false;
                this.win.receive(state);
                this.win.note = Some(match n {
                    -1 => "Windows ne voit pas les voies nécessaires de la GoXLR.".into(),
                    0 => "Les périphériques par défaut sont déjà corrects.".into(),
                    _ => format!("{n} réglage(s) audio mis à jour."),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

pub fn sound_test(client: &Client) -> Result<Value, String> {
    if !client.is_live() {
        return Ok(json!({"simulated":true,"scan":{"channels":[
            {"name":"Entrée de démonstration","status":"ok","peak_db":-18.0},
            {"name":"Sortie de démonstration","status":"ok","peak_db":-24.0}]},
            "loop":{"status":"ok","dropouts":0,"jumps":0}}));
    }
    client.live_state()?;
    let route = client.post_module(
        "/api/modules/goxlr-mini",
        "sound-test",
        json!({"seconds":22}),
    )?;
    if route["ok"] != true {
        return Err("Le moteur n’a pas isolé la voie Samples.".into());
    }
    // La durée côté moteur assure aussi le retour au routage normal si l’application se ferme.
    let loop_result = soundcheck::loop_test(5., false);
    let restored = client.post_module(
        "/api/modules/goxlr-mini",
        "sound-test",
        json!({"seconds":0}),
    );
    restored?;
    let scan = soundcheck::run(5.);
    Ok(json!({"simulated":false,"scan":scan,"loop":loop_result}))
}

/// Mesure aller-retour : la voie Sample est isolée vers le Stream Mix le temps des trois balayages.
pub fn latency_test(client: &Client) -> Result<Value, String> {
    if !client.is_live() {
        return Ok(json!({"simulated": true, "status": "ok", "total_ms": 27.4, "out_ms": 16.2, "back_ms": 11.2,
                         "min_ms": 26.9, "max_ms": 28.1, "trials": 3}));
    }
    client.live_state()?;
    let route = client.post_module("/api/modules/goxlr-mini", "sound-test", json!({"seconds": 10}))?;
    if route["ok"] != true {
        return Err("Le moteur n’a pas isolé la voie Samples.".into());
    }
    // la durée côté moteur rétablit aussi le routage si l’application se ferme pendant la mesure
    let result = soundcheck::latency_test();
    client.post_module("/api/modules/goxlr-mini", "sound-test", json!({"seconds": 0}))?;
    Ok(result)
}

pub fn show_mic(state: &str) {
    let muted = !state.is_empty();
    let subtitle = match state {
        "stream" => "Coupé du stream",
        "chat" => "Coupé de Discord",
        "phones" => "Coupé du casque",
        "lineout" => "Coupé des enceintes",
        "all" => "Toutes les sorties",
        _ => "Micro ouvert",
    };
    osd::show(osd::Notice {
        title: if muted { "Micro coupé" } else { "Micro actif" }.into(),
        subtitle: subtitle.into(),
        rgb: if muted { 0xff3b30 } else { 0x34c759 },
        icon: if muted { "mic-off" } else { "mic" },
    });
}
