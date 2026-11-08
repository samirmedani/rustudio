//! Les écrans du module GoXLR utilisent le même contrat local que l'interface distribuée.
use crate::{
    backend::Client,
    desktop::{Page, hint},
    mixer::Mixer,
};
use crate::mixer::fmt_num;
use crate::ui::{self, Icon, alpha};
use crate::ui::rgb;
use gpui::{AnyElement, ClipboardItem, Context, Div, FontWeight, IntoElement, KeyDownEvent, MouseButton, SharedString, Stateful, div, prelude::*, px};
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

    pub(crate) fn refresh_diag(&mut self, cx: &mut Context<Self>) {
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

    fn act(&self, el: Stateful<Div>, action: &'static str, body: Value, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = self.go.enabled() || matches!(action, "reconnect" | "job-cancel");
        el.when(!enabled, |d| d.opacity(0.45)).on_click(cx.listener(move |this, _, _, cx| {
            if enabled {
                this.go_send(action, body.clone(), cx)
            }
        }))
    }

    fn ask_go(&self, el: Stateful<Div>, action: &'static str, body: Value, message: impl Into<String>, cx: &mut Context<Self>) -> Stateful<Div> {
        let message = message.into();
        el.when(!self.go.enabled(), |d| d.opacity(0.45)).on_click(cx.listener(move |this, _, _, cx| {
            if this.go.enabled() {
                this.go.confirm = Some((action, body.clone(), message.clone()));
                cx.notify();
            }
        }))
    }

    fn routing(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let outputs: Vec<&str> = labels(&v.meta["outputs"]).collect();
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
        for (n, input) in labels(&v.meta["inputs"]).enumerate() {
            let (name, sub, glyph) = input_info(input);
            let mut row = div()
                .flex()
                .items_center()
                .py(px(8.))
                .border_t_1()
                .border_color(alpha(0x000000, if n == 0 { 0.1 } else { 0.06 }))
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
                let cell = if automatic {
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
        div().flex().flex_col().gap(px(16.)).child(matrix).child(vod_card)
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
                        this.go.confirm = Some(("scene-delete", json!({"name": name.clone()}), "Supprimer cette scène personnelle ?".into()));
                        cx.notify();
                    })),
                ),
            );
            tiles.push(self.act(tile, "scene", json!({"name":id}), cx).into_any_element());
        }
        let save = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(self.go_text("name", "Nom de ta scène", cx).flex_1())
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
        let scene_text = self.go.auto_scene.as_deref().map(|s| label_of(s)).unwrap_or_else(|| "Choisir une scène".into());
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
        let fallback_text = fallback.as_str().map(|f| label_of(f)).unwrap_or_else(|| "Ne rien changer".into());
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

    fn lighting(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let s = &v.state["lighting"];
        let enabled = self.go.enabled();
        let mut themes: Vec<AnyElement> = vec![];
        for (id, t) in entries(&v.meta["themes"]) {
            let mut dots = div().flex().gap(px(5.));
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
                    .bg(rgb(u32::from_str_radix(c, 16).unwrap_or(0)))
                    .border_2()
                    .border_color(if chosen { rgb(ui::ACCENT) } else { ui::lerp(alpha(0x000000, 0.12), alpha(ui::ACCENT, 0.6), t) })
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
                    .bg(if t > 0.5 { rgb(u32::from_str_radix(&hex, 16).unwrap_or(fill)) } else { rgb(fill) })
                    .border_2()
                    .border_color(ui::lerp(alpha(0x000000, 0.12), rgb(ui::ACCENT), t))
                    .when(valid, |d| d.cursor(gpui::CursorStyle::PointingHand));
                let values = if i == 0 { json!([hex, null]) } else { json!([null, hex]) };
                let square = ui::hoverable(square, key);
                squares = squares.child(if valid { self.act(square, "colour", json!({"target":id,"colours":values}), cx).into_any_element() } else { square.into_any_element() });
            }
            colours = colours.child(ui::list_row(None, colour_target(id), None, squares, n == 0));
        }
        split(vec![look.into_any_element()], vec![colours.into_any_element()])
    }

    fn microphone(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
        let s = &v.state;
        let mic = &s["mic"];
        let enabled = self.go.enabled();
        let kind = mic["type"].as_str().or(mic["kind"].as_str()).unwrap_or("dynamic").to_owned();
        let mut kinds = ui::segmented();
        for (k, name) in [("dynamic", "Dynamique"), ("condenser", "Statique · 48 V"), ("jack", "Jack 3,5 mm")] {
            let el = ui::seg(SharedString::from(format!("mic-{k}")), name, kind == k);
            kinds = kinds.child(if k == "condenser" && kind != k {
                self.ask_go(el, "mic", json!({"type":k,"suggest_gain":true}), "Activer le 48 V ? Vérifie que ton micro et son câble le supportent (micro statique).", cx)
            } else {
                self.act(el, "mic", json!({"type":k,"suggest_gain":true}), cx)
            });
        }
        let models: Vec<(String, String)> = entries(&v.meta["mic_models"])
            .map(|(id, m)| (id.clone(), format!("{} {}", m["brand"].as_str().unwrap_or(""), m["name"].as_str().or(m["label"].as_str()).unwrap_or(id)).trim().to_owned()))
            .collect();
        let model = mic["model"].as_str().unwrap_or("");
        let model_text = models.iter().find(|(id, _)| id == model).map(|(_, l)| l.clone()).unwrap_or_else(|| "Choisir mon micro".into());
        let ids: Vec<String> = models.iter().map(|(id, _)| id.clone()).collect();
        let job = self.go.job.is_object() && self.go.job["phase"].is_string();
        let mut you = ui::section(Icon::Mic, ui::RED, "Ton micro", None)
            .child(ui::list_row(None, "Modèle", None, div().w(px(240.)).child(self.dropdown("mic-model", model_text, models.iter().map(|(id, l)| (l.clone(), id == model)).collect(), enabled, move |this, i, cx| this.go_send("mic-model", json!({"model": ids[i]}), cx), cx)), true))
            .child(ui::label("Type de branchement"))
            .child(kinds)
            .child(self.slider_row("mic-gain", "Gain", mic["gain_db"].as_f64().unwrap_or(0.), (0., 72.), 1., |v| format!("{} dB", v.round()), enabled, |this, v, cx| this.go_send("mic", json!({"gain_db": v.round() as i64}), cx), cx));
        for tip in labels(&v.meta["mic_models"][model]["tips"]).take(3) {
            you = you.child(div().flex().gap(px(8.)).child(ui::icon(Icon::Sparkles, 14., rgb(ui::ACCENT))).child(ui::hint(tip.to_owned()).text_size(px(12.)).flex_1()));
        }
        you = you.child(
            div()
                .flex()
                .gap(px(8.))
                .child(even(self.act(ui::pill("mic-detect", Some(Icon::Sparkles), "Détecter", ui::Tone::Plain), "detect-mic", json!({}), cx)))
                .child(even(self.act(ui::pill("mic-auto-gain", Some(Icon::Gauge), "Caler le gain", ui::Tone::Plain), "auto-gain", json!({}), cx)))
                .child(even(self.act(ui::pill("mic-voice-setup", Some(Icon::Wave), "Assistant", ui::Tone::Plain), "voice-setup", json!({}), cx))),
        );
        you = you.when(job, |d| {
            d.child(self.act(ui::pill("mic-job-cancel", Some(Icon::Close), "Annuler la mesure", ui::Tone::Danger), "job-cancel", json!({}), cx))
        });
        let advised = v.meta["mic_type_preset"][kind.as_str()].as_str().unwrap_or("");
        let mut presets: Vec<AnyElement> = vec![];
        for (id, p) in entries(&v.meta["mic_presets"]) {
            let tile = ui::tile(SharedString::from(format!("mic-preset-{id}")), Some((preset_icon(id), preset_colour(id))), p["label"].as_str().unwrap_or(id).to_owned(), p["desc"].as_str().map(|d| d.to_owned().into()), s["mic_preset"] == id.as_str(), (id == advised).then(|| ("Conseillé".into(), ui::GREEN)));
            presets.push(self.act(tile, "mic-preset", json!({"name":id}), cx).into_any_element());
        }
        let voices = ui::section(Icon::Wave, 0x5856d6, "Préréglages de voix", Some(if s["voice_print"].is_object() { "Calculés sur ta voix".into() } else { "Crée ton empreinte pour les ajuster".into() })).child(ui::grid(presets, 2));
        let mut fx = ui::section(Icon::Sliders, ui::ACCENT, "Traitements", None);
        for (n, (key, label, sub)) in [
            ("eq", "Égaliseur", "Corrige le timbre de ta voix"),
            ("gate", "Noise gate", "Coupe le bruit de fond entre tes phrases"),
            ("comp", "Compresseur", "Égalise les passages forts et faibles"),
            ("deesser", "De-esser", "Adoucit les « s » qui sifflent"),
        ]
        .into_iter()
        .enumerate()
        {
            let on = s["fx"][key] == true;
            fx = fx.child(ui::list_row(None, label, Some(sub.into()), self.act(ui::toggle(SharedString::from(format!("fx-{key}")), on, enabled), "fx", json!({(key):!on}), cx), n == 0));
        }
        let dsp = &s["dsp"];
        let mut bands = div().flex().gap(px(4.));
        for band in labels(&v.meta["eq_bands"]) {
            let band = band.to_owned();
            let freq = dsp["eq_freq"][&band].as_f64().or_else(|| band.parse().ok()).unwrap_or(100.);
            let range = &v.meta["eq_ranges"][&band];
            let (lo, hi) = (range[0].as_f64().unwrap_or(freq), range[1].as_f64().unwrap_or(freq));
            let down = ((freq / 1.12).round()).clamp(lo, hi);
            let up = ((freq * 1.12).round()).clamp(lo, hi);
            let (b1, b2, b3) = (band.clone(), band.clone(), band.clone());
            // fréquence de la bande, puis ses deux flèches regroupées dans une même pastille sous le curseur
            let below = div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(hz(freq)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .p(px(2.))
                        .rounded_full()
                        .bg(alpha(0x767680, 0.1))
                        .child(self.act(ui::round(SharedString::from(format!("eqf-down-{band}")), Icon::ChevronLeft, 22., alpha(0x767680, 0.), rgb(ui::TEXT2)), "dsp", json!({"eq_freq":{(b1):down}}), cx))
                        .child(self.act(ui::round(SharedString::from(format!("eqf-up-{band}")), Icon::ChevronRight, 22., alpha(0x767680, 0.), rgb(ui::TEXT2)), "dsp", json!({"eq_freq":{(b2):up}}), cx)),
                );
            bands = bands.child(self.eq_column(&format!("mic-eq-{band}"), dsp["eq"][&band].as_f64().unwrap_or(0.), (-9., 9.), 1., below.into_any_element(), enabled, move |this, v, cx| this.go_send("dsp", json!({"eq":{(b3.clone()): v.round() as i64}}), cx), cx));
        }
        let eq = ui::section(Icon::Activity, ui::ACCENT, "Égaliseur du micro", Some(if s["fx"]["eq"] == true { "Actif".into() } else { "Désactivé dans Traitements".into() })).child(bands);
        let ratios: Vec<f64> = v.meta["ratios"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
        let ratio_label = move |i: f64| ratios.get(i.round() as usize).map(|r| format!("{}:1", fmt_num(*r))).unwrap_or_else(|| fmt_num(i));
        let dynamics = ui::section(Icon::Gauge, 0x34c759, "Dynamique", None)
            .child(self.slider_row("gate-threshold", "Seuil du noise gate", dsp["gate"]["threshold"].as_f64().unwrap_or(-59.), (-59., 0.), 1., |v| format!("{} dB", v.round()), enabled, |this, v, cx| this.go_send("dsp", json!({"gate":{"threshold": v.round() as i64}}), cx), cx))
            .child(self.slider_row("gate-attenuation", "Réduction du bruit", dsp["gate"]["attenuation"].as_f64().unwrap_or(0.), (0., 100.), 1., |v| format!("{} %", v.round()), enabled, |this, v, cx| this.go_send("dsp", json!({"gate":{"attenuation": v.round() as i64}}), cx), cx))
            .child(self.slider_row("comp-threshold", "Seuil du compresseur", dsp["comp"]["threshold"].as_f64().unwrap_or(0.), (-40., 0.), 1., |v| format!("{} dB", v.round()), enabled, |this, v, cx| this.go_send("dsp", json!({"comp":{"threshold": v.round() as i64}}), cx), cx))
            .child(self.slider_row("comp-ratio", "Force de la compression", dsp["comp"]["ratio"].as_f64().unwrap_or(0.), (0., 14.), 1., ratio_label, enabled, |this, v, cx| this.go_send("dsp", json!({"comp":{"ratio": v.round() as i64}}), cx), cx))
            .child(self.slider_row("comp-makeup", "Gain de compensation", dsp["comp"]["makeup"].as_f64().unwrap_or(0.), (-6., 24.), 1., |v| format!("{} dB", v.round()), enabled, |this, v, cx| this.go_send("dsp", json!({"comp":{"makeup": v.round() as i64}}), cx), cx))
            .child(self.slider_row("deesser", "De-esser", dsp["deesser"].as_f64().unwrap_or(0.), (0., 100.), 5., |v| format!("{} %", v.round()), enabled, |this, v, cx| this.go_send("dsp", json!({"deesser": v.round() as i64}), cx), cx))
            .child(self.slider_row("bleep", "Volume du bip de censure", s["bleep_db"].as_f64().unwrap_or(0.), (-36., 0.), 1., |v| format!("{} dB", v.round()), enabled, |this, v, cx| this.go_send("dsp", json!({"bleep_db": v.round() as i64}), cx), cx));
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
            .child(ui::list_row(None, "Clignoter quand le micro est coupé", None, self.act(ui::toggle("mic-blink", blink, enabled), "settings", json!({"mic_button":{"blink":!blink}}), cx), false));
        let listen = ui::section(Icon::Ear, 0xff9500, "T’écouter", None)
            .child(self.slider_row("monitor", "Retour de ta voix dans le casque", s["volumes"]["MicMonitor"].as_f64().unwrap_or(0.) / 2.55, (0., 100.), 1., |v| format!("{} %", v.round()), enabled, |this, v, cx| this.go_send("volume", json!({"channel":"MicMonitor","value": (v * 2.55).round() as i64}), cx), cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(even(self.act(ui::pill("mic-listen", Some(Icon::Play), "M’entendre 30 s", ui::Tone::Plain), "mic-listen", json!({"seconds":30}), cx)))
                            .child(even(self.act(ui::pill("mic-listen-stop", Some(Icon::Stop), "Arrêter", ui::Tone::Plain), "mic-listen", json!({"seconds":0}), cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(even(self.act(ui::pill("mic-ab-raw", None, "Voix brute", ui::Tone::Plain), "mic-ab", json!({"bypass":true}), cx)))
                            .child(even(self.act(ui::pill("mic-ab-processed", None, "Voix traitée", ui::Tone::Plain), "mic-ab", json!({"bypass":false}), cx))),
                    ),
            )
            .child(self.listen_panel(cx));
        let chart = self.voice_chart(s["mic_preset"].as_str().filter(|p| *p != "brut" && *p != "custom").unwrap_or("studio"));
        let mut print = self.voice_panel(cx);
        if s["voice_print"].is_object() {
            print = print.child(self.ask_go(ui::pill("voice-forget", Some(Icon::Trash), "Oublier l’empreinte", ui::Tone::Plain), "voice-forget", json!({}), "Effacer l’empreinte vocale de ce micro ?", cx));
        }
        split(
            vec![you.into_any_element(), voices.into_any_element(), eq.into_any_element(), dynamics.into_any_element()],
            vec![print.into_any_element(), chart.into_any_element(), fx.into_any_element(), button_card.into_any_element(), listen.into_any_element()],
        )
    }

    fn headphones(&self, v: &GoView, cx: &mut Context<Self>) -> Div {
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
        let correction = ui::section(Icon::Headphones, ui::ACCENT, "Correction du casque", None)
            .child(ui::list_row(None, "Égaliseur du casque", Some("Corrige ton casque et applique le style choisi.".into()), self.act(ui::toggle("hp-enabled", on, enabled), "hp-eq", json!({"enabled":!on}), cx), true))
            .child(ui::list_row(
                None,
                "Ton casque",
                Some("Courbe mesurée, pour un son neutre.".into()),
                div().w(px(250.)).child(self.dropdown("hp-model", if current.is_empty() { "Aucune correction".into() } else { current.clone() }, options, enabled, move |this, i, cx| {
                    let pick = if i == 0 { Value::Null } else { json!(picks[i - 1]) };
                    this.go_send("hp-eq", json!({"headphone": pick}), cx)
                }, cx)),
                false,
            ))
            .child(ui::list_row(None, "Pause pendant un live", Some("Le stream garde un son neutre quand OBS diffuse.".into()), self.act(ui::toggle("hp-pause", paused, enabled), "hp-eq", json!({"pause_live":!paused}), cx), false))
            .child(div().pt(px(6.)).child(ui::label("Voies corrigées")))
            .child(chips)
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .pt(px(6.))
                    .child(self.ask_go(ui::pill("hp-install", Some(Icon::Download), "Installer le composant", ui::Tone::Plain), "hp-eq-setup", json!({}), "Installer Equalizer APO (composant de correction du casque) sur Windows ?", cx))
                    .child(self.ask_go(ui::pill("hp-remove", Some(Icon::Trash), "Retirer", ui::Tone::Plain), "hp-eq-remove", json!({}), "Retirer le composant de correction du casque ?", cx)),
            );
        let mut styles: Vec<AnyElement> = vec![];
        for (id, st) in entries(&v.meta["hp_styles"]) {
            if let Some(required) = st["headphone"].as_str() {
                if eq["headphone"] != required {
                    continue;
                }
            }
            let tile = ui::tile(SharedString::from(format!("hp-style-{id}")), Some((style_icon(id), style_colour(id))), st["label"].as_str().unwrap_or(id).to_owned(), st["desc"].as_str().map(|d| d.to_owned().into()), eq["style"] == id.as_str(), st["headphone"].is_string().then(|| ("Pour ton casque".into(), 0x5856d6)));
            styles.push(self.act(tile, "hp-eq", json!({"style":id}), cx).into_any_element());
        }
        let style_card = ui::section(Icon::Music, 0xff2d55, "Style de son", None).child(ui::grid(styles, 3));
        let values = eq["bands"].as_array().cloned().unwrap_or_default();
        let mut bands = div().flex().gap(px(2.));
        for (i, freq) in v.meta["hp_bands"].as_array().into_iter().flatten().enumerate() {
            let Some(current) = values.get(i).and_then(Value::as_f64) else { continue };
            let all = values.clone();
            let below = div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(ui::TEXT2)).child(hz(freq.as_f64().unwrap_or(0.)));
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
                .items_center()
                .gap(px(10.))
                .child(self.go_text("name", "Nom du réglage", cx).flex_1())
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
                    .child(self.ask_go(ui::round(SharedString::from(format!("hp-delete-{name}")), Icon::Trash, 32., alpha(0x767680, 0.), rgb(ui::TEXT2)), "hp-eq-delete", json!({"name":name}), "Supprimer ce réglage de casque ?", cx)),
                n == 0,
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(split(vec![correction.into_any_element()], vec![custom.into_any_element()]))
            .child(style_card)
            .child(eq_card)
    }

    pub(crate) fn run_sound_test(&mut self, cx: &mut Context<Self>) {
        if !self.go.enabled() {
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
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn diagnostic_view(&self, cx: &mut Context<Self>) -> Div {
        let url = self.client.overlay_url();
        let actions = div()
            .flex()
            .gap(px(8.))
            .child(ui::pill("diag-read", Some(Icon::Refresh), "Actualiser", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| this.refresh_diag(cx))))
            .child(self.act(ui::pill("diag-reconnect", Some(Icon::Plug), "Reconnecter la GoXLR", ui::Tone::Plain), "reconnect", json!({}), cx));
        let mut state = ui::section(Icon::Gauge, ui::GREEN, "État de la GoXLR", None).child(actions);
        if let Some(d) = self.go.diagnostics.as_ref() {
            let report = &d["support"]["report"];
            let copy = serde_json::to_string_pretty(report).unwrap_or_default();
            let engine_ok = report["moteur"]["etat"].as_str().is_some_and(|e| e.contains("ok") || e.contains("connect") || e.contains("Démo"));
            let cuts = report["coupures_usb_24h"]["nombre"].as_u64();
            let mut rows: Vec<(Icon, String, String, ui::State)> = vec![
                (Icon::Power, "Moteur".into(), report["moteur"]["etat"].as_str().unwrap_or("inconnu").to_owned(), if engine_ok { ui::State::Ok } else { ui::State::Warn }),
                (Icon::Plug, "Coupures USB (24 h)".into(), cuts.map(|n| n.to_string()).unwrap_or_else(|| "Non mesurées".into()), if cuts.unwrap_or(0) == 0 { ui::State::Ok } else { ui::State::Warn }),
            ];
            if d["usb"]["found"] == true {
                let cpu = d["usb"]["controller_cpu"] == true;
                rows.push((Icon::Gauge, "Contrôleur USB".into(), if cpu { "Relié au processeur".into() } else { "À vérifier".into() }, if cpu { ui::State::Ok } else { ui::State::Warn }));
            }
            if d["glitches"]["readable"] == true {
                rows.push((Icon::History, "Journal Windows".into(), format!("{} évènement(s)", d["glitches"]["total"]), ui::State::Info));
            }
            for (n, (glyph, title, value, st)) in rows.into_iter().enumerate() {
                state = state.child(ui::list_row(Some((glyph, ui::TEXT2)), title, None, ui::status(value, st), n == 0));
            }
            for (i, check) in d["echo"]["checks"].as_array().into_iter().flatten().enumerate() {
                let label = check["title"].as_str().or(check["label"].as_str()).or(check["id"].as_str()).unwrap_or("Routage").to_owned();
                let ok = check["ok"] == true;
                let fix = &check["fix"];
                let trailing = if let (false, Some(action)) = (ok, ACTIONS.iter().copied().find(|a| fix["action"] == *a)) {
                    self.act(ui::pill(("diag-fix", i), Some(Icon::Check), "Corriger", ui::Tone::Primary), action, fix["body"].clone(), cx).into_any_element()
                } else {
                    ui::status(if ok { "Bon" } else { "À vérifier" }, if ok { ui::State::Ok } else { ui::State::Warn }).into_any_element()
                };
                state = state.child(ui::list_row(Some((Icon::Route, ui::TEXT2)), label, None, trailing, false));
            }
            state = state.child(ui::pill("diag-copy", Some(Icon::Download), "Copier le diagnostic anonymisé", ui::Tone::Plain).on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))));
        } else {
            state = state.child(hint("Lecture des données du moteur…"));
        }
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
            .child(ui::list_row(None, url.clone(), None, ui::pill("obs-url", Some(Icon::Download), "Copier", ui::Tone::Plain).on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(url.clone())))), true));
        split(vec![state.into_any_element(), obs.into_any_element()], vec![test.into_any_element(), self.echo_panel(cx).into_any_element()])
    }

    /// Demande de confirmation GoXLR, affichée en fenêtre par-dessus la page.
    pub(crate) fn go_confirm_box(&self, cx: &mut Context<Self>) -> Option<Div> {
        if let Some((action, body, message)) = self.go.confirm.as_ref() {
            let action = *action;
            let body = body.clone();
            return Some(
                ui::card()
                    .flex_row()
                    .items_center()
                    .gap(px(14.))
                    .border_color(alpha(ui::ORANGE, 0.35))
                    .child(ui::chip(Icon::Alert, ui::ORANGE, 34.))
                    .child(div().flex_1().text_size(px(13.5)).child(message.clone()))
                    .child(ui::pill("go-cancel", None, "Annuler", ui::Tone::Plain).on_click(cx.listener(|this, _, _, cx| {
                        this.go.clear_confirm();
                        cx.notify();
                    })))
                    .child(ui::pill("go-confirm", None, "Confirmer", ui::Tone::Primary).on_click(cx.listener(move |this, _, _, cx| {
                        this.go.clear_confirm();
                        this.go_send(action, body.clone(), cx);
                    }))),
            );
        }
        None
    }

    pub fn go_controls(&self, section: Section, cx: &mut Context<Self>) -> Div {
        let mut root = div().flex().flex_col().gap(px(16.));
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

const ORANGE_SUN: u32 = 0xff9500;

/// Deux colonnes indépendantes (gauche / droite), chacune empilée.
fn split(left: Vec<AnyElement>, right: Vec<AnyElement>) -> Div {
    let col = |items: Vec<AnyElement>| div().flex().flex_col().flex_1().min_w(px(0.)).gap(px(16.)).children(items);
    div().flex().items_start().gap(px(16.)).child(col(left)).child(col(right))
}

fn hz(f: f64) -> String {
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
fn style_colour(id: &str) -> u32 {
    match id {
        "neutre" => 0x34c759,
        id if id.contains("basse") || id.contains("rap") => 0xff2d55,
        "jeu" | "competitif" => 0x0071e3,
        "voix" => 0xff9500,
        "film" | "immersion" => 0x5856d6,
        _ => 0xaf52de,
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

/// Bouton qui partage la ligne à parts égales avec ses voisins (jamais seul à la ligne).
fn even(el: Stateful<Div>) -> Stateful<Div> {
    el.flex_1().min_w(px(0.)).justify_center()
}
