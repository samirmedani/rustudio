//! Profils locaux associés à l'exécutable Windows d'une application.
use crate::{
    backend::Client,
    camera_profiles::{app_icon, app_name},
    keyboard::Effect,
    mixer::Mixer,
    ui::{self, Icon, State, Tone},
};
use gpui::{Context, Div, PathPromptOptions, RenderImage, div, img, prelude::*, px};
use rustudio_core::{dsp::MicProfile, hpeq, mk770};
use rustudio_windows_mic as wm;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_PROFILES: usize = 32;
const MAX_BYTES: u64 = 160 * 1024;
const CHOICES: [&str; 3] = [
    "Filtres du micro",
    "Égaliseur du casque",
    "Effet lumineux du MK770",
];

fn app_key(path: &Path) -> Option<String> {
    if !path.is_absolute() || !path.extension()?.to_str()?.eq_ignore_ascii_case("exe") {
        return None;
    }
    let path = path.to_str()?;
    if path.len() > 2048 || path.chars().any(char::is_control) {
        return None;
    }
    Some(
        path.trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase(),
    )
}

#[derive(Clone, Deserialize, Serialize)]
enum Microphone {
    GoXlr(MicProfile),
    Windows { guid: String, voice: wm::Voice },
}

#[derive(Clone, Deserialize, Serialize)]
struct Headphones {
    enabled: bool,
    bands: [i32; 10],
    style: Option<String>,
    intensity: i32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AppProfile {
    app: PathBuf,
    mic: Option<Microphone>,
    headphones: Option<Headphones>,
    light: Option<Effect>,
}

impl AppProfile {
    fn kinds(&self) -> [bool; 3] {
        [
            self.mic.is_some(),
            self.headphones.is_some(),
            self.light.is_some(),
        ]
    }

    fn validate(&self) -> bool {
        app_key(&self.app).is_some()
            && self.kinds().into_iter().any(|on| on)
            && match self.mic.as_ref() {
                Some(Microphone::Windows { guid, .. }) => rustudio_core::micns::is_guid(guid),
                Some(Microphone::GoXlr(m)) => {
                    m.mic_preset == "custom" || rustudio_core::dsp::is_preset(&m.mic_preset)
                }
                None => true,
            }
            && self.headphones.as_ref().is_none_or(|h| {
                (0..=hpeq::INTENSITY_MAX).contains(&h.intensity)
                    && h.bands.iter().all(|b| (-12..=12).contains(b))
                    && h.style.as_deref().is_none_or(|s| hpeq::style(s).is_some())
            })
            && self
                .light
                .as_ref()
                .is_none_or(|e| mk770::effect(e.id).is_some() && e.speed <= mk770::SPEED_MAX)
    }

    fn capture(client: &Client, app: PathBuf, kinds: [bool; 3]) -> Result<Self, String> {
        let [mic, headphones, light] = kinds;
        let mut profile = AppProfile {
            app,
            mic: None,
            headphones: None,
            light: None,
        };
        let windows = mic.then(|| get_microphone(client)).transpose()?;
        let go_mic = windows.as_ref().is_some_and(|(state, guid)| {
            state["live"]["captures"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|c| c["guid"] == guid.as_str() && c["goxlr"] == true)
        });
        let go = (go_mic || headphones)
            .then(|| ready_goxlr(client))
            .transpose()?;
        if let Some((state, guid)) = windows {
            profile.mic = Some(if go_mic {
                let v = go.as_ref().ok_or("GoXLR Mini indisponible.")?;
                Microphone::GoXlr(goxlr_microphone(&v.state)?)
            } else {
                let voice = serde_json::from_value(state["state"]["voice"].clone())
                    .map_err(|_| "Réglages du micro Windows illisibles.")?;
                Microphone::Windows { guid, voice }
            });
        }
        if let Some(v) = go.filter(|_| headphones) {
            let eq = hpeq::normalize_hp_eq(&v.state["hp_eq"], None);
            profile.headphones = Some(Headphones {
                enabled: eq.enabled,
                bands: eq.bands.try_into().map_err(|_| "Égaliseur illisible.")?,
                style: eq.style.filter(|s| hpeq::style(s).is_some()),
                intensity: eq.intensity,
            });
        }
        if light {
            let keyboard = client.keyboard_view()?;
            profile.light = Some(
                keyboard
                    .state
                    .filter(|_| keyboard.device.status == "ready")
                    .and_then(|s| s.effects.into_iter().find(|e| e.id == s.active_effect))
                    .ok_or("Connecte le MK770 pour enregistrer son effet.")?,
            );
        }
        if !profile.validate() {
            return Err("Les réglages sélectionnés sont invalides.".into());
        }
        Ok(profile)
    }

