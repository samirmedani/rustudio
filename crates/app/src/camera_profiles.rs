//! Profils locaux de la caméra et déclenchement au premier plan d’une application associée.
use crate::{
    mixer::Mixer,
    ui::{self, Icon, Tone},
    webcam::Settings,
};
use gpui::{
    Context, Div, FocusHandle, KeyDownEvent, PathPromptOptions, RenderImage, Window, div, img,
    prelude::*, px,
};
use rustudio_windows_desktop::app_identity::{ICON_SIZE, foreground_executable, icon_bgra};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const MAX_PROFILES: usize = 32;
const MAX_BYTES: u64 = 128 * 1024;

#[derive(Clone, Deserialize, Serialize)]
struct CameraProfile {
    name: String,
    settings: Value,
    app: Option<PathBuf>,
}

fn app_key(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?;
    if !extension.eq_ignore_ascii_case("exe") {
        return None;
    }
    Some(rustudio_core::state::norm_app(path.file_name()?.to_str()?))
}

pub(crate) fn app_name(path: &Path) -> String {
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Application");
    let key = app_key(path).unwrap_or_default();
    match key.as_str() {
        "discord" => "Discord".into(),
        "obs64" | "obs32" => "OBS Studio".into(),
        _ => path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or(file)
            .to_owned(),
    }
}

pub(crate) fn profile_settings(s: &Settings) -> Value {
    json!({
        "brightness": s.brightness, "contrast": s.contrast, "saturation": s.saturation,
        "sharpness": s.sharpness, "gain": s.gain,
        "white_balance_auto": s.white_balance_auto, "white_balance": s.white_balance,
        "backlight_comp": s.backlight_comp, "power_line": s.power_line,
        "exposure_auto": s.exposure_auto, "exposure": s.exposure, "low_light_comp": s.low_light_comp,
        "focus_auto": s.focus_auto, "focus": s.focus, "zoom": s.zoom, "pan": s.pan, "tilt": s.tilt,
    })
}

pub(crate) struct CameraProfiles {
    path: PathBuf,
    entries: Vec<CameraProfile>,
    icons: HashMap<PathBuf, Arc<RenderImage>>,
    input: String,
    focus: FocusHandle,
    error: Option<String>,
    readable: bool,
    foreground: Option<String>,
}

impl CameraProfiles {
    pub(crate) fn new(path: PathBuf, focus: FocusHandle) -> Self {
        let loaded = read_profiles(&path);
        let readable = loaded.is_ok();
        let (entries, error) = match loaded {
            Ok(items) => (items, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let icons = entries
            .iter()
            .filter_map(|item| item.app.as_ref())
            .filter_map(|path| app_icon(path).map(|icon| (path.clone(), icon)))
            .collect();
        Self {
            path,
            entries,
            icons,
            input: String::new(),
            focus,
            error,
            readable,
            foreground: None,
        }
    }

    fn edit(&mut self, edit: impl FnOnce(&mut Vec<CameraProfile>) -> Result<(), String>) {
        self.error = (|| {
            if !self.readable {
                return Err("Les profils existants sont illisibles.".into());
            }
            let mut next = self.entries.clone();
            edit(&mut next)?;
            save_profiles(&self.path, &next)?;
            self.entries = next;
            self.foreground = None;
            Ok(())
        })()
        .err();
    }
}

pub(crate) fn app_icon(path: &Path) -> Option<Arc<RenderImage>> {
    crate::webcam::make_render_image(ICON_SIZE, ICON_SIZE, icon_bgra(path)?)
}

fn read_profiles(path: &Path) -> Result<Vec<CameraProfile>, String> {
    match fs::metadata(path) {
        Ok(info) if info.len() <= MAX_BYTES => {}
        Ok(_) => return Err("Le fichier des profils Caméra est trop volumineux.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Lecture des profils Caméra impossible ({e}).")),
    }
    let bytes =
        fs::read(path).map_err(|e| format!("Lecture des profils Caméra impossible ({e})."))?;
    let items: Vec<CameraProfile> =
        serde_json::from_slice(&bytes).map_err(|_| "Profils Caméra illisibles.".to_owned())?;
    if items.len() > MAX_PROFILES
        || items
            .iter()
            .any(|p| p.name.trim().is_empty() || !p.settings.is_object())
    {
        return Err("Profils Caméra invalides.".into());
    }
    Ok(items)
}

fn save_profiles(path: &Path, items: &[CameraProfile]) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(items).map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_BYTES {
        return Err("Les profils Caméra occupent trop d’espace.".into());
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
    result.map_err(|e| format!("Enregistrement des profils Caméra impossible ({e})."))
}

impl Mixer {
    pub(crate) fn watch_foreground_profiles(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(900))
                    .await;
                let Ok(watch) = this.update(cx, |this, _| {
                    this.client.is_live() && (this.has_application_profiles()
                        || this.camera_profiles.entries.iter().any(|p| p.app.is_some()))
                }) else {
                    break;
                };
                if !watch {
                    continue;
                }
                let app = cx
                    .background_executor()
                    .spawn(async { foreground_executable() })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.follow_application_profile(app.as_deref(), cx);
                        let app = app.as_deref().and_then(app_key);
                        if this.webcam.busy || this.camera_profiles.foreground == app {
                            return;
                        }
                        this.camera_profiles.foreground = app.clone();
                        let Some(app) = app else { return };
                        let matching = this.camera_profiles.entries.iter().find(|p| {
                            p.app.as_deref().and_then(app_key).as_deref() == Some(app.as_str())
                        });
                        if let Some(profile) = matching {
                            this.param_held.clear();
                            this.webcam_send(
                                "settings",
                                profile.settings.clone(),
                                "Profil caméra appliqué à l’application active",
                                cx,
                            );
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn save_camera_profile(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.webcam.live() else { return };
        if self.webcam.busy {
            return;
        }
        let settings = profile_settings(s);
        let name = self.camera_profiles.input.trim().to_owned();
        self.camera_profiles.edit(|items| {
            if name.is_empty() {
                return Err("Donne un nom au profil.".into());
            }
            if items.len() >= MAX_PROFILES {
                return Err("La liste des profils Caméra est pleine.".into());
            }
            if items.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
                return Err("Un profil porte déjà ce nom.".into());
            }
            items.push(CameraProfile {
                name,
                settings,
                app: None,
            });
            Ok(())
        });
        if self.camera_profiles.error.is_none() {
            self.camera_profiles.input.clear();
        }
        cx.notify();
    }

