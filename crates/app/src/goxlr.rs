//! Les écrans du module GoXLR utilisent le même contrat local que l'interface distribuée.
use crate::{
    backend::Client,
    desktop::{Page, hint, panel},
    mixer::{Mixer, button},
};
use crate::ui::{self, Icon};
use gpui::{ClipboardItem, Context, Div, IntoElement, KeyDownEvent, MouseButton, Stateful, div, prelude::*, px, rgb};
use serde::Deserialize;
use serde_json::{Value, json};

const MODULE: &str = "/api/modules/goxlr-mini";
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GoView {
    pub device: crate::backend::Device,
    pub state: Value,
    pub meta: Value,
    #[serde(default)]
    job: Value,
}
#[derive(Default)]
pub struct GoPage {
    pub view: Option<GoView>,
    pub(crate) busy: bool,
    pub(crate) notice: Option<(bool, String)>,
    pub(crate) voice_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) job: Value,
    confirm: Option<(&'static str, Value, String)>,
    editing: Option<usize>,
    name: String,
    app_name: String,
    auto_scene: Option<String>,
    colour: String,
    diagnostics: Option<Value>,
    sound_test: Option<Value>,
}
impl Drop for GoPage {
    fn drop(&mut self) {
        if let Some(cancel) = self.voice_cancel.as_ref() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl GoPage {
    pub fn receive(&mut self, view: Option<GoView>) {
        if let Some(v) = view.as_ref() {
            self.job = v.job.clone();
        }
        if !self.busy {
            self.view = view;
        }
    }
    pub fn clear_confirm(&mut self) {
        self.confirm = None;
    }
    pub(crate) fn enabled(&self) -> bool {
        !self.busy
            && self
                .view
                .as_ref()
                .is_some_and(|v| v.device.status == "ready")
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
            "echo":self.go_command("echo-check",json!({}))?}),
        )
    }
}