    fn apply(&self, client: &Client) -> Vec<(&'static str, Result<(), String>)> {
        let mut results = Vec::new();
        if let Some(mic) = &self.mic {
            let result = match mic {
                Microphone::GoXlr(filters) => (|| {
                    let view = ready_goxlr(client)?;
                    if goxlr_microphone(&view.state)? == *filters && !view.ab {
                        return Ok(());
                    }
                    client.go_command("mic-profile", json!(filters))?;
                    if goxlr_microphone(&ready_goxlr(client)?.state)? == *filters {
                        Ok(())
                    } else {
                        Err("Le moteur n’a pas confirmé les filtres.".into())
                    }
                })(),
                Microphone::Windows { guid, voice } => {
                    apply_windows_microphone(client, guid, voice)
                }
            };
            results.push(("Micro", result));
        }
        if let Some(hp) = &self.headphones {
            let result = ready_goxlr(client)
                .and_then(|_| {
                    client.go_command("hp-eq", json!({
                "enabled":hp.enabled, "bands":hp.bands, "style":hp.style, "intensity":hp.intensity,
            }))
                })
                .map(|_| ());
            results.push(("Casque", result));
        }
        if let Some(effect) = &self.light {
            let matches = |state: &crate::keyboard::Settings| {
                state.active_effect == effect.id
                    && state
                        .effects
                        .iter()
                        .find(|e| e.id == effect.id)
                        .is_some_and(|e| e.changes() == effect.changes())
            };
            let result = (|| {
                let keyboard = client.keyboard_view()?;
                if keyboard.device.status != "ready" {
                    return Err("MK770 indisponible.".into());
                }
                if keyboard.state.as_ref().is_some_and(matches) {
                    return Ok(());
                }
                let state = client.keyboard_command("settings", &effect.changes())?;
                if matches(&state) {
                    Ok(())
                } else {
                    Err("Le clavier n’a pas confirmé l’effet.".into())
                }
            })();
            results.push(("MK770", result));
        }
        results
    }

    fn locks(&self) -> [bool; 3] {
        [
            matches!(self.mic, Some(Microphone::GoXlr(_))) || self.headphones.is_some(),
            matches!(self.mic, Some(Microphone::Windows { .. })),
            self.light.is_some(),
        ]
    }
}

fn get_microphone(client: &Client) -> Result<(Value, String), String> {
    let state = client.get("/api/modules/windows-mic/state")?;
    let mic = state["state"]["selected"]
        .as_str()
        .map(str::to_owned)
        .ok_or("Choisis un micro dans « Mon micro ».")?;
    Ok((state, mic))
}

fn ready_goxlr(client: &Client) -> Result<crate::goxlr::GoView, String> {
    let v = client.go_view()?;
    if v.device.status != "ready" {
        return Err("GoXLR Mini indisponible.".into());
    }
    Ok(v)
}

fn goxlr_microphone(state: &Value) -> Result<MicProfile, String> {
    serde_json::from_value(
        json!({"dsp":state["dsp"],"fx":state["fx"],"mic_preset":state["mic_preset"]}),
    )
    .map_err(|_| "Réglages du micro GoXLR illisibles.".into())
}

fn apply_windows_microphone(client: &Client, guid: &str, voice: &wm::Voice) -> Result<(), String> {
    let state = client.get("/api/modules/windows-mic/state")?;
    if !state["live"]["captures"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|c| c["guid"] == guid)
    {
        return Err("Le micro de ce profil est débranché.".into());
    }
    client
        .post_module(
            crate::winmic::MODULE,
            "settings",
            json!({
                "device":guid, "enabled":voice.enabled, "gain_db":voice.gain_db,
                "hpf":voice.hpf, "hpf_hz":voice.hpf_hz, "eq":voice.eq, "dynamics":voice.dynamics,
            }),
        )
        .map(|_| ())
}

fn validate(profiles: &[AppProfile]) -> bool {
    let mut unique = std::collections::HashSet::new();
    profiles.len() <= MAX_PROFILES
        && profiles
            .iter()
            .all(|p| p.validate() && app_key(&p.app).is_some_and(|key| unique.insert(key)))
}

fn load(path: &Path) -> Result<Vec<AppProfile>, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Lecture des profils impossible ({e}).")),
    };
    let mut contents = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|e| e.to_string())?;
    if contents.len() as u64 > MAX_BYTES {
        return Err("Fichier de profils trop volumineux.".into());
    }
    let profiles: Vec<AppProfile> =
        serde_json::from_slice(&contents).map_err(|_| "Profils d’application illisibles.")?;
    if !validate(&profiles) {
        return Err("Profils d’application invalides.".into());
    }
    Ok(profiles)
}

