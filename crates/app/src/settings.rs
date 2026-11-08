//! Réglages de l'appli, configuration complète, mises à jour et retours de version.
use crate::{
    desktop::{Page, hint, panel},
    mixer::{Mixer, button},
};
use gpui::{Context, Div, PathPromptOptions, div, prelude::*, px};
use rustudio_windows_desktop::session;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

const UPDATE_BUSY: [&str; 4] = ["downloading", "verifying", "extracting", "applying"];

impl Mixer {
    /// Lecture des données de la page Réglages ou Versions.
    pub fn settings_data(client: &crate::backend::Client, page: Page) -> Result<Value, String> {
        if page == Page::Settings {
            let state = client.get("/api/state")?;
            return Ok(json!({"prefs": state["state"]["prefs"], "startup": session::startup_enabled(), "connected": state["connected"]}));
        }
        if !client.is_live() {
            return Ok(json!({"update": {"current": "Démonstration", "release_build": false, "status": "dev"},
                "history": {"versions": [{"version": "Démonstration", "notes": "Données simulées : aucune mise à jour n’est installée.", "installed": true}]}}));
        }
        Ok(json!({"update": client.get("/api/update")?, "history": client.get("/api/update/history")?}))
    }

    pub fn after_settings_load(&mut self, page: Page, data: &Value, cx: &mut Context<Self>) {
        let busy = UPDATE_BUSY.iter().any(|p| data["update"]["phase"] == *p) || data["update"]["status"] == "checking";
        if page == Page::Versions && busy {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(900)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.page == page {
                        this.library_load(page, cx);
                    }
                });
            })
            .detach();
        }
    }

    fn engine_action(&mut self, path: &'static str, action: &'static str, body: Value, done: &'static str, cx: &mut Context<Self>) {
        if self.library.busy {
            return;
        }
        self.library.busy = true;
        self.library.confirm = None;
        let client = self.client.clone();
        let page = self.page;
        let job = cx.background_executor().spawn(async move { client.post_module(path, action, body) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.library.busy = false;
                this.library.notice = Some(match result {
                    Ok(_) => (true, done.to_string()),
                    Err(error) => (false, error),
                });
                if this.page == page {
                    this.library_load(page, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn ask(&mut self, message: String, path: &'static str, action: &'static str, body: Value, done: &'static str, cx: &mut Context<Self>) {
        self.library.confirm = Some((message, path, action, body, done));
        cx.notify();
    }

    fn confirm_box(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (message, ..) = self.library.confirm.as_ref()?;
        Some(
            panel().child(div().child(message.clone())).child(
                div().flex().gap_2()
                    .child(button("lib-confirm".into(), "Confirmer".into(), true).on_click(cx.listener(|this, _, _, cx| {
                        if let Some((_, path, action, body, done)) = this.library.confirm.take() {
                            this.engine_action(path, action, body, done, cx);
                        }
                    })))
                    .child(button("lib-cancel".into(), "Annuler".into(), false).on_click(cx.listener(|this, _, _, cx| {
                        this.library.confirm = None;
                        cx.notify();
                    }))),
            ),
        )
    }

    fn notice_line(&self) -> Option<Div> {
        let (ok, text) = self.library.notice.as_ref()?;
        Some(crate::ui::notice(*ok, text.clone()))
    }

    fn export_config(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let initial = std::env::var_os("USERPROFILE").map(PathBuf::from).map(|p| p.join("Documents")).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let picker = cx.prompt_for_new_path(&initial, Some("configuration-rustudio.json"));
        cx.spawn(async move |this, cx| {
            let result = match picker.await {
                Ok(Ok(Some(path))) => {
                    let path = if path.extension().is_none() { path.with_extension("json") } else { path };
                    cx.background_executor().spawn(async move {
                        let state = client.get("/api/modules/goxlr-mini/state")?;
                        let mut config = state["state"].clone();
                        if !config.is_object() {
                            return Err("Configuration indisponible.".to_string());
                        }
                        // L'empreinte vocale et la mesure du micro restent sur ce PC.
                        for key in ["voice_print", "mic_cal"] {
                            config.as_object_mut().map(|o| o.remove(key));
                        }
                        let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
                        std::fs::write(&path, text).map_err(|e| format!("Écriture impossible : {e}"))
                    }).await
                }
                Ok(Ok(None)) => return,
                _ => Err("Impossible d’ouvrir le dossier d’export.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                this.library.notice = Some(match result {
                    Ok(()) => (true, "Configuration exportée.".into()),
                    Err(e) => (false, e),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn import_config(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let read = match picker.await {
                Ok(Ok(Some(paths))) if !paths.is_empty() => {
                    let path = paths[0].clone();
                    cx.background_executor().spawn(async move {
                        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                        if meta.len() > 4_000_000 {
                            return Err("Fichier trop volumineux.".to_string());
                        }
                        let value: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                            .map_err(|_| "Fichier illisible : JSON attendu.".to_string())?;
                        if !value.is_object() || value.get("volumes").is_none() {
                            return Err("Ce fichier n’est pas une configuration Rustudio.".into());
                        }
                        Ok(value)
                    }).await
                }
                Ok(Ok(_)) => return,
                _ => Err("Impossible d’ouvrir le sélecteur de fichiers.".into()),
            };
            let _ = this.update(cx, |this, cx| match read {
                Ok(config) => this.ask(
                    "Remplacer toute la configuration de la GoXLR Mini (mixeur, routage, éclairage, micro, scènes) par ce fichier ?".into(),
                    "/api/modules/goxlr-mini", "profile", json!({"profile": config}), "Configuration importée.", cx,
                ),
                Err(e) => {
                    this.library.notice = Some((false, e));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn settings_view(&self, data: &Value, cx: &mut Context<Self>) -> Div {
        let prefs = &data["prefs"];
        let hold = prefs["hold_ms"].as_i64().unwrap_or(600);
        let osd = prefs["osd"] != false;
        let startup = data["startup"] == true;
        let step = |delta: i64| (hold + delta).clamp(250, 1500);
        let mut root = div().flex().flex_col().gap_3();
        if let Some(n) = self.notice_line() {
            root = root.child(n);
        }
        if let Some(c) = self.confirm_box(cx) {
            root = root.child(c);
        }
        let (less, more) = (step(-50), step(50));
        let behaviour = panel().child(crate::ui::heading("Boutons et notifications"))
            .child(div().flex().items_center().justify_between()
                .child(hint("Durée d’un appui long"))
                .child(div().flex().items_center().gap_2()
                    .child(button("hold-less".into(), "−".into(), false).on_click(cx.listener(move |this, _, _, cx| {
                        this.engine_action("/api/modules/goxlr-mini", "settings", json!({"prefs": {"hold_ms": less}}), "Durée enregistrée.", cx)
                    })))
                    .child(div().w(px(70.)).child(format!("{:.2} s", hold as f64 / 1000.).replace('.', ",")))
                    .child(button("hold-more".into(), "+".into(), false).on_click(cx.listener(move |this, _, _, cx| {
                        this.engine_action("/api/modules/goxlr-mini", "settings", json!({"prefs": {"hold_ms": more}}), "Durée enregistrée.", cx)
                    })))))
            .child(div().flex().items_center().justify_between()
                .child(hint("Bulle à l’écran quand le micro change"))
                .child(crate::ui::toggle("osd-toggle", osd, true).on_click(cx.listener(move |this, _, _, cx| {
                    this.engine_action("/api/modules/goxlr-mini", "settings", json!({"prefs": {"osd": !osd}}), "Préférence enregistrée.", cx)
                }))));
        root = root.child(behaviour);
        if self.client.is_live() {
            root = root.child(panel().child(crate::ui::heading("Windows"))
                .child(div().flex().items_center().justify_between()
                    .child(hint("Démarrer Rustudio avec Windows (icône près de l’horloge)"))
                    .child(crate::ui::toggle("startup-toggle", startup, true).on_click(cx.listener(move |this, _, _, cx| {
                        this.library.notice = Some(match session::set_startup(!startup) {
                            Ok(()) => (true, "Démarrage avec Windows mis à jour.".into()),
                            Err(e) => (false, e),
                        });
                        this.library_load(Page::Settings, cx);
                    })))));
        }
        root.child(panel().child(crate::ui::heading("Configuration complète"))
            .child(hint("Sauvegarde toute la configuration de la GoXLR Mini dans un fichier, sans l’empreinte vocale. L’import remplace la configuration après confirmation."))
            .child(div().flex().gap_2()
                .child(button("config-export".into(), "Exporter ma configuration".into(), false).on_click(cx.listener(|this, _, _, cx| this.export_config(cx))))
                .child(button("config-import".into(), "Importer une configuration".into(), false).on_click(cx.listener(|this, _, _, cx| this.import_config(cx))))))
    }

    pub fn versions_view(&self, data: &Value, cx: &mut Context<Self>) -> Div {
        let u = &data["update"];
        let mut root = div().flex().flex_col().gap_3();
        if let Some(n) = self.notice_line() {
            root = root.child(n);
        }
        if let Some(c) = self.confirm_box(cx) {
            root = root.child(c);
        }
        let current = u["current"].as_str().unwrap_or("?").to_string();
        let busy = UPDATE_BUSY.iter().any(|p| u["phase"] == *p);
        let mut card = panel().child(crate::ui::heading(format!("Rustudio {current}")));
        if busy {
            let progress = u["progress"].as_f64().map(|p| format!(" · {} %", (p * 100.).round())).unwrap_or_default();
            card = card.child(hint(match u["phase"].as_str() {
                Some("downloading") => format!("Téléchargement{progress}"),
                Some("verifying") => "Vérification de l’empreinte…".into(),
                Some("extracting") => "Préparation…".into(),
                _ => "Installation : Rustudio redémarre et se rouvre tout seul.".into(),
            }));
        } else if u["phase"] == "failed" {
            card = card.child(hint(format!("Échec : {}", u["error"]["msg"].as_str().unwrap_or("mise à jour interrompue"))));
        } else if u["status"] == "available" {
            let latest = u["latest"]["version"].as_str().unwrap_or("?").to_string();
            card = card.child(hint(format!("Rustudio {latest} est disponible.")))
                .child(div().text_sm().child(u["latest"]["notes"].as_str().unwrap_or("").chars().take(1200).collect::<String>()));
            if u["installable"] == true {
                card = card.child(button("update-install".into(), format!("Installer la {latest}"), true).on_click(cx.listener(move |this, _, _, cx| {
                    this.ask(format!("Installer Rustudio {latest} ? Rustudio redémarre pendant l’installation et revient tout seul."), "/api", "update-install", json!({}), "Installation lancée.", cx)
                })));
            } else {
                card = card.child(hint(u["why_not"].as_str().unwrap_or("Installation automatique indisponible : passe par la page de la release.").to_owned()));
            }
        } else if u["release_build"] == false {
            card = card.child(hint("Version de développement : les mises à jour automatiques sont désactivées."));
        } else {
            card = card.child(hint(match u["status"].as_str() {
                Some("up-to-date") => "Rustudio est à jour.",
                Some("checking") => "Recherche en cours…",
                _ => "Aucune recherche récente.",
            }));
        }
        if self.client.is_live() && !busy {
            card = card.child(button("update-check".into(), "Rechercher une mise à jour".into(), false).on_click(cx.listener(|this, _, _, cx| {
                this.engine_action("/api", "update-check", json!({}), "Recherche terminée.", cx)
            })));
        }
        root = root.child(card);
        for item in data["history"]["versions"].as_array().into_iter().flatten() {
            let version = item["version"].as_str().unwrap_or("?").to_string();
            let expanded = self.library.expanded.as_deref() == Some(&version);
            let id = version.clone();
            let label = if item["installed"] == true { format!("{version} · installée") } else { version.clone() };
            let mut row = panel().child(div().flex().items_center().justify_between()
                .child(button(format!("release-{version}"), label, expanded).on_click(cx.listener(move |this, _, _, cx| {
                    this.library.expanded = if this.library.expanded.as_ref() == Some(&id) { None } else { Some(id.clone()) };
                    cx.notify();
                })))
                .when(item["revertible"] == true && !busy && self.client.is_live(), |d| {
                    let target = version.clone();
                    d.child(button(format!("revert-{version}"), "Revenir à cette version".into(), false).on_click(cx.listener(move |this, _, _, cx| {
                        this.ask(format!("Revenir à Rustudio {target} ? Tes réglages restent ; mêmes vérifications qu’une mise à jour."), "/api", "update-install", json!({"version": target}), "Retour lancé.", cx)
                    })))
                }));
            if expanded {
                row = row.child(div().text_sm().child(item["notes"].as_str().unwrap_or("Aucune note.").chars().take(3000).collect::<String>()));
            }
            root = root.child(row);
        }
        root
    }
}