fn group(title: &str) -> Div {
    panel().child(ui::card_title(None, title.to_owned(), None))
}
fn entries(v: &Value) -> impl Iterator<Item = (&String, &Value)> {
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
fn port_label(v: &str) -> &str {
    match v {
        "Microphone" => "Micro",
        "Music" => "Musique",
        "System" => "Système",
        "Game" => "Jeu",
        "Headphones" => "Casque",
        "BroadcastMix" => "Stream",
        "StreamMix2" => "Stream 2",
        "ChatMic" => "Discord",
        "LineOut" => "Sortie ligne",
        "LineIn" => "Entrée ligne",
        "Samples" => "Samples",
        _ => v,
    }
}

impl Mixer {
    fn go_send(&mut self, action: &'static str, data: Value, cx: &mut Context<Self>) {
        if self.go.busy {
            return;
        }
        if !self.go.enabled() && !matches!(action, "reconnect" | "job-cancel") {
            return;
        }
        self.go.busy = true;
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
                this.go.notice = Some(match result {
                    Ok(_) => (true, "Commande acceptée par le moteur.".into()),
                    Err(error) => (false, error),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_diag(&mut self, cx: &mut Context<Self>) {
        if self.go.busy {
            return;
        }
        self.go.busy = true;
        let client = self.client.clone();
        let job = cx
            .background_executor()
            .spawn(async move { client.diagnostics() });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.go.busy = false;
                match result {
                    Ok(data) => {
                        this.go.diagnostics = Some(data);
                        this.go.notice = None;
                    }
                    Err(err) => this.go.notice = Some((false, err)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn control(
        &self,
        id: impl Into<String>,
        label: impl Into<String>,
        selected: bool,
        action: &'static str,
        body: Value,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let enabled = self.go.enabled() || matches!(action, "reconnect" | "job-cancel");
        let label = label.into();
        button(id.into(), label, selected)
            .when(!enabled, |d| d.opacity(0.45))
            .on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    this.go_send(action, body.clone(), cx)
                }
            }))
    }

    fn confirm_control(
        &self,
        id: impl Into<String>,
        label: impl Into<String>,
        action: &'static str,
        body: Value,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let message = message.into();
        button(id.into(), label.into(), false)
            .when(!self.go.enabled(), |d| d.opacity(0.45))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.go.enabled() {
                    this.go.confirm = Some((action, body.clone(), message.clone()));
                    cx.notify();
                }
            }))
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
            .min_w(px(200.))
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

    fn number(
        &self,
        id: &str,
        label: &str,
        value: f64,
        step: f64,
        range: (f64, f64),
        action: &'static str,
        payload: impl Fn(f64) -> Value,
        cx: &mut Context<Self>,
    ) -> Div {
        let round = |n: f64| (n * 100.).round() / 100.;
        let a = payload(round((value - step).max(range.0)));
        let b = payload(round((value + step).min(range.1)));
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(hint(label.to_owned()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.control(format!("{id}-less"), "−", false, action, a, cx))
                    .child(div().w(px(65.)).child(format!("{value}")))
                    .child(self.control(format!("{id}-more"), "+", false, action, b, cx)),
            )
    }

    fn routing(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let mut matrix = group("Matrice du routage")
            .child(hint("Choisis les sorties qui reçoivent chaque entrée."));
        let outputs: Vec<_> = labels(&v.meta["outputs"]).collect();
        let head = |text: String| div().flex_1().min_w(px(0.)).text_sm().child(text);
        matrix = matrix.child(
            div()
                .flex()
                .gap_1()
                .child(div().w(px(100.)))
                .children(outputs.iter().map(|o| head(port_label(o).into()))),
        );
        for input in labels(&v.meta["inputs"]) {
            let mut row = div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().w(px(100.)).child(port_label(input).to_owned()));
            for &output in &outputs {
                let enabled = v.state["routing"][input][output] == true;
                let automatic =
                    v.state["vod"]["mode"] == "no-music" && v.meta["vod_output"] == output;
                row = row.child(div().flex_1().min_w(px(0.)).child(if automatic {
                    hint("Auto").into_any_element()
                } else {
                    self.control(
                        format!("route-{input}-{output}"),
                        if enabled { "Oui" } else { "Non" },
                        enabled,
                        "route",
                        json!({"input":input,"output":output,"enabled":!enabled}),
                        cx,
                    )
                    .into_any_element()
                }));
            }
            matrix = matrix.child(row);
        }
        let vod = v.state["vod"]["mode"] == "no-music";
        matrix.child(self.control(
            "vod",
            if vod {
                "VOD sans musique · active"
            } else {
                "Activer VOD sans musique"
            },
            vod,
            "settings",
            json!({"vod":{"mode":if vod{"off"}else{"no-music"}}}),
            cx,
        ))
    }

    fn scenes(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let scenes: Vec<(String, String)> = entries(&v.meta["scenes"])
            .map(|(id, s)| (id.clone(), s["label"].as_str().unwrap_or(id).to_owned()))
            .chain(entries(&v.state["user_scenes"]).map(|(id, _)| (id.clone(), id.clone())))
            .collect();
        let mut list = group("Scènes");
        for (name, label) in &scenes {
            let selected = v.state["scene"] == name.as_str();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(hint(label.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.control(
                                format!("scene-{name}"),
                                "Appliquer",
                                selected,
                                "scene",
                                json!({"name":name}),
                                cx,
                            ))
                            .when(v.state["user_scenes"].get(name).is_some(), |d| {
                                d.child(self.confirm_control(
                                    format!("scene-delete-{name}"),
                                    "Supprimer",
                                    "scene-delete",
                                    json!({"name":name}),
                                    "Supprimer cette scène personnalisée ?",
                                    cx,
                                ))
                            }),
                    ),
            );
        }
        list = list.child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(self.go_text("name", "Nom de la scène", cx))
                .child(self.control(
                    "scene-save",
                    "Enregistrer une scène",
                    false,
                    "scene-save",
                    json!({"name":self.go.name}),
                    cx,
                )),
        );
        let auto = &v.state["auto_scenes"];
        let on = auto["enabled"] == true;
        let rules = auto["rules"].as_array().cloned().unwrap_or_default();
        let fallback = auto["fallback"].clone();
        let mut automation = group("Scènes automatiques").child(self.control(
            "auto-toggle",
            if on { "Actives" } else { "Désactivées" },
            on,
            "auto-scenes",
            json!({"enabled":!on,"rules":rules,"fallback":fallback}),
            cx,
        ));
        for (i, rule) in rules.iter().enumerate() {
            let mut remaining = rules.clone();
            remaining.remove(i);
            automation = automation.child(
                div()
                    .flex()
                    .justify_between()
                    .child(hint(format!(
                        "{} → {}",
                        rule["app"].as_str().unwrap_or(""),
                        rule["scene"].as_str().unwrap_or("")
                    )))
                    .child(self.control(
                        format!("auto-delete-{i}"),
                        "Retirer",
                        false,
                        "auto-scenes",
                        json!({"enabled":on,"rules":remaining,"fallback":fallback}),
                        cx,
                    )),
            );
        }
        let mut selection = div().flex().flex_wrap().gap_2();
        let mut fallbacks = div().flex().flex_wrap().gap_2().child(self.control(
            "auto-fallback-none",
            "Ne rien changer",
            fallback.is_null(),
            "auto-scenes",
            json!({"enabled":on,"rules":rules,"fallback":null}),
            cx,
        ));
        for (id, label) in &scenes {
            let picked = id.clone();
            selection = selection.child(
                button(
                    format!("auto-pick-{id}"),
                    label.clone(),
                    self.go.auto_scene.as_ref() == Some(id),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.go.auto_scene = Some(picked.clone());
                    cx.notify();
                })),
            );
            fallbacks = fallbacks.child(self.control(
                format!("auto-fallback-{id}"),
                label.clone(),
                fallback == id.as_str(),
                "auto-scenes",
                json!({"enabled":on,"rules":rules,"fallback":id}),
                cx,
            ));
        }
        automation = automation
            .child(hint("Programme (ex. obs64.exe) et scène à déclencher"))
            .child(self.go_text("app", "Nom du programme", cx))
            .child(selection);
        if let Some(scene) = self
            .go
            .auto_scene
            .as_ref()
            .filter(|_| !self.go.app_name.trim().is_empty())
        {
            let mut next = rules.clone();
            next.push(json!({"app":self.go.app_name.trim(),"scene":scene}));
            automation = automation.child(self.control(
                "auto-add",
                "Ajouter la règle",
                false,
                "auto-scenes",
                json!({"enabled":on,"rules":next,"fallback":fallback}),
                cx,
            ));
        }
        ui::columns(vec![list.into_any_element(), automation.child(hint("Scène de secours")).child(fallbacks).into_any_element()])
    }

    fn lighting(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let s = &v.state["lighting"];
        let mut themes = div().flex().flex_wrap().gap_2();
        for (id, t) in entries(&v.meta["themes"]) {
            themes = themes.child(self.control(
                format!("theme-{id}"),
                t["label"].as_str().unwrap_or(id),
                s["theme"] == id.as_str(),
                "theme",
                json!({"name":id}),
                cx,
            ));
        }
        let mut changes = group("Thèmes et animations").child(themes);
        for (field, label, value) in [
            ("fader_style", "Style des faders", &s["fader_style"]["A"]),
            ("animation", "Animation", &s["animation"]),
        ] {
            let mut buttons = div().flex().flex_wrap().gap_2();
            for opt in labels(
                &v.meta[if field == "animation" {
                    "animations"
                } else {
                    "fader_styles"
                }],
            ) {
                buttons = buttons.child(self.control(
                    format!("light-{field}-{opt}"),
                    light_label(opt),
                    value == opt,
                    "lighting",
                    json!({(field):opt}),
                    cx,
                ));
            }
            changes = changes.child(hint(label)).child(buttons);
        }
        for (key, label) in [("mod1", "Amplitude (%)"), ("mod2", "Vitesse (%)")] {
            changes = changes.child(self.number(
                key,
                label,
                s[key].as_f64().unwrap_or(50.),
                5.,
                (0., 100.),
                "lighting",
                |n| json!({(key):n as i32}),
                cx,
            ));
        }
        let mut colour = group("Couleurs par bouton")
            .child(hint(
                "Entre une couleur hexadécimale sur six caractères, puis choisis son emplacement.",
            ))
            .child(self.go_text("colour", "Ex. 64D2FF", cx));
        let hex = self.go.colour.trim().trim_start_matches('#');
        let valid = hex.len() == 6 && hex.bytes().all(|c| c.is_ascii_hexdigit());
        for (id, values) in entries(&v.state["colours"]) {
            let mut row = div()
                .flex()
                .items_center()
                .justify_between()
                .child(hint(colour_target(id)));
            let mut positions = div().flex().gap_2();
            for (i, current) in values.as_array().into_iter().flatten().enumerate().take(2) {
                let current = current.as_str().unwrap_or("000000");
                let fill = u32::from_str_radix(current, 16).unwrap_or(0);
                positions = positions.child(div().w(px(22.)).h(px(22.)).rounded_md().bg(rgb(fill)));
                if valid {
                    let values = if i == 0 {
                        json!([hex, null])
                    } else {
                        json!([null, hex])
                    };
                    positions = positions.child(self.control(
                        format!("colour-{id}-{i}"),
                        format!("Couleur {}", i + 1),
                        false,
                        "colour",
                        json!({"target":id,"colours":values}),
                        cx,
                    ));
                }
            }
            row = row.child(positions);
            colour = colour.child(row);
        }
        ui::columns(vec![changes.into_any_element(), colour.into_any_element()])
    }

    fn microphone(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let s = &v.state;
        let mic = &s["mic"];
        let mut kinds = div().flex().flex_wrap().gap_2();
        for (kind, name) in [
            ("dynamic", "Dynamique"),
            ("condenser", "Condensateur · 48 V"),
            ("jack", "Jack"),
        ] {
            kinds=kinds.child(if kind=="condenser" && mic["kind"]!=kind && mic["type"]!=kind {
                self.confirm_control(format!("mic-{kind}"),name,"mic",json!({"type":kind,"suggest_gain":true}),
                    "Activer le 48 V ? Vérifie que ton micro et ton câblage le prennent en charge.",cx)
            }else{self.control(format!("mic-{kind}"),name,mic["kind"]==kind||mic["type"]==kind,"mic",json!({"type":kind,"suggest_gain":true}),cx)});
        }
        let mut models = div().flex().flex_wrap().gap_2();
        for (id, m) in entries(&v.meta["mic_models"]) {
            models = models.child(self.control(
                format!("mic-model-{id}"),
                m["label"].as_str().or(m["name"].as_str()).unwrap_or(id),
                mic["model"] == id.as_str(),
                "mic-model",
                json!({"model":id}),
                cx,
            ));
        }
        let mut presets = div().flex().flex_wrap().gap_2();
        for (id, m) in entries(&v.meta["mic_presets"]) {
            presets = presets.child(self.control(
                format!("mic-preset-{id}"),
                m["label"].as_str().unwrap_or(id),
                s["mic_preset"] == id.as_str(),
                "mic-preset",
                json!({"name":id}),
                cx,
            ));
        }
        let muted = s["mutes"]["Mic"] == true || s["partial"]["Mic"].is_string();
        let mut modes = div().flex().flex_wrap().gap_2();
        for (mode, label) in [
            ("toggle", "Bascule"),
            ("hold", "Couper en maintenant"),
            ("ptt", "Parler en maintenant"),
        ] {
            modes = modes.child(self.control(
                format!("mic-button-{mode}"),
                label,
                s["mic_button"]["mode"] == mode,
                "settings",
                json!({"mic_button":{"mode":mode}}),
                cx,
            ));
        }
        let mut targets = div().flex().flex_wrap().gap_2();
        for &(target, label) in &rustudio_core::consts::MUTE_TARGET_LABELS {
            targets = targets.child(self.control(
                format!("mic-target-{target}"),
                label,
                s["mic_button"]["target"] == target,
                "settings",
                json!({"mic_button":{"target":target}}),
                cx,
            ));
        }
        let blink = s["mic_button"]["blink"] == true;
        let input = group("Entrée et gain")
            .child(kinds)
            .child(models)
            .child(self.number(
                "mic-gain",
                "Gain (dB)",
                mic["gain_db"].as_f64().unwrap_or(0.),
                1.,
                (0., 72.),
                "mic",
                |n| json!({"gain_db":n as i32}),
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.control(
                        "mic-mute",
                        if muted {
                            "Réactiver"
                        } else {
                            "Couper le micro"
                        },
                        muted,
                        "mute",
                        json!({"channel":"Mic","muted":!muted,"target":"all"}),
                        cx,
                    ))
                    .child(self.control(
                        "mic-detect",
                        "Détecter le type",
                        false,
                        "detect-mic",
                        json!({}),
                        cx,
                    ))
                    .child(self.control(
                        "mic-auto-gain",
                        "Caler le gain",
                        false,
                        "auto-gain",
                        json!({}),
                        cx,
                    ))
                    .child(self.control(
                        "mic-voice-setup",
                        "Assistant voix",
                        false,
                        "voice-setup",
                        json!({}),
                        cx,
                    ))
                    .child(self.control(
                        "mic-job-cancel",
                        "Annuler la mesure",
                        false,
                        "job-cancel",
                        json!({}),
                        cx,
                    )),
            )
            .child(hint("Bouton physique du micro"))
            .child(modes)
            .child(targets)
            .child(self.control(
                "mic-blink",
                if blink {
                    "Clignotement actif"
                } else {
                    "Clignotement désactivé"
                },
                blink,
                "settings",
                json!({"mic_button":{"blink":!blink}}),
                cx,
            ));
        let mut fx = group("Voix et traitements").child(presets);
        let mut toggles = div().flex().flex_wrap().gap_2();
        for (key, label) in [
            ("eq", "Égaliseur"),
            ("gate", "Noise gate"),
            ("comp", "Compresseur"),
            ("deesser", "De-esser"),
        ] {
            let on = s["fx"][key] == true;
            toggles = toggles.child(self.control(
                format!("fx-{key}"),
                format!("{label} · {}", if on { "actif" } else { "arrêté" }),
                on,
                "fx",
                json!({(key):!on}),
                cx,
            ));
        }
        fx = fx.child(toggles);
        let dsp = &s["dsp"];
        let mut bands = group("Égaliseur du micro");
        for (band, val) in entries(&dsp["eq"]) {
            let f = dsp["eq_freq"][band]
                .as_f64()
                .or_else(|| band.parse().ok())
                .unwrap_or(100.);
            let r = &v.meta["eq_ranges"][band];
            bands = bands.child(
                div()
                    .flex()
                    .gap_4()
                    .child(div().flex_1().child(self.number(
                        &format!("dsp-{band}"),
                        "Gain (dB)",
                        val.as_f64().unwrap_or(0.),
                        1.,
                        (-9., 9.),
                        "dsp",
                        |n| json!({"eq":{(band):n as i32}}),
                        cx,
                    )))
                    .child(div().flex_1().child(self.number(
                        &format!("dsp-hz-{band}"),
                        "Fréquence (Hz)",
                        f,
                        10.,
                        (r[0].as_f64().unwrap_or(f), r[1].as_f64().unwrap_or(f)),
                        "dsp",
                        |n| json!({"eq_freq":{(band):n}}),
                        cx,
                    ))),
            );
        }
        let mut dynamics = group("Dynamique et retour");
        for (group, key, label, min, max) in [
            ("gate", "threshold", "Seuil de la porte (dB)", -59., 0.),
            ("gate", "attenuation", "Réduction de la porte (%)", 0., 100.),
            ("comp", "threshold", "Seuil du compresseur (dB)", -40., 0.),
            ("comp", "ratio", "Ratio du compresseur", 0., 14.),
            ("comp", "makeup", "Compensation (dB)", -6., 24.),
        ] {
            dynamics = dynamics.child(self.number(
                &format!("{group}-{key}"),
                label,
                dsp[group][key].as_f64().unwrap_or(0.),
                1.,
                (min, max),
                "dsp",
                |n| json!({(group):{(key):n as i32}}),
                cx,
            ));
        }
        dynamics = dynamics
            .child(self.number(
                "deess",
                "De-esser (%)",
                dsp["deesser"].as_f64().unwrap_or(0.),
                5.,
                (0., 100.),
                "dsp",
                |n| json!({"deesser":n as i32}),
                cx,
            ))
            .child(self.number(
                "bleep",
                "Volume du bip (dB)",
                s["bleep_db"].as_f64().unwrap_or(0.),
                1.,
                (-36., 0.),
                "dsp",
                |n| json!({"bleep_db":n as i32}),
                cx,
            ))
            .child(self.number(
                "monitor",
                "Volume du retour (0–255)",
                s["volumes"]["MicMonitor"].as_f64().unwrap_or(0.),
                5.,
                (0., 255.),
                "volume",
                |n| json!({"channel":"MicMonitor","value":n as i32}),
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.control(
                        "mic-listen",
                        "Retour direct · 30 s",
                        false,
                        "mic-listen",
                        json!({"seconds":30}),
                        cx,
                    ))
                    .child(self.control(
                        "mic-listen-stop",
                        "Arrêter le retour",
                        false,
                        "mic-listen",
                        json!({"seconds":0}),
                        cx,
                    ))
                    .child(self.control(
                        "mic-ab-raw",
                        "Écouter la voix brute",
                        false,
                        "mic-ab",
                        json!({"bypass":true}),
                        cx,
                    ))
                    .child(self.control(
                        "mic-ab-processed",
                        "Rétablir les traitements",
                        false,
                        "mic-ab",
                        json!({"bypass":false}),
                        cx,
                    )),
            )
            .when(s["voice_print"].is_object(), |d| {
                d.child(self.confirm_control(
                    "voice-forget",
                    "Oublier l’empreinte",
                    "voice-forget",
                    json!({}),
                    "Effacer l’empreinte vocale de ce micro ?",
                    cx,
                ))
            });
        let chart = self.voice_chart(s["mic_preset"].as_str().filter(|p| *p != "brut" && *p != "custom").unwrap_or("studio"));
        ui::columns(vec![
            input.into_any_element(),
            self.voice_panel(cx).into_any_element(),
            fx.into_any_element(),
            chart.into_any_element(),
            bands.into_any_element(),
            self.listen_panel(cx).into_any_element(),
            dynamics.into_any_element(),
        ])
    }

    fn headphones(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let eq = &v.state["hp_eq"];
        let enabled = eq["enabled"] == true;
        let paused = eq["pause_live"] == true;
        let mut model = div().flex().flex_wrap().gap_2().child(self.control(
            "hp-none",
            "Aucune correction",
            eq["headphone"].is_null(),
            "hp-eq",
            json!({"headphone":null}),
            cx,
        ));
        for (name, _) in entries(&v.meta["headphones"]) {
            model = model.child(self.control(
                format!("hp-model-{name}"),
                name.clone(),
                eq["headphone"] == name.as_str(),
                "hp-eq",
                json!({"headphone":name}),
                cx,
            ));
        }
        let mut styles = div().flex().flex_wrap().gap_2();
        for (id, s) in entries(&v.meta["hp_styles"]) {
            if let Some(required) = s["headphone"].as_str() {
                if eq["headphone"] != required {
                    continue;
                }
            }
            styles = styles.child(self.control(
                format!("hp-style-{id}"),
                s["label"].as_str().unwrap_or(id),
                eq["style"] == id.as_str(),
                "hp-eq",
                json!({"style":id}),
                cx,
            ));
        }
        let mut custom = group("Profils de casque").child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(self.go_text("name", "Nom du profil casque", cx))
                .child(self.control(
                    "hp-save",
                    "Enregistrer",
                    false,
                    "hp-eq-save",
                    json!({"name":self.go.name}),
                    cx,
                )),
        );
        for (name, _) in entries(&eq["custom"]) {
            let style = format!("custom:{name}");
            custom = custom.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(hint(name.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.control(
                                format!("hp-apply-{name}"),
                                "Appliquer",
                                eq["style"] == style.as_str(),
                                "hp-eq",
                                json!({"style":style}),
                                cx,
                            ))
                            .child(self.confirm_control(
                                format!("hp-delete-{name}"),
                                "Supprimer",
                                "hp-eq-delete",
                                json!({"name":name}),
                                "Supprimer ce profil de casque ?",
                                cx,
                            )),
                    ),
            );
        }
        let mut filters = group("Égaliseur 10 bandes");
        let values = eq["bands"].as_array().cloned().unwrap_or_default();
        for (i, hz) in v.meta["hp_bands"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if let Some(current) = values.get(i).and_then(Value::as_f64) {
                filters = filters.child(self.number(
                    &format!("hp-band-{i}"),
                    &format!("{hz} Hz"),
                    current,
                    0.5,
                    (-12., 12.),
                    "hp-eq",
                    |n| {
                        let mut values = values.clone();
                        values[i] = json!(n);
                        json!({"bands":values})
                    },
                    cx,
                ));
            }
        }
        let mut channels = div().flex().flex_wrap().gap_2();
        for channel in labels(&v.meta["hp_channels"]) {
            let mut selected = eq["channels"].as_array().cloned().unwrap_or_default();
            let on = selected.iter().any(|v| v == channel);
            if on {
                selected.retain(|v| v != channel);
            } else {
                selected.push(json!(channel));
            }
            channels = channels.child(self.control(
                format!("hp-channel-{channel}"),
                port_label(channel),
                on,
                "hp-eq",
                json!({"channels":selected}),
                cx,
            ));
        }
        let main = group("Correction casque")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.control(
                        "hp-enabled",
                        if enabled {
                            "Égaliseur actif"
                        } else {
                            "Égaliseur désactivé"
                        },
                        enabled,
                        "hp-eq",
                        json!({"enabled":!enabled}),
                        cx,
                    ))
                    .child(self.control(
                        "hp-pause",
                        if paused {
                            "Pause en live active"
                        } else {
                            "Pause en live désactivée"
                        },
                        paused,
                        "hp-eq",
                        json!({"pause_live":!paused}),
                        cx,
                    ))
                    .child(self.confirm_control(
                        "hp-install",
                        "Installer l’égaliseur",
                        "hp-eq-setup",
                        json!({}),
                        "Installer le composant de correction casque sur Windows ?",
                        cx,
                    ))
                    .child(self.confirm_control(
                        "hp-remove",
                        "Retirer l’égaliseur",
                        "hp-eq-remove",
                        json!({}),
                        "Retirer le composant de correction casque ?",
                        cx,
                    )),
            )
            .child(hint("Voies corrigées"))
            .child(channels)
            .child(hint("Choisir un casque"))
            .child(model)
            .child(hint("Styles"))
            .child(styles);
        ui::columns(vec![main.into_any_element(), filters.into_any_element(), custom.into_any_element()])
    }

    fn run_sound_test(&mut self, cx: &mut Context<Self>) {
        if !self.go.enabled() {
            return;
        }
        self.go.busy = true;
        self.go.sound_test = None;
        let client = self.client.clone();
        let work = cx
            .background_executor()
            .spawn(async move { crate::win_session::sound_test(&client) });
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
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn diagnostic_view(&self, cx: &mut Context<Self>) -> Div {
        let url = self.client.overlay_url();
        let mut panel = group("Diagnostic et incrustation").child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    button("diag-read".into(), "Actualiser".into(), false)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_diag(cx))),
                )
                .child(self.control(
                    "diag-reconnect",
                    "Reconnecter la GoXLR",
                    false,
                    "reconnect",
                    json!({}),
                    cx,
                ))
                .child(
                    button("obs-url".into(), "Copier le lien OBS".into(), false).on_click(
                        cx.listener(move |_, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                        }),
                    ),
                ),
        );
        if let Some(d) = self.go.diagnostics.as_ref() {
            let report = &d["support"]["report"];
            let copy = serde_json::to_string_pretty(report).unwrap_or_default();
            panel = panel
                .child(hint(format!(
                    "Moteur : {} · Coupures USB (24 h) : {}",
                    report["moteur"]["etat"].as_str().unwrap_or("inconnu"),
                    report["coupures_usb_24h"]["nombre"]
                        .as_u64()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "non mesurées".into())
                )))
                .child(
                    button(
                        "diag-copy".into(),
                        "Copier le diagnostic anonymisé".into(),
                        false,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                    })),
                );
            if d["usb"]["found"] == true {
                panel = panel.child(hint(if d["usb"]["controller_cpu"] == true {
                    "Contrôleur USB du processeur"
                } else {
                    "Contrôleur USB à vérifier"
                }));
            }
            if d["glitches"]["readable"] == true {
                panel = panel.child(hint(format!(
                    "Journal Windows : {} évènement(s)",
                    d["glitches"]["total"]
                )));
            }
            for (i, check) in d["echo"]["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let label = check["title"]
                    .as_str()
                    .or(check["label"].as_str())
                    .or(check["id"].as_str())
                    .unwrap_or("Routage");
                let mut row = div().flex().justify_between().child(hint(format!(
                    "{} {label}",
                    if check["ok"] == true {
                        "✓"
                    } else {
                        "À vérifier :"
                    }
                )));
                let fix = &check["fix"];
                if let Some(action) = ACTIONS.iter().copied().find(|a| fix["action"] == *a) {
                    row = row.child(self.control(
                        format!("diag-fix-{i}"),
                        "Corriger",
                        false,
                        action,
                        fix["body"].clone(),
                        cx,
                    ));
                }
                panel = panel.child(row);
            }
        } else {
            panel = panel.child(hint("Actualise pour lire les données du moteur."));
        }
        panel = panel.child(
            button(
                "sound-test".into(),
                "Tester les voies audio et les coupures".into(),
                false,
            )
            .on_click(cx.listener(|this, _, _, cx| this.run_sound_test(cx))),
        );
        if let Some(result) = self.go.sound_test.as_ref() {
            panel = panel.child(hint(format!(
                "{}Boucle audio : {} · Coupures : {} · Sauts : {}",
                if result["simulated"] == true {
                    "Démonstration · "
                } else {
                    ""
                },
                result["loop"]["status"].as_str().unwrap_or("indisponible"),
                result["loop"]["dropouts"],
                result["loop"]["jumps"]
            )));
            if let Some(error) = result["loop"]["error"].as_str() {
                panel = panel.child(hint(error.to_owned()));
            }
            for row in result["scan"]["channels"].as_array().into_iter().flatten() {
                panel = panel.child(hint(format!(
                    "{} : {}",
                    row["name"].as_str().unwrap_or("Voie audio"),
                    row["status"].as_str().unwrap_or("mesurée")
                )));
            }
        }
        panel = panel.child(self.echo_panel(cx));
        panel
    }

    pub fn go_controls(&self, section: Section, cx: &mut Context<Self>) -> Div {
        let mut root = div().flex().flex_col().gap(px(16.));
        if let Some((ok, notice)) = &self.go.notice {
            root = root.child(ui::notice(*ok, notice.clone()));
        }
        if let Some((action, body, message)) = self.go.confirm.as_ref() {
            let action = *action;
            let body = body.clone();
            root = root.child(
                ui::card()
                    .child(ui::card_title(Some((Icon::Alert, ui::ORANGE)), "Confirmation", None))
                    .child(hint(message.clone()))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(ui::pill("go-confirm", None, "Confirmer", ui::Tone::Primary).on_click(cx.listener(move |this, _, _, cx| {
                                this.go.clear_confirm();
                                this.go_send(action, body.clone(), cx);
                            })))
                            .child(ui::pill("go-cancel", None, "Annuler", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                                this.go.clear_confirm();
                                cx.notify();
                            }))),
                    ),
            );
        }
        if self.go.busy {
            root = root.child(hint("Échange avec le moteur…"));
        }
        if section == Section::Diagnostic {
            return root.child(self.diagnostic_view(cx));
        }
        let Some(view) = self.go.view.as_ref() else {
            return root.child(ui::card().items_center().py(px(36.)).child(ui::chip(Icon::Refresh, ui::ACCENT, 52.)).child(ui::title("Lecture des réglages", 17.)).child(hint("En attente des réglages de la GoXLR Mini.")));
        };
        if view.device.status != "ready" {
            root = root.child(ui::notice(false, "GoXLR indisponible : réglages désactivés jusqu’à son retour."));
        }
        root.child(match section {
            Section::Routing => self.routing(view, cx),
            Section::Scenes => self.scenes(view, cx),
            Section::Light => self.lighting(view, cx),
            Section::Microphone => self.microphone(view, cx),
            Section::Headphones => self.headphones(view, cx),
            Section::Diagnostic => unreachable!(),
        })
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
    if d["support"]["windows"] != "Démonstration" || d["glitches"]["simulated"] != true {
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