fn store(path: &Path, profiles: &[AppProfile]) -> Result<(), String> {
    if !validate(profiles) {
        return Err("La liste de profils est pleine ou contient un doublon.".into());
    }
    let data = serde_json::to_vec(profiles).map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_BYTES {
        return Err("Fichier de profils trop volumineux.".into());
    }
    let result = (|| -> std::io::Result<()> {
        let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
        fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        file.write_all(&data)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    })();
    result.map_err(|e| format!("Enregistrement du profil impossible ({e})."))
}

type Results = Vec<(&'static str, Result<(), String>)>;

pub(crate) struct ApplicationProfiles {
    path: PathBuf,
    entries: Vec<AppProfile>,
    icons: HashMap<PathBuf, Arc<RenderImage>>,
    checked: [bool; 3],
    readable: bool,
    saving: bool,
    applying: Option<PathBuf>,
    foreground: Option<String>,
    error: Option<String>,
    last: Option<(PathBuf, Results)>,
}

impl ApplicationProfiles {
    #[cfg(debug_assertions)]
    pub(crate) fn load_layout_fixture(&mut self) {
        let app = std::env::temp_dir().join("Studio-Demo.exe");
        self.entries = vec![AppProfile {
            app: app.clone(),
            mic: None,
            headphones: None,
            light: Some(Effect {
                id: 0,
                brightness: 93,
                speed: 0,
                reverse: false,
                colour: "#556677".into(),
            }),
        }];
        self.last = Some((
            app,
            vec![
                ("MK770", Ok(())),
                (
                    "Micro",
                    Err("Le micro de ce profil est actuellement débranché.".into()),
                ),
            ],
        ));
    }

    pub(crate) fn new(path: PathBuf) -> Self {
        let loaded = load(&path);
        let readable = loaded.is_ok();
        let (entries, error) = match loaded {
            Ok(items) => (items, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        Self {
            path,
            entries,
            icons: HashMap::new(),
            checked: [false; 3],
            readable,
            saving: false,
            applying: None,
            foreground: None,
            error,
            last: None,
        }
    }

    fn edit(&mut self, change: impl FnOnce(&mut Vec<AppProfile>)) -> Result<(), String> {
        if !self.readable {
            return Err("Les profils existants sont illisibles.".into());
        }
        let mut next = self.entries.clone();
        change(&mut next);
        store(&self.path, &next)?;
        self.entries = next;
        self.icons
            .retain(|path, _| self.entries.iter().any(|p| &p.app == path));
        self.foreground = None;
        Ok(())
    }
}

impl Mixer {
    pub(crate) fn has_application_profiles(&self) -> bool {
        !self.application_profiles.entries.is_empty()
    }

    pub(crate) fn load_application_icons(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .application_profiles
            .entries
            .iter()
            .filter(|p| !self.application_profiles.icons.contains_key(&p.app))
            .map(|p| p.app.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        let job = cx.background_executor().spawn(async move {
            paths
                .into_iter()
                .filter_map(|p| app_icon(&p).map(|icon| (p, icon)))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let icons = job.await;
            let _ = this.update(cx, |this, cx| {
                let book = &mut this.application_profiles;
                for (path, icon) in icons {
                    if book.entries.iter().any(|p| p.app == path) {
                        book.icons.entry(path).or_insert(icon);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn follow_application_profile(
        &mut self,
        app: Option<&Path>,
        cx: &mut Context<Self>,
    ) {
        let app = app.and_then(app_key);
        if self.application_profiles.foreground == app {
            return;
        }
        let candidate = self
            .application_profiles
            .entries
            .iter()
            .find(|p| app_key(&p.app).is_some_and(|key| Some(key) == app))
            .cloned();
        if let Some(profile) = candidate
            && !self.apply_application_profile(profile, cx)
        {
            return;
        }
        self.application_profiles.foreground = app;
    }

    fn apply_application_profile(&mut self, profile: AppProfile, cx: &mut Context<Self>) -> bool {
        let [go, mic, keyboard] = profile.locks();
        let b = &self.application_profiles;
        if b.saving
            || b.applying.is_some()
            || (go && (self.go.busy || self.go.intensity_sending || self.wizard.is_some()))
            || (mic && self.winmic.busy)
            || (keyboard && self.keyboard.busy)
        {
            return false;
        }
        if go {
            self.go.busy = true;
        }
        if mic {
            self.winmic.busy = true;
        }
        if keyboard {
            self.keyboard.busy = true;
        }
        self.application_profiles.applying = Some(profile.app.clone());
        let client = self.client.clone();
        let work = cx.background_executor().spawn(async move {
            let results = profile.apply(&client);
            let go_view = go.then(|| client.go_view().ok()).flatten();
            let keyboard_view = keyboard.then(|| client.keyboard_view().ok()).flatten();
            (profile.app, results, go_view, keyboard_view)
        });
        cx.spawn(async move |this, cx| {
            let (app, results, go_view, keyboard_view) = work.await;
            let _ = this.update(cx, |this, cx| {
                if go {
                    this.go.busy = false;
                    if go_view.is_some() {
                        this.go.receive(go_view);
                    }
                    this.resume_mic_preset(cx);
                }
                if mic {
                    this.finish_microphone_profile(cx);
                }
                if keyboard {
                    this.keyboard.busy = false;
                    if keyboard_view.is_some() {
                        this.keyboard.receive(keyboard_view);
                    }
                }
                this.application_profiles.applying = None;
                this.application_profiles.last = Some((app, results));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        true
    }

    fn choose_profile_app(&mut self, cx: &mut Context<Self>) {
        let profiles = &mut self.application_profiles;
        if profiles.saving
            || profiles.applying.is_some()
            || !profiles.checked.into_iter().any(|on| on)
        {
            return;
        }
        let kinds = profiles.checked;
        profiles.saving = true;
        profiles.error = None;
        let client = self.client.clone();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let selection = match picker.await {
                Ok(Ok(Some(paths))) => Ok(paths.into_iter().next()),
                Ok(Ok(None)) => Ok(None),
                _ => Err("Sélecteur de fichier indisponible.".to_owned()),
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let Some(path) = selection? else {
                        return Ok(None);
                    };
                    if app_key(&path).is_none() || !path.is_file() {
                        return Err("Choisis un fichier d’application Windows (.exe).".to_owned());
                    }
                    let icon = rustudio_windows_desktop::app_identity::icon_bgra(&path);
                    Ok(Some((AppProfile::capture(&client, path, kinds)?, icon)))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let book = &mut this.application_profiles;
                book.saving = false;
                book.error = result
                    .and_then(|selection| {
                        if let Some((profile, icon)) = selection {
                            let path = profile.app.clone();
                            let key = app_key(&path);
                            book.edit(|items| {
                                items.retain(|p| app_key(&p.app) != key);
                                items.push(profile);
                            })?;
                            if let Some(icon) = icon.and_then(|p| {
                                let size = rustudio_windows_desktop::app_identity::ICON_SIZE;
                                crate::webcam::make_render_image(size, size, p)
                            }) {
                                book.icons.insert(path, icon);
                            }
                            book.last = None;
                        }
                        Ok(())
                    })
                    .err();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn application_profiles_view(&self, cx: &mut Context<Self>) -> Div {
        let book = &self.application_profiles;
        let editable = book.readable && !book.saving && book.applying.is_none();
        let mut toggles = div().flex().flex_wrap().gap(px(16.));
        for (i, label) in CHOICES.into_iter().enumerate() {
            let toggle =
                ui::toggle(("profile-part", i), book.checked[i], editable).when(editable, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.application_profiles.checked[i] ^= true;
                        cx.notify();
                    }))
                });
            toggles = toggles.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(toggle)
                    .child(ui::label(label)),
            );
        }
        let mut body = div().flex().flex_col().gap(px(16.)).child(
            ui::section(
                Icon::Bookmark,
                ui::ACCENT,
                "Associer les réglages actuels",
                None,
            )
            .child(ui::caption(
                "Coche ce que tu veux retrouver quand l’application passe au premier plan.",
            ))
            .child(toggles)
            .child(
                div().flex().child(
                    ui::pill(
                        "profile-app-select",
                        Some(Icon::Plus),
                        if book.saving {
                            "Enregistrement…"
                        } else {
                            "Choisir l’application…"
                        },
                        Tone::Primary,
                    )
                    .when(!editable || !book.checked.into_iter().any(|on| on), |d| {
                        d.opacity(0.5)
                    })
                    .when(editable, |d| {
                        d.on_click(cx.listener(|this, _, _, cx| this.choose_profile_app(cx)))
                    }),
                ),
            )
            .when_some(book.error.as_ref(), |d, text| {
                d.child(ui::status(text.clone(), State::Warn))
            }),
        );
        for profile in &book.entries {
            let name = app_name(&profile.app);
            let mut card = ui::card().child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(book.icons.get(&profile.app).map_or_else(
                        || ui::chip(Icon::Monitor, ui::ACCENT, 28.).into_any_element(),
                        |icon| {
                            img(icon.clone())
                                .size(px(28.))
                                .object_fit(gpui::ObjectFit::Contain)
                                .into_any_element()
                        },
                    ))
                    .child(div().flex_1().min_w(px(0.)).child(ui::title(name, 16.))),
            );
            let labels: Vec<&str> = CHOICES
                .into_iter()
                .zip(profile.kinds())
                .filter_map(|(label, on)| on.then_some(label))
                .collect();
            card = card.child(ui::caption(labels.join(" · ")));
            if book.applying.as_ref() == Some(&profile.app) {
                card = card.child(ui::status("Application en cours…", State::Info));
            } else if let Some((_, results)) =
                book.last.as_ref().filter(|(path, _)| path == &profile.app)
            {
                for (label, result) in results {
                    card = card.child(match result {
                        Ok(()) => ui::status(format!("{label} · Réglages appliqués"), State::Ok),
                        Err(e) => ui::status(format!("{label} · {e}"), State::Warn),
                    });
                }
            }
            let apply = profile.clone();
            let remove = profile.app.clone();
            let id = remove.to_string_lossy().into_owned();
            let apply = ui::pill(
                gpui::SharedString::from(format!("profile-apply-{id}")),
                Some(Icon::Play),
                "Appliquer",
                Tone::Plain,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let started = this.apply_application_profile(apply.clone(), cx);
                this.application_profiles.error =
                    (!started).then(|| "Attends la fin du réglage en cours.".into());
                cx.notify();
            }));
            let remove = ui::pill(
                gpui::SharedString::from(format!("profile-remove-{id}")),
                Some(Icon::Trash),
                "Retirer",
                Tone::Plain,
            )
            .when(editable, |d| {
                d.on_click(cx.listener(move |this, _, _, cx| {
                    let book = &mut this.application_profiles;
                    book.error = book.edit(|items| items.retain(|p| p.app != remove)).err();
                    if book.error.is_none() {
                        book.last = None;
                    }
                    cx.notify();
                }))
            });
            body = body.child(
                card.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(apply)
                        .child(remove),
                ),
            );
        }
        body
    }
}

pub(crate) fn verify(client: &Client) -> Result<(), String> {
    if client.is_live() {
        return Err("Le test des profils exige le simulateur.".into());
    }
    let app = std::env::temp_dir().join("Application-Demo.exe");
    let target = client.get("/api/modules/windows-mic/state")?;
    let captures = target["live"]["captures"]
        .as_array()
        .ok_or("Micros de démonstration manquants.")?;
    let device = |goxlr| {
        captures
            .iter()
            .find(|c| c["goxlr"] == goxlr)
            .and_then(|c| c["guid"].as_str())
            .ok_or("Entrée de démonstration manquante.")
    };
    let goxlr = device(true)?;
    let usb = device(false)?;
    let select =
        |guid| client.post_module(crate::winmic::MODULE, "settings", json!({"selected":guid}));
    select(goxlr)?;
    client.go_command("mic-preset", json!({"name":"podcast"}))?;
    client.go_command("hp-eq", json!({"style":"jeu","intensity":127}))?;
    client.keyboard_command(
        "settings",
        &json!({"active_effect":1,"speed":3,"brightness":93}),
    )?;
    let profile = AppProfile::capture(client, app.clone(), [true; 3])?;
    if !matches!(profile.mic, Some(Microphone::GoXlr(_))) {
        return Err("Profil GoXLR absent.".into());
    }
    client.go_command("mic-preset", json!({"name":"brut"}))?;
    client.go_command("hp-eq", json!({"style":"neutre"}))?;
    client.keyboard_command("settings", &json!({"active_effect":0}))?;
    for (name, result) in profile.apply(client) {
        result.map_err(|e| format!("{name} · {e}"))?;
    }
    let state = client.go_view()?.state;
    if state["mic_preset"] != "podcast"
        || state["hp_eq"]["style"] != "jeu"
        || state["hp_eq"]["intensity"] != 127
    {
        return Err("Profil GoXLR ou casque non rétabli.".into());
    }

    select(usb)?;
    client.post_module(
        crate::winmic::MODULE,
        "settings",
        json!({"device":usb,"gain_db":6,"eq":[0,0,1,1,0]}),
    )?;
    let usb_profile = AppProfile::capture(client, app, [true, false, false])?;
    client.post_module(
        crate::winmic::MODULE,
        "settings",
        json!({"device":usb,"gain_db":0,"eq":[0,0,0,0,0]}),
    )?;
    for (name, result) in usb_profile.apply(client) {
        result.map_err(|e| format!("{name} · {e}"))?;
    }
    let (state, current) = get_microphone(client)?;
    if current != usb
        || state["state"]["voice"]["gain_db"] != 6
        || state["state"]["voice"]["eq"] != json!([0, 0, 1, 1, 0])
    {
        return Err("Profil micro Windows non rétabli.".into());
    }
    println!("OK : profils par application, GoXLR, micro Windows, casque et éclairage simulés.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(app: PathBuf) -> AppProfile {
        AppProfile {
            app,
            mic: None,
            headphones: None,
            light: Some(Effect {
                id: 1,
                brightness: 93,
                speed: 3,
                reverse: false,
                colour: "#556677".into(),
            }),
        }
    }

    #[test]
    fn les_executables_de_meme_nom_dans_deux_dossiers_restent_distincts() {
        let root = std::env::temp_dir();
        let first = fixture(root.join("Studio").join("capsule.exe"));
        let second = fixture(root.join("Secours").join("capsule.exe"));
        let duplicate = fixture(root.join("studio").join("CAPSULE.EXE"));
        assert!(validate(&[first.clone(), second]));
        assert!(!validate(&[first, duplicate]));
        assert!(app_key(Path::new("capsule.exe")).is_none());
        assert!(app_key(&root.join("capsule.txt")).is_none());
    }

    #[test]
    fn une_ecriture_refusee_laisse_le_profil_sauvegarde_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("applications.json");
        let good = fixture(tmp.path().join("demo.exe"));
        store(&file, std::slice::from_ref(&good)).unwrap();
        assert!(store(&file, &[good.clone(), good.clone()]).is_err());
        let actual = load(&file).unwrap().pop().unwrap();
        assert_eq!(
            (actual.app, actual.light.unwrap().brightness),
            (good.app, 93)
        );
    }
}