    fn choose_camera_app(&mut self, name: String, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let path = match picker.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else { return };
            let key = app_key(&path);
            let selected = path.clone();
            let (valid, icon) = cx
                .background_executor()
                .spawn(async move {
                    let valid = selected.is_file();
                    let icon = valid.then(|| icon_bgra(&selected)).flatten();
                    (valid, icon)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.camera_profiles.edit(|items| {
                    let key = key.ok_or("Choisis l’exécutable Windows de l’application (.exe).")?;
                    if !valid {
                        return Err("L’application est introuvable.".into());
                    }
                    if items.iter().any(|p| {
                        p.name != name
                            && p.app.as_deref().and_then(app_key).as_deref() == Some(&key)
                    }) {
                        return Err("Un profil est déjà associé à cette application.".into());
                    }
                    let item = items
                        .iter_mut()
                        .find(|p| p.name == name)
                        .ok_or("Profil introuvable.")?;
                    item.app = Some(path.clone());
                    Ok(())
                });
                if this.camera_profiles.error.is_none()
                    && let Some(icon) = icon.and_then(|pixels| {
                        crate::webcam::make_render_image(ICON_SIZE, ICON_SIZE, pixels)
                    })
                {
                    this.camera_profiles.icons.insert(path, icon);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn camera_profiles_card(
        &self,
        window: &Window,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let profiles = &self.camera_profiles;
        let input = ui::field(
            "camera-profile-name",
            &profiles.focus,
            &profiles.input,
            "Nom du profil",
            profiles.focus.is_focused(window),
            enabled,
        )
        .min_w(px(150.))
        .flex_1()
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
            match ui::edit_text(&mut this.camera_profiles.input, event, 40, |_| true, cx) {
                ui::Edit::Submit => this.save_camera_profile(cx),
                ui::Edit::Changed => {}
                ui::Edit::Ignored => return,
            }
            cx.stop_propagation();
            cx.notify();
        }));
        let mut card = ui::section(Icon::Bookmark, ui::ACCENT, "Mes profils", None).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(input)
                .child(
                    ui::pill(
                        "camera-profile-save",
                        Some(Icon::Check),
                        "Enregistrer mes réglages",
                        Tone::Primary,
                    )
                    .when(!enabled, |d| d.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.save_camera_profile(cx))),
                ),
        );
        if let Some(error) = &profiles.error {
            card = card.child(ui::notice(false, error.clone()));
        }
        for (i, profile) in profiles.entries.iter().enumerate() {
            let settings = profile.settings.clone();
            let name = profile.name.clone();
            let unlink = profile.name.clone();
            let glyph = profile
                .app
                .as_ref()
                .and_then(|path| profiles.icons.get(path))
                .map(|icon| img(icon.clone()).size(px(18.)));
            card = card.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        ui::pill(
                            ("camera-profile", i),
                            Some(Icon::Bookmark),
                            profile.name.clone(),
                            Tone::Plain,
                        )
                        .when(enabled, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| {
                                this.param_held.clear();
                                this.webcam_send(
                                    "settings",
                                    settings.clone(),
                                    "Profil caméra appliqué",
                                    cx,
                                );
                            }))
                        }),
                    )
                    .child(
                        ui::pill(
                            ("camera-profile-app", i),
                            glyph.is_none().then_some(Icon::External),
                            profile
                                .app
                                .as_deref()
                                .map(app_name)
                                .unwrap_or_else(|| "Associer une appli".into()),
                            Tone::Plain,
                        )
                        .children(glyph)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_camera_app(name.clone(), cx)
                        })),
                    )
                    .when(profile.app.is_some(), |d| {
                        d.child(
                            ui::pill(
                                ("camera-profile-unlink", i),
                                Some(Icon::Close),
                                "Dissocier",
                                Tone::Plain,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.camera_profiles.edit(|items| {
                                        let p = items
                                            .iter_mut()
                                            .find(|p| p.name == unlink)
                                            .ok_or("Profil introuvable.")?;
                                        p.app = None;
                                        Ok(())
                                    });
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            );
        }
        card
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_profil_garde_lapplication_sur_disque_et_suit_ses_mises_a_jour() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("camera.json");
        let app = PathBuf::from("Discord.exe");
        let items = vec![CameraProfile {
            name: "Appel".into(),
            settings: json!({"brightness": 130}),
            app: Some(app),
        }];
        save_profiles(&path, &items).unwrap();
        let mut loaded = read_profiles(&path).unwrap();
        assert_eq!(loaded[0].settings["brightness"], 130);
        assert_eq!(
            loaded[0].app.as_deref().and_then(app_key),
            app_key(Path::new("app-2/Discord.EXE"))
        );
        loaded[0].app = None;
        save_profiles(&path, &loaded).unwrap();
        assert!(read_profiles(&path).unwrap()[0].app.is_none());
    }
}
